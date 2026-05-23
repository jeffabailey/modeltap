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
