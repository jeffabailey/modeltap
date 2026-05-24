# Warm Start

The warm-start path is the launch-time routine that paints inventory from the SQLite cache before any cold-scan runs. It is the user-visible payoff of [[modeltap-store]] (the `< 100 ms` p90 K-INFO-1 budget is met here, not in the store crate).

## Rationale

ADR-015 splits launch into two phases: a fast warm paint from cache, then a background cold reconcile that owns mutating decisions. Warm-start is the first half; it never mutates state and never blocks on a cold scan.

`tokio::task::spawn_blocking` is the pattern the architecture mandates (architecture-design.md §7.1) because the `modeltap-store` crate is deliberately synchronous `rusqlite` — running it on the async runtime's worker pool would stall reactor progress. Each cache call therefore hops to the blocking pool and back.

Cache-path resolution lives in [[crates/modeltap-app/src/adapters/cache_path.rs|modeltap-app::adapters::cache_path]], not in the store. Keeping `dirs::data_dir()` out of `modeltap-store` lets the store crate stay platform-agnostic and lets acceptance tests pin the path via env-var without touching the store API.

## Cache-path resolver

[[crates/modeltap-app/src/adapters/cache_path.rs|`resolve(cli_override, env_override)`]] returns the path of the SQLite cache file with a three-tier override chain.

CLI override wins first (the future `--cache-path <path>` flag, currently unused). Then `MODELTAP_CACHE_PATH` — the production caller passes `std::env::var_os("MODELTAP_CACHE_PATH").as_deref()`. Then `dirs::data_dir().join("modeltap").join("cache.sqlite")` — the documented default per AC-23-1.

The resolver returns `CachePathError::NoDataDir` only when `dirs::data_dir()` itself cannot resolve. This is rare on supported platforms (macOS, Linux, WSL) but possible if `$HOME` is unset under test. Callers MUST propagate the error and fall through to cold-start (C-INFO-2).

## Warm-start orchestration

[[crates/modeltap-app/src/orchestration/warm_start.rs|`warm_start::run`]] is the async entry point. Inputs: `WarmStartConfig { cache_enabled, log_dir }` and the resolved `cache_path`. Output: `WarmStartResult { inventory, source }` plus a side-effect JSONL line.

`cache_enabled = false` (driven by `--no-cache` and `[cache] enabled = false`) short-circuits to `WarmStartSource::Disabled` with an empty inventory — cold-start then owns the launch entirely.

## Opt-out

Step 04-02 wires two user-facing opt-out levers to a single composition-root boolean: the [[crates/modeltap-app/src/main.rs|`--no-cache` flag]] and the file-based [[crates/modeltap-app/src/config.rs|`AppConfig.cache.enabled`]]. The flag wins when both are set.

The TOML lives at `~/.modeltap/config.toml` (or `MODELTAP_CONFIG_PATH` for tests) under `[cache] enabled = false`. A user with `cache.enabled = true` can still bypass for one launch via `--no-cache`. When the combined boolean is false, `Cache::open` is NEVER called downstream: the warm-start orchestrator, the tool-detail cache path, and the reconcile-writeback all short-circuit on the same gate.

The byte-precise invariant is asserted by [[tests/src/fixtures/dir_manifest.rs|`DirManifest`]] — a recursive `(relative_path, size, mtime)` snapshot over `xdg-data/modeltap/`. The cache-opt-out acceptance suite ([[tests/acceptance/cache_opt_out.rs|`cache_opt_out.rs`]]) snapshots the directory before each launch, runs the modeltap binary, re-snapshots, and asserts `before.assert_equal(&after)`. AC-23-8 + AC-23-9 are proven this way: zero new bytes means no `cache.sqlite`, no `-wal`, no `-shm`.

INT-INFO-6 (`modeltap --version` exits 0 with a corrupt cache) is satisfied by clap's auto-version handler — `#[command(version)]` on the `Cli` struct exits before `main()`'s body runs, so the cache resolution path is never reached. The fourth opt-out scenario seeds a 16 KB non-SQLite blob at `MODELTAP_CACHE_PATH` to prove this directly: if the version path ever regressed to opening the cache, the test would fail.

`Cache::open(_)` is wrapped in `spawn_blocking`. Branching on `CacheOpenResult` distinguishes `OpenedFresh` (empty schema → cold-start will populate), `OpenedExisting` (paint), and `OpenedAfterMigration { from, to }` (paint, but the composition root may surface a banner).

When the path proceeds to paint, a single `spawn_blocking` reads `cache.tools()` + `cache.models_for_tool(tool_id)` per tool inside one blocking hop — single round-trip per architecture-design.md §8.1.

The `launch.warm_paint_ms` JSONL event is emitted only on the `OpenedExisting` / `OpenedAfterMigration` paths (the warm path). Fresh and disabled paths emit no warm-paint event; cold-start logs its own `launch.first_paint_ms` later. Writes are best-effort — an unwritable log dir never blocks the launch (C-INFO-2 again).

## Per-tool TTL eligibility

Step 04-03 partitions the painted tools by per-tool freshness (US-25 AC-25-2 / AC-25-4). `WarmStartConfig.tool_ttl_seconds` (default 86_400 = 24h, loaded from `[cache] tool_ttl_seconds`) gates each row.

[[crates/modeltap-store/src/repo/tools.rs|`Cache::ttl_eligible(tool_id, ttl_seconds, now)`]] returns true iff the cached row's `last_scan_at >= now - ttl_seconds`. `now` is taken as a parameter (not from the wall clock) so the orchestrator stays deterministic under test. An absent row returns false — no cached evidence means cold-start owns the tool.

The partition runs inside the same `spawn_blocking` as the paint read. For every tool in `cache.tools()`: TTL-eligible rows feed `models_for_tool(_)` and their model rows append to the inventory; ineligible rows return their `tool_id` into `WarmStartResult.stale_tool_ids` for the downstream cold-scan dispatcher. Fresh and stale tools coexist in a single launch.

`WarmStartResult.stale_tool_ids` is always empty on the `Disabled` / `Fresh` paths (no cache to age out from). On `Existing` / `AfterMigration`, the field is the cold-scan worklist.

## Transient I/O fallback

A per-tool read error inside the partition loop does NOT abort warm-start (AC-25-7). The affected tool joins `stale_tool_ids` and the partition continues — surviving tools still paint.

The covered error variants are `CacheError::Io { .. }` and `CacheError::Sqlite(_)`. The launch always proceeds; the outer call site at the composition root falls through to cold-start whether warm-start returned partial data or `Err(_)` (C-INFO-2).

The fallback covers two seams: the `ttl_eligible` per-tool probe AND the `models_for_tool` per-tool read. A consistent table-level failure (e.g., `cache_models` mid-launch DROP) routes every tool to the stale list and yields an empty inventory — no panic. `MalformedRow` is intentionally NOT caught: a non-parseable column is a row-shape bug, not a transient I/O event, and should surface to the outer C-INFO-2 fallback at the call site.

## Production cache path resolution

[[crates/modeltap-app/src/adapters/cache_path.rs|`cache_path::resolve(None, None)`]] is the production path when no `--cache-path` flag (future) and no `MODELTAP_CACHE_PATH` env override are set. It returns `dirs::data_dir().join("modeltap").join("cache.sqlite")` (AC-23-1 / OQ-1).

The `dirs::data_dir()` crate resolves to `$HOME/Library/Application Support` on macOS and `$XDG_DATA_HOME` (or `$HOME/.local/share` when unset) on Linux. Acceptance tests pin both `HOME` and `XDG_DATA_HOME` to a tempdir so the resolver result is deterministic on every supported host.

The 24h default keeps the warm-paint window forgiving for the common case (Devon's daily TUI launch sees a near-empty stale list); operators with rapidly changing tool inventories can shorten it via `~/.modeltap/config.toml`'s `[cache] tool_ttl_seconds`.

## Launch metrics instrumentation

Step 04-05 (closes Phase 04) introduces [[crates/modeltap-app/src/instrumentation/launch_metrics.rs|`LaunchMetrics`]], a single JSONL facade for the four `launch.*` duration events the cache-state-model and integration-checkpoints acceptance suites read out of `<log_dir>/launch.log`.

Four events, four budgets per outcome-kpis.md §K-INFO-1 / K-INFO-7 / K3a / K3b. `launch.cache_open_ms` ≤ 100 ms (K-INFO-7) measures `Cache::open` + `tools()` + per-tool `models_for_tool(_)` round-trip; emitted from the warm-start orchestrator after the partition `spawn_blocking` joins.

`launch.warm_paint_ms` ≤ 150 ms (K-INFO-1 / K3a, debug envelope; release-build target ≤ 100 ms) measures cache-painted inventory first hitting the TUI buffer; emitted from the warm-start orchestrator on the `Existing` / `AfterMigration` paths only — `Fresh` and `Disabled` short-circuit before the emission so cold-start owns the paint metric.

`launch.first_paint_ms` ≤ 150 ms (K3b) is the cold-start skeleton-paint window; emitted from the composition root only when warm-start did NOT paint cached inventory (`source == None` OR `Disabled` / `Fresh`). `launch.full_inventory_paint_ms` ≤ 1150 ms (K3b) is emitted on every launch after discovery completes — both paths converge into a full inventory eventually, and the budget applies to both.

The facade replaces the per-boundary `emit_*_event` helpers previously inlined in `warm_start.rs` + `main.rs`. Each `record_*` method writes one line of shape `{"schema":"modeltap.launch.v1","event":...,"duration_ms":N}\n` and silently swallows I/O errors so an unwritable log dir never blocks the launch (C-INFO-2 + AC-23-11). `cache.write_wait_ms` from step 04-04 stays in `main.rs` because its `wait_ms` field name diverges from the `duration_ms` contract of the four paint events.

## @perf scenario gating

The K-INFO budgets (≤ 150 ms warm paint, ≤ 100 ms cache open) are calibrated against release builds; the three `cache_kpi` scenarios in [[tests/acceptance/cache_kpi.rs|`cache_kpi.rs`]] early-return on `cfg!(debug_assertions)` to prevent false reds on developer laptops.

outcome-kpis.md §K-INFO-1 explicitly notes the debug-build envelope is 1.5× the release ceiling, which is why the gating exists.

The early-return is a `return;` at function head — not a `#[cfg]` attribute on the test itself — so `cargo check -p modeltap-acceptance --tests` still type-checks the facade wiring and the fixture round-trip. CI exercises the K-INFO assertions via `cargo test --release --test cache_kpi`, and the local development loop stays fast.
