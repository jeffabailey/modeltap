//! modeltap composition root (per ADR-005 + ADR-006 + ADR-007).
//!
//! Step 01-03 wires the `AppState` from the discovery results so the TUI
//! has actual tool slots + model rows to render, and runs the production
//! interactive event loop alongside the headless variant. Both paths use
//! the same pure `update()` and `view()`.

mod actions;
mod discovery;
mod headless;
mod interactive;
// `observability` was promoted from `mod` (bin-private) to
// `pub mod` in lib.rs so `orchestration::revalidate` (lib-side) can emit
// `revalidate.invoked` JSONL events and integration tests can drive the
// K5 gate without spawning the binary
// (tool-model-info-sqlite-cache step 05-02 part 2/2).
use modeltap_app::observability;

use modeltap_app::adapters::cache_path;
// tool-model-info-sqlite-cache step 04-02 (AC-23-8 / AC-23-9): the app-level
// config loader exposes `[cache] enabled` from `~/.modeltap/config.toml`. The
// CLI `--no-cache` flag and `cache.enabled = false` combine here at the
// composition root — flag wins when both set.
use modeltap_app::config;
// tool-model-info-sqlite-cache step 04-05 (closes Phase 04): launch-metrics
// JSONL facade for the four launch.* duration events. Cold-start emits
// `first_paint_ms` + `full_inventory_paint_ms` here at the composition root;
// warm-start emits `cache_open_ms` + `warm_paint_ms` from the orchestrator.
use modeltap_app::instrumentation::launch_metrics::LaunchMetrics;
use modeltap_app::inventory_build;
// Step 05-01: orchestration::reconcile module is wired and compilable; per-loop
// Msg dispatch lands in step 05-03 (manual-refresh keymaps). The composition
// root references the module path here so any future regression in the orchestrator
// surfaces at the same call site that will dispatch it.
#[allow(unused_imports)]
use modeltap_app::orchestration::reconcile;
use modeltap_app::orchestration::warm_start::{self, WarmStartConfig, WarmStartSource};
use modeltap_app::platform::{current_platform, Platform};
// `registry` moved from `mod registry;` (private to the bin) to
// `pub mod registry` in lib.rs so integration tests can exercise the
// `MODELTAP_TEST_PLUGINS` seam without spawning the binary
// (tool-model-info-sqlite-cache step 01-03).
use modeltap_app::registry;

// `refresh` lives in the library half (src/lib.rs) so integration tests
// can call `modeltap_app::refresh::refresh_tool` without re-compiling the
// composition root. The bin imports it via the lib name.
use modeltap_app::refresh;

// Force linkage of plugin crates so their `inventory::submit!` blocks
// register their PluginFactory entries. Without these `as _` imports,
// the linker elides the plugin crates and inventory::iter::<PluginFactory>()
// returns empty (per ADR-001 §"Plugin registration mechanism" caveat).
use modeltap_plugin_atomic_chat as _;
use modeltap_plugin_gpt4all as _;
use modeltap_plugin_hf as _;
use modeltap_plugin_lm_studio as _;
use modeltap_plugin_ollama as _;

// US-18 5th-plugin certification fixture. Linked behind the `test-fixtures`
// Cargo feature so production binaries (built with `--no-default-features`)
// do NOT register the synthetic plugin.
#[cfg(feature = "test-fixtures")]
use modeltap_plugin_atomic_chat_fixture as _;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::Parser;
use modeltap_core::{ToolId, ToolStatus};
use modeltap_tui::{check_terminal_width, install_panic_hook, AppState, ToolView};

use crate::discovery::{run_discovery, InventorySummary, PluginOutcome};
use crate::headless::HeadlessConfig;
use crate::observability::{LaunchLogger, RecordKind};

#[derive(Debug, Parser)]
#[command(
    name = "modeltap",
    version,
    about = "TUI to discover and clean up local AI models"
)]
struct Cli {
    /// Force headless mode (TestBackend + scripted input). Equivalent to
    /// `MODELTAP_HEADLESS=1`.
    #[arg(long)]
    headless: bool,

    /// In headless mode, render one frame and exit cleanly.
    #[arg(long)]
    quit_after_paint: bool,

    /// Skip the warm-start cache entirely; cold-start always runs.
    /// Equivalent to `[cache] enabled = false` (AC-23-8 / AC-23-9).
    #[arg(long)]
    no_cache: bool,
}

fn main() -> ExitCode {
    install_panic_hook();

    // US-20 AC-3: native Windows is not a supported target. Refuse to start
    // with the documented WSL guidance before we touch the runtime, the
    // logger, or the TUI. The check honors `MODELTAP_FORCE_PLATFORM` so CI
    // can simulate a Windows host from a macOS / Linux runner.
    if current_platform() == Platform::Windows {
        eprintln!(
            "Windows is supported only via WSL — see \
             https://learn.microsoft.com/windows/wsl/install"
        );
        return ExitCode::from(64);
    }

    let cli = Cli::parse();

    let headless_env = std::env::var("MODELTAP_HEADLESS").ok().as_deref() == Some("1");
    let headless = cli.headless || headless_env;

    let log_dir = std::env::var_os("MODELTAP_LOG_DIR").map(PathBuf::from);
    let mut logger = LaunchLogger::open(log_dir.clone());
    logger.record(RecordKind::LaunchStarted);

    // Step 04-05: launch-metrics facade + reference instant for K3b
    // (cold-start first_paint_ms + full_inventory_paint_ms). Warm-start
    // emits its own cache_open_ms + warm_paint_ms from inside the
    // orchestrator. Pulling `launch_start` here means the cold-start
    // timings count CLI parsing + logger setup as part of the user-
    // perceived "launch -> paint" latency, matching what Devon observes.
    let launch_metrics = LaunchMetrics::new(log_dir.clone());
    let launch_start = Instant::now();

    let cols = resolve_terminal_cols(headless);
    if let Err(err) = check_terminal_width(cols) {
        eprintln!("{}", err);
        return ExitCode::from(2);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("modeltap: failed to construct tokio runtime: {e}");
            return ExitCode::from(1);
        }
    };

    // Construct two independent plugin sets via the factory iterator: one is
    // consumed by `run_discovery` (each handle moves into its tokio task),
    // the other is retained for action dispatch (zap-all et al.) so we don't
    // need to re-resurrect plugins after discovery returns. Plugin
    // constructors are stateless w.r.t. each other, so two instances are
    // semantically equivalent to one (per ADR-001).
    let plugins_for_discovery = registry::collect_plugins();
    let plugins_for_actions = registry::collect_plugins();

    // US-18 AC-7: capture the registered plugin set BEFORE discovery so a
    // panicking plugin still appears in `launch.inventory.tools_registered`.
    // Riley's release dashboards consume this list as the canonical inventory
    // of the deployed plugin set; it must be independent of per-plugin
    // discovery success AND independent of the atomic-chat fixture's runtime
    // opt-in env var. We therefore source this list directly from
    // `inventory::iter::<PluginFactory>()` (every linked factory) rather than
    // from `registry::collect_plugins()` (which filters the fixture out when
    // the opt-in env var is unset, to keep prior acceptance tests stable).
    let mut tools_registered: Vec<String> = inventory::iter::<modeltap_core::PluginFactory>()
        .map(|f| (f.make)().name().to_string())
        .collect();
    tools_registered.sort();

    // Pre-discovery contract check: every plugin's `accepted_formats()` MUST
    // be non-empty (per US-16.AC-3). The compatibility engine's defensive
    // branch will already render any offender's models as `?` (Unknown), but
    // the warning is what makes the bug visible to plugin authors. Surfaced
    // via `tracing::warn!` to the diagnostics log target.
    let plugin_capabilities = registry::collect_plugins()
        .iter()
        .map(|p| (p.name(), p.accepted_formats().to_vec()))
        .collect::<modeltap_core::logic::compatibility::PluginCapabilityMap>();
    let _empty_offenders = inventory_build::warn_on_empty_capabilities(&plugin_capabilities);

    // tool-model-info-sqlite-cache step 01-04: warm-start path.
    //
    // Cache opt-out has two paths — the CLI `--no-cache` flag (already
    // present on `Cli`) AND the `[cache] enabled = false` setting in
    // `~/.modeltap/config.toml` (step 04-02, US-23 AC-23-8 / AC-23-9). We
    // combine: cache is enabled iff BOTH the flag is absent AND the config
    // says enabled (the flag dominates).
    //
    // When `MODELTAP_CACHE_PATH` is set, it pins the cache file location
    // (used by acceptance tests for HOME isolation, and as a power-user
    // override). When unset, `cache_path::resolve` falls through to
    // `dirs::data_dir().join("modeltap").join("cache.sqlite")` — the
    // documented default per AC-23-1. Production launches MUST hit this
    // fallback; a previous guard (`|| cache_env_override.is_none()`)
    // accidentally short-circuited warm-start to `None` on every launch
    // without the env var, meaning real users never opened the cache.
    let app_config = config::load_from_env();
    let cache_enabled = !cli.no_cache && app_config.cache.enabled;
    let cache_env_override = std::env::var_os("MODELTAP_CACHE_PATH");
    let warm_start_outcome = if !cache_enabled {
        // Cache disabled (CLI flag or config) — skip warm-start
        // unconditionally. Honors AC-23-8 / AC-23-9 (zero cache bytes
        // written when the user opts out).
        None
    } else {
        let resolved = cache_path::resolve(None, cache_env_override.as_deref());
        match resolved {
            Ok(cache_file) => {
                let config = WarmStartConfig {
                    cache_enabled: true,
                    log_dir: log_dir.clone(),
                    tool_ttl_seconds: app_config.cache.tool_ttl_seconds,
                    now: std::time::SystemTime::now(),
                };
                match runtime.block_on(warm_start::run(&config, &cache_file)) {
                    Ok(result) => Some(result),
                    Err(e) => {
                        // C-INFO-2: cache failure NEVER prevents launch. Log
                        // and continue to cold-start.
                        eprintln!("modeltap: warm-start cache read failed: {e}; falling back to cold-start");
                        None
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "modeltap: cache path resolution failed: {e}; falling back to cold-start"
                );
                None
            }
        }
    };
    // Step 01-04 keeps the cold-start unconditional: the warm-start result is
    // observed (for the JSONL event) but the existing per-plugin discovery
    // still runs. Step 01-05 makes the warm-paint short-circuit the
    // initial paint and dispatches the cold path as a background reconcile.
    let warm_start_source = warm_start_outcome.as_ref().map(|r| r.source);

    // Step 04-05 (K3b cold-start preservation): emit `launch.first_paint_ms`
    // ONLY when warm-start did NOT paint cached inventory. The Existing /
    // AfterMigration paths already emitted `launch.warm_paint_ms` (K3a);
    // Disabled / Fresh / None mean cold-start owns the user-visible paint
    // and the skeleton-paint window is what K3b measures.
    //
    // `first_paint_ms` is the boundary at which an empty/skeleton AppState
    // would be ready to render — in the current synchronous arch this is
    // the same Instant the discovery work begins. Future async-paint work
    // (background reconcile) will widen the gap; the event-name contract
    // stays stable.
    let warm_painted_inventory = matches!(
        warm_start_source,
        Some(WarmStartSource::Existing) | Some(WarmStartSource::AfterMigration { .. })
    );
    if !warm_painted_inventory {
        let first_paint_ms = launch_start.elapsed().as_millis() as u64;
        launch_metrics.record_first_paint(first_paint_ms);
    }

    let inventory_start = Instant::now();
    let summary: InventorySummary = runtime.block_on(run_discovery(plugins_for_discovery));
    let full_inventory_ms = inventory_start.elapsed().as_millis() as u64;

    // Step 04-05 (K3b cold-start preservation): emit
    // `launch.full_inventory_paint_ms` from launch_start to the moment
    // discovery completes. Emitted unconditionally because every launch
    // (warm OR cold) eventually completes a full inventory pass; the
    // budget (≤ 1150 ms p90) applies to both paths.
    let full_inventory_paint_ms = launch_start.elapsed().as_millis() as u64;
    launch_metrics.record_full_inventory_paint(full_inventory_paint_ms);

    // Step 01-04: post-cold-start, write the discovered tools/models back to
    // the cache so the next launch's warm-start has fresh data. Executed
    // whenever the warm-start path ran (cache enabled, regardless of
    // whether the env override pinned the path or the default resolver
    // produced it). Full background-reconcile semantics (TTL, partial
    // refresh) land in phase 04.
    if warm_start_source.is_some() {
        if let Ok(cache_file) = cache_path::resolve(None, cache_env_override.as_deref()) {
            runtime.block_on(reconcile_writeback(cache_file, &summary, log_dir.clone()));
        }
    }

    let model_count = summary.total_models();
    logger.record(RecordKind::LaunchTiming {
        plugin_timings_ms: summary.plugin_timings_ms(),
        full_inventory_ms,
        model_count,
    });
    logger.record(RecordKind::LaunchInventory {
        total_models: model_count,
        total_disk_usage_bytes: summary.total_disk_usage_bytes(),
        dedupable_count: summary.dedupable_count(),
        format_locked_count: summary.format_locked_count(),
        tool_errors: summary.tool_errors(),
        tools_registered: tools_registered.clone(),
    });
    // Per-model JSONL entries (writes to models.log next to launch.log) so
    // acceptance tests can assert per-model metadata (display_label, format,
    // status) without going through the TUI.
    for outcome in &summary.outcomes {
        if let Ok(models) = &outcome.result {
            let tool_name = outcome.tool.to_string();
            for m in models {
                logger.record(RecordKind::DiscoveredModel {
                    tool: tool_name.clone(),
                    id_in_tool: m.id_in_tool.clone(),
                    display_label: m.display_label.0.clone(),
                    format: format_label(m.format),
                    status: status_label(&m.status),
                    size_bytes: m.size_bytes,
                });
            }
        }
    }

    let initial_state = build_app_state(&summary);

    // Step 01-08: extract per-tool discovered models for the background
    // hash pool. Done here (before the headless/interactive branch) so both
    // event loops receive the same data. Plugins that errored out contribute
    // an empty Vec so they're visible in the per-tool list (no jobs queued).
    let discovered_per_tool: Vec<(ToolId, Vec<modeltap_core::DiscoveredModel>)> = summary
        .outcomes
        .iter()
        .map(|o| {
            let models = match &o.result {
                Ok(v) => v.clone(),
                Err(_) => Vec::new(),
            };
            (o.tool, models)
        })
        .collect();

    // Step 02-01 (US-21): resolve the tool-detail orchestrator's cache
    // path the same way warm-start does. When `--no-cache` was passed OR
    // `cache.enabled = false`, we skip the cache half of the merge (the
    // orchestrator falls back to `inspect_tool()` alone). Otherwise we
    // resolve via the same three-tier fallback as warm-start (CLI → env →
    // `dirs::data_dir()`), so production launches use the documented
    // default path and acceptance tests honour `MODELTAP_CACHE_PATH`.
    let tool_detail_cache_path: Option<PathBuf> = if !cache_enabled {
        None
    } else {
        cache_path::resolve(None, cache_env_override.as_deref()).ok()
    };

    // Step 02-03 (US-21 AC-21-9 / INT-INFO-8): resolve the panic-isolation
    // diagnostics directory. `MODELTAP_DIAGNOSTICS_DIR` (test override) wins;
    // production falls back to `~/.modeltap`. `None` disables on-disk panic
    // logging entirely (in-TUI sentinel is unaffected).
    let diagnostics_dir: Option<PathBuf> = std::env::var_os("MODELTAP_DIAGNOSTICS_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".modeltap")));

    if headless {
        let config = HeadlessConfig {
            cols,
            rows: 40,
            input: std::env::var("MODELTAP_HEADLESS_INPUT").unwrap_or_default(),
            quit_after_paint: cli.quit_after_paint,
            cache_path: tool_detail_cache_path.clone(),
            log_dir: log_dir.clone(),
            diagnostics_dir: diagnostics_dir.clone(),
        };
        let exit = headless::run(
            config,
            initial_state,
            logger,
            plugins_for_actions,
            discovered_per_tool,
        );
        return ExitCode::from(exit as u8);
    }

    // Production interactive event loop. Drives the same `update()` and
    // `view()` as the headless harness — only the backend
    // (CrosstermBackend on real stdout) and the input source (live
    // keypress polling via crossterm::event) differ. The headless harness
    // remains the deterministic acceptance-test driver; this is the path
    // a user reaches by running `modeltap` with no flags on a real TTY.
    match interactive::run(
        &runtime,
        initial_state,
        logger,
        plugins_for_actions,
        discovered_per_tool,
        tool_detail_cache_path,
        log_dir.clone(),
    ) {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("modeltap: interactive loop failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn resolve_terminal_cols(headless: bool) -> u16 {
    if headless {
        return std::env::var("MODELTAP_TERM_COLS")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(100);
    }
    crossterm::terminal::size()
        .map(|(cols, _)| cols)
        .unwrap_or(0)
}

/// Project the discovery summary into the TUI's `AppState`. One `ToolView`
/// per plugin outcome; `ToolStatus::Ok` for plugins that returned models,
/// `NotInstalled` / `Error` for the others. The `AppState` constructor
/// sorts alphabetically and lands the default selection on the first
/// installed tool.
///
/// Step 04-03 (cross-tool-model-unify) wires the `[All Unified]` synthetic
/// slot into the live `AppState`. The slot is appended AFTER every real tool
/// (per ADR-014 ordering); navigation, render dispatch, and badge-count
/// derivation were already in place from steps 04-01 and 04-02. The us_u7
/// acceptance suite (now unignored) drives the contract; the v1 us_03
/// wrap-cycle assertion was updated in lockstep to acknowledge the new
/// 5th slot.
fn build_app_state(summary: &InventorySummary) -> AppState {
    let tools: Vec<ToolView> = summary
        .outcomes
        .iter()
        .map(plugin_outcome_to_view)
        .collect();
    let mut state = AppState::new_with_default_selection(tools);
    state.append_all_unified_slot();
    state
}

/// Stable string label for a `Format` variant. Used in JSONL events; the TUI
/// uses its own renderer. We prefer literal `&'static str` over Debug to keep
/// the schema invariant under `derive(Debug)` evolution.
fn format_label(f: modeltap_core::Format) -> &'static str {
    use modeltap_core::Format::*;
    match f {
        Gguf => "Gguf",
        Safetensors => "Safetensors",
        Bin => "Bin",
        Awq => "Awq",
        Gptq => "Gptq",
        OllamaBlob => "OllamaBlob",
        Mlx => "Mlx",
        Other => "Other",
    }
}

fn status_label(s: &modeltap_core::ModelStatus) -> &'static str {
    use modeltap_core::ModelStatus::*;
    match s {
        Healthy => "Healthy",
        BrokenSymlink { .. } => "BrokenSymlink",
        Corrupt { .. } => "Corrupt",
        Unreadable { .. } => "Unreadable",
    }
}

/// Background-reconcile writeback (tool-model-info-sqlite-cache step 01-04;
/// extended in step 04-04 for per-tool transactional writes + concurrent-
/// process safety). After cold-start completes, write each per-tool discovery
/// result back to the cache so the next launch's warm-start has fresh data.
/// Every rusqlite call is wrapped in `tokio::task::spawn_blocking`
/// (architecture-design.md §7.1). Errors are swallowed (logged to stderr) —
/// cache failure must never prevent the inventory view (C-INFO-2).
///
/// Step 04-04 (US-23 Scenarios 4-5 / AC-23-10): each per-tool write goes
/// through `Cache::reconcile_tool` which wraps the row plus its models in a
/// single `BEGIN IMMEDIATE` transaction. The returned wait duration is
/// emitted as `cache.write_wait_ms` to `<log_dir>/launch.log` so the
/// concurrent-writers acceptance scenario can verify the busy_timeout path
/// fired. Writes from process A and process B serialize via SQLite's own
/// busy-wait — no advisory locking, no PID detection (ADR-015 §"Concurrency").
///
/// Full background-reconcile semantics (TTL eligibility, partial refresh,
/// drift detection) land in phase 04.
async fn reconcile_writeback(
    cache_file: std::path::PathBuf,
    summary: &InventorySummary,
    log_dir: Option<PathBuf>,
) {
    use modeltap_store::types::{CachedModel, CachedTool};
    use modeltap_store::{Cache, CacheOpenResult};
    use std::collections::BTreeMap;
    use std::time::SystemTime;

    // Open the cache via spawn_blocking — sync rusqlite at the edge of
    // async land.
    let opened = match tokio::task::spawn_blocking(move || Cache::open(&cache_file)).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            eprintln!("modeltap: reconcile-writeback open failed: {e}");
            return;
        }
        Err(e) => {
            eprintln!("modeltap: reconcile-writeback join failed: {e}");
            return;
        }
    };
    let cache = match opened {
        CacheOpenResult::OpenedExisting(c) => c,
        CacheOpenResult::OpenedFresh(c) => c,
        CacheOpenResult::OpenedAfterMigration { cache, .. } => cache,
        CacheOpenResult::OpenedAfterRecovery { cache, .. } => cache,
    };

    // Project the discovery summary into cache rows.
    let mut tools: Vec<CachedTool> = Vec::new();
    let mut models_per_tool: Vec<(ToolId, Vec<CachedModel>)> = Vec::new();
    let now = SystemTime::now();
    for outcome in &summary.outcomes {
        // tool-model-info-sqlite-cache step 02-02 (AC-21-4): per-tool
        // `DiscoverError::Io` / `PermissionDenied` / `UnexpectedLayout` /
        // `ManifestParse` are projected into a cache row whose `last_error`
        // + `last_error_at` carry the failure reason. The detail-screen
        // orchestrator then surfaces the message verbatim per AC-21-4. The
        // `NotInstalled` variant is intentionally NOT a "tool error" — it
        // matches the `(not installed)` left-pane status and writes no row.
        let models = match &outcome.result {
            Ok(models) => models,
            Err(modeltap_core::DiscoverError::NotInstalled) => continue,
            Err(err) => {
                tools.push(CachedTool {
                    tool_id: outcome.tool,
                    install_path: std::path::PathBuf::new(),
                    detected_version: None,
                    plugin_version: env!("CARGO_PKG_VERSION").to_string(),
                    model_count: 0,
                    disk_usage_bytes: 0,
                    largest_model_id: None,
                    last_scan_at: now,
                    last_scan_duration_ms: 0,
                    last_error: Some(err.to_string()),
                    last_error_at: Some(now),
                    search_paths: Vec::new(),
                });
                continue;
            }
        };
        let total_bytes: u64 = models.iter().map(|m| m.size_bytes).sum();
        let largest_id = models
            .iter()
            .max_by_key(|m| m.size_bytes)
            .map(|m| m.id_in_tool.clone());
        tools.push(CachedTool {
            tool_id: outcome.tool,
            install_path: std::path::PathBuf::new(),
            detected_version: None,
            plugin_version: env!("CARGO_PKG_VERSION").to_string(),
            model_count: models.len() as u64,
            disk_usage_bytes: total_bytes,
            largest_model_id: largest_id,
            last_scan_at: now,
            last_scan_duration_ms: 0,
            last_error: None,
            last_error_at: None,
            search_paths: Vec::new(),
        });

        let cached_models: Vec<CachedModel> = models
            .iter()
            .map(|m| CachedModel {
                model_id: m.id_in_tool.clone(),
                tool_id: outcome.tool,
                display_name: m.display_label.0.clone(),
                format: Some(format_label(m.format).to_string()),
                quantisation: None,
                size_bytes: m.size_bytes,
                sha256: None,
                architecture: None,
                parameters_billions: None,
                context_length: None,
                dedup_group_id: None,
                metadata_kv: BTreeMap::new(),
                metadata_introspected_at: None,
                last_seen_at: now,
                last_validated_at: None,
            })
            .collect();
        models_per_tool.push((outcome.tool, cached_models));
    }

    // Pair each tool row with its models for a single transactional write
    // per tool (step 04-04). Tools that ended in `last_error` carry an empty
    // model slice — the row still lands so the detail screen can surface
    // the error message (AC-21-4).
    let models_lookup: BTreeMap<ToolId, Vec<CachedModel>> = models_per_tool.into_iter().collect();
    let mut work: Vec<(CachedTool, Vec<CachedModel>)> = Vec::with_capacity(tools.len());
    for t in tools {
        let models = models_lookup.get(&t.tool_id).cloned().unwrap_or_default();
        work.push((t, models));
    }

    // Write back via spawn_blocking — one task drives every per-tool
    // transaction in sequence. The maximum observed wait at BEGIN IMMEDIATE
    // is returned so the composition root can emit `cache.write_wait_ms`.
    let join = tokio::task::spawn_blocking(move || {
        let mut max_wait_ms: u64 = 0;
        for (tool, models) in &work {
            let wait = cache.reconcile_tool(tool, models)?;
            let wait_ms = wait.as_millis().min(u128::from(u64::MAX)) as u64;
            if wait_ms > max_wait_ms {
                max_wait_ms = wait_ms;
            }
        }
        Ok::<u64, modeltap_store::CacheError>(max_wait_ms)
    })
    .await;
    match join {
        Ok(Ok(max_wait_ms)) => {
            emit_cache_write_wait_event(log_dir.as_deref(), max_wait_ms);
        }
        Ok(Err(e)) => {
            eprintln!("modeltap: reconcile-writeback row write failed: {e}");
        }
        Err(e) => {
            eprintln!("modeltap: reconcile-writeback join failed: {e}");
        }
    }
}

/// Append a single `cache.write_wait_ms` JSONL line to `<log_dir>/launch.log`.
/// Mirrors `warm_start::emit_warm_paint_event`. Best-effort — failures are
/// swallowed so an unwritable log dir never blocks the launch (C-INFO-2 +
/// AC-23-11). Per acceptance-test-plan.md §A: emitted on every reconcile
/// write so the concurrent-writers scenario can assert `>= 0` and `<= 5000`
/// — the busy_timeout PRAGMA caps the wait at 5 seconds.
fn emit_cache_write_wait_event(log_dir: Option<&std::path::Path>, wait_ms: u64) {
    use std::fs::OpenOptions;
    use std::io::Write;

    let Some(dir) = log_dir else {
        return;
    };
    let path = dir.join("launch.log");
    let envelope = serde_json::json!({
        "schema": "modeltap.launch.v1",
        "event": "cache.write_wait_ms",
        "wait_ms": wait_ms,
    });
    let mut serialized = envelope.to_string();
    serialized.push('\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(serialized.as_bytes()));
}

fn plugin_outcome_to_view(outcome: &PluginOutcome) -> ToolView {
    let tool: ToolId = outcome.tool;
    match &outcome.result {
        Ok(models) => ToolView {
            tool,
            status: ToolStatus::Ok,
            model_ids: models.iter().map(|m| m.id_in_tool.clone()).collect(),
            model_sizes_bytes: models.iter().map(|m| m.size_bytes).collect(),
        },
        Err(modeltap_core::DiscoverError::NotInstalled) => ToolView {
            tool,
            status: ToolStatus::NotInstalled,
            model_ids: Vec::new(),
            model_sizes_bytes: Vec::new(),
        },
        Err(other) => ToolView {
            tool,
            status: ToolStatus::Error {
                reason: other.to_string(),
            },
            model_ids: Vec::new(),
            model_sizes_bytes: Vec::new(),
        },
    }
}
