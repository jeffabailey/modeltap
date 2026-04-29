//! Headless TUI mode (per `docs/feature/modeltap-tui/distill/acceptance-test-plan.md` §4).
//!
//! When `MODELTAP_HEADLESS=1` (or `--headless`), the binary runs the same
//! `update()` and `view()` functions as production, but renders against
//! ratatui's `TestBackend` and consumes scripted input from
//! `MODELTAP_HEADLESS_INPUT` instead of the real terminal.
//!
//! This contract gives the acceptance suite a deterministic, scriptable, fast
//! test harness while preserving production code paths.

use std::io::Write;

use modeltap_tui::{update, view, AppState, Msg, UpdateEffect};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::observability::{LaunchLogger, RecordKind};

/// Configuration parsed from CLI args + env at startup.
pub struct HeadlessConfig {
    pub cols: u16,
    pub rows: u16,
    /// Scripted input. Empty string means "no input — paint once and quit when
    /// `--quit-after-paint` is set".
    pub input: String,
    /// When true, render one frame and exit cleanly. Used by `launch.timing`
    /// and the K3 benchmark.
    pub quit_after_paint: bool,
}

/// Run the headless event loop. Returns the process exit code.
pub fn run(config: HeadlessConfig, initial_state: AppState, mut logger: LaunchLogger) -> i32 {
    let mut terminal = match Terminal::new(TestBackend::new(config.cols, config.rows)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("modeltap: failed to construct TestBackend: {e}");
            return 1;
        }
    };

    let mut state = initial_state;

    // Initial paint — required by US-01 AC-1 (cold start to first paint).
    if let Err(e) = terminal.draw(|f| view(&state, f)) {
        eprintln!("modeltap: initial paint failed: {e}");
        return 1;
    }

    // Parse scripted input once; reuse for the loop and the summary count.
    let scripted_msgs = parse_script(&config.input);
    let scripted_count = scripted_msgs.len();

    // Process scripted input one Msg at a time. After each Msg, redraw.
    for msg in scripted_msgs {
        let (next, effect) = update(state, msg);
        state = next;
        if let Err(e) = terminal.draw(|f| view(&state, f)) {
            eprintln!("modeltap: redraw failed: {e}");
            return 1;
        }
        apply_effect(&effect, &mut logger);
        if state.should_quit {
            break;
        }
    }

    if !state.should_quit && !config.quit_after_paint {
        eprintln!("modeltap: headless mode invoked without input and without --quit-after-paint");
        return 1;
    }

    print_frame(&terminal);

    let summary = serde_json::json!({
        "schema": "modeltap.session_summary.v1",
        "frames_captured": 1 + scripted_count,
        "exit_reason": exit_reason(&state),
        "exit_code": state.exit_code,
        "log_path": logger.path().map(|p| p.display().to_string()),
    });
    println!("{}", summary);

    state.exit_code
}

fn exit_reason(state: &AppState) -> &'static str {
    match state.exit_code {
        0 => "user_quit",
        130 => "ctrl_c",
        _ => "other",
    }
}

fn print_frame(terminal: &Terminal<TestBackend>) {
    let backend = terminal.backend();
    let buffer = backend.buffer();
    for y in 0..buffer.area.height {
        let mut line = String::with_capacity(buffer.area.width as usize);
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        let _ = std::io::stdout().write_all(line.trim_end().as_bytes());
        let _ = std::io::stdout().write_all(b"\n");
    }
}

fn apply_effect(effect: &UpdateEffect, logger: &mut LaunchLogger) {
    if effect.emit_launch_ended {
        logger.record(RecordKind::LaunchEnded);
    }
}

/// Parse the simple scripted-input format. Recognized tokens:
///   - `q`           → `Msg::Quit`
///   - `^C`          → `Msg::CtrlC`
///   - `<right>`     → `Msg::SelectNextTool`
///   - `<left>`      → `Msg::SelectPrevTool`
///   - `<up>`        → `Msg::SelectPrevRow`
///   - `<down>`      → `Msg::SelectNextRow`
///   - `<tab>`       → `Msg::ToggleFocus`
///   - any other single character → `Msg::UnboundKey`
///   - whitespace is skipped.
///
/// The richer DSL (wait_for, type) lands in subsequent steps; for step 01-03
/// only the tokens above are needed by the @us-03 acceptance scenarios.
fn parse_script(raw: &str) -> Vec<Msg> {
    let mut out = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '^' => match chars.next() {
                Some('C') => out.push(Msg::CtrlC),
                Some(_) => out.push(Msg::UnboundKey),
                None => {}
            },
            '<' => {
                let mut tag = String::new();
                let mut closed = false;
                for tc in chars.by_ref() {
                    if tc == '>' {
                        closed = true;
                        break;
                    }
                    tag.push(tc);
                }
                if !closed {
                    out.push(Msg::UnboundKey);
                    continue;
                }
                let msg = match tag.as_str() {
                    "right" => Msg::SelectNextTool,
                    "left" => Msg::SelectPrevTool,
                    "down" => Msg::SelectNextRow,
                    "up" => Msg::SelectPrevRow,
                    "tab" => Msg::ToggleFocus,
                    _ => Msg::UnboundKey,
                };
                out.push(msg);
            }
            'q' => out.push(Msg::Quit),
            _ if c.is_whitespace() => {}
            _ => out.push(Msg::UnboundKey),
        }
    }
    out
}
