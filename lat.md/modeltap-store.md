# modeltap-store

The [[crates/modeltap-store/src/lib.rs|modeltap-store crate]] persists the inventory that modeltap-app reads on warm-start. It owns the SQLite cache file, the v0→v1 migration, and the minimum read/write API for the walking skeleton.

The crate depends on `modeltap-core` plus `rusqlite`, `rusqlite_migration`, `thiserror`, `serde`, `serde_json`, `time` — and nothing else. No async runtime, no ratatui, no `dirs` (path resolution lives in modeltap-app::adapters::cache_path).

## Rationale

ADR-015 reverses the v0.2.x stateless-rediscovery rule.

The new constraint is two-part: paint from cache fast on warm-start (≤ 100 ms p90, per K-INFO-1), but never let the cache be authoritative when mutating state. The crate enforces the first half; the second half (pre-mutate revalidation) is wired into modeltap-app's orchestration layer in phase 05.

`rusqlite` (bundled) was chosen over `sqlx` because the cache is single-process per scenario and the bundled feature avoids macOS/Ubuntu CI version skew. `rusqlite_migration` was chosen for forward-only embedded SQL migrations.

## Cache open and PRAGMA invariants

[[crates/modeltap-store/src/open.rs]] opens or creates the cache file at the caller-resolved path. Four PRAGMAs are set before any other query runs.

`journal_mode=WAL` allows two `modeltap` processes (Devon's running TUI + an ad-hoc CLI invocation) to read concurrently without blocking each other. Required by AC-23-2 and the US-26 concurrent-process scenarios.

`busy_timeout=5000` is the only concurrency mechanism the crate uses. No file locks, no advisory locks, no PID detection. Writers serialize via SQLite's own busy-wait.

`foreign_keys=ON` enforces the composite FK from `cache_model_files.(model_id, tool_id)` to `cache_models` and the column FK from `cache_models.tool_id` to `cache_tools`. SQLite defaults to OFF per connection — we set it per-open. Test fixtures that call `write_model_files` MUST seed the parent `cache_tools` + `cache_models` rows first or SQLite rejects with extended_code 787 / "FOREIGN KEY constraint failed" (see [[crates/modeltap-app/tests/orchestration_revalidate.rs|`seed_parent_rows`]] for the canonical helper).

`user_version` is read after open and routes the connection to the migrator if low. Three results: `OpenedFresh` (no file), `OpenedExisting` (at expected version), or `OpenedAfterMigration` (rolled forward). The composition root distinguishes these because the warm-start UX differs.

`Cache::open_in_memory()` returns an in-memory SQLite for unit tests. The migration runs identically in memory and on disk, which keeps the test suite from needing real tempdirs for every scenario.

## Schema versions (v0 → v1 → v2)

The v1 migration in `crates/modeltap-store/migrations/0001_initial.sql` creates `cache_meta`, `cache_tools`, `cache_models`, and `cache_model_files` per the design's data-models DDL.

`rusqlite_migration` advances `PRAGMA user_version` by one per `M::up` applied, so the version bump lands atomically with each migration's schema creation. The `migrations()` chain in [[crates/modeltap-store/src/migrate.rs]] lists `0001` then `0002` — order is load-bearing.

`EXPECTED_SCHEMA_VERSION: u32 = 2` is the public constant the migrator compares against the live PRAGMA. A pre-existing v1 cache opens as `OpenedAfterMigration { from: 1, to: 2 }`; the migration is forward-only and purely additive, so existing `cache_models`/`cache_tools` rows survive untouched (pinned by [[crates/modeltap-store/tests/migration_0002.rs]]).

The v2 migration `crates/modeltap-store/migrations/0002_add_sha256_persistence.sql` adds the file-level `cache_sha256` table for US-27 SHA256 persistence (ADR-018, Release 3). It is the Tier-3 file-level source of truth keyed by absolute `path`, carrying the `(mtime_epoch_ns, size_bytes, inode, dev)` validity quad plus `content_hash` (lowercase hex) and `computed_at` (ISO-8601 UTC). An index on `(inode, dev)` lets a hardlinked path short-circuit by physical identity. The migration does NOT alter any v1 table.

`sha256` is also stored as TEXT (lowercase hex) on `cache_models` (the Tier-2 denormalized warm-paint fast path), not BLOB. The choice trades 2× storage for human-readable rows under the `sqlite3` CLI, and the partial index `WHERE sha256 IS NOT NULL` keeps lookups fast even though the column is sparse. ADR-018's 3-tier hierarchy: in-process `Sha256Cache` (RAM, session) → `cache_sha256` (file-level, opt-in via `[cache] persist_sha256`) → `cache_models.sha256` (model-level denormalized) → compute.

## ToolsRepo and ModelsRepo CRUD

[[crates/modeltap-store/src/repo/tools.rs]] and [[crates/modeltap-store/src/repo/models.rs]] expose the minimum repository surface for the warm-paint read path: `write_tool`, `tools()`, `write_models`, `models_for_tool`.

Both repos accept a borrowed `&rusqlite::Connection` per call rather than owning one. This keeps the repo types `Send + Sync` and lets the composition root in `modeltap-app` choose between a single shared connection (current) and a connection pool (a future option if multi-thread reads ever materialize).

The full repository surface — including `delete_tool`, `replace_models`, `model_files_for`, `cache_meta_get/set`, and the corruption-recovery escape hatch — lands in Phase 04 when the cache state model becomes user-visible. The Phase 01 slice is deliberately the minimum that lets the walking skeleton commit a single end-to-end vertical without dragging the rest of the API along.

[[crates/modeltap-store/src/repo/sha256.rs]] is the Tier-3 `cache_sha256` repo (US-27): `upsert_sha256` (insert-or-replace on the `path` PK), `get_sha256_by_path` (None when absent), `invalidate_sha256` (idempotent delete, used by drift invalidation), and `all_sha256` (drives `modeltap cache verify`). It reuses the `(mtime,size,inode,dev)` quad via [[crates/modeltap-store/src/types.rs|FileStat]] and shares the `mtime_to_epoch_ns` / `epoch_ns_to_system_time` / iso8601 converters with the revalidator and tools repos (promoted to `pub(crate)`).

## Recovery routine

[[crates/modeltap-store/src/recovery.rs]] holds the `RecoveryReason` enum and the rename-then-reopen routine that `Cache::open` calls when one of three recoverable failure modes fires. AC-23-11 mandates that a corrupt cache must never block launch — modeltap always proceeds to cold-start.

The three failure modes detected at open time are `RecoveryReason::Corrupted` (rusqlite returns SQLITE_CORRUPT when probing the header or running the migrator's read-side check), `RecoveryReason::Downgrade { found, expected }` (the live `PRAGMA user_version` is greater than `EXPECTED_SCHEMA_VERSION`, meaning a newer modeltap wrote this file), and `RecoveryReason::MigrationFailed { from, to }` (rusqlite_migration's `to_latest` returned an error mid-roll-forward).

The routine itself is the same in all three cases. The bad file is renamed via `std::fs::rename` to a sibling path — `cache.sqlite.corrupt-<YYYY-MM-DDTHHMMSS>` for the corrupt and migration-failed paths, `cache.sqlite.future-version-<n>` for the downgrade path. The rename is best-effort: if the target directory is read-only or the rename fails, the error is absorbed and recovery still proceeds. A line tagged `cache_recovery reason=<reason> renamed_to=<new_path>` is appended to `${MODELTAP_DIAGNOSTICS_DIR:-~/.modeltap}/diagnostics.log` using the same writer pattern as [[plugin-inspect-overrides]] uses for `inspect_panic`. A fresh empty SQLite is then opened at the original path and `Cache::open` returns `CacheOpenResult::OpenedAfterRecovery { reason, renamed_to, cache }` so the composition root can route the recovery reason into the TUI banner state.

The TUI surface for the recovery event is [[crates/modeltap-tui/src/render/recovery_banner.rs]]. The banner reads `AppState.recovery_reason: Option<RecoveryReason>` and renders a single-line dismissable strip across the top of the main view when populated. Esc clears the field. Per AC-23-7 the banner never blocks the content below it — the inventory list, footer, and status bar render at their normal positions and respond to input as usual; the banner only consumes one row of vertical space.

The acceptance contract is that a launch against a corrupt cache file, a future-version cache file, or a cache file whose migration fails on the way to the current schema version, all three reach the same end-state: the broken file sits renamed next to its replacement, the diagnostics log carries one new line, and the user lands at the normal inventory view with a one-line banner explaining what happened. Cache failure is never user-blocking.

## Concurrent process safety

Two `modeltap` processes share one `cache.sqlite` without blocking, crashing, or surfacing `SQLITE_BUSY` to the user. The contract is AC-23-10; the mechanism is two PRAGMAs and a tiny test seam.

`journal_mode=WAL` (set at [[crates/modeltap-store/src/open.rs|Cache::open]]) lets concurrent readers proceed without contending for the write lock. Two `Cache::open(path)` handles on different threads can call `tools()` simultaneously and both observe the same row set — the unit test `concurrent_reads_succeed_under_wal` in [[crates/modeltap-store/tests/concurrent.rs]] pins this against a real tempfile, and the acceptance scenario "Two modeltap processes can read the cache concurrently via SQLite WAL" extends the same invariant to two separate binaries.

`busy_timeout=5000` (also set at open) is the only concurrency mechanism the writer side uses. When two transactions race for the write lock on `BEGIN IMMEDIATE`, the loser busy-waits up to 5 seconds for the winner to commit, then proceeds. No advisory locks, no PID detection, no application-level retry loop.

`Cache::reconcile_tool` is the per-tool write API the warm-start orchestrator drives ([[crates/modeltap-app/src/main.rs|modeltap-app::reconcile_writeback]]). It opens a `BEGIN IMMEDIATE` transaction, UPSERTs the `cache_tools` row, UPSERTs every `cache_models` row, and commits — all atomic per tool. The function returns the wall-clock `Duration` spent waiting at `BEGIN IMMEDIATE` so the caller can emit a `cache.write_wait_ms` JSONL event. Zero on an uncontested write; up to `busy_timeout` when a peer process held the lock.

`MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS=N` is the cfg-gated test seam that drives the concurrent-writers acceptance scenario. When set AND the build was compiled with `cfg(any(test, feature = "test-harness"))`, `reconcile_tool` sleeps N ms BEFORE COMMIT. Release builds (`cargo build --release` without the `test-harness` feature) NEVER read this env var — the seam is compiled out entirely. This is the R3 / OQ-3 invariant: production binaries cannot be tricked into slow-writing by a hostile env var.

The `cache.write_wait_ms` JSONL line in `<MODELTAP_LOG_DIR>/launch.log` carries the schema `modeltap.launch.v1` and one field, `wait_ms: u64`. The concurrent-writers acceptance scenario asserts process B's emitted value falls in `[0, 5000]` — exceeding the upper bound would mean SQLite returned `SQLITE_BUSY` to the writer, which the contract forbids.

The acceptance contract is that concurrent processes never crash, never surface `SQLITE_BUSY`, and the last writer wins via `ON CONFLICT(tool_id) DO UPDATE`. The two `#[test]`s in [[crates/modeltap-store/tests/concurrent.rs]] cover the store-internals path end-to-end against real tempfiles; the standalone `cache_concurrent` binary-boundary acceptance test was removed in the lean-UI-suite consolidation (the WAL + busy_timeout PRAGMAs it exercised are set unconditionally at `Cache::open`, covered by the store tests).

## Pre-mutate revalidator

[[crates/modeltap-store/src/revalidate.rs]] is the store-side half of K5. ADR-015 §3 forbids the cache from enabling a stale-data destructive action; `Cache::verify_against_fs(model_id)` is the seam every mutation orchestrator runs before invoking a plugin's destructive method.

`verify_against_fs` reads every `cache_model_files` row for the given `model_id`, re-`stat()`s the path, and compares each result to the cached `(mtime_epoch_ns, size_bytes, inode, dev)` quad per architecture-design.md §8.2. The scan short-circuits at the first disagreement.

Outcome is [[crates/modeltap-store/src/types.rs|ValidationResult]]: `Match` when every file's quad still matches the cached row, `Drift { fresh: FileStat }` when at least one quad differs, or `Gone` when at least one `std::fs::metadata` call returns `ErrorKind::NotFound`. A model with zero `cache_model_files` rows returns `Match` — there is no cached state to be stale against, and the orchestrator decides separately whether such a model is safe to mutate.

[[crates/modeltap-store/src/types.rs|FileStat::matches]] is the pure helper called out by acceptance-test-plan.md §9 CM-D — no I/O, just the four-field equality check. It exists as a named method so revalidator call sites read `if cached.matches(&fresh)` rather than relying on the `PartialEq` derive implicitly.

The companion `Cache::write_model_files` API is the minimum write surface needed by the revalidator fixtures and unit tests. It UPSERTs `cache_model_files` rows inside a single transaction via `ON CONFLICT(path) DO UPDATE`. The richer per-tool upsert + cascading-delete surface lands when an in-tree plugin starts populating these rows from `Tool::inspect_model`.

Orchestrator-side `revalidate::pre_mutate` lives at [[crates/modeltap-app/src/orchestration/revalidate.rs]] and wraps `cache.verify_against_fs` so async mutation sites don't block the runtime. The R8 `spawn_blocking` hop is reserved for when per-call cost grows non-trivial (>10 ms); today the inline call reads more naturally and mirrors how `Cache::tools()` is invoked from `warm_start::run`.

It emits one `revalidate.invoked` JSONL line per call to `<MODELTAP_LOG_DIR>/launch.log` with schema `modeltap.launch.v1` and fields `tool|model|outcome|duration_ms`. Outcome strings are stable: `proceed`, `drift`, `gone`, `store_error`.

The four destructive entry points — [[crates/modeltap-app/src/actions/unify.rs]], [[crates/modeltap-app/src/actions/zap.rs]], [[crates/modeltap-app/src/actions/delete_one.rs]], [[crates/modeltap-app/src/actions/folder_delete.rs]] — each run `pre_mutate` before invoking the plugin's destructive method.

`PreMutateOutcome::{Drift, Gone}` returns the K5 cache-stale error to the caller without ever entering the plugin; `StoreError` fails closed. Only `Proceed` reaches the plugin. The zap path is special: it `discover()`s the per-tool inventory first to enumerate model ids, then revalidates each before invoking `delete_all`. The folder-delete path revalidates every `ModelMeta` in the targeted `<author>/<repo>` group.

Together with part 1's store-side `verify_against_fs`, this closes K5: a stale cache cannot enable a destructive action.

Fixtures `devon-cache-mtime-drift` (file touched after the cache row was written) and `devon-cache-file-gone` (file removed after the cache row was written) live at [[tests/src/fixtures/cache_fixtures.rs]] and back the unit tests in [[crates/modeltap-store/tests/revalidate.rs]].

## Drift re-introspect and gone auto-refresh

Step 05-04 layers UX behaviour on top of K5: `Drift` triggers a re-introspect+writeback (AC-26-6); `Gone` triggers a per-tool refresh (AC-26-7). Both emit JSONL events to `launch.log` correlated with the originating `revalidate.invoked` line.

[[crates/modeltap-app/src/orchestration/revalidate.rs|re_introspect_after_drift]] is the orchestrator helper the action layer calls when `pre_mutate` returns `Drift { fresh, cached }`. It invokes the plugin's `Tool::inspect_model` for the drifted model, emits `inspect.invoked source=pre_mutate_drift` to `launch.log`, then writes the fresh `(size_bytes, metadata_kv, metadata_introspected_at)` back to the `cache_models` row plus the fresh quad to the `cache_model_files` row. The next `verify_against_fs` on the same model returns `Match` — the cache has caught up. When the plugin returns `InspectError::Unsupported` (the trait default) the size + introspected-at writeback still lands; metadata fields are layered in only when the plugin supplies them. Return value is `ReintrospectOutcome::Reintrospected { fresh }` on success, `PluginError` reserved for future selectors, or `StoreError` when the writeback itself fails.

[[crates/modeltap-app/src/orchestration/revalidate.rs|auto_refresh_after_gone]] is the lightweight companion for the Gone path. It emits `refresh.tool source=pre_mutate_gone` to `launch.log` so observability sees the auto-refresh trigger. The actual per-tool reconcile is enqueued by the composition root via `ReconcileScope::Tool(...)` — the orchestrator helper is the audit-trail emission, not the work itself. Crucially, the gone path NEVER calls a destructive plugin method; the fixture filesystem remains byte-identical pre/post (the [[tests/src/fixtures/dir_manifest.rs|DirManifest]] invariant the acceptance scenarios assert).

[[crates/modeltap-app/src/observability.rs|RecordKind::InspectInvoked]] and [[crates/modeltap-app/src/observability.rs|RecordKind::RefreshTool]] are the two new variants. Both carry the privacy-preserving fields the K-INFO schema mandates: registered plugin id, the `model_id_in_tool` (logical, not a path), and a stable `source` discriminator. No paths, no blob hex digests.

The drift/gone helpers are covered in-process (Strategy A — no subprocess) against the `devon-cache-mtime-drift` and `devon-cache-file-gone` fixtures by `modeltap-app` orchestration tests. The drift scenario asserts `revalidate.invoked outcome=drift` + `inspect.invoked source=pre_mutate_drift` events appear, the `cache_models` row's `size_bytes` matches the post-drift on-disk size, `metadata_introspected_at` is set, the cache_model_files quad is refreshed so `verify_against_fs` now returns `Match`, and the test-tool root DirManifest is unchanged. The gone scenario asserts `revalidate.invoked outcome=gone` + `refresh.tool source=pre_mutate_gone` events appear, the `pre_mutate` return is `Gone`, and the DirManifest invariant holds. (The `cache_revalidate` acceptance binary + its `steps/revalidate.rs` were removed in the lean-UI-suite consolidation; the drift/gone orchestrator helpers remain covered by `modeltap-app` orchestration tests.)

The TUI-visible surface — the dialog's "Re-introspecting before proceeding..." progress line, the reclaim-delta re-confirm annotation when the recomputed reclaim differs by more than 1 byte, and the right-pane "file no longer exists; refreshing inventory" line — is gated on a `launch.log` timing seam (the now-removed `manual_refresh` acceptance binary `#[ignore]`d these pending that seam). The behavioural core (orchestrator emits the right JSONL, updates the cache, refuses destructive action) is fully exercised today by `modeltap-app` orchestration tests; the SNAPSHOT-seam assertions land in a follow-up TUI step once the timing seam is exposed.

## Architecture lints R7 + R8 + R9

Three static lints in [[crates/modeltap-app/tests/architecture.rs]] guard the layering invariants this crate depends on. R9 is the load-bearing K5-extension lint; R7 and R8 cap modeltap-store as a leaf crate.

They run inside the standard `cargo test -p modeltap-app --test architecture` invocation — no separate harness, no slow build, no `#[ignore]`. A contributor adding a 5th destructive call site without `pre_mutate`, or pulling `tokio` into `modeltap-store`, fails CI immediately.

R7 asserts that ONLY `modeltap-app` may path-depend on `modeltap-store`. The TUI must not know SQLite exists; the core must remain pure; plugins must not depend on a sibling layer crate. The `modeltap-acceptance` test crate is allow-listed because it is `publish = false` and is not part of any shipped binary — it depends on `modeltap-store` for cache-introspection helpers under `tests/src/fixtures/cache_fixtures.rs`. A future contributor pulling `modeltap-store` into, say, `modeltap-tui` for a debug helper fails the lint with a clear offender message.

R8 asserts `modeltap-store` itself does NOT depend on `tokio`, `ratatui`, or `crossterm` — in either `[dependencies]` OR `[dev-dependencies]`. The cache layer is sync rusqlite; adding tokio would create two concurrency models in the same crate, and adding ratatui / crossterm would couple a storage layer to a rendering layer. The async bridge happens at the `modeltap-app` boundary via `tokio::task::spawn_blocking`; the TUI bridge happens via projection types in `modeltap-app::orchestration`. The lint walks `cargo metadata --no-deps` for the `modeltap-store` package's `dependencies[]` array (which flattens both regular and dev deps with a `kind` discriminator) and asserts none of the three forbidden names appear.

R9 is the K5-extension safety lint. Every method-call expression under `crates/modeltap-app/src/orchestration/` AND `crates/modeltap-app/src/actions/` that targets one of the four destructive `Tool` trait methods (`link`, `delete_one`, `delete_all`, `delete_folder`) MUST be preceded — in the same fn body — by an invocation of `revalidate::pre_mutate(...)`. Step 05-02 wired the four current sites (unify, zap, delete_one, folder_delete); R9 is the static guarantee that a future contributor adding a 5th call site without a guard cannot ship. The lint uses a hand-rolled `syn::visit::Visit` walker (`r9_walk_file` / `r9_walk_file_with_source` in [[crates/modeltap-app/tests/architecture.rs]]) that tracks `pre_mutate_seen` per-fn and emits an `R9Violation` for each unguarded destructive method-call. Two companion negative tests — `r9_walker_reports_unguarded_destructive_call` and `r9_walker_accepts_guarded_destructive_call` — feed the walker synthetic `syn::File` fixtures and assert the walker correctly flags the unguarded case AND correctly accepts the guarded case, so the lint is provably sound against both false-negatives and false-positives.

`R9_DESTRUCTIVE_METHODS` is the const list of the four trait method names. If a future roadmap adds a 5th destructive method (e.g. `replace_model`), append it to the const AND wire it through `pre_mutate` at every new call site — ADR-015 §"Enforcement" records this discipline.

## Release-build absence checks (OQ-3)

Two `#[ignore]`-marked acceptance tests in [[tests/acceptance/release_build_check.rs]] verify the OQ-3 invariant that test-only env-var seams compile out of release builds.

The checks run `cargo build --release --no-default-features -p modeltap-app --bin modeltap` and then `strings target/release/modeltap | grep -F <env-var>` for both `MODELTAP_TEST_PLUGINS` (the registry seam in [[crates/modeltap-app/src/registry.rs]]) and `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS` (the busy-wait seam in [[crates/modeltap-store/src/repo/tools.rs]]). Both must yield zero matches — a hit means a `#[cfg(any(test, feature = "test-harness"))]` gate is broken or a feature is leaking through `default = [...]`.

The tests are `#[ignore]`-marked because `cargo build --release` is slow on a clean tree (60-180 s). CI's release-prep job invokes them explicitly via `cargo test --test release_build_check -- --ignored`. Locally, run the same command before any `git push` to main; the per-step crafter inner-loop does not pay the cost.
