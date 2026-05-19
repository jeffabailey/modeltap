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
mod observability;

use modeltap_app::adapters::cache_path;
use modeltap_app::inventory_build;
use modeltap_app::orchestration::warm_start::{self, WarmStartConfig};
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
    // We only invoke the warm-start when `MODELTAP_CACHE_PATH` is set so the
    // launch never silently writes to the user's real
    // `$XDG_DATA_HOME/modeltap/cache.sqlite` until the walking-skeleton
    // scenario (step 01-05) wires the full opt-in. `--no-cache` skips the
    // warm-start regardless.
    let cache_env_override = std::env::var_os("MODELTAP_CACHE_PATH");
    let warm_start_outcome = if cli.no_cache || cache_env_override.is_none() {
        // Cache disabled or no test override — skip warm-start (cold-start
        // owns the launch). Honors AC-23-8 / AC-23-9 (zero cache bytes
        // written) and preserves existing-acceptance-test behavior.
        None
    } else {
        let resolved = cache_path::resolve(None, cache_env_override.as_deref());
        match resolved {
            Ok(cache_file) => {
                let config = WarmStartConfig {
                    cache_enabled: true,
                    log_dir: log_dir.clone(),
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

    let inventory_start = Instant::now();
    let summary: InventorySummary = runtime.block_on(run_discovery(plugins_for_discovery));
    let full_inventory_ms = inventory_start.elapsed().as_millis() as u64;

    // Step 01-04: post-cold-start, write the discovered tools/models back to
    // the cache so the next launch's warm-start has fresh data. Stub-level —
    // executed only when the warm-start path ran (cache env override set,
    // `--no-cache` absent). Full background-reconcile semantics (TTL, partial
    // refresh) land in phase 04.
    if warm_start_source.is_some() {
        if let Some(env) = cache_env_override.as_deref() {
            if let Ok(cache_file) = cache_path::resolve(None, Some(env)) {
                runtime.block_on(reconcile_writeback(cache_file, &summary));
            }
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
    // `MODELTAP_CACHE_PATH` is unset, we skip the cache half of the merge
    // (the orchestrator falls back to `inspect_tool()` alone). When set,
    // we hand the resolved absolute path to the event loop so the
    // composition root can pass it into `orchestration::open_tool_detail`.
    let tool_detail_cache_path: Option<PathBuf> = if cli.no_cache || cache_env_override.is_none() {
        None
    } else {
        cache_path::resolve(None, cache_env_override.as_deref()).ok()
    };

    if headless {
        let config = HeadlessConfig {
            cols,
            rows: 40,
            input: std::env::var("MODELTAP_HEADLESS_INPUT").unwrap_or_default(),
            quit_after_paint: cli.quit_after_paint,
            cache_path: tool_detail_cache_path.clone(),
            log_dir: log_dir.clone(),
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

/// Stub background-reconcile writeback (tool-model-info-sqlite-cache step
/// 01-04). After cold-start completes, write each per-tool discovery result
/// back to the cache so the next launch's warm-start has fresh data. Every
/// rusqlite call is wrapped in `tokio::task::spawn_blocking`
/// (architecture-design.md §7.1). Errors are swallowed (logged to stderr) —
/// cache failure must never prevent the inventory view (C-INFO-2).
///
/// Full background-reconcile semantics (TTL eligibility, partial refresh,
/// drift detection) land in phase 04.
async fn reconcile_writeback(cache_file: std::path::PathBuf, summary: &InventorySummary) {
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
        let Ok(models) = &outcome.result else {
            continue;
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

    // Write back via spawn_blocking — one task for all rows.
    let join = tokio::task::spawn_blocking(move || {
        for t in &tools {
            cache.write_tool(t)?;
        }
        for (tool_id, models) in &models_per_tool {
            cache.write_models(tool_id, models)?;
        }
        Ok::<(), modeltap_store::CacheError>(())
    })
    .await;
    if let Ok(Err(e)) = join {
        eprintln!("modeltap: reconcile-writeback row write failed: {e}");
    }
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
