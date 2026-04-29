//! modeltap composition root (per ADR-005 + ADR-006 + ADR-007).
//!
//! Step 01-01 wires:
//! - clap CLI parsing for `--headless` and `--quit-after-paint`.
//! - Terminal-size guard (US-01 AC-4): refuse < 80 columns with exit 2.
//! - Panic hook installation (US-01 AC-5): restore terminal before printing.
//! - JSONL launch log open (kpi-instrumentation §2): emit launch.started.
//! - Headless event loop (acceptance-test-plan §4): TestBackend + scripted input.
//! - Production event loop: deferred to step 01-03 once arrow-key navigation
//!   exists (the WS exit gate is satisfied by the headless path; the production
//!   loop is the same `update()`/`view()` pair under a CrosstermBackend, added
//!   once there is more than the empty pane to render).

mod headless;
mod observability;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use modeltap_tui::{check_terminal_width, install_panic_hook};

use crate::headless::HeadlessConfig;
use crate::observability::{LaunchLogger, RecordKind};

/// CLI arguments. ADR-007 says edges use anyhow + clap; domain code stays
/// thiserror. This is the edge.
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

    /// In headless mode, render one frame and exit cleanly. Used by the K3
    /// benchmark and by acceptance scenarios that only assert on first paint.
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

    // Terminal-size guard (US-01 AC-4). In headless mode we read MODELTAP_TERM_COLS
    // (per acceptance-test-plan §4); production reads from crossterm.
    let cols = resolve_terminal_cols(headless);
    if let Err(err) = check_terminal_width(cols) {
        eprintln!("{}", err);
        return ExitCode::from(2);
    }

    if headless {
        let config = HeadlessConfig {
            cols,
            rows: 40,
            input: std::env::var("MODELTAP_HEADLESS_INPUT").unwrap_or_default(),
            quit_after_paint: cli.quit_after_paint,
        };
        let exit = headless::run(config, logger);
        // Per Cli::ExitCode, only u8 is safe. POSIX 130 fits.
        return ExitCode::from(exit as u8);
    }

    // Production interactive event loop is implemented in step 01-03 once
    // arrow-key navigation exists. For now, refuse to launch outside headless
    // mode with a clear message — there is nothing yet to interact with.
    eprintln!(
        "modeltap: interactive mode is implemented in step 01-03; \
         use --headless or MODELTAP_HEADLESS=1 for the walking-skeleton scaffold"
    );
    ExitCode::from(64)
}

fn resolve_terminal_cols(headless: bool) -> u16 {
    if headless {
        // Acceptance tests provide MODELTAP_TERM_COLS; default to 100 columns
        // (per acceptance-test-plan §4 "fixed 100x40 size").
        return std::env::var("MODELTAP_TERM_COLS")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(100);
    }
    crossterm::terminal::size()
        .map(|(cols, _)| cols)
        .unwrap_or(0)
}
