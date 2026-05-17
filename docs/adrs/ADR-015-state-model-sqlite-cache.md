# ADR-015: State Model — SQLite-Backed Cache With Pre-Mutate Revalidation

## Status

Accepted (2026-05-17). **Supersedes ADR-003** (State Model — Stateless Rediscovery, No Persistent Index).

Closes the 9 architectural questions enumerated in `docs/feature/tool-model-info-sqlite-cache/discuss/requirements.md` §"What the new ADR must close".

## Context

The intake brief for `tool-model-info-sqlite-cache` is unambiguous:

> "Let's also refactor this code so it stores information about all the data collected from the tools and models in a local sqlite database."

This **reverses ADR-003**, which established stateless rediscovery on every launch. The reversal is warranted now because:

1. **Cumulative launch cost is real.** Devon Park opens modeltap many times per day; the ~1.15 s discovery cost per launch is acceptable in isolation but punishing across a workflow.
2. **Inspection (US-21, US-22) raises the bar for metadata freshness across launches.** Detail screens that re-introspect on every open feel slow; persisted metadata feels instant.
3. **SHA256 lazy-compute (ADR-002) is thrown away on quit** — wasteful for users with large libraries, and the cache enables future persistence (US-27, Release 3).
4. **The cache's safety risks are addressed by an explicit design rule** that did not exist when ADR-003 was written: **cache is paint-only; filesystem is authoritative on mutate.**

ADR-003's "Negative Consequence" item — "Users with very large libraries (1000+ models) may notice discovery latency. If users complain, ADR-003 may be revisited" — explicitly anticipated this supersession.

## Decision

**modeltap maintains a SQLite-backed cache of tool and model inventory at `$XDG_DATA_HOME/modeltap/cache.sqlite` (resolved via the `dirs` crate). The cache is paint-only on read paths and pre-mutate-revalidated on write paths. Cache failure NEVER blocks launch — cold-start (ADR-003's path) is the guaranteed-good fallback for every failure mode.**

### The 9 items this ADR closes

#### 1. Persistence on/off semantics + opt-out path

The cache is **enabled by default**. Opt-out via:
- **CLI flag:** `--no-cache` — true bypass; zero bytes written to the cache file or its location for the launch.
- **Config file:** `[cache] enabled = false` in `~/.modeltap/config.toml` — same semantics as `--no-cache`.

**Precedence:** CLI flag wins when both present. When opted out, modeltap behaves identically to the pre-supersession ADR-003 state model.

Verification: integration test `tests/acceptance/cache_disabled.rs::no_cache_writes` monitors the cache path for any write during a `--no-cache` launch.

#### 2. Refresh policy (TTL + manual + background reconcile)

Three refresh mechanisms, all per-tool:

| Trigger | Scope | When |
|---|---|---|
| Background reconcile on launch | All tools (parallel) | After warm-paint completes; per-tool runs in parallel via existing ADR-003 discovery orchestrator |
| Manual `[r]` | Currently-selected tool | User-initiated, mid-session |
| Manual `[Shift+R]` | All tools (parallel) | User-initiated, mid-session |

**Per-tool TTL:** Default 24 hours (`cache.tool_ttl_seconds = 86400`). Tools whose `last_scan_at` is older than the TTL do NOT paint from cache at warm-start; they cold-start instead. The TTL is configurable globally; per-tool override is deferred (Q-INFO-4 in DISCUSS).

#### 3. Pre-mutate revalidation algorithm

Before any destructive filesystem action (`unify`, `zap`, `delete_one`, `folder_delete`), every targeted file's `(mtime, size, inode, dev)` quad is re-stat'd against the cache:

```
For each target_path in targets:
    fresh_stat = std::fs::metadata(target_path)  // may return io::Error
    cached_stat = cache.files_for_model(model_id) → find matching path

    if cached_stat is None:
        return ValidationResult::Gone

    if fresh_stat fails with NotFound:
        return ValidationResult::Gone

    if (fresh_stat.mtime, .size, .inode, .dev) == cached_stat quad:
        return ValidationResult::Match
    else:
        return ValidationResult::Drift { fresh: fresh_stat }
```

**Why the quad and not `(mtime, size)`:** `inode + dev` defeats accidental mtime-preserving file replacement (e.g., `cp --preserve=timestamps`). The quad makes false-positive "cache valid" results require an adversarial actor with low-level filesystem access.

**Composition-root behavior on each result:**

| ValidationResult | Action |
|---|---|
| Match | Proceed to mutation |
| Drift | Call `Tool::inspect_model` to refresh metadata; update cache; refresh confirmation dialog; user re-confirms if reclaim/dedup changed beyond rounding |
| Gone | Abort the action; emit "file no longer exists; refreshing inventory"; auto-trigger per-tool refresh |

#### 4. Cache file location + permissions

**Path:** `$XDG_DATA_HOME/modeltap/cache.sqlite`, resolved via `dirs::data_dir().join("modeltap/cache.sqlite")`.

Platform resolution:
- **Linux:** `~/.local/share/modeltap/cache.sqlite` (or `$XDG_DATA_HOME/modeltap/cache.sqlite` if set)
- **macOS:** `~/Library/Application Support/modeltap/cache.sqlite`
- **Windows WSL:** Linux path resolution (parent constraint: WSL-only on Windows)

**Override:** `MODELTAP_CACHE_PATH` environment variable, for testing and power users.

**Permissions:** File created with default umask (typically `0644` on Unix). No special permissions required — the cache contains paths and sizes, not secrets. Privacy guarantee: cache never leaves the local machine (no network I/O introduced).

#### 5. Corruption recovery procedure

Three failure modes resolve to **the same outcome**: rename file, log event, cold-start.

| Trigger | Detected by | Rename target |
|---|---|---|
| `SQLITE_CORRUPT` on open | `rusqlite::Connection::open` returns error code 11 | `cache.sqlite.corrupt-<ISO-8601-timestamp>` |
| Schema downgrade (cache version > binary expected) | `PRAGMA user_version` > `EXPECTED_SCHEMA_VERSION` | `cache.sqlite.future-version-<n>` |
| Migration failure | `rusqlite_migration` returns error mid-migration | `cache.sqlite.corrupt-<ISO-8601-timestamp>` |

**Recovery procedure:**

```
1. Compute new path (corrupt or future-version variant)
2. std::fs::rename(cache_path, new_path)  -- best-effort; absorb errors
3. Append diagnostics.log: cache_recovery reason=<reason> renamed_to=<path>
4. Return OpenedFresh(new empty cache at original path)
5. TUI shows a dismissable recovery banner on first paint
6. Cold-start discovery proceeds normally; populates the new empty cache
```

**Recovery MUST always succeed.** Empty cache + cold-start is the ADR-003 baseline; guaranteed-good fallback. Verified by integration test `tests/acceptance/cache_recovery.rs::corrupted_cache_does_not_block_launch`.

#### 6. Multi-process concurrency

SQLite WAL journal mode + `busy_timeout`. No file locking beyond what SQLite provides natively.

**Open-time PRAGMAs:**
```sql
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;  -- ms
```

**Semantics:**
- Multiple modeltap processes can read concurrently from their own snapshots without blocking each other.
- Write transactions serialize via SQLite's internal locking; if process B tries to write while process A's transaction is in flight, B waits up to 5 seconds before returning `SQLITE_BUSY`.
- Each process's view is **its own snapshot** — drift between two open instances is acceptable; pre-mutate revalidation gates destructive actions against the filesystem regardless of cached state.

**No PID detection, no advisory locks.** Per intake Q5 / parent ADR-009 inheritance: detect-and-prompt-then-retry is the family pattern.

#### 7. SHA256 cache lifecycle (interaction with ADR-013)

**Release 2 (this feature):** SHA256 values written into `cache_models.sha256` opportunistically — whenever the background hash pool (ADR-013) completes a hash during the session, the result is written to the cache before the session ends.

On the next warm-start, `cache_models.sha256` is read into the in-process `Sha256Cache` (parent's ADR-002 cache); ADR-013's background hash pool **does not re-hash** these entries. The lazy-hash path (ADR-002) still applies for `sha256 IS NULL` rows.

**No automatic invalidation in Release 2.** The pre-mutate revalidator's `(mtime, size, inode, dev)` quad implicitly invalidates: any stat drift triggers re-introspect (Release 2) or background re-hash (Release 3, US-27).

**Release 3 (US-27, deferred):** explicit `cache_sha256` table with the full validity quad keyed at the file level (not the model level). Opt-in via `[cache] persist_sha256 = true` config. See ADR-018 for the seam between ADR-013 (in-process hash pool) and US-27 (cross-launch hash persistence).

#### 8. Schema versioning + migration tooling

**Versioning:** `PRAGMA user_version` stores the applied schema version. The binary embeds a compile-time `modeltap_store::EXPECTED_SCHEMA_VERSION` constant.

**Migration tooling:** `rusqlite_migration` crate (per ADR-017). Migrations live in `crates/modeltap-store/migrations/NNNN_<description>.sql`, embedded at compile time. The runner applies migrations in filename order until `user_version` matches `EXPECTED_SCHEMA_VERSION`.

**Migration discipline (C-INFO-6):**
- **Forward-only.** No down migrations.
- **Additive where possible.** New tables, new nullable columns. Destructive changes (rename, type change) are deferred or paired with corruption-recovery rebuild.
- **Idempotent.** Re-running a failed migration produces the same end state OR fails identically (no partial state). `rusqlite_migration` provides this guarantee.

**Files in v1:**
- `0001_initial.sql` — creates `cache_meta`, `cache_tools`, `cache_models`, `cache_model_files`; bumps `user_version` to 1.

Full DDL in `docs/feature/tool-model-info-sqlite-cache/design/data-models.md`.

#### 9. Folder-group-bulk-delete coordination

The in-flight `folder-group-bulk-delete` DELIVER (62h roadmap, approved) and this feature are **fully additive**. Specifically:

- **Trait extension:** ADR-010 added `Tool::delete_folder()` (method #7). This feature adds `Tool::inspect_tool()` and `Tool::inspect_model()` (methods #8 and #9). All three use the default-body pattern. No signature collision.
- **HF plugin modules:** `plugins/hf/src/folder_delete.rs` (folder-group-bulk-delete) and `plugins/hf/src/inspect.rs` (this feature) are sibling modules with no shared code paths.
- **Pre-mutate integration:** the folder-delete code path (`modeltap-app::orchestration::execute_folder_delete`) gains a `revalidate::pre_mutate(&targets)` call when this feature lands. Folder-group-bulk-delete DELIVER does NOT need to add this — it's part of this feature's diff. INT-INFO-7 records the integration AC.

ADR-010 stays in force unchanged.

## Alternatives Considered

### A — Keep ADR-003 (stateless rediscovery only)

**Pros:**
- No persistence bugs, no cache invalidation, no migration, no corruption recovery, no locking.
- Inventory is always fresh.

**Cons (now dominant):**
- Cumulative launch cost is the dogfooding-validated problem this feature solves.
- Inspection feature's value collapses without persisted metadata (every detail-screen open re-introspects).
- ADR-003's own §"Migration trigger" anticipated this supersession.

**Rejected** because the user explicitly directed this reversal in the intake brief.

### B — JSON / TOML file index (no SQLite)

Store inventory as `~/.modeltap/cache.json` or `cache.toml`.

**Pros:**
- No SQLite dependency. Human-readable for debugging.

**Cons:**
- No transactional writes — partial writes during power loss corrupt the file with no fine-grained recovery.
- No WAL — concurrent-process safety is hand-rolled.
- Query patterns ("models for tool X with last_seen_at older than 24h") are full-file reads in Rust; SQLite indexes them natively.
- Schema migrations are hand-rolled.
- Single-file format makes "rebuild from filesystem" the only recovery option; SQLite's per-table integrity check is more granular.

**Rejected** for transactional/recovery/concurrency reasons. SQLite buys all three for the cost of one ~1 MB embedded C library.

### C — Per-tool JSON files

`~/.modeltap/cache/ollama.json`, `~/.modeltap/cache/hf.json`, etc.

**Pros:**
- Per-tool isolation: one corrupted file doesn't poison the others.
- Simpler than SQLite.

**Cons:**
- Same transactional / concurrent-write problems as Alternative B, multiplied by tool count.
- Cross-tool queries (dedup grouping by SHA256) require reading every file.
- `cache.tool_ttl_seconds` per-tool override (Q-INFO-4 deferred) becomes "rename a file" — fragile.
- Filesystem-as-database is an antipattern at this scale.

**Rejected.**

### D — SQLite (CHOSEN)

The decision above.

**Pros:** transactional writes, WAL concurrency, native indexed queries, vetted migration framework, single-file portability. ~1 MB dep cost (rusqlite bundled).

**Cons:**
- New dependency (rusqlite + rusqlite_migration + dirs). Acceptable; all permissive licenses, well-maintained.
- Sync API; bridged to tokio via `spawn_blocking`. Pattern already established by ADR-013.

## Consequences

### Positive

- **Warm-start ≤100 ms p90 achievable** (K-INFO-1; the load-bearing user-visible improvement).
- **Inspection feature shipping with persisted metadata** (US-21, US-22 deliver fully).
- **SHA256 persistence is now a one-migration extension** (US-27 in Release 3).
- **Architecture-lint R9 invariant** (every mutation site preceded by `pre_mutate`) is a strong K5 guarantee — stronger than the parent's "rely on tests" posture.
- **Recoverability is explicit, tested, and bounded.** Cache failures are categorized; each has a recovery path; all paths converge on cold-start.

### Negative

- **New crate to maintain** (`modeltap-store`). ~600 LoC estimated; small audit surface.
- **Three new deps** (`rusqlite`, `rusqlite_migration`, `dirs`). All MIT/Apache-2.0; minimal transitive tree.
- **Cache layer adds ~50 ms to startup** (K-INFO-7 guardrail). Within budget per measurement plan.
- **Architecture-lint complexity grows.** R7-R9 add ~50 LoC of `syn` AST inspection in `tests/architecture.rs`.
- **Mutation safety now depends on a runtime check.** The R9 architecture-lint catches static violations; the runtime `pre_mutate` call is the dynamic enforcement. Both are required.

### Neutral

- **Cold-start is unchanged.** ADR-003's path is the fallback; users with `--no-cache` see identical behavior to v0.2.x.
- **Privacy posture unchanged.** Cache stays local; no telemetry uploaded; parent C5 carries forward.
- **ADR-002 (SHA256 dedup key) unchanged.** SHA256 is still the primary dedup identity; this feature just persists it across launches.

## Enforcement

| Mechanism | Verifies |
|---|---|
| `tests/architecture.rs::r7_only_app_depends_on_store` | R7 (only `modeltap-app` may depend on `modeltap-store`) |
| `tests/architecture.rs::r8_store_no_tokio_no_ratatui` | R8 (`modeltap-store` does not depend on `tokio` or `ratatui`) |
| `tests/architecture.rs::r9_pre_mutate_guard` | R9 (every destructive trait call in `modeltap-app/src/orchestration/` preceded by `revalidate::pre_mutate`) |
| `tests/acceptance/cache_recovery.rs::*` | corruption + downgrade + migration-failure recovery (§5) |
| `tests/acceptance/cache_disabled.rs::no_cache_writes` | `--no-cache` is a true bypass (§1) |
| `tests/acceptance/cache_safety.rs::pre_mutate_revalidation_invoked` | revalidator is called at runtime before every mutation (§3) |
| `crates/modeltap-store/tests/migration.rs` | migrations are idempotent, forward-only (§8) |
| `crates/modeltap-store/tests/concurrent.rs` | WAL + busy_timeout under two-process load (§6) |

## Migration trigger

This ADR will be superseded if:

- Users report SQLite as a meaningful operational burden (corruption rate >0.1% of launches over a 90-day window).
- A second persistent backend becomes desirable (e.g., a remote shared cache for team-of-Devons scenarios — out of scope for v1.x).
- The cache file size exceeds 100 MB for typical users (would indicate a schema design flaw, not a model-count problem).

None of these are likely in the foreseeable future. v1 of this feature can run for years on this ADR.

## Cross-references

- ADR-003 (Superseded — State Model: Stateless Rediscovery)
- ADR-001 (Plugin Dispatch via `Box<dyn Tool>`) — unchanged
- ADR-005 (Async Runtime: Tokio) — unchanged; sync-bridge via `spawn_blocking` per §"Sync-store-from-async-app" in `architecture-design.md`
- ADR-006 (TUI Architecture) — extended with new `Msg`/`Cmd` variants for refresh and detail screens
- ADR-010 (Folder-Group Delete via Default-Method Trait Extension) — unchanged; this ADR's trait extension uses the same pattern
- ADR-013 (Background SHA256 Hash Pool) — interaction documented in §7 and ADR-018
- ADR-016 (Tool Trait Extension: `inspect_*()`) — sister ADR
- ADR-017 (Schema Migration Strategy: `rusqlite_migration`) — sister ADR
- ADR-018 (SHA256 Persistence Boundary) — sister ADR (records the seam for US-27 Release 3)
- `docs/feature/tool-model-info-sqlite-cache/discuss/requirements.md` — DISCUSS input
- `docs/feature/tool-model-info-sqlite-cache/design/architecture-design.md` — full design
