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
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use modeltap_core::domain::last_action::LastAction;
use modeltap_core::{DiscoveredModel, Tool, ToolId};
use modeltap_tui::app_state::Screen;
use modeltap_tui::dialogs::delete_one_confirm::DeleteOneConfirmState;
use modeltap_tui::{
    keymap, left_pane_body_rows, right_pane_body_rows, update, view, AppState, Msg, UpdateEffect,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio_util::sync::CancellationToken;

use crate::actions::delete_one::{self, DeleteOneOutcome, DeleteOneResult};
use crate::actions::reclassify;
use crate::actions::unify::{self, UnifyOutcome, UnifyResult};
use crate::actions::zap::{self, ZapOutcome, ZapResult};
use crate::observability::{LaunchLogger, RecordKind};
use modeltap_app::hash_pool::{self, HashPoolHandle};
use modeltap_app::hash_pool_wiring::build_hash_jobs;
use modeltap_app::refresh;
use modeltap_app::sha256_cache::{Sha256Cache, Sha2Hasher};
use modeltap_core::ports::Hasher;

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
    discovered: Vec<(ToolId, Vec<DiscoveredModel>)>,
) -> io::Result<i32> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(
        &mut terminal,
        runtime,
        initial_state,
        &mut logger,
        &plugins,
        &discovered,
    );

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
    discovered: &[(ToolId, Vec<DiscoveredModel>)],
) -> io::Result<i32> {
    let mut state = initial_state;

    // Sync viewport sizes from the real terminal BEFORE the first paint so
    // the initial render and any pre-input scroll math use the actual
    // pane heights instead of the AppState defaults (28 rows).
    sync_viewport_sizes(terminal, &mut state)?;

    // Initial paint — required by US-01 AC-1 (cold start to first paint).
    // K3 / NFR-1: NO file I/O, NO pool spawn before this draw returns.
    terminal.draw(|f| view(&state, f))?;

    // ----- Step 01-08: spawn background hash pool AFTER first paint -------
    //
    // Per ADR-013: jobs are queued from the just-finished discovery; the pool
    // hashes them on a fixed worker set; results flow into `update()` via the
    // unbounded `msg_tx`/`msg_rx` channel below. The channel + pool live for
    // the lifetime of the event loop and are torn down on `Msg::Quit` /
    // `Msg::CtrlC` via `pool.shutdown()` (200 ms budget per AC-U1.5).
    let per_tool_refs: Vec<(ToolId, &[DiscoveredModel])> = discovered
        .iter()
        .map(|(t, models)| (*t, models.as_slice()))
        .collect();
    let jobs = build_hash_jobs(&per_tool_refs);
    state.hash_state.total = jobs.len() as u64;

    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
    let cache = Sha256Cache::new();
    let hasher: Arc<dyn Hasher + Send + Sync> = Arc::new(Sha2Hasher::new());
    let cancel = CancellationToken::new();
    let pool: HashPoolHandle = hash_pool::spawn(
        jobs,
        cache,
        hasher,
        msg_tx.clone(),
        cancel.clone(),
        runtime.handle(),
    );

    while !state.should_quit {
        // Drain any background hash-pool messages BEFORE blocking on input,
        // so the user sees `Hashing N/M...` progress between keystrokes.
        // `try_recv` is non-blocking — at most a single fast pass per tick.
        let mut drained_any = false;
        loop {
            match msg_rx.try_recv() {
                Ok(msg) => {
                    let (next, _eff) = update(state, msg);
                    state = next;
                    drained_any = true;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
        if drained_any {
            terminal.draw(|f| view(&state, f))?;
            if state.should_quit {
                break;
            }
        }

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

                // Sync viewport sizes BEFORE update() so compute_scroll_offset
                // sees the real terminal layout. Without this, AppState's
                // hardcoded default (28 rows) would over-estimate the visible
                // window on small terminals and the highlighted row could
                // scroll off-screen.
                sync_viewport_sizes(terminal, &mut state)?;

                let msg = translate_key(&state, key);
                let (next, effect) = update(state, msg);
                state = next;
                terminal.draw(|f| view(&state, f))?;
                apply_effect(
                    &effect, logger, plugins, runtime, &mut state, &msg_tx, discovered,
                );
                terminal.draw(|f| view(&state, f))?;
            }
            Event::Resize(_, _) => {
                // Update viewport sizes on resize so subsequent keypresses
                // (and the redraw below) see the new pane heights.
                sync_viewport_sizes(terminal, &mut state)?;
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

    // ----- Step 01-08: clean shutdown of the hash pool (AC-U1.5) ---------
    //
    // Quit budget is 500 ms total; the pool's internal join timeout is
    // 200 ms (HashPoolHandle::shutdown). The remaining 300 ms is the
    // terminal teardown headroom. Drop msg_tx FIRST so workers see EOF on
    // their channel and exit promptly; then `block_on(shutdown)` cancels
    // and joins.
    drop(msg_tx);
    runtime.block_on(async {
        let _ = pool.shutdown().await;
    });

    Ok(state.exit_code)
}

/// Sync `state.visible_rows` and `state.left_visible_rows` from the real
/// terminal layout. Call this before every `update()` dispatch (and on
/// resize / before the initial paint) so `compute_scroll_offset` sees the
/// actual rendered window heights instead of the AppState defaults. Without
/// this, on terminals shorter than 28 rows pressing Down scrolls the
/// highlighted row off-screen.
///
/// `terminal.size()` is the cheapest backend call we can make per tick;
/// it returns the cached size from the last `draw` (or queries the backend
/// on the first call). The layout helpers are pure fns over a `Rect`.
fn sync_viewport_sizes(
    terminal: &Terminal<CrosstermBackend<Stdout>>,
    state: &mut AppState,
) -> io::Result<()> {
    let size = terminal.size()?;
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    state.visible_rows = right_pane_body_rows(area);
    state.left_visible_rows = left_pane_body_rows(area);
    Ok(())
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
/// scripted token.
///
/// US-05b Shared mode: when `state.delete_one_dialog` is open in Shared mode,
/// `[y]` / `[n]` resolve the dialog directly via `Msg::DeleteOneConfirmShared`
/// / `Msg::DeleteOneCancelShared` instead of being buffered as typed input.
/// Mirrors `headless::token_to_msg`'s `delete_one_shared_open` branch — without
/// this, pressing `y` to confirm would just append `y` to the (unused) typed
/// buffer and the dialog would never close.
///
/// Main-pane delete: outside any dialog, `Msg::DeleteFromOne` is lifted to
/// `Msg::OpenDeleteOneDialog(...)` when on `Screen::Main`. The pure update
/// treats `DeleteFromOne` as a no-op; the orchestrator owns dialog
/// construction (it needs the highlighted row + cross-tool classification).
fn translate_key(state: &AppState, key: KeyEvent) -> Msg {
    // Ctrl+C must always interrupt regardless of dialog state.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Msg::CtrlC;
    }
    let delete_one_shared_open = state
        .delete_one_dialog
        .as_ref()
        .is_some_and(|d| d.is_shared());
    if delete_one_shared_open {
        return match key.code {
            KeyCode::Esc => Msg::DialogCancel,
            KeyCode::Char('y') => Msg::DeleteOneConfirmShared,
            KeyCode::Char('n') => Msg::DeleteOneCancelShared,
            _ => Msg::UnboundKey,
        };
    }
    let dialog_open = state.zap_dialog.is_some()
        || state.unify_dialog.is_some()
        || state.delete_one_dialog.is_some();
    if dialog_open {
        return keymap::dispatch_in_dialog(key, state.unify_dialog.is_some());
    }
    let raw = keymap::dispatch(key);
    lift_delete_one_in_main(state, raw)
}

/// On the main screen, lift `Msg::DeleteFromOne` (no-op in pure update) into
/// `Msg::OpenDeleteOneDialog(state)` after building a `DeleteOneConfirmState`
/// snapshot from the highlighted right-pane row. The targeted tool is the
/// currently-selected tool; the targeted model id is `model_ids[selected_row]`.
///
/// `was_shared` is classified conservatively (per ADR-002): true iff the same
/// `model_id` appears under another tool's `ToolView` (best-effort proxy for
/// cross-tool registration without a content-hash dedup index). When true,
/// the dialog opens in Shared mode (low-friction `[y/n]`); otherwise Unique
/// (typed-id confirmation). Outside the main screen, the message passes
/// through unchanged so the existing detail-screen [d] flow still works.
fn lift_delete_one_in_main(state: &AppState, msg: Msg) -> Msg {
    if !matches!(msg, Msg::DeleteFromOne) {
        return msg;
    }
    if !matches!(state.current_screen, Screen::Main) {
        return msg;
    }
    let Some(tool_view) = state.current_tool() else {
        return msg;
    };
    let Some(model_id) = tool_view.model_ids.get(state.selected_row) else {
        return msg;
    };
    let size_bytes = tool_view
        .model_sizes_bytes
        .get(state.selected_row)
        .copied()
        .unwrap_or(0);
    let target_tool = tool_view.tool;
    let target_id = model_id.clone();
    let was_shared = state
        .real_tools_iter()
        .any(|t| t.tool != target_tool && t.model_ids.iter().any(|id| id == &target_id));
    let dialog = DeleteOneConfirmState::for_model(target_tool, target_id, size_bytes, was_shared);
    Msg::OpenDeleteOneDialog(dialog)
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
    msg_tx: &tokio::sync::mpsc::UnboundedSender<Msg>,
    discovered: &[(ToolId, Vec<DiscoveredModel>)],
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

    if let Some(trigger) = effect.trigger_delete_one.clone() {
        // US-05b (ADR-009): user confirmed a single-model delete.
        //
        // Resolving on-disk path: ToolView intentionally carries only ids +
        // sizes (the right-pane summary doesn't need paths). Re-walking
        // discover() once per delete keystroke is the simplest way to recover
        // the path without reshaping ToolView. Cost is one tool's discovery
        // walk on a destructive-action keystroke — not a hot path.
        if let Some(plugin) = find_plugin(plugins, trigger.tool) {
            let path_opt = match runtime.block_on(plugin.discover()) {
                Ok(models) => models
                    .iter()
                    .find(|m| m.id_in_tool == trigger.model_id)
                    .map(|m| m.on_disk_path.clone()),
                Err(e) => {
                    tracing::warn!(
                        target: "modeltap.action.delete_one",
                        "discover failed during path resolution for {}: {e}",
                        trigger.tool.0
                    );
                    None
                }
            };
            if let Some(path) = path_opt {
                let outcome: DeleteOneOutcome = runtime.block_on(delete_one::run(
                    plugin,
                    trigger.tool,
                    trigger.model_id.clone(),
                    path,
                    trigger.size_bytes,
                    trigger.was_shared,
                    logger,
                ));
                let action = build_delete_one_last_action(&outcome);
                let (next, _) = update(std::mem::take(state), Msg::SetLastAction(action));
                *state = next;

                // US-11.AC-1 — incremental refresh after delete so the summary
                // total reflects the post-delete byte count.
                let tool_id = trigger.tool;
                match runtime.block_on(refresh::refresh_tool_incremental(plugin)) {
                    Ok(view) => {
                        let (next, _) = update(std::mem::take(state), Msg::RefreshSucceeded(view));
                        *state = next;
                    }
                    Err(refresh::RefreshError::NotInstalled) => {
                        tracing::info!(
                            target: "modeltap.refresh",
                            "refresh after delete_one: tool {} not installed",
                            tool_id.0
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "modeltap.refresh",
                            "refresh after delete_one failed for {}: {e}",
                            tool_id.0
                        );
                        let (next, _) = update(std::mem::take(state), Msg::RefreshFailed(tool_id));
                        *state = next;
                    }
                }
            } else {
                tracing::warn!(
                    target: "modeltap.action.delete_one",
                    "could not resolve on-disk path for {} in {}",
                    trigger.model_id, trigger.tool.0
                );
            }
        } else {
            tracing::warn!(
                target: "modeltap.action.delete_one",
                "no plugin for {}",
                trigger.tool.0
            );
        }
    }

    // Step 01-11 (US-U6): unify orchestration mirrors headless — when the
    // detail-screen wiring is plumbed (a future step), the production loop
    // must also reclassify-after-unify and schedule the SummaryDeltaExpired
    // timer so the same Msg::SummaryDeltaExpired arrives via msg_tx.
    if let Some(plan) = effect.trigger_unify.clone() {
        // Step 01-12 (WS activation): mirror headless's plan-path resolution.
        // The pure update layer constructs unify plans with synthetic per-row
        // paths (`/<tool>/<id_in_tool>`); the composition root resolves those
        // to real on-disk paths from the discovered inventory before invoking
        // `unify::run`. Without this, the orchestrator would call `Tool::link`
        // with synthetic targets and silently no-op.
        let plan = crate::headless::resolve_plan_paths(plan, discovered);
        let target_name = match &state.current_screen {
            Screen::Detail(d) => d.model.id.clone(),
            _ => plan
                .canonical
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("model")
                .to_string(),
        };
        let outcome: UnifyOutcome = runtime.block_on(unify::run(
            plan.clone(),
            plugins,
            logger,
            effect.cross_fs_choice,
        ));

        // Reclassify pure step BEFORE SetLastAction (per step 01-11 spec).
        // Canonical tool is passed explicitly — `actions::unify::run` omits
        // it from `tools_unified` (no link performed for the canonical
        // itself); without it, the reclassify pass would not rewrite the
        // canonical's inode entry and the dedup recompute would still see
        // distinct inodes (row glyph stays `=` instead of flipping to `#`).
        *state = reclassify::reclassify_after_unify(
            std::mem::take(state),
            &outcome,
            plan.canonical.tool,
        );

        let last_action = build_unify_last_action(&outcome, target_name);
        let (next, _) = update(std::mem::take(state), Msg::SetLastAction(last_action));
        *state = next;

        // 5s SummaryDeltaExpired timer — uses msg_tx so the production loop
        // drains it on the next tick and clears `state.summary_delta`.
        let tx_clone = msg_tx.clone();
        runtime.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let _ = tx_clone.send(Msg::SummaryDeltaExpired);
        });

        // US-11.AC-1 — refresh every participating tool slot so the summary
        // "Disk:" total reflects post-link sizes.
        for link in &plan.links {
            let tool_id = link.tool;
            if let Some(plugin) = find_plugin(plugins, tool_id) {
                match runtime.block_on(refresh::refresh_tool_incremental(plugin)) {
                    Ok(view) => {
                        let (next, _) = update(std::mem::take(state), Msg::RefreshSucceeded(view));
                        *state = next;
                    }
                    Err(refresh::RefreshError::NotInstalled) => {}
                    Err(_) => {
                        let (next, _) = update(std::mem::take(state), Msg::RefreshFailed(tool_id));
                        *state = next;
                    }
                }
            }
        }
    }

    // Forward-compatibility: dry-run / running-tool-retry are only dispatched
    // from the detail screen, which is not yet reachable via real keypresses
    // (see module docstring). If one of them ever fires from production
    // input, log a warning so the gap is observable.
    if effect.trigger_dry_run.is_some() || effect.trigger_running_tool_retry.is_some() {
        tracing::warn!(
            target: "modeltap.interactive",
            "detail-screen effect dispatched from production loop \
             (no real-key path opens the Detail screen yet); skipping"
        );
    }

    // Suppress unused-by-some-paths warning when UnifyResult is only matched
    // via the bin adapter; we still want the import to be reachable here so
    // future enhancements (e.g. surfacing AlreadyUnified specially) compile.
    let _ = std::convert::identity::<fn(UnifyResult) -> UnifyResult>(|r| r);
}

/// Map a `DeleteOneOutcome` to a `LastAction` for the right-pane banner.
/// Reuses the zap LastAction constructors because the single-model destructive
/// path produces the same banner shape — the `action.zap_one` JSONL event is
/// what distinguishes the two observability streams. Mirrors
/// `headless::build_delete_one_last_action`.
fn build_delete_one_last_action(outcome: &DeleteOneOutcome) -> LastAction {
    match outcome.outcome {
        DeleteOneResult::Success => {
            LastAction::for_zap_success(outcome.tool, outcome.bytes_reclaimed, 0)
        }
        DeleteOneResult::NotFound | DeleteOneResult::Failed => {
            LastAction::for_zap_failed(outcome.tool)
        }
    }
}

/// Map a `UnifyOutcome` to a `LastAction` for the right-pane banner. Mirrors
/// `headless::build_unify_last_action`; kept private to interactive.rs to
/// avoid cross-module coupling.
fn build_unify_last_action(outcome: &UnifyOutcome, target_name: String) -> LastAction {
    let hardlink_count = outcome.tools_unified.len();
    match outcome.outcome {
        UnifyResult::Success => {
            LastAction::for_unify_success(target_name, outcome.bytes_reclaimed, hardlink_count)
        }
        UnifyResult::AlreadyUnified => {
            LastAction::for_unify_already_unified(target_name, hardlink_count)
        }
        UnifyResult::Partial => {
            use modeltap_core::domain::last_action::TargetError;
            let failures: Vec<TargetError> = outcome
                .failures
                .iter()
                .map(|f| TargetError {
                    path: f.target.display().to_string(),
                    reason: f.reason.clone(),
                })
                .collect();
            let successes = hardlink_count as u64;
            LastAction::for_unify_partial(target_name, outcome.bytes_reclaimed, successes, failures)
        }
        UnifyResult::Failed => LastAction::for_unify_failed(target_name),
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
