# ADR-017: Schema Migration Strategy — `rusqlite_migration`

## Status

Accepted (2026-05-17). Closes Q-INFO-3 from `docs/feature/tool-model-info-sqlite-cache/discuss/requirements.md`.

Sister to ADR-015 (the cache requires a migration framework) and ADR-016 (the trait extension's persistence story depends on schema evolution).

## Context

The `tool-model-info-sqlite-cache` feature introduces the first **persistent on-disk schema** in the modeltap codebase. Schema evolution across modeltap releases is a question DISCUSS deferred to DESIGN.

The constraints (per `requirements.md` C-INFO-6):

1. **Forward-only.** No down migrations. Users upgrade modeltap binaries; they do not roll back schemas.
2. **Additive where possible.** New tables, new nullable columns are preferred. Destructive changes (rename, type change) are deferred or paired with corruption-recovery rebuild.
3. **Idempotent.** Re-running a failed migration produces the same end state OR fails identically.
4. **No async runtime change.** The existing cache layer is sync (per ADR-015 §"Sync-store-from-async-app"); the migration framework must not force tokio coupling.

## Decision

**`rusqlite_migration` crate (v1.2), embedded SQL migrations under `crates/modeltap-store/migrations/NNNN_<description>.sql`. The `from-directory` feature gates compile-time inclusion. The store's `Migrator` module wraps the framework's `Migrations` type.**

### Migration file layout

```
crates/modeltap-store/
└── migrations/
    ├── 0001_initial.sql                 # v1 schema (cache_meta, cache_tools, cache_models, cache_model_files)
    ├── 0002_add_sha256_persistence.sql  # (deferred, US-27 / Release 3) — adds cache_sha256 table
    └── 0003_add_per_tool_ttl_override.sql  # (post-Release-2 optional, Q-INFO-4) — adds cache_tools.ttl_seconds_override
```

Files are append-only after merge. Editing a shipped migration is FORBIDDEN — the next migration corrects it instead.

### Versioning

`PRAGMA user_version` in the SQLite file stores the applied schema version. The binary embeds a compile-time constant:

```rust
// modeltap-store/src/migrate.rs
pub const EXPECTED_SCHEMA_VERSION: u32 = 1;  // bumped in lockstep with each new migration file
```

### Migrator behavior

```
Cache::open(path):
  Connection::open(path)
  PRAGMA journal_mode = WAL
  PRAGMA busy_timeout = 5000
  match read PRAGMA user_version:
    v == EXPECTED_SCHEMA_VERSION:
      return OpenedExisting
    v < EXPECTED_SCHEMA_VERSION:
      run Migrator.to_latest(conn):  # rusqlite_migration applies missing migrations in order
        on success: return OpenedAfterMigration { from: v, to: EXPECTED_SCHEMA_VERSION }
        on failure: enter Recovery (rename to .corrupt-<ts>, fresh cache)
    v > EXPECTED_SCHEMA_VERSION:
      enter Recovery (rename to .future-version-<n>, fresh cache)
```

The Recovery path is shared with ADR-015's corruption recovery — three failure modes converge on a single rename-and-cold-start procedure.

### Migration discipline

When adding a new migration:

1. **Create `migrations/NNNN_<description>.sql`** where `NNNN` is the next sequential number (zero-padded to 4 digits).
2. **Bump `EXPECTED_SCHEMA_VERSION`** in `migrate.rs` to `NNNN`.
3. **Add a test under `crates/modeltap-store/tests/migration.rs`** that:
   - Asserts the migration applies cleanly to a fresh DB (v0 → vNNNN).
   - Asserts the migration applies cleanly to a DB at vNNNN-1 (the immediately-prior version).
   - Asserts the post-migration schema matches the documented DDL in `data-models.md`.
4. **Update `data-models.md`** with the new DDL.
5. **Reference the migration filename in the matching ADR or feature design.**

The discipline is enforced via:
- A `crates/modeltap-store/tests/migration.rs::test_expected_version_matches_files` test that asserts `EXPECTED_SCHEMA_VERSION` equals the highest-numbered file in `migrations/`.
- Code review checklist item.

## Alternatives Considered

### A — `rusqlite_migration` (CHOSEN)

**Pros:**
- **Minimal dependency.** ~500 LoC; one transitive dep (rusqlite itself, which we already need).
- **Embedded SQL.** Migrations live as `.sql` files in the source tree; `from-directory` feature embeds them at compile time. No file-system lookup at runtime.
- **Forward-only by design.** Matches C-INFO-6.
- **Idempotency built-in.** The framework checks `user_version` before applying; re-running has no effect.
- **No async runtime coupling.** Sync API matches the cache layer.
- **MIT/Apache-2.0 license.**
- **Active maintenance** (quarterly releases as of 2026-05).

**Cons:**
- Single primary maintainer. Mitigation: small API surface (~50 LoC of calls) means we could fork or inline the relevant logic in <100 LoC if abandoned.
- One more dep to audit. Acceptable trade-off for the migration discipline it enforces.

### B — Hand-rolled SQL + custom migrator

```rust
// pseudo-code
fn migrate(conn: &Connection) -> Result<()> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current < 1 {
        conn.execute_batch(include_str!("../migrations/0001_initial.sql"))?;
        conn.execute("PRAGMA user_version = 1", [])?;
    }
    if current < 2 {
        conn.execute_batch(include_str!("../migrations/0002_add_sha256_persistence.sql"))?;
        conn.execute("PRAGMA user_version = 2", [])?;
    }
    Ok(())
}
```

**Pros:**
- Zero new dependency. Matches a hypothetical "lean dependencies" preference.
- Full control over migration semantics.

**Cons:**
- **We rewrite what `rusqlite_migration` already does.** Reinvented wheel.
- **Subtle bugs are likely** in version-comparison and migration-order logic. The framework has already battle-tested these.
- **Adding a new migration** requires editing both the SQL file AND the Rust migrator function — two places to update; easy to drift.
- **No idempotency guarantee** without writing additional logic.

**Rejected** because the dep cost (50 LoC of framework calls) is lower than the maintenance cost (200+ LoC of hand-rolled migrator + tests for the framework's already-tested behavior).

### C — `sqlx::migrate!` macro

**Pros:**
- Industry-standard Rust migration tooling.
- Compile-time SQL checking (if used with `query!` macros downstream).

**Cons:**
- **Requires switching from `rusqlite` to `sqlx::SqliteConnection`.** Sqlx is async-first; using its sync API is awkward.
- **Compile-time SQL checking requires a `DATABASE_URL` at build time.** Hostile to CI environments without a pre-seeded DB.
- **Dep footprint is ~3× rusqlite's.** Pulls a connection pool, an executor abstraction, and migration tooling we'd use a tiny slice of.
- **No future need for Postgres/MySQL.** The whole sqlx value prop (database portability) is irrelevant for a local-CLI tool.

**Rejected** as disproportionate.

### D — `refinery` migration crate

**Pros:**
- Backend-agnostic (works with rusqlite, sqlx, postgres).
- Mature.

**Cons:**
- **Heavier than `rusqlite_migration`** without buying features we need.
- **Backend-agnostic abstraction** isn't useful when we're rusqlite-only.
- Slightly larger dep tree.

**Rejected** as overkill for SQLite-only.

## Consequences

### Positive

- **Migration discipline is enforced by the framework**, not by code review alone.
- **Adding a new migration is mechanical:** write SQL file, bump constant, write test, update docs.
- **Idempotency is free.** The framework checks `user_version` before applying.
- **Compile-time embedding** means the binary is self-contained; no `migrations/` directory shipped alongside.

### Negative

- **One more dep** (`rusqlite_migration`). Small surface, permissive license, active maintenance — acceptable.
- **No down migrations.** This is a deliberate choice (C-INFO-6); users with corrupted state recover via the rename-and-cold-start path, not via schema rollback.

### Neutral

- **Schema migrations are not user-visible.** They happen on launch; the user sees a brief log line in `diagnostics.log` and nothing in the TUI unless a migration fails (in which case the recovery banner appears per ADR-015 §5).

## Enforcement

### Test matrix in `crates/modeltap-store/tests/migration.rs`

1. **Fresh-DB application:** `user_version = 0` → run migrations → `user_version = EXPECTED_SCHEMA_VERSION`; assert tables exist with correct DDL.
2. **Already-migrated DB:** `user_version = EXPECTED_SCHEMA_VERSION` → run migrations → no-op; assert DDL unchanged.
3. **Partial-migration failure (simulated via injection):** the next open detects partial state; corruption-recovery triggers; the next-next open succeeds against a fresh DB.
4. **`EXPECTED_SCHEMA_VERSION` matches files:** the test introspects the `migrations/` directory and asserts the constant equals the highest-numbered file. Prevents the "added file, forgot constant bump" failure mode.
5. **Forward path from every prior version:** for each shipped version `v`, populate a DB at `v`, run migrations, assert end state is `EXPECTED_SCHEMA_VERSION`. (For v1 there is only one such test; the matrix grows with each release.)

### CI integration

`cargo test --workspace` runs the migration tests on every PR. Per CLAUDE.md CI discipline, `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` runs before merge to main.

## Migration trigger

This ADR will be reconsidered if:

- `rusqlite_migration` is abandoned (single-maintainer risk). Mitigation: inline a 100-LoC equivalent in `modeltap-store/src/migrate.rs`. The API surface we use is tiny.
- A second persistent backend becomes desirable (unlikely for v1.x — see ADR-015 §"Migration trigger").
- A migration genuinely requires irreversible destructive change (a column rename that cannot be expressed as add-then-deprecate). In that case, the migration is paired with a corruption-recovery rebuild — accept the precedent and document.

## Cross-references

- ADR-015 (State Model: SQLite-Backed Cache) — establishes the requirement for a migration framework.
- ADR-016 (Tool Trait Extension: `inspect_*`) — its persistence story depends on schema evolution.
- ADR-018 (SHA256 Persistence Boundary) — describes the future v2 migration for US-27.
- `docs/feature/tool-model-info-sqlite-cache/design/data-models.md` — v1 DDL.
- `docs/feature/tool-model-info-sqlite-cache/discuss/prioritization.md` §"Schema versioning strategy" — DISCUSS rationale.
