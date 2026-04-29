//! modeltap composition root (per ADR-005 + ADR-006 + ADR-007).
//!
//! Step 01-03 wires the `AppState` from the discovery results so the TUI
//! has actual tool slots + model rows to render, and runs the production
//! interactive event loop alongside the headless variant. Both paths use
//! the same pure `update()` and `view()`.

mod actions;
mod discovery;
mod headless;
mod observability;
mod registry;

use modeltap_app::inventory_build;

// `refresh` lives in the library half (src/lib.rs) so integration tests
// can call `modeltap_app::refresh::refresh_tool` without re-compiling the
// composition root. The bin imports it via the lib name.
use modeltap_app::refresh;

// Force linkage of plugin crates so their `inventory::submit!` blocks
// register their PluginFactory entries. Without these `as _` imports,
// the linker elides the plugin crates and inventory::iter::<PluginFactory>()
// returns empty (per ADR-001 §"Plugin registration mechanism" caveat).
use modeltap_plugin_hf as _;
use modeltap_plugin_llama_cli as _;
use modeltap_plugin_lm_studio as _;
use modeltap_plugin_ollama as _;

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
}

fn main() -> ExitCode {
    install_panic_hook();
    let cli = Cli::parse();

    let headless_env = std::env::var("MODELTAP_HEADLESS").ok().as_deref() == Some("1");
    let headless = cli.headless || headless_env;

    let log_dir = std::env::var_os("MODELTAP_LOG_DIR").map(PathBuf::from);
    let mut logger = LaunchLogger::open(log_dir);
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

    let inventory_start = Instant::now();
    let summary: InventorySummary = runtime.block_on(run_discovery(plugins_for_discovery));
    let full_inventory_ms = inventory_start.elapsed().as_millis() as u64;

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

    if headless {
        let config = HeadlessConfig {
            cols,
            rows: 40,
            input: std::env::var("MODELTAP_HEADLESS_INPUT").unwrap_or_default(),
            quit_after_paint: cli.quit_after_paint,
        };
        let exit = headless::run(config, initial_state, logger, plugins_for_actions);
        return ExitCode::from(exit as u8);
    }

    // Production interactive event loop arrives in the next step (01-04 or
    // an early sub-step of Phase 02 once a real keyboard polling integration
    // test exists). For step 01-03 only headless mode is wired so the
    // @walking-skeleton @us-03 scenarios run end-to-end without requiring a
    // real PTY. The state is fully constructed (initial_state above) so the
    // production loop will only need to add a CrosstermBackend + key polling
    // shell when it lands.
    let _ = initial_state;
    eprintln!(
        "modeltap: interactive mode lands in a follow-up step; \
         use --headless or MODELTAP_HEADLESS=1 for the headless harness"
    );
    ExitCode::from(64)
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
fn build_app_state(summary: &InventorySummary) -> AppState {
    let tools: Vec<ToolView> = summary
        .outcomes
        .iter()
        .map(plugin_outcome_to_view)
        .collect();
    AppState::new_with_default_selection(tools)
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
