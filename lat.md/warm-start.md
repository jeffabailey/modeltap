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

[[crates/modeltap-app/tests/acceptance/cache_production_default.rs|`cache_production_default.rs`]] is the regression test that pins `HOME` (and `XDG_DATA_HOME` for Linux parity) to a tempdir, runs `modeltap --quit-after-paint` with NO `MODELTAP_CACHE_PATH` override, and asserts the platform-default cache file appears non-empty. It exists to catch a 2026-05-18 latent bug where the launch path short-circuited warm-start to `None` whenever the env override was unset — i.e. every production launch — bypassing the resolver entirely. The three call sites in [[crates/modeltap-app/src/main.rs|main.rs]] (warm-start gate, writeback gate, `tool_detail_cache_path` gate) all now branch on `cache_enabled` alone; the resolver itself owns the three-tier fallback.

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

## Background reconcile orchestrator

Step 05-01 adds [[crates/modeltap-app/src/orchestration/reconcile.rs|`reconcile::run(scope, plugins, config)`]] — the post-warm-paint orchestrator that walks every registered plugin's `discover()`, diffs the result against the cached rows, and writes the merged inventory back atomically per tool.

`ReconcileScope::All` is the post-warm-paint default and the future `[Shift+R]` semantic. `ReconcileScope::Tool(ToolId)` is the future `[r]` per-tool refresh. Both manual-refresh hotkeys land in step 05-03; this orchestrator exposes the entry point the hotkeys will dispatch into.

Each per-tool reconcile runs inside ONE `tokio::task::spawn_blocking` that opens the cache, reads the cached signature, computes the diff, and writes the new rows in a single `BEGIN IMMEDIATE..COMMIT` transaction via [[crates/modeltap-store/src/repo/tools.rs|`Cache::atomic_reconcile_write`]]. On `Err(_)` the transaction rolls back automatically via rusqlite's `Drop` — the cache stays at last-known-good per AC-26-3 — and a `reconcile_failed tool=<id> reason=<text>` line is appended to `<diagnostics_dir>/diagnostics.log` before the per-tool `ToolFailed` event is dispatched.

## Inventory diff (pure function)

[[crates/modeltap-core/src/logic/inventory_diff.rs|`compute_inventory_diff(tool_id, cached, fresh) -> InventoryDiff`]] is the pure-domain core, projecting both sides into sorted `added_models` / `removed_models` / `modified_models` vectors.

Inputs are `&[ModelSignature]` on both sides. `sha256_changed` is suppressed when either side is `None` so the lazy hash pool never produces false-positive drift events as it catches up. The function is its own driving port — calling it directly from tests IS port-to-port testing per the nw-tdd-methodology convention for pure domain functions. The orchestrator emits `Msg::ReconcileCompleted { tool, has_diff }` where `has_diff = !diff.is_empty()` so the renderer's silent-ack lookup stays O(1).

## Silent-ack indicator

AC-26-4: when a background reconcile produces a non-empty drift, the renderer paints a blue `*` next to the affected tool row for 3 seconds. State lives in `AppState.silent_ack_until` as `BTreeMap<ToolId, Instant>`.

`Msg::ReconcileCompleted { has_diff: true }` inserts `(tool, now + 3s)`; the tick timer (lands in step 05-03) dispatches `Msg::DismissSilentAck { tool }` when the wall-clock crosses the stored instant. Per-tool granularity matches the AC: simultaneous reconciles surface independent indicators with independent expiries — dismissing one never clears any other. `has_diff: false` is a state-noop; a no-op reconcile produces no user-visible indicator.

INT-INFO-3 holds inside the orchestrator's row projection: [[crates/modeltap-app/src/orchestration/reconcile.rs|`project_to_cache_rows`]] sets `tool.disk_usage_bytes = sum(models.size_bytes)` by construction, so the renderer's later total assertion is satisfied within 1-byte rounding (all accumulators are u64).

Per-loop Msg dispatch wiring (calling `reconcile::run` from inside the headless / interactive event loops) is deferred to step 05-03 when the `[r]` / `[Shift+R]` keymap dispatch joins the same call site.

## Manual refresh

Step 05-03 lands the user-facing hotkeys for the orchestrator from step 05-01. [[crates/modeltap-tui/src/keymap.rs]] registers `[r]` (refreshes the selected tool) and `[Shift+R]` (refreshes every registered plugin in parallel).

Both hotkeys are silent no-ops while any dialog is open per AC-24-5 — the keymap routes dispatch through `dispatch_in_dialog` while a dialog is up, which translates `KeyCode::Char(_)` to `Msg::DialogTextInput`, never to `Msg::RequestRefresh`.

The dispatch is peek-then-translate, mirroring [[model-detail-tui]]'s open-detail pattern. `Msg::RequestRefresh(RefreshScope)` is captured by [[crates/modeltap-app/src/interactive.rs|`interactive::translate_key` → `dispatch_request_refresh`]] / [[crates/modeltap-app/src/headless.rs|`headless::dispatch_request_refresh`]] BEFORE the pure `update` runs. The pure `update::apply_request_refresh` inserts the affected `ToolId`s into `state.reconciling` so the summary-bar suffix renders on the very next paint; the composition root then runs the work and dispatches completion `Msg`s.

[[crates/modeltap-tui/src/view/provenance.rs|`format_provenance(now, last_scan_at) -> String`]] is the pure helper (CM-D §9 of `acceptance-test-plan.md`) returning `"just now"` (< 5 sec or clock skew), `"<N> sec ago"`, `"<N> min ago"`, `"<N> hours ago"`, `"<N> days ago"`, or `"never reconciled"` when `last_scan_at` is `None`. Saturating arithmetic throughout — never panics on `SystemTime` math. The summary bar in [[crates/modeltap-tui/src/render/summary_bar.rs|`summary_text_at(state, now)`]] always appends `" | as of <Z>"` in the main view, gains a `", refreshing <tool>..."` (single in-flight) or `", reconciling..."` (multi-tool) suffix while `AppState.reconciling` is non-empty, and inlines `" (<tool> refreshed)"` after the most recent completion from `state.last_refreshed_tool`.

Completion runs through the existing step 05-01 variants — there is NO `Msg::RefreshCompleted`. `Msg::ReconcileCompleted { tool, has_diff }` clears the entry from `state.reconciling`, sets `state.last_refreshed_tool = Some(tool)`, and bumps `state.last_scan_at = Some(SystemTime::now())` so the next render shows `"as of just now (<tool> refreshed)"` per AC-24-7's 1000 ms latency budget (`@k-info-2-refresh-1s`). `Msg::ReconcileFailed { tool }` clears `reconciling` but deliberately does NOT bump `last_scan_at` — the cache stays at last-known-good per AC-26-3.

The keymap `[r]` binding migrated off `Msg::RetryRefresh` to `Msg::RequestRefresh(RefreshScope::Tool(ToolId("")))`. The empty-string sentinel resolves at peek-then-dispatch time: the composition root reads `state.current_tool()` and substitutes the real selected `ToolId`. `RefreshScope::All` enumerates `state.real_tools_iter()`. `RetryRefresh` stays in `Msg` for any in-process retry call sites not yet audited for migration (US-11 legacy).

The four scenarios in [[tests/acceptance/manual_refresh.rs]] are `#[ignore]`d pending a launch.log timing seam — behavioural coverage for step 05-03 lives in the unit tests inside `view::provenance::tests`, `keymap::tests`, and `render::summary_bar::tests` instead.

Interim production path: the [[crates/modeltap-app/src/interactive.rs|interactive]] dispatcher currently invokes `refresh::refresh_tool_incremental` per target (in-process discovery walk) and synthesises a `Msg::ReconcileCompleted { has_diff: false }` per success. A follow-up will swap that for a real `orchestration::reconcile::run(scope, plugins, config)` dispatch once `PluginFactory.make` returns `Arc<dyn Tool + Send + Sync>` across all 7 plugin crates — the orchestrator's signature requires the upcast that `Box<dyn Tool>` cannot provide. The user-visible suffix transition (AC-24-2 / AC-24-7) is unaffected; the cache-writeback half (US-26 silent-ack indicator) remains driven by the post-warm-paint orchestrator from step 05-01.

## Bottom-bar width policy (bugfix, 2026-05-27)

The Main bar uses a cascading width-aware drop + a conditional `[r]` label so the 100-col headless terminal budget always preserves `[?] help` + `[q] quit` and US-11.AC-2's `"[r] retry"` wording.

Step 05-03 added `[r] refresh` + `[R] refresh-all` without pruning anything else; the rendered width overflowed 100 cols and silently clipped `[?] help` + `[q] quit`. The v0.2.7 release pipeline's `cargo test --workspace --locked` gate caught it via three acceptance tests (`us_01_launch_quit::devon_launches_and_sees_two_pane_layout`, `us_08_bottom_bar::unavailable_shortcuts_are_dimmed_in_bottom_bar`, `us_11_updated_totals::refresh_failure_shows_degraded_indicator`).

[[crates/modeltap-tui/src/render/bottom_bar.rs|`dropped_entries(ctx)`]] is the cascading width-aware drop policy. When `max_width` is set AND the section is Main AND the bar overflows, it progressively omits the lowest-priority entries until the remainder fits: `[F] folder-delete` → `[R] refresh-all` → `[z] zap tool` → `[d] delete-from-one`. Beyond these four every remaining entry is load-bearing UX (navigation arrows, `[u] unify`, `[r] refresh/retry`, `[?] help`, `[q] quit`) so the cascade stops there.

[[crates/modeltap-tui/src/render/bottom_bar.rs|`label_for(entry, ctx)`]] is the single rendered-label override. Two entries swap their `SHORTCUT_TABLE` label at render time: the Up-arrow row uses `up_down_bar_label(ctx.focus)` (focus-aware "tools" vs "models"), and the `[r]` row renders `"[r] retry"` instead of `"[r] refresh"` when `ctx.has_refresh_failures` is true — same key, same `Msg::RequestRefresh`, label only — restoring US-11.AC-2 wording without re-introducing the legacy hidden state.

Detail and Help bars never overflow at supported widths, so both the cascade and the `[r]` label override are scoped to the Main bar.
