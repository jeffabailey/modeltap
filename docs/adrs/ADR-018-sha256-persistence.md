# ADR-018: SHA256 Persistence Boundary and Relationship with ADR-013

## Status

Accepted (2026-05-17). Sister to ADR-015 (cache layer) and ADR-013 (background SHA256 hash pool).

This ADR records **the seam** between Release 2 (this feature) and Release 3 (US-27, deferred). US-27's full schema and opt-in flag are recorded here so that DELIVER for Release 2 builds toward the right shape — even though Release 2 does NOT implement persistent SHA256 caching.

## Context

ADR-013 specifies a fixed pool of `min(num_cpus, 4)` `tokio::spawn_blocking` workers that compute SHA256 hashes **in-process** after first paint completes. The pool's results land in an in-process `Sha256Cache` (parent's ADR-002) and are dropped on exit.

ADR-015 introduces persistent storage via SQLite. The natural question: where does the SHA256 cache live across launches, and how does it interact with the background hash pool?

DISCUSS deferred this to Release 3 (US-27, opt-in via `[cache] persist_sha256 = true`). Two architectural concerns must be settled now (Release 2) so Release 3 can be additive:

1. **Where does the SHA256 value live in the schema?** A column on `cache_models`? A separate `cache_sha256` table? Both?
2. **What is the relationship between the in-process `Sha256Cache` (ADR-013), the cache's `cache_models.sha256` column (this feature), and the future `cache_sha256` table (US-27)?**

## Decision

### Release 2 (this feature):

**SHA256 values are stored in `cache_models.sha256` (TEXT, nullable, indexed partial).** The column is populated opportunistically: whenever ADR-013's background hash pool completes a hash during the session, the result is written to the row in `cache_models` along with the next reconcile / mutation write.

**Read path at warm-start:** the warm-paint reader loads `cache_models.sha256` into the in-process `Sha256Cache` so ADR-013's pool does NOT re-hash entries that were hashed in a prior session.

**Invalidation:** the pre-mutate revalidator's `(mtime, size, inode, dev)` quad check implicitly invalidates — any stat drift on a model's files triggers re-introspection (Release 2) or background re-hash (Release 3). In Release 2, drift detected at pre-mutate time results in `cache_models.sha256 = NULL` being written back, forcing the next hash request to recompute.

### Release 3 (US-27, deferred — opt-in):

**A new `cache_sha256` table** keyed at the file level (path-as-PK), storing `(path, mtime_epoch_ns, size_bytes, inode, dev, content_hash, computed_at)`. Migrating to this richer schema:

- Adds the `cache_sha256` table via `migrations/0002_add_sha256_persistence.sql` (per ADR-017).
- Keeps `cache_models.sha256` (the model-level convenience copy) — it remains the de-normalized fast-path for warm-paint display.
- Adds a `[cache] persist_sha256 = true` opt-in config flag (default `false` to start; flip default to `true` in a later release after dogfooding).
- Adds a `modeltap cache verify` developer command (under a new `modeltap cache <subcommand>` family in `modeltap-cli`).

**Why the file-level table in addition to the model-level column:**

- Some models have multiple files (HF model dir with `model-00001-of-00003.safetensors`, `model-00002-of-00003.safetensors`, ...). Per-file SHA256 + per-file validity quad is the only correct grain.
- Hardlink dedup means the same `(inode, dev)` may map to multiple paths (after `unify`); the file-level table avoids quadratic duplication.
- The model-level `sha256` column is then a denormalized "primary file" or "dedup-key" hash, computed by the plugin from the file-level rows.

### The 3-tier SHA256 cache hierarchy after Release 3

| Tier | Lifetime | Source | Storage |
|---|---|---|---|
| 1. In-process `Sha256Cache` (ADR-002) | Session | Background hash pool (ADR-013) | RAM |
| 2. Model-level `cache_models.sha256` | Across launches | Tier 1 writeback at action / reconcile time | SQLite TEXT column, indexed |
| 3. File-level `cache_sha256` (US-27, deferred) | Across launches | Tier 1 writeback for opt-in users | SQLite table with `(mtime, size, inode, dev)` validity quad |

**Read priority at SHA256 lookup time:** Tier 1 → Tier 3 (if enabled) → Tier 2 → compute. Hits at any tier short-circuit; misses fall through to compute via the pool.

## Alternatives Considered

### A — Defer all SHA256 persistence to Release 3 (model-level column also deferred)

**Pros:**
- Release 2 is simpler. Only `cache_models` and `cache_model_files` schema; no SHA256 column.

**Cons:**
- **Wastes session work.** ADR-013's hash pool runs every session; its results are dropped on exit. Even without US-27's full opt-in scheme, persisting the hash in the existing `cache_models` row is a one-column-write cost for a meaningful cross-launch benefit.
- **Release 3 schema migration becomes a 2-step change** (add column, then add table) rather than just "add table."

**Rejected** for the simple reason that the column is essentially free in Release 2 (one TEXT column, one partial index) and provides immediate value for any user who completes a unify in one session and re-opens modeltap later.

### B — Only the file-level table; no `cache_models.sha256` column

**Pros:**
- Cleanest schema (one place SHA256 lives).
- File-level is the "true" grain.

**Cons:**
- **Warm-paint display query becomes a JOIN.** Showing the dedup-key glyph on the model row requires joining `cache_models` to `cache_sha256` via path. JOIN avoidance matters at the 100 ms warm-paint budget.
- **Release 2 must ship without ANY SHA256 persistence**, blocking opportunistic writeback for un-opt-in users.

**Rejected** for the warm-paint latency reason. The model-level column is a denormalized read-path optimization.

### C — Model-level column only; no file-level table ever

**Pros:**
- Simpler in perpetuity.

**Cons:**
- **Cannot express multi-file models correctly.** HF models with sharded safetensors files each have their own `(mtime, size, inode, dev)` quad; a single model-level SHA256 cannot represent that.
- **`modeltap cache verify` (US-27) needs per-file detail** to detect drift granularly.

**Rejected for Release 3.** Acceptable for Release 2 if we accept that multi-file model SHA256 = "plugin-defined aggregation" rather than "exact per-file content hash." Release 3 fixes this with the file-level table.

### D — This ADR's decision (CHOSEN): both tiers, sequenced

Model-level `cache_models.sha256` in Release 2; file-level `cache_sha256` table additively in Release 3.

## Consequences

### Positive

- **Release 2 ships a meaningful cross-launch SHA256 benefit** without the full US-27 scope.
- **Release 3 is purely additive** — a new migration adds the file-level table; existing model-level column continues to serve warm-paint display.
- **The 3-tier hierarchy is explicit** so DELIVER for Release 2 knows what shape to target.
- **ADR-013 in-process hash pool keeps working identically.** The pool's `Msg::HashComputed` handler is extended to write to `cache_models.sha256` (Release 2) and, when opted-in, to `cache_sha256` (Release 3).

### Negative

- **`cache_models.sha256` is technically denormalized** once Release 3 ships. Acceptable; the column is a read-path cache that the file-level table is the source of truth for.
- **Migration discipline:** future schema changes must preserve the denormalization. Release 3's migration must populate `cache_models.sha256` from `cache_sha256` for existing rows, not just add the new table.

### Neutral

- **ADR-013 unchanged.** The pool's interface, worker count, and cancellation semantics are identical. Only its writeback target changes (one extra repo call on `Msg::HashComputed`).
- **ADR-002 unchanged.** SHA256 is still the primary dedup identity; cache representation is implementation detail.

## Architecture enforcement

Beyond ADR-015's R7-R9, this ADR adds one informational rule for Release 3 (not enforced in Release 2):

- **R10 (Release 3, post-US-27):** the in-process `Sha256Cache` may be populated from any tier; new SHA256 computations MUST be writeback-cached to whichever persistent tier(s) are enabled. The writeback is **best-effort**: a write failure to the cache MUST NOT block the user-facing action. (The cache is an optimization, never load-bearing for correctness.)

R10 is documented here for forward consistency; it lands as an architecture-lint extension when US-27 ships.

## Implementation guidance (for DELIVER — Release 2 only)

For Release 2 of this feature, software-crafter implements:

- `cache_models.sha256` column (already in `0001_initial.sql` per `data-models.md`).
- `cache_models.dedup_group_id` column (already in `0001_initial.sql`).
- Warm-start read: `cache_models.sha256` populates the in-process `Sha256Cache` at startup.
- ADR-013 hash pool extension: on `Msg::HashComputed { model_id, hash }`, also call `cache.write_models(...)` with the updated sha256 (spawn_blocking).
- Pre-mutate revalidator: on `Drift` for a model with non-NULL `cache_models.sha256`, set the value to NULL and write back; the next hash request recomputes.

Release 3 (US-27) is **out of scope** for this DELIVER. The `cache_sha256` table, the opt-in flag, the `modeltap cache verify` subcommand, and R10 enforcement all land in a future DELIVER.

## Architecture enforcement tooling recommendation

Per ADR-015 §"Enforcement", this project uses hand-rolled `syn` AST inspection in `tests/architecture.rs`. For the SHA256 writeback path specifically (Release 2):

- **Recommended:** unit test in `crates/modeltap-store/tests/sha256_writeback.rs` that asserts a `Msg::HashComputed` event results in a `cache_models.sha256` update within the same transaction as the next per-tool reconcile write.
- **Alternative:** the existing R9 invariant covers the mutation safety; SHA256 writeback is a non-safety best-effort path and does not require a hard architecture-lint rule.

## Cross-references

- ADR-002 (SHA256 as dedup identity) — unchanged.
- ADR-013 (Background SHA256 Hash Pool) — unchanged; writeback target extends to the cache.
- ADR-015 (State Model: SQLite-Backed Cache) — sister ADR; this ADR records the seam for one of its sub-decisions.
- ADR-016 (Tool Trait Extension) — unrelated; included only for ADR-numbering continuity.
- ADR-017 (Schema Migration Strategy) — the future US-27 migration uses the `0002_add_sha256_persistence.sql` filename established here.
- US-27 in `docs/feature/tool-model-info-sqlite-cache/discuss/user-stories.md` — the Release 3 driver.
- `docs/feature/tool-model-info-sqlite-cache/design/data-models.md` — full v1 DDL.
