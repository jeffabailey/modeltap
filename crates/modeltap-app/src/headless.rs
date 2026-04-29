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

use modeltap_core::domain::last_action::LastAction;
use modeltap_core::{Tool, ToolId};
use modeltap_tui::{update, view, AppState, Msg, UpdateEffect};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::actions::zap::{self, ZapOutcome, ZapResult};
use crate::observability::{LaunchLogger, RecordKind};
use crate::refresh;

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
pub fn run(
    config: HeadlessConfig,
    initial_state: AppState,
    mut logger: LaunchLogger,
    plugins: Vec<Box<dyn Tool>>,
) -> i32 {
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

    // Tokens parsed up-front (script tokens are independent of state); we
    // resolve each token to the right Msg per-iteration based on whether a
    // dialog is open at that moment.
    let tokens = tokenize_script(&config.input);
    let token_count = tokens.len();

    // Lazy-construct a tokio runtime ONLY if a zap actually fires (the @us-01
    // K3 path must not pay the runtime cost).
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("modeltap: failed to construct tokio runtime: {e}");
            return 1;
        }
    };

    for token in tokens {
        let msg = token_to_msg(&token, state.zap_dialog.is_some());
        let (next, effect) = update(state, msg);
        state = next;
        if let Err(e) = terminal.draw(|f| view(&state, f)) {
            eprintln!("modeltap: redraw failed: {e}");
            return 1;
        }
        apply_effect(&effect, &mut logger, &plugins, &rt, &mut state);
        if state.should_quit {
            break;
        }
    }

    if !state.should_quit && !config.quit_after_paint {
        eprintln!("modeltap: headless mode invoked without input and without --quit-after-paint");
        return 1;
    }

    // Final repaint so footer messages set by zap are visible in the captured
    // frame.
    if let Err(e) = terminal.draw(|f| view(&state, f)) {
        eprintln!("modeltap: final paint failed: {e}");
        return 1;
    }

    print_frame(&terminal);

    let summary = serde_json::json!({
        "schema": "modeltap.session_summary.v1",
        "frames_captured": 1 + token_count,
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

fn apply_effect(
    effect: &UpdateEffect,
    logger: &mut LaunchLogger,
    plugins: &[Box<dyn Tool>],
    rt: &tokio::runtime::Runtime,
    state: &mut AppState,
) {
    if effect.emit_launch_ended {
        logger.record(RecordKind::LaunchEnded);
    }
    if let Some(tool_id) = effect.trigger_zap {
        if let Some(plugin) = find_plugin(plugins, tool_id) {
            let outcome: ZapOutcome = rt.block_on(zap::run(plugin, logger));
            // Build the structured LastAction and dispatch it as a Msg so
            // the Elm-style update is the only place that mutates AppState
            // (per ADR-006).
            let action = build_last_action(&outcome);
            let (next, _) = update(std::mem::take(state), Msg::SetLastAction(action));
            *state = next;

            // Per US-06.AC-4 / US-11.AC-1: re-run discover() ONLY for the
            // affected tool to keep the summary refresh under 500 ms (the
            // alternative — re-running every plugin's discover() — scales
            // O(N plugins) and would break the budget once HF/llama-cli
            // populate). Failures here are logged but non-fatal: the
            // existing slot stays in place so the UI doesn't go blank.
            match rt.block_on(refresh::refresh_tool(plugin)) {
                Ok(view) => {
                    let (next, _) = update(std::mem::take(state), Msg::RefreshTool(view));
                    *state = next;
                }
                Err(e) => {
                    tracing::warn!(
                        target: "modeltap.refresh",
                        "refresh_tool failed for {}: {e}",
                        tool_id.0
                    );
                }
            }
        } else {
            // Pathological — UI selected a tool that's not in the plugin set.
            tracing::warn!(target: "modeltap.action.zap", "no plugin for {}", tool_id.0);
        }
    }
}

/// Map a `ZapOutcome` to a structured `LastAction` for the right-pane banner.
/// Bytes-retained is 0 in the WS slice — cross-tool sharing classifier lands
/// in 03-01.
fn build_last_action(outcome: &ZapOutcome) -> LastAction {
    match outcome.outcome {
        ZapResult::Success => LastAction::for_zap_success(outcome.tool, outcome.bytes_reclaimed, 0),
        ZapResult::Partial => {
            // WS slice never produces Partial from a real plugin; render
            // it as Failed so the user is not misled into thinking a partial
            // success is reflected. Once 03-03 lands, switch to the proper
            // Partial constructor with target-error detail.
            LastAction::for_zap_failed(outcome.tool)
        }
        ZapResult::Empty | ZapResult::Failed => LastAction::for_zap_failed(outcome.tool),
    }
}

fn find_plugin(plugins: &[Box<dyn Tool>], tool_id: ToolId) -> Option<&dyn Tool> {
    plugins
        .iter()
        .find(|p| p.name().0 == tool_id.0)
        .map(|b| b.as_ref())
}

/// One scripted token. Parsed once up-front; resolved to a `Msg` per
/// iteration based on whether a dialog is open at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ScriptToken {
    Char(char),
    Tag(String),
    CtrlC,
}

fn tokenize_script(raw: &str) -> Vec<ScriptToken> {
    let mut out = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '^' => match chars.next() {
                Some('C') => out.push(ScriptToken::CtrlC),
                Some(other) => out.push(ScriptToken::Char(other)),
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
                    out.push(ScriptToken::Char('<'));
                    continue;
                }
                out.push(ScriptToken::Tag(tag));
            }
            _ if c.is_whitespace() => {}
            _ => out.push(ScriptToken::Char(c)),
        }
    }
    out
}

/// Resolve a `ScriptToken` to an `Msg`, accounting for whether a typed-input
/// dialog is currently open. Mirrors `keymap::dispatch_in_dialog` in spirit
/// (printable chars go to the dialog buffer; only Esc/Enter/Backspace are
/// dialog control). Outside a dialog, the script-token-to-Msg mapping
/// matches the @us-03 acceptance contract.
fn token_to_msg(token: &ScriptToken, dialog_open: bool) -> Msg {
    if dialog_open {
        return match token {
            ScriptToken::CtrlC => Msg::CtrlC,
            ScriptToken::Tag(t) => match t.as_str() {
                "esc" => Msg::DialogCancel,
                "enter" => Msg::DialogConfirm,
                "backspace" => Msg::DialogBackspace,
                _ => Msg::UnboundKey,
            },
            ScriptToken::Char(c) => Msg::DialogTextInput(*c),
        };
    }
    match token {
        ScriptToken::CtrlC => Msg::CtrlC,
        ScriptToken::Tag(t) => match t.as_str() {
            "right" => Msg::SelectNextTool,
            "left" => Msg::SelectPrevTool,
            "down" => Msg::SelectNextRow,
            "up" => Msg::SelectPrevRow,
            "tab" => Msg::ToggleFocus,
            "esc" => Msg::DialogCancel,
            "enter" => Msg::DialogConfirm,
            "backspace" => Msg::DialogBackspace,
            _ => Msg::UnboundKey,
        },
        ScriptToken::Char(c) => match c {
            'q' => Msg::Quit,
            'z' => Msg::ZapTool,
            _ => Msg::UnboundKey,
        },
    }
}
