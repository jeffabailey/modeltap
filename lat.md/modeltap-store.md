# modeltap-store

The [[crates/modeltap-store/src/lib.rs|modeltap-store crate]] persists the inventory that modeltap-app reads on warm-start. It owns the SQLite cache file, the v0→v1 migration, and the minimum read/write API for the walking skeleton.

The crate depends on `modeltap-core` plus `rusqlite`, `rusqlite_migration`, `thiserror`, `serde`, `serde_json`, `time` — and nothing else. No async runtime, no ratatui, no `dirs` (path resolution lives in modeltap-app::adapters::cache_path).

## Rationale

ADR-015 reverses the v0.2.x stateless-rediscovery rule.

The new constraint is two-part: paint from cache fast on warm-start (≤ 100 ms p90, per K-INFO-1), but never let the cache be authoritative when mutating state. The crate enforces the first half; the second half (pre-mutate revalidation) is wired into modeltap-app's orchestration layer in phase 05.

`rusqlite` (bundled) was chosen over `sqlx` because the cache is single-process per scenario and the bundled feature avoids macOS/Ubuntu CI version skew. `rusqlite_migration` was chosen for forward-only embedded SQL migrations.

## Cache open and PRAGMA invariants

[[crates/modeltap-store/src/open.rs]] opens or creates the cache file at the caller-resolved path. Three PRAGMAs are set before any other query runs.

`journal_mode=WAL` allows two `modeltap` processes (Devon's running TUI + an ad-hoc CLI invocation) to read concurrently without blocking each other. Required by AC-23-2 and the US-26 concurrent-process scenarios.

`busy_timeout=5000` is the only concurrency mechanism the crate uses. No file locks, no advisory locks, no PID detection. Writers serialize via SQLite's own busy-wait.

`user_version` is read after open and routes the connection to the migrator if low. Three results: `OpenedFresh` (no file), `OpenedExisting` (at expected version), or `OpenedAfterMigration` (rolled forward). The composition root distinguishes these because the warm-start UX differs.

`Cache::open_in_memory()` returns an in-memory SQLite for unit tests. The migration runs identically in memory and on disk, which keeps the test suite from needing real tempdirs for every scenario.

## Schema v0 to v1

The single migration in `crates/modeltap-store/migrations/0001_initial.sql` creates `cache_meta`, `cache_tools`, `cache_models`, and `cache_model_files` per the design's data-models DDL.

The last statement of the migration is `PRAGMA user_version = 1;` so the version bump lands atomically with the schema creation.

`EXPECTED_SCHEMA_VERSION: u32 = 1` is the public constant the migrator compares against the live PRAGMA. Phase 04 introduces a v1→v2 migration that adds TTL columns to `cache_models` and a `last_full_reconcile` row to `cache_meta`; the constant bumps to 2 then.

`sha256` is stored as TEXT (lowercase hex) on `cache_models`, not BLOB. The choice trades 2× storage for human-readable rows under the `sqlite3` CLI, and the partial index `WHERE sha256 IS NOT NULL` keeps lookups fast even though the column is sparse in v1.

## ToolsRepo and ModelsRepo CRUD

[[crates/modeltap-store/src/repo/tools.rs]] and [[crates/modeltap-store/src/repo/models.rs]] expose the minimum repository surface for the warm-paint read path: `write_tool`, `tools()`, `write_models`, `models_for_tool`.

Both repos accept a borrowed `&rusqlite::Connection` per call rather than owning one. This keeps the repo types `Send + Sync` and lets the composition root in `modeltap-app` choose between a single shared connection (current) and a connection pool (a future option if multi-thread reads ever materialize).

The full repository surface — including `delete_tool`, `replace_models`, `model_files_for`, `cache_meta_get/set`, and the corruption-recovery escape hatch — lands in Phase 04 when the cache state model becomes user-visible. The Phase 01 slice is deliberately the minimum that lets the walking skeleton commit a single end-to-end vertical without dragging the rest of the API along.

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

The acceptance contract is that concurrent processes never crash, never surface `SQLITE_BUSY`, and the last writer wins via `ON CONFLICT(tool_id) DO UPDATE`. The two `#[test]`s in [[crates/modeltap-store/tests/concurrent.rs]] cover the store-internals path; the two acceptance scenarios in [[tests/acceptance/cache_concurrent.rs]] cover the modeltap-binary boundary.

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
