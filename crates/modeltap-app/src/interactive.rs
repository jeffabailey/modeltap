//! Production interactive event loop: real terminal via `CrosstermBackend`
//! and live keypress polling.
//!
//! This is the production counterpart to `crate::headless` — it shares the
//! same Elm-style `update()` from `modeltap_tui`, the same `view()` render
//! pipeline, the same plugin registry and the same `LaunchLogger`. Only the
//! backend (real `Stdout` / `CrosstermBackend` instead of ratatui's
//! `TestBackend`) and the input source (real keypresses polled from
//! `crossterm::event` instead of scripted tokens parsed from an env var)
//! differ.
//!
//! Closes the deferral from `main.rs` ("interactive mode lands in a
//! follow-up step"). The TUI components — `update`, `render`, `dialogs`,
//! `screens` — were already exercised by the headless harness; this module
//! is the real-terminal wiring.
//!
//! ## Lifecycle
//!
//! 1. Enable raw mode + enter the alternate screen.
//! 2. Construct a `Terminal<CrosstermBackend<Stdout>>`.
//! 3. Initial render of the supplied `AppState`.
//! 4. Event loop: poll keys with a short timeout (so the loop stays
//!    responsive to resize and signal-driven shutdown), translate via
//!    `keymap::dispatch` / `keymap::dispatch_in_dialog`, run `update()`,
//!    interpret the returned `UpdateEffect`, redraw.
//! 5. On `state.should_quit`, leave the alternate screen + disable raw
//!    mode and return `state.exit_code` as a `std::process::ExitCode`.
//!
//! ## Mutual exclusion with the panic hook
//!
//! `modeltap_tui::install_panic_hook` already wraps the default panic hook
//! with a terminal-restore call. We do NOT duplicate that here — only the
//! happy-path teardown.

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use modeltap_core::domain::last_action::LastAction;
use modeltap_core::{Tool, ToolId};
use modeltap_tui::{keymap, update, view, AppState, Msg, UpdateEffect};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::actions::zap::{self, ZapOutcome, ZapResult};
use crate::observability::{LaunchLogger, RecordKind};
use crate::refresh;

/// How long to block on `event::poll` per loop tick. Short enough to stay
/// responsive on resize / signal-driven teardown; long enough that an idle
/// TUI does not pin a CPU core. The headless harness has no analog — its
/// loop is fully script-driven.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Run the production interactive event loop.
///
/// `runtime` owns the tokio executor. We use it to `block_on` async action
/// orchestrators (zap, refresh) when the pure `update()` returns an
/// effect that needs side-effects. `plugins` is the action-side plugin
/// registry (the discovery-side set was consumed in `main()`).
///
/// Returns the desired process exit code: `0` for a user-driven quit
/// (`q`) and `130` for SIGINT-style interrupt (`Ctrl+C`), per the
/// AppState/exit_code contract.
pub fn run(
    runtime: &tokio::runtime::Runtime,
    initial_state: AppState,
    mut logger: LaunchLogger,
    plugins: Vec<Box<dyn Tool>>,
) -> io::Result<i32> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, runtime, initial_state, &mut logger, &plugins);

    // Always restore the terminal — even if the event loop returned an
    // error, the user must get their shell back. Errors during teardown
    // are swallowed (best-effort) because we are about to exit anyway and
    // the original error is more informative.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);

    result
}

/// The actual event loop, factored out so the surrounding `run()` can
/// guarantee terminal teardown via the `let result = ...; restore; result`
/// pattern even when `event_loop` returns `Err`.
fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    runtime: &tokio::runtime::Runtime,
    initial_state: AppState,
    logger: &mut LaunchLogger,
    plugins: &[Box<dyn Tool>],
) -> io::Result<i32> {
    let mut state = initial_state;

    // Initial paint — required by US-01 AC-1 (cold start to first paint).
    terminal.draw(|f| view(&state, f))?;

    while !state.should_quit {
        // Block up to POLL_INTERVAL waiting for an event. Returning `false`
        // means "no event yet" — we loop and try again. This keeps the
        // tokio runtime free for any background work and lets us redraw on
        // a regular cadence if needed (current code only redraws on input
        // and on the post-effect transient frames).
        if !event::poll(POLL_INTERVAL)? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                // crossterm on Windows / some terminals emits Press AND
                // Release events for every keystroke. We only act on Press
                // to avoid double-dispatch (one key press should only
                // produce one `Msg`). On platforms that don't distinguish
                // (kind == Any / no kind reported), the default fallthrough
                // still treats it as a press because KeyEventKind::Press is
                // the documented default for legacy terminals.
                if !is_press(&key) {
                    continue;
                }

                let msg = translate_key(&state, key);
                let (next, effect) = update(state, msg);
                state = next;
                terminal.draw(|f| view(&state, f))?;
                apply_effect(&effect, logger, plugins, runtime, &mut state);
                terminal.draw(|f| view(&state, f))?;
            }
            Event::Resize(_, _) => {
                // ratatui's `terminal.draw` recomputes the frame's `Rect`
                // from the current backend size, so the cheapest correct
                // resize handler is just to redraw.
                terminal.draw(|f| view(&state, f))?;
            }
            // Other Event variants (Mouse, Paste, FocusGained, FocusLost)
            // are not in the modeltap input contract — silently ignored
            // per US-03 AC-6 (unbound input is a no-op).
            _ => {}
        }
    }

    Ok(state.exit_code)
}

/// True iff this `KeyEvent` should drive an `update()`. Modern crossterm
/// reports `KeyEventKind::Press` / `Release` / `Repeat`; we accept Press
/// and Repeat (auto-repeat on a held arrow key should still navigate) and
/// drop Release. Older crossterm versions / legacy terminals always
/// report `Press`-equivalent so this is a no-op there.
fn is_press(key: &KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

/// Translate a real `KeyEvent` into the appropriate `Msg`, accounting for
/// whether a typed-input dialog is currently open. Mirrors the headless
/// harness's `token_to_msg` but consumes a real `KeyEvent` instead of a
/// scripted token. The dialog-open check is identical to headless.
fn translate_key(state: &AppState, key: KeyEvent) -> Msg {
    let dialog_open = state.zap_dialog.is_some()
        || state.unify_dialog.is_some()
        || state.delete_one_dialog.is_some();
    if dialog_open {
        keymap::dispatch_in_dialog(key)
    } else {
        keymap::dispatch(key)
    }
}

/// Interpret an `UpdateEffect`. Mirrors the production-relevant subset of
/// `headless::apply_effect`:
///
/// - `emit_launch_ended` writes the `launch.ended` JSONL record.
/// - `trigger_zap` runs the async zap orchestrator on the supplied
///   runtime, dispatches `Msg::SetLastAction(...)` back into update, and
///   re-runs incremental refresh for the affected tool slot.
///
/// The unify / delete-one / dry-run / running-tool effects are unreachable
/// from real keys today — they require a Detail screen, which the keymap
/// does not yet expose an `Enter`-binding for in production (the headless
/// harness reaches it via a `MODELTAP_HEADLESS_DETAIL_REGS` env-var seam).
/// We log a tracing warning if one of those effects ever fires so the gap
/// is visible.
fn apply_effect(
    effect: &UpdateEffect,
    logger: &mut LaunchLogger,
    plugins: &[Box<dyn Tool>],
    runtime: &tokio::runtime::Runtime,
    state: &mut AppState,
) {
    if effect.emit_launch_ended {
        logger.record(RecordKind::LaunchEnded);
    }

    if let Some(tool_id) = effect.trigger_zap {
        if let Some(plugin) = find_plugin(plugins, tool_id) {
            let outcome: ZapOutcome = runtime.block_on(zap::run(plugin, logger));
            let action = build_zap_last_action(&outcome);
            let (next, _) = update(std::mem::take(state), Msg::SetLastAction(action));
            *state = next;

            // Mirror headless's incremental-refresh-after-zap (US-06.AC-4 /
            // US-11.AC-1): re-run discover() ONLY for the affected tool so
            // the summary stays under the 500 ms budget.
            match runtime.block_on(refresh::refresh_tool_incremental(plugin)) {
                Ok(view) => {
                    let (next, _) = update(std::mem::take(state), Msg::RefreshSucceeded(view));
                    *state = next;
                }
                Err(refresh::RefreshError::NotInstalled) => {
                    tracing::info!(
                        target: "modeltap.refresh",
                        "refresh_tool_incremental: tool {} not installed",
                        tool_id.0
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        target: "modeltap.refresh",
                        "refresh_tool_incremental failed for {}: {e}",
                        tool_id.0
                    );
                    let (next, _) = update(std::mem::take(state), Msg::RefreshFailed(tool_id));
                    *state = next;
                }
            }
        } else {
            tracing::warn!(target: "modeltap.action.zap", "no plugin for {}", tool_id.0);
        }
    }

    // Forward-compatibility: the remaining effects only fire from the
    // Detail screen which is not yet reachable via real keypresses (see
    // module docstring). If one of them ever fires from production input,
    // log a warning so the gap is observable.
    if effect.trigger_unify.is_some()
        || effect.trigger_dry_run.is_some()
        || effect.trigger_delete_one.is_some()
        || effect.trigger_running_tool_retry.is_some()
    {
        tracing::warn!(
            target: "modeltap.interactive",
            "detail-screen effect dispatched from production loop \
             (no real-key path opens the Detail screen yet); skipping"
        );
    }
}

/// Map a `ZapOutcome` to a `LastAction` for the right-pane banner. Uses
/// the same constructors as `headless::build_last_action` — the result
/// shape is identical regardless of which loop dispatched the action.
fn build_zap_last_action(outcome: &ZapOutcome) -> LastAction {
    match outcome.outcome {
        ZapResult::Success => LastAction::for_zap_success(outcome.tool, outcome.bytes_reclaimed, 0),
        ZapResult::Partial | ZapResult::Empty | ZapResult::Failed => {
            LastAction::for_zap_failed(outcome.tool)
        }
    }
}

fn find_plugin(plugins: &[Box<dyn Tool>], tool_id: ToolId) -> Option<&dyn Tool> {
    plugins
        .iter()
        .find(|p| p.name().0 == tool_id.0)
        .map(|b| b.as_ref())
}
