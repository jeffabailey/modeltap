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
use std::path::{Path, PathBuf};
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
    cache_path: Option<PathBuf>,
    log_dir: Option<PathBuf>,
) -> io::Result<i32> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Probe terminal graphics-protocol support and pre-encode every tool
    // icon for the current protocol. Failure is non-fatal: terminals
    // without Kitty/iTerm2/Sixel/half-block support (or non-tty stdouts
    // when running under unusual harnesses) simply render the left pane
    // text-only with the icon column blank. The headless test backend
    // never reaches this code path.
    let _ = modeltap_tui::render::icons::try_init();

    let result = event_loop(
        &mut terminal,
        runtime,
        initial_state,
        &mut logger,
        &plugins,
        &discovered,
        OrchestrationPaths {
            cache_path: cache_path.as_deref(),
            log_dir: log_dir.as_deref(),
        },
    );

    // Always restore the terminal — even if the event loop returned an
    // error, the user must get their shell back. Errors during teardown
    // are swallowed (best-effort) because we are about to exit anyway and
    // the original error is more informative.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);

    result
}

/// Bundles the orchestration-side filesystem paths (cache + launch log dir)
/// that `event_loop` forwards to the tool-detail dispatcher. Grouped into a
/// borrowed struct so `event_loop`'s arg count stays within clippy's
/// `too_many_arguments` budget (7) after the step 02-01 wiring of US-21.
struct OrchestrationPaths<'a> {
    cache_path: Option<&'a Path>,
    log_dir: Option<&'a Path>,
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
    paths: OrchestrationPaths<'_>,
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
                // Step 02-01 (US-21): capture OpenToolDetail's tool_id BEFORE
                // we move `msg` into `update()` so the composition root can
                // dispatch the async tool-detail orchestrator AFTER the pure
                // update has transitioned us to `Screen::ToolDetail{..,
                // detail: None}`. `Msg::OpenToolDetail` does not flow through
                // `UpdateEffect` (the screen transition is the whole effect)
                // so this peek replaces the effect-trigger pattern.
                let open_tool_detail_id = match &msg {
                    Msg::OpenToolDetail(tool_id) => Some(*tool_id),
                    _ => None,
                };
                // Step 03-01 part 2/N (US-22): peek OpenDetail / ReintrospectModel
                // BEFORE the pure update consumes the Msg so we can dispatch
                // the async model-detail orchestrator AFTER the pure update
                // has transitioned into `Screen::Detail(state)`. Mirrors the
                // OpenToolDetail peek-then-dispatch pattern above.
                //
                // `Msg::OpenDetail` carries a fresh `DetailScreenState` (with
                // `metadata: None`) — we want the orchestrator to fill the
                // Metadata section in WarmIfCached mode.
                //
                // `Msg::ReintrospectModel` is dispatched from the [r] keymap on
                // the detail screen — we re-read the current detail state and
                // re-run the orchestrator in ForceReintrospect mode.
                let open_model_detail = match &msg {
                    Msg::OpenDetail(detail) => extract_model_detail_dispatch(
                        detail,
                        modeltap_app::orchestration::open_model_detail::RunMode::WarmIfCached,
                    ),
                    Msg::ReintrospectModel => match &state.current_screen {
                        Screen::Detail(detail) => extract_model_detail_dispatch(
                            detail,
                            modeltap_app::orchestration::open_model_detail::RunMode::ForceReintrospect,
                        ),
                        _ => None,
                    },
                    _ => None,
                };
                let (next, effect) = update(state, msg);
                state = next;
                terminal.draw(|f| view(&state, f))?;
                apply_effect(
                    &effect, logger, plugins, runtime, &mut state, &msg_tx, discovered,
                );
                if let Some(tool_id) = open_tool_detail_id {
                    dispatch_open_tool_detail(
                        runtime,
                        plugins,
                        paths.cache_path,
                        paths.log_dir,
                        &mut state,
                        tool_id,
                    );
                    terminal.draw(|f| view(&state, f))?;
                }
                if let Some((tool_id, model_id, run_mode)) = open_model_detail {
                    dispatch_open_model_detail(
                        runtime,
                        plugins,
                        paths.cache_path,
                        paths.log_dir,
                        &mut state,
                        tool_id,
                        model_id,
                        run_mode,
                    );
                    terminal.draw(|f| view(&state, f))?;
                }
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
    // US-05c AC-5 (step 02-02): thread the currently-active tool through the
    // keymap so the Shift+F guard short-circuits when a non-HF tool is
    // selected. `current_tool()` returns `None` for the synthetic [All
    // Unified] slot, which makes the guard inert there (no folder to act
    // on anyway).
    let active_tool = state.current_tool().map(|t| t.tool);
    let raw = keymap::dispatch_with_active_tool(key, state.focus, active_tool.as_ref());
    let raw = lift_delete_one_in_main(state, raw);
    lift_delete_one_in_detail(state, raw)
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

/// On the detail screen, lift `Msg::DeleteFromOne` into
/// `Msg::OpenDeleteOneDialog(state)` after building a `DeleteOneConfirmState`
/// snapshot from the screen's registrations. This is the production
/// counterpart to `headless::lift_delete_one_in_detail` (fix-delete-one-hang
/// step 01-02 / RCA Cause B).
///
/// The headless version honours the `MODELTAP_HEADLESS_DELETE_TARGET` and
/// `MODELTAP_HEADLESS_DELETE_ID_IN_TOOL` env-var seams used by the US-05b
/// acceptance suite to drive scripted scenarios. Production drops both
/// seams: it ALWAYS targets the FIRST registration and ALWAYS uses the
/// model's display id for the dialog's `model_id`. (Once the Detail screen
/// grows row-cursor navigation, the "first" choice will be replaced with
/// the highlighted registration; that is a follow-up.)
///
/// `was_shared` is computed conservatively (per ADR-002 + RCA Section 5
/// Fix 2): true iff the screen has 2+ registrations (the same model
/// content lives under another tool's tree, so deleting one preserves the
/// content elsewhere); false for single-tool registrations (typed-id
/// confirmation mode).
///
/// The `check_running_tools` gate from the headless version is intentionally
/// NOT replicated here. The gate also lives in `apply_effect` for
/// destructive operations; lifting it once at the orchestrator-effect layer
/// keeps the lift pure (no I/O on a keystroke). The follow-up that wires the
/// running-tool dialog for delete-one in production will reintroduce the
/// gate at the appropriate seam.
///
/// Outside the detail screen — or when the screen has no registrations —
/// `Msg::DeleteFromOne` passes through unchanged so `update.rs`'s no-op
/// arm absorbs it (correct: there is nothing to delete).
fn lift_delete_one_in_detail(state: &AppState, msg: Msg) -> Msg {
    if !matches!(msg, Msg::DeleteFromOne) {
        return msg;
    }
    let Screen::Detail(detail) = &state.current_screen else {
        return msg;
    };
    let Some(reg) = detail.registrations.first() else {
        return msg;
    };
    let was_shared = detail.registrations.len() >= 2;
    let size_bytes = std::fs::metadata(&reg.path)
        .map(|m| m.len())
        .unwrap_or(detail.model.canonical_size_bytes);
    let dialog =
        DeleteOneConfirmState::for_model(reg.tool, detail.model.id.clone(), size_bytes, was_shared);
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
            // Step 05-02 part 2/2: cache + cache_log_dir wired as None here;
            // step 05-04 cucumber will thread real values through the composition
            // root. None preserves v0 behaviour (K5 gate is a no-op).
            let outcome: ZapOutcome = runtime.block_on(zap::run(plugin, logger, None, None));
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
                    // Step 05-02 part 2/2 — K5 pre-mutate gate. Cache is
                    // not yet threaded into the production event loop;
                    // step 05-04 will plumb `Some(&cache)` here so the
                    // K5 gate fires on every delete-one keystroke. Until
                    // then we preserve current behaviour: no gate, no
                    // JSONL `revalidate.invoked` event.
                    None,
                    None,
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
            // Step 05-02 part 2/2 — K5 pre-mutate gate. Cache is not yet
            // threaded into the production event loop; step 05-04 will
            // plumb `Some(&cache)` here so the K5 gate fires on every
            // unify keystroke. Until then we preserve current behaviour:
            // no gate, no JSONL `revalidate.invoked` event.
            None,
            None,
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
        // Step 05-02 part 2/2: K5 gate fired. Surface as the same failure
        // banner shape for now — the JSONL event already carries `cache_stale`
        // for downstream observability. A dedicated banner copy ("cache out of
        // date — refresh and retry") lands when LastAction gains a CacheStale
        // variant in step 05-04.
        DeleteOneResult::CacheStale => LastAction::for_zap_failed(outcome.tool),
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
        // Step 05-02 part 2/2: K5 gate fired. Surface as the same failure
        // banner shape for now — the JSONL event already carries `cache_stale`
        // for downstream observability. A dedicated banner copy lands when
        // LastAction gains a CacheStale variant in step 05-04.
        UnifyResult::CacheStale => LastAction::for_unify_failed(target_name),
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
        // Step 05-02 part 2/2: K5 gate fired. Surface as the same failure
        // banner shape for now — the JSONL event already carries `cache_stale`
        // for downstream observability. A dedicated banner copy lands when
        // LastAction gains a CacheStale variant in step 05-04.
        ZapResult::CacheStale => LastAction::for_zap_failed(outcome.tool),
    }
}

fn find_plugin(plugins: &[Box<dyn Tool>], tool_id: ToolId) -> Option<&dyn Tool> {
    plugins
        .iter()
        .find(|p| p.name().0 == tool_id.0)
        .map(|b| b.as_ref())
}

/// Step 02-01 (US-21): run the tool-detail orchestrator after a
/// `Msg::OpenToolDetail(tool_id)` transitioned `Screen::ToolDetail{detail:
/// None}`. Composes the cached `CachedTool` row with `Tool::inspect_tool()`
/// into a unified `ToolDetail` and dispatches `Msg::ToolDetailReady` back
/// into `update()` so the screen leaves its loading state.
///
/// On orchestrator error (plugin not in registry, cache I/O failed) we log
/// and skip the ready-dispatch — the user sees the loading-state placeholder
/// and can Esc back. Future steps may add a `Msg::ToolDetailFailed` variant
/// to surface a richer error banner.
///
/// Mirrors `headless::dispatch_open_tool_detail` — the production loop must
/// stay byte-for-byte equivalent so users get the same screen in real
/// terminals as the acceptance suite captures.
fn dispatch_open_tool_detail(
    runtime: &tokio::runtime::Runtime,
    plugins: &[Box<dyn Tool>],
    cache_path: Option<&Path>,
    log_dir: Option<&Path>,
    state: &mut AppState,
    tool_id: ToolId,
) {
    let Some(plugin) = find_plugin(plugins, tool_id) else {
        tracing::warn!(
            target: "modeltap.tool_detail",
            "Msg::OpenToolDetail dispatched for tool {} not in plugin registry; \
             screen stays in loading state",
            tool_id.0
        );
        return;
    };
    // Step 02-03 part 2/3: panic-isolation diagnostics dir defaults to `None`
    // on the interactive path. Wiring `MODELTAP_DIAGNOSTICS_DIR` / `~/.modeltap`
    // through interactive::run is deferred — the in-TUI sentinel still renders.
    let config = modeltap_app::orchestration::open_tool_detail::OpenToolDetailConfig {
        log_dir: log_dir.map(|p| p.to_path_buf()),
        diagnostics_dir: None,
    };
    match runtime.block_on(modeltap_app::orchestration::open_tool_detail::run(
        tool_id, plugin, cache_path, &config,
    )) {
        Ok(detail) => {
            let (next, _eff) = update(
                std::mem::take(state),
                Msg::ToolDetailReady(Box::new(detail)),
            );
            *state = next;
        }
        Err(e) => {
            tracing::warn!(
                target: "modeltap.tool_detail",
                "open_tool_detail orchestration failed for {}: {e}",
                tool_id.0
            );
        }
    }
}

/// Step 03-01 part 2/N (US-22): extract (tool_id, model_id, run_mode) from a
/// `DetailScreenState` so the composition root can dispatch the model-detail
/// orchestrator. The tool_id is read from the FIRST `DetailRegistration` —
/// the orchestrator only needs one plugin to consult for `inspect_model()`
/// and the AC-22 cache writeback is keyed on (tool_id, model_id).
///
/// Returns `None` when there are no registrations (a synthetic / empty
/// detail row); the orchestrator dispatch is skipped and the screen renders
/// without a Metadata section (graceful degradation per AC-22-4 fallback).
///
/// Pure (no I/O, no AppState mutation) so both interactive.rs and
/// headless.rs can call it identically — bit-for-bit equivalent metadata
/// rendering across real terminals and the acceptance harness.
fn extract_model_detail_dispatch(
    detail: &modeltap_tui::screens::detail::DetailScreenState,
    run_mode: modeltap_app::orchestration::open_model_detail::RunMode,
) -> Option<(
    ToolId,
    modeltap_core::domain::inspect::ModelId,
    modeltap_app::orchestration::open_model_detail::RunMode,
)> {
    let first_reg = detail.registrations.first()?;
    let model_id = modeltap_core::domain::inspect::ModelId(detail.model.id.clone());
    Some((first_reg.tool, model_id, run_mode))
}

/// Step 03-01 part 2/N (US-22): run the model-detail orchestrator after a
/// `Msg::OpenDetail(_)` (or `Msg::ReintrospectModel` re-issue) transitioned
/// us into `Screen::Detail(state)`. Composes the cached `CachedModel` row
/// with `Tool::inspect_model()` into a `ModelDetail`, then dispatches
/// `Msg::ModelDetailReady(Box::new(MetadataSection))` back into `update()`
/// so the screen's Metadata section paints per AC-22-4.
///
/// On orchestrator error (plugin not in registry, cache I/O failed) we log
/// and skip the ready-dispatch — the detail screen renders WITHOUT the
/// Metadata section (legacy US-13 path). Future steps may add a
/// `Msg::ModelDetailFailed` variant to surface a richer error banner.
///
/// Mirrors `headless::dispatch_open_model_detail` — the production loop and
/// the acceptance harness produce byte-identical metadata renders for the
/// US-22 acceptance assertions.
#[allow(clippy::too_many_arguments)]
fn dispatch_open_model_detail(
    runtime: &tokio::runtime::Runtime,
    plugins: &[Box<dyn Tool>],
    cache_path: Option<&Path>,
    log_dir: Option<&Path>,
    state: &mut AppState,
    tool_id: ToolId,
    model_id: modeltap_core::domain::inspect::ModelId,
    run_mode: modeltap_app::orchestration::open_model_detail::RunMode,
) {
    let Some(plugin) = find_plugin(plugins, tool_id) else {
        tracing::warn!(
            target: "modeltap.model_detail",
            "Msg::OpenDetail dispatched for tool {} not in plugin registry; \
             detail screen renders without metadata",
            tool_id.0
        );
        return;
    };
    // Step 02-03 part 2/3 parity: panic-isolation diagnostics dir defaults
    // to `None` on the interactive path. Wiring `MODELTAP_DIAGNOSTICS_DIR`
    // / `~/.modeltap` through interactive::run is deferred — the in-TUI
    // sentinel still renders.
    let config = modeltap_app::orchestration::open_model_detail::OpenModelDetailConfig {
        log_dir: log_dir.map(|p| p.to_path_buf()),
        diagnostics_dir: None,
    };
    match runtime.block_on(modeltap_app::orchestration::open_model_detail::run(
        tool_id,
        model_id.clone(),
        plugin,
        cache_path,
        &config,
        run_mode,
    )) {
        Ok(detail) => {
            let metadata = modeltap_tui::screens::detail::MetadataSection {
                kv: detail.metadata_kv,
                source: tool_id.0.to_string(),
                introspected_at: detail.introspected_at,
            };
            let (next, _eff) = update(
                std::mem::take(state),
                Msg::ModelDetailReady(Box::new(metadata)),
            );
            *state = next;
        }
        Err(e) => {
            tracing::warn!(
                target: "modeltap.model_detail",
                "open_model_detail orchestration failed for tool={} model={}: {e}",
                tool_id.0, model_id
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
//
// Tests live inline (rather than under `tests/`) because `interactive` is a
// `mod` private to the binary at `src/main.rs`; surfacing it to a `tests/`
// integration target would require restructuring `lib.rs` + `main.rs` to
// promote `interactive`, `actions`, and `observability` into the library
// half. The fix-delete-one-hang step 01-02 boundary keeps the change
// surgical, and the existing precedent in `headless.rs` (private bin module
// with inline `#[cfg(test)] mod tests`) is followed here.
#[cfg(test)]
mod tests {
    //! Unit tests for `lift_delete_one_in_detail` (RCA Cause B fix).
    //!
    //! Production fix for fix-delete-one-hang step 01-02. The headless
    //! harness at `headless::lift_delete_one_in_detail` was the only place
    //! a Detail-screen 'd' keypress was lifted into
    //! `Msg::OpenDeleteOneDialog`. The production interactive event loop's
    //! `translate_key` only chained `lift_delete_one_in_main`, so pressing
    //! 'd' on the Detail screen fell through to `update.rs::Msg::DeleteFromOne`'s
    //! no-op arm — silently doing nothing.
    //!
    //! These tests pin the contract for the new
    //! `lift_delete_one_in_detail`:
    //!
    //! 1. On `Screen::Detail` with non-empty `registrations`,
    //!    `Msg::DeleteFromOne` is lifted into `Msg::OpenDeleteOneDialog(_)`.
    //! 2. On `Screen::Detail` with EMPTY `registrations`, the message
    //!    passes through unchanged (no model to delete; the lift is a
    //!    no-op rather than a panic).
    //! 3. On `Screen::Main`, the message passes through unchanged
    //!    (`lift_delete_one_in_main` handles that path; this lift is a
    //!    Detail-only concern).

    use std::path::PathBuf;

    use modeltap_core::logic::unification_status::{DetailModelView, DetailRegistration};
    use modeltap_core::{DisplayLabel, Format, ModelStatus};
    use modeltap_tui::screens::detail::DetailScreenState;

    use super::*;

    fn detail_state_with_two_regs() -> DetailScreenState {
        let registrations = vec![
            DetailRegistration {
                tool: ToolId("ollama"),
                path: PathBuf::from("/var/empty/ollama/blobs/sha256-aaa"),
                inode: Some(1001),
            },
            DetailRegistration {
                tool: ToolId("hf"),
                path: PathBuf::from("/var/empty/hf/foo/model.safetensors"),
                inode: Some(1002),
            },
        ];
        DetailScreenState::new(
            DetailModelView {
                id: "mistralai/Mistral-7B-v0.3".to_string(),
                format: Format::Gguf,
                format_quant: Some("q4_K_M".to_string()),
                canonical_size_bytes: 4_400_000_000,
                display_label: DisplayLabel::from("mistralai/Mistral-7B-v0.3"),
                status: ModelStatus::Healthy,
            },
            registrations,
            None,
        )
    }

    fn detail_state_with_no_regs() -> DetailScreenState {
        DetailScreenState::new(
            DetailModelView {
                id: "ghost-model".to_string(),
                format: Format::Gguf,
                format_quant: None,
                canonical_size_bytes: 0,
                display_label: DisplayLabel::from("ghost-model"),
                status: ModelStatus::Healthy,
            },
            vec![],
            None,
        )
    }

    fn empty_main_state() -> AppState {
        AppState::new_with_default_selection(vec![])
    }

    #[test]
    fn lift_delete_one_in_detail_opens_dialog_when_detail_has_registrations() {
        let mut state = empty_main_state();
        state.current_screen = Screen::Detail(detail_state_with_two_regs());

        let result = lift_delete_one_in_detail(&state, Msg::DeleteFromOne);

        match result {
            Msg::OpenDeleteOneDialog(dialog) => {
                // Target the FIRST registration (ollama) per the production
                // rule documented in step 01-02 implementation notes.
                assert_eq!(
                    dialog.tool,
                    ToolId("ollama"),
                    "lift must target the first registration's tool"
                );
                assert_eq!(
                    dialog.model_id, "mistralai/Mistral-7B-v0.3",
                    "lift must use the model's display id (no env-var override in production)"
                );
                // Two registrations → was_shared = true → Shared mode dialog.
                assert!(
                    dialog.is_shared(),
                    "two registrations means was_shared=true (Shared/[y/n] mode)"
                );
            }
            other => {
                panic!("expected Msg::OpenDeleteOneDialog(_) on Detail with regs; got {other:?}")
            }
        }
    }

    #[test]
    fn lift_delete_one_in_detail_passes_msg_unchanged_when_detail_has_empty_registrations() {
        let mut state = empty_main_state();
        state.current_screen = Screen::Detail(detail_state_with_no_regs());

        let result = lift_delete_one_in_detail(&state, Msg::DeleteFromOne);

        assert!(
            matches!(result, Msg::DeleteFromOne),
            "empty registrations → no model to delete → pass through unchanged; got {result:?}"
        );
    }

    #[test]
    fn lift_delete_one_in_detail_passes_msg_unchanged_when_screen_is_main() {
        let state = empty_main_state(); // current_screen defaults to Main

        let result = lift_delete_one_in_detail(&state, Msg::DeleteFromOne);

        assert!(
            matches!(result, Msg::DeleteFromOne),
            "Main screen is handled by lift_delete_one_in_main; this lift must \
             pass the message through unchanged. got {result:?}"
        );
    }

    #[test]
    fn lift_delete_one_in_detail_passes_non_delete_msg_unchanged() {
        let mut state = empty_main_state();
        state.current_screen = Screen::Detail(detail_state_with_two_regs());

        let result = lift_delete_one_in_detail(&state, Msg::ToggleHelp);

        assert!(
            matches!(result, Msg::ToggleHelp),
            "the lift only acts on Msg::DeleteFromOne; other msgs pass through. got {result:?}"
        );
    }
}
