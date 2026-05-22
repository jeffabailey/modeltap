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
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::Arc;

use modeltap_core::domain::last_action::LastAction;
use modeltap_core::logic::canonical_selector::{select_canonical, CandidatePath};
use modeltap_core::logic::plan::{build_plan, PlanCandidate, UnifyPlan};
use modeltap_core::ports::fs_probe::{FsProbe, ProbeError};
use modeltap_core::ports::Hasher;
use modeltap_core::{DiscoveredModel, Tool, ToolId};
use modeltap_tui::app_state::{FocusPane, Screen};
use modeltap_tui::dialogs::delete_one_confirm::DeleteOneConfirmState;
use modeltap_tui::dialogs::running_tool_prompt::{PendingGatedAction, RunningToolDialog};
use modeltap_tui::dialogs::unify_confirm::UnifyMode;
use modeltap_tui::screens::detail::DetailScreenState;
use modeltap_tui::{update, view, AppState, Msg, UpdateEffect};

use modeltap_app::hash_pool::{self, HashPoolHandle};
use modeltap_app::hash_pool_wiring::build_hash_jobs;
use modeltap_app::lsof_adapter::LsofAdapter;
use modeltap_app::sha256_cache::{Sha256Cache, Sha2Hasher};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use tokio_util::sync::CancellationToken;

use crate::actions::delete_one::{self, DeleteOneOutcome};
use crate::actions::folder_delete::{
    self, FolderDeleteOutcome, FolderDeleteResult, SidecarEnumerator,
};
use crate::actions::reclassify;
use crate::actions::unify::{self, DryRunOutcome, UnifyOutcome, UnifyResult};
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
    /// Resolved absolute path to `cache.sqlite` when `MODELTAP_CACHE_PATH`
    /// was set at startup (and `--no-cache` was NOT passed), else `None`.
    /// Threaded down to the tool-detail orchestrator (step 02-01) so the
    /// cache half of the merge can run; `None` skips the cache read and
    /// falls back to `inspect_tool()` alone.
    pub cache_path: Option<PathBuf>,
    /// `MODELTAP_LOG_DIR` if set, else `None`. Threaded to the tool-detail
    /// orchestrator so the `tool_detail.open_ms` JSONL event lands next to
    /// `launch.log` / `models.log`.
    pub log_dir: Option<PathBuf>,
    /// Step 02-03 (US-21 AC-21-9 / INT-INFO-8): directory under which
    /// `diagnostics.log` is written when a plugin's `inspect_tool` panics.
    /// Resolved from `MODELTAP_DIAGNOSTICS_DIR` (test override) or
    /// `~/.modeltap` (production). `None` disables on-disk panic logging
    /// without changing the in-TUI panic surface (the sentinel still
    /// renders).
    pub diagnostics_dir: Option<PathBuf>,
}

/// Run the headless event loop. Returns the process exit code.
pub fn run(
    config: HeadlessConfig,
    initial_state: AppState,
    mut logger: LaunchLogger,
    plugins: Vec<Box<dyn Tool>>,
    discovered: Vec<(ToolId, Vec<DiscoveredModel>)>,
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
    // K3 / NFR-1: NO file I/O, NO pool spawn before this draw returns.
    if let Err(e) = terminal.draw(|f| view(&state, f)) {
        eprintln!("modeltap: initial paint failed: {e}");
        return 1;
    }

    // Tokens parsed up-front (script tokens are independent of state); we
    // resolve each token to the right Msg per-iteration based on whether a
    // dialog is open at that moment.
    let tokens = tokenize_script(&config.input);
    let token_count = tokens.len();

    // Construct a multi-thread tokio runtime so the hash pool's worker tasks
    // and queue-pusher task make progress while the script driver loop is
    // synchronously waiting on `<hash-complete>` (no `block_on` is in flight).
    // A `current_thread` runtime would only drive tasks while
    // `runtime.block_on(...)` is active, which would deadlock the
    // `<hash-complete>` sentinel: the loop polls `try_recv` + `thread::sleep`
    // and never enters the runtime, so workers never run.
    //
    // Production (interactive.rs) uses `new_multi_thread` for the same
    // reason; the headless harness now mirrors that choice. The K3 paint
    // path still skips spawning the pool when the script is empty
    // (`spawn_pool` flag below), so the runtime cost is paid only when
    // there's actual work.
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("modeltap: failed to construct tokio runtime: {e}");
            return 1;
        }
    };

    // ----- Step 01-08: spawn background hash pool AFTER first paint -------
    //
    // The `--quit-after-paint` mode (K3 / launch-timing benchmark) exits
    // immediately after one paint; spawning a pool there would just be wasted
    // work because the channel is never drained. Skip the spawn in that case.
    let spawn_pool = !(config.quit_after_paint && tokens.is_empty());
    let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
    let cancel = CancellationToken::new();
    let pool: Option<HashPoolHandle> = if spawn_pool {
        let per_tool_refs: Vec<(ToolId, &[DiscoveredModel])> = discovered
            .iter()
            .map(|(t, models)| (*t, models.as_slice()))
            .collect();
        let jobs = build_hash_jobs(&per_tool_refs);
        state.hash_state.total = jobs.len() as u64;

        let cache = Sha256Cache::new();
        let hasher: Arc<dyn Hasher + Send + Sync> = Arc::new(Sha2Hasher::new());
        Some(hash_pool::spawn(
            jobs,
            cache,
            hasher,
            msg_tx.clone(),
            cancel.clone(),
            rt.handle(),
        ))
    } else {
        None
    };

    for (idx, token) in tokens.iter().enumerate() {
        // Drain any background hash-pool messages BEFORE the next scripted
        // token so the per-iteration captured frame reflects the latest
        // `Hashing N/M...` progress. `try_recv` is non-blocking; the test
        // harness's deterministic-frame contract is preserved (no waiting,
        // just consume what's already arrived).
        loop {
            match msg_rx.try_recv() {
                Ok(msg) => {
                    let (next, _eff) = update(state, msg);
                    state = next;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
        // <hash-complete> sentinel (step 01-09): block this iteration until
        // the hash pool reports completion (or we observe should_quit).
        // Drains msg_rx during the wait so HashComputed / HashFailed /
        // HashProgressTick messages are applied as they arrive. No new
        // env-var seam — this is a pure script-grammar sync point so
        // acceptance tests can deterministically observe post-hash state
        // without sleep-based polling.
        if let ScriptToken::Tag(t) = token {
            if t == "hash-complete" {
                // Bounded wait — total + a small safety margin. The walking-
                // skeleton fixture is small (~4 KB); production installs are
                // larger but acceptance tests use synthetic blobs that hash
                // well under this budget.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                while !state.hash_state.is_complete()
                    && std::time::Instant::now() < deadline
                    && !state.should_quit
                {
                    // Drain any pending messages (advances completed counter).
                    loop {
                        match msg_rx.try_recv() {
                            Ok(msg) => {
                                let (next, _eff) = update(state, msg);
                                state = next;
                            }
                            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                        }
                    }
                    // If still not complete, yield briefly so workers can make
                    // progress without spinlock-heating the CPU. 5ms cadence
                    // gives 200 polls/second — plenty for the 250ms throttle.
                    if !state.hash_state.is_complete() {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
                // After waiting (success OR timeout): repaint to surface the
                // post-hashing state in the next captured frame.
                if let Err(e) = terminal.draw(|f| view(&state, f)) {
                    eprintln!("modeltap: <hash-complete> redraw failed: {e}");
                    return 1;
                }
                continue; // skip to next script token
            }
            // US-05c walking-skeleton (step 01-05): `<folder-delete>` sentinel
            // dispatches the folder-group bulk-delete orchestration directly,
            // bypassing the (not-yet-implemented) production confirmation
            // dialog. The targeted folder is read from
            // `MODELTAP_HEADLESS_FOLDER_PATH` (same env-var seam pattern as
            // `MODELTAP_HEADLESS_DETAIL_REGS` for US-10 / US-05b).
            if t == "folder-delete" {
                let folder_path = match std::env::var("MODELTAP_HEADLESS_FOLDER_PATH") {
                    Ok(p) if !p.is_empty() => p,
                    _ => {
                        eprintln!(
                            "modeltap: <folder-delete> requires \
                             MODELTAP_HEADLESS_FOLDER_PATH"
                        );
                        return 1;
                    }
                };
                // Step 04-03: pre-flight refusal gates per F-FGD-8 / ADR-010.
                // Both checks happen BEFORE any confirmation-state computation
                // so the user sees no dialog frame for the refusal path.
                //
                //   1. AC-15 — cache-writeable check: `<HF_HOME>/hub/` must be
                //      writeable (probe-and-remove). If false, dispatch
                //      `run_preflight_refused(ReadonlyCache, …)`.
                //   2. AC-20 — folder-still-exists check: the targeted
                //      `models--<author>--<repo>/` subtree must still be on
                //      disk. If false (out-of-band remove between launch and
                //      Shift+F), dispatch `run_preflight_refused(FolderMissing,
                //      …)` and signal that the inventory will refresh.
                //
                // Both paths short-circuit BEFORE any cancel-reason logic so
                // the JSONL event carries `outcome=refused_*` rather than
                // `cancelled_*` even when the user's typed input would also
                // mismatch.
                let hub_root = modeltap_plugin_hf::discover::resolve_hub_root();
                let preflight_refusal: Option<folder_delete::PreflightRefusal> =
                    if !folder_delete::is_cache_writable(&hub_root) {
                        Some(folder_delete::PreflightRefusal::ReadonlyCache)
                    } else if let Some((author, repo_name)) = folder_path.split_once('/') {
                        let repo_dir = hub_root.join(format!("models--{author}--{repo_name}"));
                        if !repo_dir.exists() {
                            Some(folder_delete::PreflightRefusal::FolderMissing)
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                if let Some(refusal) = preflight_refusal {
                    let outcome = folder_delete::run_preflight_refused(
                        ToolId("hf"),
                        folder_path.clone(),
                        refusal,
                        &mut logger,
                    );
                    let last_action = build_folder_delete_last_action(&outcome);
                    let (next, _) =
                        update(std::mem::take(&mut state), Msg::SetLastAction(last_action));
                    state = next;
                    if let Err(e) = terminal.draw(|f| view(&state, f)) {
                        eprintln!("modeltap: <folder-delete> redraw failed: {e}");
                        return 1;
                    }
                    continue;
                }

                // Step 02-01: M2 confirmation-safety seam. The headless
                // harness simulates the dialog state machine by replaying
                // the user's typed input + decision through
                // `FolderConfirmState::decide_on_*`. When the decision is
                // Confirm, dispatch the WS happy-path (folder_delete::run).
                // When CancelMismatch / CancelEscape, dispatch the cancel-
                // and-emit path (folder_delete::run_cancelled) so the JSONL
                // event carries `outcome=cancelled_mismatch` /
                // `outcome=cancelled_escape` with `outcomes_count=0` and the
                // plugin is never called (fixture stays byte-identical).
                let typed_input = std::env::var("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT")
                    .unwrap_or_else(|_| folder_path.clone());
                let decision_mode = std::env::var("MODELTAP_HEADLESS_FOLDER_DECISION_MODE")
                    .unwrap_or_else(|_| "confirm".to_string());
                let cancel_reason: Option<folder_delete::CancelReason> =
                    match decision_mode.as_str() {
                        "esc" => Some(folder_delete::CancelReason::Escape),
                        "enter" | "confirm" => {
                            if typed_input == folder_path {
                                None
                            } else {
                                Some(folder_delete::CancelReason::Mismatch)
                            }
                        }
                        other => {
                            eprintln!(
                            "modeltap: unknown MODELTAP_HEADLESS_FOLDER_DECISION_MODE={other:?}"
                        );
                            return 1;
                        }
                    };

                // Step 06-01 (K-FGD-2 / D3): derive the keystroke_count from
                // the typed input + the final decision keystroke (Enter or
                // Esc). Each char in `typed_input` corresponds to exactly one
                // `FolderConfirmState::handle_char` call in the production
                // dialog state machine; the +1 is the terminal Enter or Esc
                // that resolves the dialog. Shift+F is excluded because it
                // never reached the dialog (it OPENED the dialog from main
                // view). This matches the production counter's semantics for
                // a user who types each character exactly once without
                // Backspace or Ctrl+W corrections — the no-correction lower
                // bound that the M6 keystroke <= 40 budget targets.
                let keystroke_count: u64 = (typed_input.chars().count() as u64).saturating_add(1);

                if let Some(cancel) = cancel_reason {
                    let outcome = folder_delete::run_cancelled(
                        ToolId("hf"),
                        folder_path,
                        cancel,
                        keystroke_count,
                        &mut logger,
                    );
                    let last_action = build_folder_delete_last_action(&outcome);
                    let (next, _) =
                        update(std::mem::take(&mut state), Msg::SetLastAction(last_action));
                    state = next;
                } else if let Some(plugin) = find_plugin(&plugins, ToolId("hf")) {
                    // `hub_root` declared above as part of the pre-flight check
                    // is re-used here for the happy-path dispatch.
                    let enumerator = HfSidecarEnumerator;
                    let outcome = rt.block_on(folder_delete::run(
                        plugin,
                        ToolId("hf"),
                        folder_path,
                        &hub_root,
                        &enumerator,
                        keystroke_count,
                        &mut logger,
                    ));
                    let last_action = build_folder_delete_last_action(&outcome);
                    let (next, _) =
                        update(std::mem::take(&mut state), Msg::SetLastAction(last_action));
                    state = next;
                } else {
                    tracing::warn!(
                        target: "modeltap.action.folder_delete",
                        "no hf plugin available; <folder-delete> is a no-op"
                    );
                }
                if let Err(e) = terminal.draw(|f| view(&state, f)) {
                    eprintln!("modeltap: <folder-delete> redraw failed: {e}");
                    return 1;
                }
                continue;
            }
        }
        let dialog_open = state.zap_dialog.is_some()
            || state.unify_dialog.is_some()
            || state.delete_one_dialog.is_some();
        let cross_fs_open = state.cross_fs_dialog.is_some();
        let unify_open = state.unify_dialog.is_some();
        let delete_one_shared_open = state
            .delete_one_dialog
            .as_ref()
            .is_some_and(|d| d.is_shared());
        let raw_msg = token_to_msg(
            token,
            dialog_open,
            cross_fs_open,
            unify_open,
            delete_one_shared_open,
            state.focus,
        );
        // Intercept Msg::Unify on the detail screen so we can build the
        // UnifyPlan from the registrations + plugins (the plan needs `stat`
        // results, which the pure update() can't compute). Outside the
        // detail screen, `Msg::Unify` stays a no-op per the keymap docs.
        // US-14: peek at the next token — if it is `n`, the user wants
        // dry-run preview, so even with cross-fs targets we must open the
        // unify dialog (read-only preview path).
        let next_is_dry_run = matches!(tokens.get(idx + 1), Some(ScriptToken::Char('n')));
        let msg = lift_unify_in_detail(&state, raw_msg, next_is_dry_run);
        // US-05b (step 03-06): intercept Msg::DeleteFromOne on the detail
        // screen and build a DeleteOneConfirmState targeting the registration
        // identified by `MODELTAP_HEADLESS_DELETE_TARGET` (or the first
        // registration when unset). Outside the detail screen, the message
        // stays a no-op.
        let msg = lift_delete_one_in_detail(&state, msg);
        // US-U5: rewrite the placeholder `Msg::ToggleTarget(0)` produced by
        // `token_to_msg` for `<space>` while a unify dialog is open into
        // `Msg::ToggleTarget(selected_target_idx)` so the toggle hits the
        // currently-cursored row.
        let msg = lift_toggle_in_unify_dialog(&state, msg);
        // Intercept Enter on the main screen so we can open the detail
        // screen with synthesized registrations from the AppState.
        let msg = lift_enter_in_main(&state, msg);
        // Step 02-01 (US-21): when MODELTAP_HEADLESS_TOOL_DETAIL=1 is set,
        // lift `Msg::ToggleFolderExpansion` (the production Enter dispatch
        // on Main) into `Msg::OpenToolDetail(selected_tool_id)`. The
        // env-var gate prevents this from clashing with existing scenarios
        // that rely on Enter doing folder expand/collapse OR opening the
        // model detail (US-10 path, which uses MODELTAP_HEADLESS_DETAIL_REGS).
        let msg = lift_enter_in_left_pane_to_tool_detail(&state, msg);
        // Step 02-01: capture OpenToolDetail's tool_id BEFORE moving `msg`
        // into `update()` so we can run the async orchestrator AFTER the
        // pure update has transitioned us into `Screen::ToolDetail{detail:
        // None}`. Mirrors interactive.rs::run's peek-then-dispatch pattern.
        let open_tool_detail_id = match &msg {
            Msg::OpenToolDetail(tool_id) => Some(*tool_id),
            _ => None,
        };
        // Step 03-01 part 2/N (US-22): peek OpenDetail / ReintrospectModel
        // BEFORE the pure update consumes the Msg so we can dispatch the
        // async model-detail orchestrator AFTER the pure update has
        // transitioned into `Screen::Detail(state)`. Mirrors the
        // OpenToolDetail peek-then-dispatch pattern above and the same
        // peek block in interactive.rs::event_loop — the headless harness
        // must stay bit-for-bit equivalent so US-22 acceptance assertions
        // capture identical frames in real terminals.
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
        if let Err(e) = terminal.draw(|f| view(&state, f)) {
            eprintln!("modeltap: redraw failed: {e}");
            return 1;
        }
        apply_effect(
            &effect,
            &mut logger,
            &plugins,
            &rt,
            &mut state,
            Some(&msg_tx),
            &discovered,
        );
        // Step 02-01 (US-21): run the tool-detail orchestrator now that the
        // pure update transitioned us into `Screen::ToolDetail{detail: None}`.
        // Skipped silently when no plugin matches (logged via tracing).
        if let Some(tool_id) = open_tool_detail_id {
            dispatch_open_tool_detail(
                &rt,
                &plugins,
                config.cache_path.as_deref(),
                config.log_dir.as_deref(),
                config.diagnostics_dir.as_deref(),
                &mut state,
                tool_id,
            );
            if let Err(e) = terminal.draw(|f| view(&state, f)) {
                eprintln!("modeltap: tool-detail redraw failed: {e}");
                return 1;
            }
            // Frame-capture seam: the tool-detail screen is opened
            // synchronously by `Msg::OpenToolDetail` and populated
            // synchronously by `Msg::ToolDetailReady` (the orchestrator
            // resolves in the same iteration). Print THIS frame so
            // acceptance assertions can substring-match the rendered
            // detail fields BEFORE the next iteration's `<esc>` closes
            // the screen. Same pattern as the dry-run / running-tool /
            // already-unified / confirm frame-capture seams below.
            print_frame(&terminal);
        }
        // Step 03-01 part 2/N (US-22): run the model-detail orchestrator
        // now that the pure update transitioned us into `Screen::Detail(_)`.
        // Skipped silently when no plugin matches the first registration's
        // tool_id (logged via tracing). Mirrors the interactive.rs dispatch
        // block above so US-22 acceptance frames render identically across
        // the real terminal and the test backend.
        if let Some((tool_id, model_id, run_mode)) = open_model_detail {
            dispatch_open_model_detail(
                &rt,
                &plugins,
                config.cache_path.as_deref(),
                config.log_dir.as_deref(),
                config.diagnostics_dir.as_deref(),
                &mut state,
                tool_id,
                model_id,
                run_mode,
            );
            if let Err(e) = terminal.draw(|f| view(&state, f)) {
                eprintln!("modeltap: model-detail redraw failed: {e}");
                return 1;
            }
            // Frame-capture seam (mirrors tool-detail above): the model-
            // detail Metadata section is populated synchronously by
            // `Msg::ModelDetailReady` in the same iteration that opened
            // the screen. Print THIS frame so US-22 acceptance assertions
            // see the rendered metadata BEFORE the next iteration's <esc>
            // closes the screen.
            print_frame(&terminal);
        }
        // US-14 frame-capture seam: when `apply_effect` dispatched
        // `UnifyDryRunCompleted`, the unify dialog just transitioned into
        // `UnifyMode::DryRunPreview { lines }`. The next iteration's `<esc>`
        // will close the dialog and the FINAL captured frame will no longer
        // show the preview. Paint+print THIS post-effect frame so US-14 AC-2
        // / AC-3 (frame must contain "(dry-run) Would..." / "WARNING") can
        // assert against the transient overlay. Gated by mode so non-dry-run
        // effects (zap, real unify) keep their existing single-final-frame
        // capture contract — preserving the negative assertions in
        // us_06::last_action_message_clears_when_devon_navigates.
        let dry_run_visible = state
            .unify_dialog
            .as_ref()
            .is_some_and(|d| matches!(d.mode, UnifyMode::DryRunPreview { .. }));
        if dry_run_visible {
            if let Err(e) = terminal.draw(|f| view(&state, f)) {
                eprintln!("modeltap: dry-run preview redraw failed: {e}");
                return 1;
            }
            print_frame(&terminal);
        }
        // US-17 (intake Q5; step 03-07) running-tool dialog frame-capture
        // seam. The dialog is OPENED transiently between the user's gated
        // keystroke (u/d) and their dismissal (<esc> / [r] retry). Without
        // this capture, the final frame would never show the dialog text and
        // the AC-2 / AC-3 assertions ("running"/"close"/"retry" /
        // "Running-tool detection unavailable") would fail. Same pattern as
        // the dry-run capture above.
        if state.running_tool_dialog.is_some() {
            if let Err(e) = terminal.draw(|f| view(&state, f)) {
                eprintln!("modeltap: running-tool dialog redraw failed: {e}");
                return 1;
            }
            print_frame(&terminal);
        }
        // US-U4 (step 03-01) AlreadyUnified informational dialog frame-
        // capture seam. Pressing `u` on a `#` row opens the unify dialog in
        // `UnifyMode::AlreadyUnified`; per `decide_on_enter`, the very next
        // `<enter>` Cancels and closes it. Without this capture the FINAL
        // frame would no longer show the informational text and AC-U4.3
        // ("frame must contain 'already unified'") would fail. Same pattern
        // as the dry-run / running-tool seams above.
        let already_unified_visible = state
            .unify_dialog
            .as_ref()
            .is_some_and(|d| matches!(d.mode, UnifyMode::AlreadyUnified));
        if already_unified_visible {
            if let Err(e) = terminal.draw(|f| view(&state, f)) {
                eprintln!("modeltap: already-unified dialog redraw failed: {e}");
                return 1;
            }
            print_frame(&terminal);
        }
        // US-U5 (step 03-02) Confirm-mode unify dialog frame-capture seam.
        // The dialog opens on `u` and is closed by `<enter>` (apply) or
        // `<esc>` (cancel). Without this capture the FINAL frame (post-quit)
        // would never show the reclaim-preview body and the AC-U5.1
        // assertions ("Total reclaim:", "[Enter] Apply", "[space] Toggle")
        // would fail. Same pattern as the dry-run / running-tool /
        // already-unified seams above.
        let confirm_visible = state
            .unify_dialog
            .as_ref()
            .is_some_and(|d| matches!(d.mode, UnifyMode::Confirm));
        if confirm_visible {
            if let Err(e) = terminal.draw(|f| view(&state, f)) {
                eprintln!("modeltap: unify-confirm dialog redraw failed: {e}");
                return 1;
            }
            print_frame(&terminal);
        }
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

    // ----- Step 01-08: clean shutdown of the hash pool (AC-U1.5) ---------
    //
    // Drop msg_tx so workers see EOF on their channel and exit promptly;
    // then `block_on(shutdown)` cancels and joins with the 200 ms internal
    // budget. The 500 ms quit envelope (AC-U1.5) is comfortably preserved.
    drop(msg_tx);
    if let Some(pool) = pool {
        rt.block_on(async {
            let _ = pool.shutdown().await;
        });
    } else {
        // No pool to shut down (quit-after-paint fast path) — but the
        // CancellationToken is still owned here; cancel for symmetry.
        cancel.cancel();
    }

    // The pool's `CancellationToken` cannot abort in-flight `spawn_blocking`
    // SHA256 jobs (they are CPU-bound, not async). On large fixtures (e.g.,
    // 12.8 GB devon-multi-tool) those jobs can run for tens of seconds. The
    // default `Runtime::drop` blocks until every blocking task drains, which
    // would exceed the AC-U1.5 quit envelope and stall acceptance tests.
    //
    // Replace the implicit drop with `shutdown_timeout(300 ms)` so the
    // remaining quit budget (after the 200 ms pool join) is bounded; any
    // still-running blocking thread is detached and the process exits.
    let summary = serde_json::json!({
        "schema": "modeltap.session_summary.v1",
        "frames_captured": 1 + token_count,
        "exit_reason": exit_reason(&state),
        "exit_code": state.exit_code,
        "log_path": logger.path().map(|p| p.display().to_string()),
    });
    println!("{}", summary);

    let exit_code = state.exit_code;
    rt.shutdown_timeout(std::time::Duration::from_millis(300));
    exit_code
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
    msg_tx: Option<&tokio::sync::mpsc::UnboundedSender<Msg>>,
    discovered: &[(ToolId, Vec<DiscoveredModel>)],
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
            // O(N plugins) and would break the budget once HF/Loose GGUFs
            // populate). Failures dispatch `Msg::RefreshFailed(tool_id)` so
            // the UI shows `(refresh failed)` + [r] retry per US-11.AC-2.
            //
            // Test-only seam: `MODELTAP_FORCE_REFRESH_FAIL=<tool_id>` forces
            // refresh_tool_incremental to return Err(Unreadable) for the
            // matching tool. Production paths never set this env-var.
            let forced_fail = std::env::var("MODELTAP_FORCE_REFRESH_FAIL")
                .ok()
                .map(|s| s == tool_id.0)
                .unwrap_or(false);
            let result: Result<_, refresh::RefreshError> = if forced_fail {
                Err(refresh::RefreshError::Unreadable {
                    tool: tool_id.0.to_string(),
                    reason: "MODELTAP_FORCE_REFRESH_FAIL test seam".to_string(),
                })
            } else {
                rt.block_on(refresh::refresh_tool_incremental(plugin))
            };
            match result {
                Ok(view) => {
                    let (next, _) = update(std::mem::take(state), Msg::RefreshSucceeded(view));
                    *state = next;
                }
                Err(refresh::RefreshError::NotInstalled) => {
                    // NotInstalled is a non-failure terminal state — leave
                    // the slot untouched, no degraded indicator.
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
            // Pathological — UI selected a tool that's not in the plugin set.
            tracing::warn!(target: "modeltap.action.zap", "no plugin for {}", tool_id.0);
        }
    }
    if let Some(plan) = effect.trigger_dry_run.clone() {
        // US-14: walk the SAME plan value descriptively (no fs mutation),
        // emit the action.unify_dry_run JSONL event, and dispatch
        // Msg::UnifyDryRunCompleted(lines) so the dialog enters
        // DryRunPreview mode. Plan stays unchanged in state.unify_dialog.
        let outcome: DryRunOutcome = unify::dry_run(&plan, logger);
        let (next, _) = update(
            std::mem::take(state),
            Msg::UnifyDryRunCompleted(outcome.lines),
        );
        *state = next;
    }
    if let Some(plan) = effect.trigger_unify.clone() {
        // Step 01-12 (WS activation): the pure update layer constructs unify
        // plans with synthetic per-row paths (`/<tool>/<model_id>`) because
        // `AppState::ToolView` does not carry on-disk paths. The composition
        // root resolves those synthetic paths against the `discovered`
        // inventory BEFORE handing the plan to `unify::run`, otherwise the
        // hardlink targets would be the synthetic strings and the action
        // would silently no-op (US-U5 inode-merge invariant would fail).
        let plan = resolve_plan_paths(plan, discovered);
        // Synthesize the on-screen target name from the model id in the
        // detail screen state (if any) — fall back to the canonical's
        // basename. Used for the LastAction banner only.
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
        let outcome: UnifyOutcome = rt.block_on(unify::run(
            plan.clone(),
            plugins,
            logger,
            effect.cross_fs_choice,
        ));

        // Step 01-11 (US-U6): recompute the dedup view-model BEFORE
        // dispatching SetLastAction. The reclassify pass refreshes the
        // affected (tool, model_id) inodes, recomputes
        // `state.dedup_summary` via the canonical
        // `logic::dedup::dedup_summary`, and sets `summary_delta` for the
        // transient "(was X)" annotation. Pure call — no I/O, no async.
        // The canonical tool is passed explicitly because
        // `actions::unify::run` does NOT include it in `tools_unified`
        // (no link is performed for the canonical itself); without it the
        // reclassify pass would leave the canonical's inode entry on its
        // pre-unify (distinct) inode and the row glyph would stay `=`.
        *state = reclassify::reclassify_after_unify(
            std::mem::take(state),
            &outcome,
            plan.canonical.tool,
        );

        let last_action = build_unify_last_action(&outcome, target_name);
        let (next, _) = update(std::mem::take(state), Msg::SetLastAction(last_action));
        *state = next;

        // Step 01-11 (US-U6 AC-U6.5): schedule the 5-second
        // SummaryDeltaExpired dispatch so the renderer collapses the
        // "(was X)" annotation. Only fires when we have a live msg_tx —
        // the headless --quit-after-paint path has none and would
        // otherwise leak a timer task into a dropped runtime.
        if let Some(tx) = msg_tx {
            let tx_clone = tx.clone();
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                let _ = tx_clone.send(Msg::SummaryDeltaExpired);
            });
        }

        // US-11.AC-1 (model-count-steady scenario): re-run incremental
        // refresh for every participating tool so the summary "Disk:" total
        // reflects the post-link sizes. Model count stays the same because
        // each tool still registers the model at its own path; only the
        // backing inode is shared. Failures are routed through
        // Msg::RefreshFailed (degraded indicator).
        for link in &plan.links {
            let tool_id = link.tool;
            if let Some(plugin) = find_plugin(plugins, tool_id) {
                let forced_fail = std::env::var("MODELTAP_FORCE_REFRESH_FAIL")
                    .ok()
                    .map(|s| s == tool_id.0)
                    .unwrap_or(false);
                let result: Result<_, refresh::RefreshError> = if forced_fail {
                    Err(refresh::RefreshError::Unreadable {
                        tool: tool_id.0.to_string(),
                        reason: "MODELTAP_FORCE_REFRESH_FAIL test seam".to_string(),
                    })
                } else {
                    rt.block_on(refresh::refresh_tool_incremental(plugin))
                };
                match result {
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
        // Also refresh the canonical's tool slot in case the canonical's
        // own tool wasn't already in the link list (unify retains the
        // canonical's bytes; the slot's count/bytes don't change but the
        // refresh is the cheapest way to keep semantics consistent).
        let canonical_tool = plan.canonical.tool;
        if !plan.links.iter().any(|l| l.tool == canonical_tool) {
            if let Some(plugin) = find_plugin(plugins, canonical_tool) {
                if let Ok(view) = rt.block_on(refresh::refresh_tool_incremental(plugin)) {
                    let (next, _) = update(std::mem::take(state), Msg::RefreshSucceeded(view));
                    *state = next;
                }
            }
        }
    }
    if let Some(trigger) = effect.trigger_delete_one.clone() {
        // US-05b (step 03-06; ADR-009): the user confirmed a single-model
        // delete. Resolve the targeted registration's on-disk path from the
        // detail-screen registrations (the dialog snapshot only carries the
        // model id + tool + size; the path is the orchestrator's job).
        let on_disk_path = path_for_delete_target(state, trigger.tool, &trigger.model_id);
        if let (Some(plugin), Some(path)) = (find_plugin(plugins, trigger.tool), on_disk_path) {
            let outcome: DeleteOneOutcome = rt.block_on(delete_one::run(
                plugin,
                trigger.tool,
                trigger.model_id.clone(),
                path,
                trigger.size_bytes,
                trigger.was_shared,
                logger,
            ));
            let last_action = build_delete_one_last_action(&outcome);
            let (next, _) = update(std::mem::take(state), Msg::SetLastAction(last_action));
            *state = next;

            // US-11.AC-1 — re-run incremental refresh for the affected tool
            // so the summary reflects the post-delete byte count.
            let tool_id = trigger.tool;
            if let Ok(view) = rt.block_on(refresh::refresh_tool_incremental(plugin)) {
                let (next, _) = update(std::mem::take(state), Msg::RefreshSucceeded(view));
                *state = next;
            } else {
                tracing::info!(
                    target: "modeltap.refresh",
                    "refresh after delete_one failed or not-installed for {}",
                    tool_id.0
                );
            }
        } else {
            tracing::warn!(
                target: "modeltap.action.delete_one",
                "no plugin or path for delete_one of {} in {}",
                trigger.model_id,
                trigger.tool.0
            );
        }
    }
}

/// Step 01-12 (WS activation): resolve every synthetic `PathBuf` in a
/// `UnifyPlan` against the discovered model inventory. The pure update layer
/// (in `modeltap-tui`) builds plans with synthetic per-row paths
/// (`/<tool>/<model_id>`) because `AppState::ToolView` does not carry
/// on-disk paths; the composition root fills in the real paths here so the
/// plan handed to `actions::unify::run` is fully populated and the
/// hardlink/copy operations target real files.
///
/// The resolution is keyed by `(tool, id_in_tool)`. When a candidate's path
/// already exists on disk (`std::fs::symlink_metadata` succeeds), we leave it
/// as-is — this preserves the detail-screen path (US-10 / US-19), which
/// already canonicalizes via `build_plan_from_detail` and stat-s real files.
/// Only the main-screen path (US-U4 walking-skeleton) needs resolution.
pub(crate) fn resolve_plan_paths(
    mut plan: UnifyPlan,
    discovered: &[(ToolId, Vec<DiscoveredModel>)],
) -> UnifyPlan {
    fn lookup_path(
        tool: ToolId,
        synthetic_path: &std::path::Path,
        discovered: &[(ToolId, Vec<DiscoveredModel>)],
    ) -> Option<PathBuf> {
        // The synthetic path produced by `build_unify_plan_for_row` in
        // `modeltap-tui::update::synthetic_row_path` is
        // `/<tool>/<id_in_tool>`. The `id_in_tool` may itself contain `/`
        // (HF's id is `<org>/<repo>/<filename>`), so we strip the
        // `/<tool>/` prefix rather than relying on `file_name()`.
        let prefix = format!("/{}/", tool.0);
        let path_str = synthetic_path.to_str()?;
        let model_id = path_str.strip_prefix(&prefix)?;
        discovered
            .iter()
            .find(|(t, _)| *t == tool)
            .and_then(|(_, models)| {
                models
                    .iter()
                    .find(|m| m.id_in_tool == model_id)
                    .map(|m| m.on_disk_path.clone())
            })
    }
    // Resolve canonical only when its path is synthetic (does not exist
    // on disk). Detail-screen plans already carry real paths.
    if !plan.canonical.path.exists() {
        if let Some(real) = lookup_path(plan.canonical.tool, &plan.canonical.path, discovered) {
            // Re-stat at the real path so device + inode reflect the actual
            // file (the synthetic path was constructed with whatever the
            // hash-state cache held, which may be (0, 0) for unhashed rows).
            if let Ok(meta) = std::fs::metadata(&real) {
                plan.canonical.path = real;
                plan.canonical.exists = true;
                plan.canonical.device = meta.dev();
                plan.canonical.inode = meta.ino();
                plan.canonical.size_bytes = meta.len();
            } else {
                plan.canonical.path = real;
            }
        }
    }
    for link in &mut plan.links {
        if !link.target.exists() {
            if let Some(real) = lookup_path(link.tool, &link.target, discovered) {
                link.target = real;
            }
        }
    }
    plan
}

/// Resolve the on-disk path for a delete-one trigger by looking up the
/// matching tool's registration in the current Detail screen state. Returns
/// `None` when the screen isn't Detail OR no registration matches the tool.
fn path_for_delete_target(
    state: &AppState,
    tool: ToolId,
    _model_id: &str,
) -> Option<std::path::PathBuf> {
    let Screen::Detail(detail) = &state.current_screen else {
        return None;
    };
    detail
        .registrations
        .iter()
        .find(|r| r.tool == tool)
        .map(|r| r.path.clone())
}

/// Map a `DeleteOneOutcome` to a structured `LastAction` for the right-pane
/// banner. Reuses `LastAction::for_zap_*` constructors because the single-
/// model destructive path produces the same banner shape (tool + bytes
/// reclaimed + outcome) — the JSONL event is what distinguishes the two
/// observability streams (`action.zap_one` vs `action.zap_all`).
fn build_delete_one_last_action(outcome: &DeleteOneOutcome) -> LastAction {
    use crate::actions::delete_one::DeleteOneResult;
    match outcome.outcome {
        DeleteOneResult::Success => {
            LastAction::for_zap_success(outcome.tool, outcome.bytes_reclaimed, 0)
        }
        DeleteOneResult::NotFound | DeleteOneResult::Failed => {
            LastAction::for_zap_failed(outcome.tool)
        }
    }
}

/// On the detail screen, lift `Msg::Unify` (the keymap-bound no-op variant)
/// into a `Msg::OpenUnifyDialog(plan)` after building the plan from the
/// detail screen's registrations + a `stat` of each path. On any other
/// screen — or when the registrations don't form a valid multi-path unify —
/// the original message passes through unchanged.
///
/// US-19 (step 03-03): when the constructed plan has 1+ cross-filesystem
/// target, lift to `Msg::OpenCrossFsDialog(plan)` instead so the user gets
/// the per-target [s] skip / [c] copy / [x] cancel choice per ADR-008's
/// refuse-default policy.
///
/// US-14 (step 03-05): when `next_is_dry_run` is true (the script's next
/// token is `n`), the user wants to preview via dry-run — open the unify
/// dialog regardless of cross-fs so `[n]` can dispatch the dry-run preview.
/// The dry-run output itself surfaces per-target "WARNING: target on
/// different filesystem" lines (per `unify::dry_run`), satisfying US-14
/// AC-3 without changing the destructive-path routing.
fn lift_unify_in_detail(state: &AppState, msg: Msg, next_is_dry_run: bool) -> Msg {
    if !matches!(msg, Msg::Unify) {
        return msg;
    }
    let Screen::Detail(detail) = &state.current_screen else {
        return msg;
    };
    let Some(plan) = build_plan_from_detail(detail) else {
        // Can't build a plan (e.g., single-tool model, missing files) —
        // leave Msg::Unify as a no-op. The dialog opens only when there's
        // something to do.
        return msg;
    };
    // US-17 (intake Q5; step 03-07) — detect-and-prompt-then-retry. Before
    // ANY further dialog, probe the in-scope paths via lsof; if a registered
    // tool process holds them open, REFUSE the action by routing to the
    // running-tool prompt. Dry-run preview is read-only and bypasses the
    // gate (the user is just looking at the plan; no fs mutation).
    if !next_is_dry_run {
        let target_paths: Vec<PathBuf> = plan
            .links
            .iter()
            .filter(|l| !l.already_linked)
            .map(|l| l.target.clone())
            .chain(std::iter::once(plan.canonical.path.clone()))
            .collect();
        if let Some(running_msg) = check_running_tools(&target_paths, PendingGatedAction::Unify) {
            return running_msg;
        }
    }
    // US-19 — any active cross-fs target routes to the choice dialog,
    // EXCEPT when the next script token is `n` (US-14 dry-run preview);
    // dry-run is read-only so it must be reachable even when the
    // destructive path would be blocked by the cross-fs choice dialog.
    let has_cross_fs = plan
        .links
        .iter()
        .any(|l| !l.already_linked && l.cross_filesystem);
    if has_cross_fs && !next_is_dry_run {
        Msg::OpenCrossFsDialog(plan)
    } else {
        Msg::OpenUnifyDialog(plan)
    }
}

/// US-17 (step 03-07; intake Q5): probe `target_paths` via the lsof adapter.
/// Returns `Some(Msg::OpenRunningToolPrompt(_))` when the gate should fire —
/// either a running tool was detected (`Detected` mode) or lsof is missing
/// (`LsofUnavailable` mode). Returns `None` when the probe returned an empty
/// list (no running tools — the action proceeds normally).
///
/// The IO error case (probe returned `Err(Io(_))`) is treated as "no
/// detection available, do not block" per the conservative-when-uncertain
/// rule (ADR-002): we'd rather let the user proceed than break unify on a
/// transient lsof glitch. Only the explicit `LsofUnavailable` (binary
/// missing) raises the unavailability dialog.
fn check_running_tools(target_paths: &[PathBuf], action: PendingGatedAction) -> Option<Msg> {
    if target_paths.is_empty() {
        return None;
    }
    let adapter = LsofAdapter::new();
    match adapter.detect_running_tools(target_paths) {
        Ok(processes) if !processes.is_empty() => Some(Msg::OpenRunningToolPrompt(
            RunningToolDialog::detected(processes, action),
        )),
        Ok(_) => None,
        Err(ProbeError::LsofUnavailable { .. }) => Some(Msg::OpenRunningToolPrompt(
            RunningToolDialog::lsof_unavailable(action),
        )),
        Err(_) => None,
    }
}

/// Build a `UnifyPlan` by stat-ing each registration's path. Used by the
/// headless harness when intercepting `Msg::Unify` on the detail screen.
/// Returns None when no candidate has both a non-empty stat result AND the
/// resulting candidates can produce a non-degenerate plan (every
/// registration must point at an existing file). The headless harness
/// treats None as "leave Msg::Unify as a no-op".
fn build_plan_from_detail(detail: &DetailScreenState) -> Option<UnifyPlan> {
    if detail.registrations.is_empty() {
        return None;
    }
    // US-19 fake-fs-probe injection (test seam only). The env var is a
    // colon-separated list of canonicalized path prefixes; any registration
    // whose canonicalized path starts with one of these prefixes has its
    // `device` overridden to a synthetic non-canonical value so the
    // `cross_filesystem` flag fires for that target. Production paths never
    // match (the env var is unset), so this is zero-impact in real runs.
    let fake_cross_fs: Vec<PathBuf> = std::env::var("MODELTAP_FAKE_CROSS_FS_PATHS")
        .ok()
        .map(|s| s.split(':').map(PathBuf::from).collect())
        .unwrap_or_default();
    let mut candidates: Vec<CandidatePath> = Vec::new();
    let mut plan_candidates: Vec<PlanCandidate> = Vec::new();
    for reg in &detail.registrations {
        // Resolve symlinks to the underlying blob path. Real-production HF
        // discovery already does this (per plugins/hf/src/discover.rs); the
        // headless detail-regs JSON seam may pass a snapshot symlink, so we
        // canonicalize here to mirror production semantics. `canonicalize`
        // returns Err for non-existent paths, in which case we fall back to
        // the original path so the build_plan defensive branches still fire.
        let resolved_path = std::fs::canonicalize(&reg.path).unwrap_or_else(|_| reg.path.clone());
        let (exists, mut device, inode, size_bytes) = match std::fs::metadata(&resolved_path) {
            Ok(m) => (true, m.dev(), m.ino(), m.len()),
            Err(_) => (false, 0, 0, 0),
        };
        // Apply the fake-fs-probe override. We use a sentinel device id
        // (`u64::MAX`) so it cannot collide with any real `dev_t`; per-path
        // mismatch is enough for `build_plan` to flag `cross_filesystem`.
        if path_matches_fake_cross_fs(&resolved_path, &fake_cross_fs) {
            device = u64::MAX;
        }
        candidates.push(CandidatePath {
            tool: reg.tool,
            path: resolved_path.clone(),
            exists,
            size_bytes,
            // The detail-screen-driven path doesn't know which is an Ollama
            // blob; rely on the lexicographic tiebreak in select_canonical.
            // (Production wires this via the plugin's path-classifier.)
            is_ollama_blob: reg.tool == ToolId("ollama"),
        });
        plan_candidates.push(PlanCandidate {
            tool: reg.tool,
            path: resolved_path,
            exists,
            device,
            inode,
            size_bytes,
        });
    }
    let canonical = select_canonical(&candidates)?;
    let canonical_plan = plan_candidates
        .iter()
        .find(|p| p.path == canonical.path)?
        .clone();
    build_plan(&canonical_plan, &plan_candidates)
}

/// US-05b (step 03-06): on the detail screen, lift `Msg::DeleteFromOne`
/// into `Msg::OpenDeleteOneDialog(state)` after building a
/// `DeleteOneConfirmState` snapshot from the screen's registrations. The
/// targeted tool is selected by the `MODELTAP_HEADLESS_DELETE_TARGET` env
/// var (matching the registration's tool id); when unset, the FIRST
/// registration is used. The `was_shared` flag is computed conservatively
/// (per ADR-002): true iff the screen has 2+ registrations (the same model
/// content lives under another tool's tree, so deleting one preserves the
/// content elsewhere); false for single-tool registrations (typed-id mode).
///
/// Outside the detail screen — or when no registration matches the target —
/// `Msg::DeleteFromOne` passes through unchanged (keymap no-op).
fn lift_delete_one_in_detail(state: &AppState, msg: Msg) -> Msg {
    if !matches!(msg, Msg::DeleteFromOne) {
        return msg;
    }
    let Screen::Detail(detail) = &state.current_screen else {
        return msg;
    };
    if detail.registrations.is_empty() {
        return msg;
    }
    let target_tool_str = std::env::var("MODELTAP_HEADLESS_DELETE_TARGET").ok();
    let target_reg = match &target_tool_str {
        Some(s) => detail.registrations.iter().find(|r| r.tool.0 == s.as_str()),
        None => detail.registrations.first(),
    };
    let Some(reg) = target_reg else {
        return msg;
    };
    // US-17 (step 03-07; intake Q5): detect-and-prompt-then-retry gate. The
    // delete-one path scopes the probe to the targeted registration's path
    // (the only file we'd remove). If a registered tool process holds it
    // open, refuse with the running-tool prompt — NO destructive side-effect.
    let target_paths = vec![reg.path.clone()];
    if let Some(running_msg) = check_running_tools(&target_paths, PendingGatedAction::DeleteOne) {
        return running_msg;
    }
    let was_shared = detail.registrations.len() >= 2;
    let size_bytes = std::fs::metadata(&reg.path)
        .map(|m| m.len())
        .unwrap_or(detail.model.canonical_size_bytes);
    // The dialog's `model_id` is what the orchestrator passes to
    // `Tool::delete_one(model.id_in_tool)`. For Ollama / HF, the per-tool
    // id_in_tool is distinct from the display id; the test seam
    // `MODELTAP_HEADLESS_DELETE_ID_IN_TOOL` overrides the dialog's
    // model_id so production-shape lookups work in fixtures. When unset,
    // fall back to the display id (correct for Loose GGUFs / lm-studio
    // where id_in_tool == filename and the dialog's typed-id matches the
    // display id).
    let dialog_model_id = std::env::var("MODELTAP_HEADLESS_DELETE_ID_IN_TOOL")
        .unwrap_or_else(|_| detail.model.id.clone());
    let dialog =
        DeleteOneConfirmState::for_model(reg.tool, dialog_model_id, size_bytes, was_shared);
    Msg::OpenDeleteOneDialog(dialog)
}

/// On the main screen, lift Enter into `Msg::OpenDetail(...)` so the
/// headless harness can navigate from the row list into the detail screen.
/// The detail screen's registrations are synthesized from the headless test
/// fixture environment via `MODELTAP_HEADLESS_DETAIL_REGS` — a JSON array
/// of `{tool, path}` entries. Falls through unchanged when the env-var is
/// not set OR when we're not on the main screen.
///
/// Step 01-07 broadened the input set: production now dispatches
/// `Msg::ToggleFolderExpansion` for Enter (not `DialogConfirm`). The
/// detail-screen path is OPT-IN via the env-var. When the env-var is set
/// (US-10 / US-13 / US-19 acceptance tests), we lift either the legacy
/// `DialogConfirm` shape OR the new `ToggleFolderExpansion` shape into
/// `Msg::OpenDetail`. When the env-var is absent (this step's scenarios),
/// the `ToggleFolderExpansion` Msg passes through to the production
/// `update` handler.
fn lift_enter_in_main(state: &AppState, msg: Msg) -> Msg {
    if !matches!(msg, Msg::DialogConfirm | Msg::ToggleFolderExpansion) {
        return msg;
    }
    if !matches!(state.current_screen, Screen::Main) {
        return msg;
    }
    if state.zap_dialog.is_some() || state.unify_dialog.is_some() {
        return msg; // dialog confirm — let update handle it
    }
    let Some(detail) = synthesize_detail_from_env(state) else {
        return msg;
    };
    Msg::OpenDetail(detail)
}

/// Step 02-01 (US-21): on the main screen with `MODELTAP_HEADLESS_TOOL_DETAIL=1`
/// set, lift `Msg::ToggleFolderExpansion` (the production Enter dispatch) into
/// `Msg::OpenToolDetail(selected_tool_id)`. The env-var gate keeps the
/// existing scenarios (folder collapse/expand from step 01-07, model detail
/// from US-10 via `MODELTAP_HEADLESS_DETAIL_REGS`) bit-for-bit unchanged —
/// only the new tool-detail acceptance scenarios opt in.
///
/// The selected tool id is read from `state.current_tool()` (the
/// `LeftPaneSlot::Real(ToolView)` at `state.selected_tool`). Synthetic slots
/// (e.g. `[All Unified]`) return `None` so this lift is inert there.
fn lift_enter_in_left_pane_to_tool_detail(state: &AppState, msg: Msg) -> Msg {
    if !matches!(msg, Msg::ToggleFolderExpansion) {
        return msg;
    }
    if !matches!(state.current_screen, Screen::Main) {
        return msg;
    }
    if state.zap_dialog.is_some()
        || state.unify_dialog.is_some()
        || state.delete_one_dialog.is_some()
    {
        return msg;
    }
    if std::env::var("MODELTAP_HEADLESS_TOOL_DETAIL").as_deref() != Ok("1") {
        return msg;
    }
    let Some(tool_view) = state.current_tool() else {
        return msg;
    };
    Msg::OpenToolDetail(tool_view.tool)
}

/// Step 02-01 (US-21): run the tool-detail orchestrator after a
/// `Msg::OpenToolDetail(tool_id)` transitioned `Screen::ToolDetail{detail:
/// None}`. Composes the cached `CachedTool` row with `Tool::inspect_tool()`
/// into a unified `ToolDetail` and dispatches `Msg::ToolDetailReady` back
/// into `update()` so the screen leaves its loading state.
///
/// Mirrors `interactive::dispatch_open_tool_detail` — the headless harness
/// and the production loop produce byte-identical screens for the
/// acceptance-test asserts. On orchestrator error (plugin not in registry,
/// cache I/O failed) we tracing::warn and skip the ready-dispatch so the
/// user sees the loading-state placeholder (Esc returns).
fn dispatch_open_tool_detail(
    rt: &tokio::runtime::Runtime,
    plugins: &[Box<dyn Tool>],
    cache_path: Option<&std::path::Path>,
    log_dir: Option<&std::path::Path>,
    diagnostics_dir: Option<&std::path::Path>,
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
    let config = modeltap_app::orchestration::open_tool_detail::OpenToolDetailConfig {
        log_dir: log_dir.map(|p| p.to_path_buf()),
        diagnostics_dir: diagnostics_dir.map(|p| p.to_path_buf()),
    };
    match rt.block_on(modeltap_app::orchestration::open_tool_detail::run(
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
/// Pure (no I/O, no AppState mutation) and mirrors interactive.rs's
/// identically-named helper so both event loops produce bit-for-bit
/// equivalent metadata across real terminals and the acceptance harness.
fn extract_model_detail_dispatch(
    detail: &DetailScreenState,
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
/// Mirrors `interactive::dispatch_open_model_detail` — the headless harness
/// and the production loop produce byte-identical metadata renders for the
/// US-22 acceptance assertions.
#[allow(clippy::too_many_arguments)]
fn dispatch_open_model_detail(
    rt: &tokio::runtime::Runtime,
    plugins: &[Box<dyn Tool>],
    cache_path: Option<&std::path::Path>,
    log_dir: Option<&std::path::Path>,
    diagnostics_dir: Option<&std::path::Path>,
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
    let config = modeltap_app::orchestration::open_model_detail::OpenModelDetailConfig {
        log_dir: log_dir.map(|p| p.to_path_buf()),
        diagnostics_dir: diagnostics_dir.map(|p| p.to_path_buf()),
    };
    match rt.block_on(modeltap_app::orchestration::open_model_detail::run(
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

/// US-U5: rewrite `Msg::ToggleTarget(0)` (placeholder produced by
/// `token_to_msg` for `<space>` while a unify dialog is open) into
/// `Msg::ToggleTarget(selected_target_idx)` so the toggle hits the row the
/// per-target cursor is currently over. Falls through unchanged when no
/// unify dialog is open OR the message is something else. The other
/// `Msg::ToggleTarget(n)` variants (n != 0) are passed through verbatim
/// so future production callers can inject explicit indices.
fn lift_toggle_in_unify_dialog(state: &AppState, msg: Msg) -> Msg {
    let Msg::ToggleTarget(0) = &msg else {
        return msg;
    };
    let Some(dialog) = state.unify_dialog.as_ref() else {
        return msg;
    };
    Msg::ToggleTarget(dialog.selected_target_idx)
}

/// Build a `DetailScreenState` from the `MODELTAP_HEADLESS_DETAIL_REGS`
/// env-var JSON payload. The env-var is the headless-test-only seam that
/// lets a test inject the cross-tool registrations a real production
/// orchestrator would compute from the Inventory's dedup-key index. Format:
///
/// ```json
/// {
///   "id": "mistralai/Mistral-7B-v0.3",
///   "regs": [
///     {"tool": "ollama",    "path": "/abs/path/a"},
///     {"tool": "hf",        "path": "/abs/path/b"},
///     {"tool": "Loose GGUFs", "path": "/abs/path/c"}
///   ]
/// }
/// ```
fn synthesize_detail_from_env(_state: &AppState) -> Option<DetailScreenState> {
    use modeltap_core::logic::unification_status::{DetailModelView, DetailRegistration};
    use modeltap_core::{ContentHash, DisplayLabel, Format, ModelStatus};

    let raw = std::env::var("MODELTAP_HEADLESS_DETAIL_REGS").ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let id = value.get("id")?.as_str()?.to_string();
    let regs_json = value.get("regs")?.as_array()?;
    let mut registrations: Vec<DetailRegistration> = Vec::new();
    for r in regs_json {
        let tool_str = r.get("tool")?.as_str()?;
        let path_str = r.get("path")?.as_str()?;
        let tool = match tool_str {
            "ollama" => ToolId("ollama"),
            "hf" => ToolId("hf"),
            "lm-studio" => ToolId("lm-studio"),
            _ => continue,
        };
        let path = PathBuf::from(path_str);
        let inode = std::fs::metadata(&path).ok().map(|m| m.ino());
        registrations.push(DetailRegistration { tool, path, inode });
    }
    if registrations.is_empty() {
        return None;
    }
    // Compute canonical_size from the largest existing reg.
    let canonical_size_bytes = registrations
        .iter()
        .filter_map(|r| std::fs::metadata(&r.path).ok().map(|m| m.len()))
        .max()
        .unwrap_or(0);
    let model_view = DetailModelView {
        id: id.clone(),
        format: Format::Other,
        format_quant: None,
        canonical_size_bytes,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Healthy,
    };
    // Provide a fake content hash so the detail screen renders fully (the
    // hash isn't asserted in US-10 scenarios; the Hasher port wiring is a
    // separate seam).
    let hash = ContentHash([0xAA; 32]);
    Some(DetailScreenState::new(
        model_view,
        registrations,
        Some(hash),
    ))
}

/// Map a `UnifyOutcome` to a structured `LastAction` for the right-pane
/// banner. `target_name` is the model identifier (display id) — NOT a tool
/// id — because unify acts on a single model across multiple tools.
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
            // 03-03 will plumb per-target failure detail; for now we
            // construct a partial banner with the failures the orchestrator
            // collected.
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

/// HF-plugin-owned sidecar walker, injected into `folder_delete::run` via the
/// `SidecarEnumerator` port so the orchestrator stays plugin-agnostic.
struct HfSidecarEnumerator;

impl SidecarEnumerator for HfSidecarEnumerator {
    fn enumerate(
        &self,
        repo_dir: &std::path::Path,
        model_files: &[std::path::PathBuf],
    ) -> Vec<modeltap_core::types::Sidecar> {
        modeltap_plugin_hf::folder_delete::enumerate_sidecars(repo_dir, model_files)
    }
}

/// Map a `FolderDeleteOutcome` to a structured `LastAction` for the right-pane
/// banner (US-05c, step 01-05). Step 02-01 extends this to handle the cancel
/// paths — they currently surface as `for_folder_delete_failed` because the
/// dedicated cancel-banner ("dialog closed — no destructive action") lands
/// later in M3 along with the mixed-mode rendering work. The JSONL event is
/// the precise observability stream for the cancel paths.
fn build_folder_delete_last_action(outcome: &FolderDeleteOutcome) -> LastAction {
    match outcome.outcome {
        FolderDeleteResult::Success => LastAction::for_folder_delete_success(
            outcome.folder_path.clone(),
            outcome.bytes_reclaimed,
            outcome.bytes_retained,
            outcome.files_total,
            outcome.files_removed,
        ),
        // Step 04-01 (ADR-010 § Concurrency): partial-failure banner with
        // per-file failure detail. The retry hint is rendered only when at
        // least one failure was an in-use-by-tool case ("file open by ...").
        // Other failures (permission denied, NotFound) leave the hint
        // suppressed — re-running Shift+F there won't change the outcome
        // until the user fixes the permission bits out-of-band.
        FolderDeleteResult::Partial => {
            let retry_hint = derive_retry_hint(&outcome.failures);
            LastAction::for_folder_delete_partial(
                outcome.folder_path.clone(),
                outcome.bytes_reclaimed,
                outcome.bytes_retained,
                outcome.files_total,
                outcome.files_removed,
                outcome.failures.clone(),
                retry_hint,
            )
        }
        // Step 04-03: pre-flight refusal variants surface their user-facing
        // message in the banner's `target` slot so the existing `view_lines`
        // renderer (modeltap-tui::render::last_action) puts the refusal
        // text into the rendered header line WITHOUT requiring any TUI
        // changes. The folder-path is omitted from the header so the
        // message fits inside the right pane (typically ~80 cols). The
        // path is still present in the JSONL event for observability.
        // Acceptance tests grep stdout for the message substring per
        // AC-15 / AC-20.
        FolderDeleteResult::RefusedReadonlyCache => {
            let msg = folder_delete::PreflightRefusal::ReadonlyCache.message();
            LastAction::for_folder_delete_failed(msg.to_string())
        }
        FolderDeleteResult::RefusedFolderMissing => {
            let msg = folder_delete::PreflightRefusal::FolderMissing.message();
            LastAction::for_folder_delete_failed(msg.to_string())
        }
        FolderDeleteResult::Failed
        | FolderDeleteResult::CancelledMismatch
        | FolderDeleteResult::CancelledEscape => {
            LastAction::for_folder_delete_failed(outcome.folder_path.clone())
        }
    }
}

/// Step 04-01: derive the trailing retry-hint line from the per-file failure
/// reasons. If any failure is an in-use-by-tool case (`"file open by ..."`),
/// the hint instructs the user to close that tool and press Shift+F again.
/// Otherwise no hint — the user must remediate out-of-band before retrying.
fn derive_retry_hint(
    failures: &[modeltap_core::domain::last_action::TargetError],
) -> Option<String> {
    let tool = failures
        .iter()
        .find_map(|f| f.reason.strip_prefix("file open by "))
        .map(|s| s.to_string())?;
    Some(format!("Press [F] again after closing {tool} to finish"))
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

/// True iff `path` (or any of its ancestors) appears in `fake_cross_fs`. The
/// list of fake-cross-fs paths is treated as a prefix list — registering a
/// directory marks every file beneath it as cross-fs from the canonical's
/// perspective. Used by the US-19 acceptance harness; never set in production.
fn path_matches_fake_cross_fs(path: &std::path::Path, fake_cross_fs: &[PathBuf]) -> bool {
    if fake_cross_fs.is_empty() {
        return false;
    }
    fake_cross_fs
        .iter()
        .any(|p| path.starts_with(p) || p == path)
}

/// Resolve a `ScriptToken` to an `Msg`, accounting for whether a typed-input
/// dialog is currently open. Mirrors `keymap::dispatch_in_dialog` in spirit
/// (printable chars go to the dialog buffer; only Esc/Enter/Backspace are
/// dialog control). Outside a dialog, the script-token-to-Msg mapping
/// matches the @us-03 acceptance contract.
///
/// US-19 cross-fs dialog: when `cross_fs_open` is true, `s` / `c` / `x`
/// (and Esc / Enter, per the refuse-default policy) are interpreted as
/// `Msg::CrossFsSkip` / `Msg::CrossFsCopy` / `Msg::CrossFsCancel`.
///
/// US-14 dry-run: when `unify_open` is true, the `n` key is interpreted as
/// `Msg::UnifyDryRun` (which dispatches `actions::unify::dry_run` against
/// the dialog's plan without mutating fs).
///
/// US-05b: when `delete_one_shared_open` is true (delete-one dialog in
/// Shared mode), `y` is `Msg::DeleteOneConfirmShared` and `n` is
/// `Msg::DeleteOneCancelShared`. Other characters are silently ignored
/// (the dialog resolves only on y/n/Esc per the dialog state machine).
fn token_to_msg(
    token: &ScriptToken,
    dialog_open: bool,
    cross_fs_open: bool,
    unify_open: bool,
    delete_one_shared_open: bool,
    focus: FocusPane,
) -> Msg {
    if cross_fs_open {
        return match token {
            ScriptToken::CtrlC => Msg::CtrlC,
            ScriptToken::Tag(t) => match t.as_str() {
                // Esc and Enter both default to refuse per ADR-008 OQ-4.
                "esc" | "enter" => Msg::CrossFsCancel,
                _ => Msg::UnboundKey,
            },
            ScriptToken::Char(c) => match c {
                's' => Msg::CrossFsSkip,
                'c' => Msg::CrossFsCopy,
                'x' => Msg::CrossFsCancel,
                _ => Msg::UnboundKey,
            },
        };
    }
    if dialog_open {
        return match token {
            ScriptToken::CtrlC => Msg::CtrlC,
            ScriptToken::Tag(t) => match t.as_str() {
                "esc" => Msg::DialogCancel,
                "enter" => Msg::DialogConfirm,
                "backspace" => Msg::DialogBackspace,
                // US-U5: while the unify dialog is open, [space] toggles
                // the active checkbox for the currently-selected target row,
                // and [up]/[down] move the per-target cursor. The
                // `unify_open` guard is required because the zap and
                // delete-one dialogs share the dialog_open branch but do
                // not own a per-row cursor. The actual toggle index is
                // resolved at the call site via `lift_toggle_in_unify_dialog`
                // (this fn does not have access to the dialog state).
                "space" if unify_open => Msg::ToggleTarget(0),
                "up" if unify_open => Msg::UnifyDialogSelectPrev,
                "down" if unify_open => Msg::UnifyDialogSelectNext,
                _ => Msg::UnboundKey,
            },
            ScriptToken::Char(c) => {
                // US-05b: in delete-one Shared mode, `y` and `n` resolve the
                // dialog directly (low-friction confirmation). Other keys
                // are silently ignored (the keymap docs treat them as
                // unbound while the [y/n] prompt is active).
                if delete_one_shared_open {
                    return match c {
                        'y' => Msg::DeleteOneConfirmShared,
                        'n' => Msg::DeleteOneCancelShared,
                        _ => Msg::UnboundKey,
                    };
                }
                // US-14: `[n]` while unify dialog is open dispatches the
                // dry-run preview (no fs mutation). The zap dialog's typed-
                // input buffer never sees `n` because the two dialogs are
                // mutually exclusive (one open at a time).
                if unify_open && *c == 'n' {
                    Msg::UnifyDryRun
                } else {
                    Msg::DialogTextInput(*c)
                }
            }
        };
    }
    match token {
        ScriptToken::CtrlC => Msg::CtrlC,
        ScriptToken::Tag(t) => match t.as_str() {
            "right" => Msg::SelectNextTool,
            "left" => Msg::SelectPrevTool,
            // Focus-aware Up/Down: when the left pane has focus, the arrows
            // navigate tools so a single mental model ("arrows move the
            // cursor in the focused pane") works for both panes. Right-pane
            // focus retains the legacy row-navigation semantics. Mirrors
            // `keymap::dispatch_focus_aware` exactly.
            "down" => match focus {
                FocusPane::Left => Msg::SelectNextTool,
                FocusPane::Right => Msg::SelectNextRow,
            },
            "up" => match focus {
                FocusPane::Left => Msg::SelectPrevTool,
                FocusPane::Right => Msg::SelectPrevRow,
            },
            "tab" => Msg::ToggleFocus,
            // Outside any dialog, Esc mirrors production keymap (`dispatch`):
            // it pops Detail back to Main via `Msg::CloseDetail`. The dialog
            // branch above intercepts Esc-during-dialog to `Msg::DialogCancel`.
            "esc" => Msg::CloseDetail,
            // Step 01-07: outside any dialog, Enter toggles folder expansion
            // on the main view. The legacy "open detail" path is still
            // available — when `MODELTAP_HEADLESS_DETAIL_REGS` is set,
            // `lift_enter_in_main` rewrites this Msg into `Msg::OpenDetail`
            // so US-10 / US-13 / US-19 acceptance tests keep working
            // unchanged. Mirrors the production keymap's SHORTCUT_TABLE
            // entry for `[Enter] expand/collapse`.
            "enter" => Msg::ToggleFolderExpansion,
            "backspace" => Msg::DialogBackspace,
            _ => Msg::UnboundKey,
        },
        ScriptToken::Char(c) => match c {
            'q' => Msg::Quit,
            'z' => Msg::ZapTool,
            'u' => Msg::Unify,
            'd' => Msg::DeleteFromOne,
            '?' => Msg::ToggleHelp,
            _ => Msg::UnboundKey,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CHARACTERIZATION TEST (step 01-09).
    ///
    /// `<hash-complete>` is a NEW script-grammar sentinel introduced by
    /// step 01-09. It is intentionally implemented WITHOUT a tokenizer
    /// change because the existing `tokenize_script` already maps any
    /// `<...>` sequence to `ScriptToken::Tag(<inside>)`. This test
    /// pins that contract: the sentinel must parse to exactly one
    /// `Tag("hash-complete")` token so the script driver loop can
    /// recognise it before `token_to_msg` would (otherwise) classify
    /// it as `Msg::UnboundKey`.
    ///
    /// If a future refactor changes the tokenizer to special-case any
    /// `<...>` sequence, this test will fail and force re-evaluation
    /// of the sentinel contract.
    #[test]
    fn tokenize_script_parses_hash_complete_sentinel_to_single_tag_token() {
        let tokens = tokenize_script("<hash-complete>");
        assert_eq!(
            tokens,
            vec![ScriptToken::Tag("hash-complete".to_string())],
            "<hash-complete> must parse to exactly one Tag token so the \
             script driver loop can recognise it as a sync sentinel"
        );
    }
}
