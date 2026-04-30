//! Pure Elm-style `update()` (per ADR-006).
//!
//! `update(state, msg) -> (state, effect)` is a pure function — no I/O, no
//! mutation of inputs, no clocks. The composition root interprets the
//! returned `UpdateEffect` (write JSONL events, exit, dispatch zap-all, etc.).

use modeltap_core::logic::plan::UnifyPlan;
use modeltap_core::ToolId;

use crate::app_state::{AppState, FocusPane, Screen};
use crate::dialogs::cross_fs_choice::{CrossFsChoice, CrossFsChoiceDialog};
use crate::dialogs::unify_confirm::{UnifyDecision, UnifyDialogState};
use crate::dialogs::zap_confirm::{ZapConfirmState, ZapDecision};
use crate::msg::Msg;

/// Side-effects the composition root must perform after this update. The
/// pure update function only describes effects; it does not execute them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateEffect {
    /// When set, the composition root should emit a `launch.ended` JSONL
    /// event before exiting. True ONLY for `Msg::Quit` (NOT for Ctrl+C —
    /// per the master-acceptance KPI invariant).
    pub emit_launch_ended: bool,

    /// When `Some(tool_id)`, the user has confirmed the zap action for that
    /// tool (typed name matched). The composition root invokes
    /// `actions::zap::run` to call `Tool::delete_all` and emit the
    /// `action.zap_all` JSONL event.
    pub trigger_zap: Option<ToolId>,

    /// When `Some(plan)`, the user has confirmed the unify action. The
    /// composition root invokes `actions::unify::run` to call each plugin's
    /// `Tool::link` per the plan, run the all-or-revert sequence (per
    /// ADR-008), and emit the `action.unify` JSONL event.
    pub trigger_unify: Option<UnifyPlan>,

    /// When `Some(choice)` AND `trigger_unify.is_some()`, the user has chosen
    /// the per-target cross-filesystem fallback semantics (US-19, ADR-008):
    /// `Skip` leaves cross-fs targets untouched; `Copy` duplicates the
    /// canonical's bytes to each cross-fs target. `None` means the orchestrator
    /// uses the default link path (US-10, no cross-fs targets).
    pub cross_fs_choice: Option<CrossFsChoice>,

    /// US-14: when `Some(plan)`, the user pressed `[n]` in the unify confirm
    /// dialog. The composition root invokes `actions::unify::dry_run(plan)`
    /// (which emits the `action.unify_dry_run` JSONL event and returns the
    /// formatted preview lines) and then dispatches
    /// `Msg::UnifyDryRunCompleted(lines)` so the dialog transitions to
    /// `UnifyMode::DryRunPreview { lines }`. The plan value is preserved
    /// unchanged in `state.unify_dialog.plan` per the ADR-006 same-value-type
    /// principle.
    pub trigger_dry_run: Option<UnifyPlan>,
}

/// Pure transition. Takes ownership of `state` and returns the next state.
pub fn update(state: AppState, msg: Msg) -> (AppState, UpdateEffect) {
    match msg {
        Msg::Quit => (
            AppState {
                should_quit: true,
                exit_code: 0,
                ..state
            },
            UpdateEffect {
                emit_launch_ended: true,
                ..UpdateEffect::default()
            },
        ),
        Msg::CtrlC => (
            AppState {
                should_quit: true,
                exit_code: 130,
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::SelectNextTool => (
            advance_tool(clear_last_action(state), 1),
            UpdateEffect::default(),
        ),
        Msg::SelectPrevTool => (
            advance_tool(clear_last_action(state), -1),
            UpdateEffect::default(),
        ),
        Msg::SelectNextRow => (
            advance_row(clear_last_action(state), 1),
            UpdateEffect::default(),
        ),
        Msg::SelectPrevRow => (
            advance_row(clear_last_action(state), -1),
            UpdateEffect::default(),
        ),
        Msg::ToggleFocus => (
            AppState {
                focus: match state.focus {
                    FocusPane::Left => FocusPane::Right,
                    FocusPane::Right => FocusPane::Left,
                },
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::ZapTool => (open_zap_dialog(state), UpdateEffect::default()),
        Msg::DialogTextInput(c) => (
            mutate_dialog(state, |d| d.handle_char(c)),
            UpdateEffect::default(),
        ),
        Msg::DialogBackspace => (
            mutate_dialog(state, |d| d.handle_backspace()),
            UpdateEffect::default(),
        ),
        Msg::DialogConfirm => decide_dialog(state, DialogKey::Enter),
        Msg::DialogCancel => decide_dialog(state, DialogKey::Esc),
        Msg::OpenUnifyDialog(plan) => (
            AppState {
                unify_dialog: Some(UnifyDialogState::from_plan(plan)),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::SetLastAction(action) => (
            AppState {
                last_action: Some(action),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::RefreshTool(view) => (replace_tool_slot(state, view), UpdateEffect::default()),
        // US-11 Result-flavored refresh variants (step 03-04).
        Msg::RefreshSucceeded(view) => {
            let tool_id = view.tool;
            let mut next = replace_tool_slot(state, view);
            // The successful refresh resolves any prior failure marker.
            next.refresh_failed_tools.remove(&tool_id);
            (next, UpdateEffect::default())
        }
        Msg::RefreshFailed(tool_id) => {
            let mut next = state;
            next.refresh_failed_tools.insert(tool_id);
            (next, UpdateEffect::default())
        }
        // RetryRefresh is a state-noop in the pure update; the composition
        // root sees the message and re-spawns the refresh task. The failed
        // marker stays in place until RefreshSucceeded arrives.
        Msg::RetryRefresh(_) => (state, UpdateEffect::default()),
        Msg::OpenDetail(detail) => (
            AppState {
                current_screen: Screen::Detail(detail),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::CloseDetail => (
            AppState {
                current_screen: Screen::Main,
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::ToggleHelp => (toggle_help(state), UpdateEffect::default()),
        // ----- US-19 cross-filesystem fallback dialog (ADR-008, step 03-03) ---
        Msg::OpenCrossFsDialog(plan) => (
            AppState {
                cross_fs_dialog: Some(CrossFsChoiceDialog::from_plan(plan)),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::CrossFsSkip => decide_cross_fs(state, CrossFsKey::Skip),
        Msg::CrossFsCopy => decide_cross_fs(state, CrossFsKey::Copy),
        Msg::CrossFsCancel => decide_cross_fs(state, CrossFsKey::Cancel),
        // Unify and DeleteFromOne are wired in subsequent steps (03-02, 03-06).
        // Here they are bound to non-noop Msg variants so the INT-6
        // invariant holds — every visible shortcut maps to a real Msg —
        // while the state remains unchanged.
        Msg::Unify => (state, UpdateEffect::default()),
        Msg::DeleteFromOne => (state, UpdateEffect::default()),
        // ----- US-14 dry-run preview (step 03-05) -----------------------
        Msg::UnifyDryRun => decide_dry_run(state),
        Msg::UnifyDryRunCompleted(lines) => apply_dry_run_lines(state, lines),
        Msg::UnboundKey => (state, UpdateEffect::default()),
    }
}

/// Toggle the layered help overlay (US-08). When `current_screen` is anything
/// other than `Help`, wrap it in `Screen::Help { previous: <current> }`. When
/// it is already `Help`, restore the wrapped previous screen. This lets `?`
/// open AND close the overlay symmetrically, and Esc-from-help maps to the
/// same `Msg::ToggleHelp` as a second `?`.
fn toggle_help(state: AppState) -> AppState {
    let next_screen = match state.current_screen {
        Screen::Help { previous } => *previous,
        other => Screen::Help {
            previous: Box::new(other),
        },
    };
    AppState {
        current_screen: next_screen,
        ..state
    }
}

/// Clear `last_action` (US-06: any nav Msg dismisses the post-action banner).
fn clear_last_action(state: AppState) -> AppState {
    AppState {
        last_action: None,
        ..state
    }
}

/// Replace the matching `ToolView` slot in `state.tools` with the freshly-
/// discovered view. Tools are matched by `ToolId`; if no slot matches (e.g.
/// a future plugin id we don't know yet) the state is returned unchanged.
fn replace_tool_slot(mut state: AppState, view: crate::app_state::ToolView) -> AppState {
    if let Some(slot) = state.tools.iter_mut().find(|t| t.tool == view.tool) {
        *slot = view;
    }
    state
}

/// Move the tool selection forward or backward (cyclic). Resets the row
/// selection and scroll offset because the new tool has its own row list.
fn advance_tool(state: AppState, delta: i32) -> AppState {
    let n = state.tools.len();
    if n == 0 {
        return state;
    }
    let current = state.selected_tool as i32;
    let next = ((current + delta).rem_euclid(n as i32)) as usize;
    AppState {
        selected_tool: next,
        selected_row: 0,
        scroll_offset: 0,
        ..state
    }
}

/// Move the row selection within the current tool. Clamps at boundaries
/// (no wrap; long lists scroll instead of wrapping). Updates `scroll_offset`
/// so the cursor stays inside the visible window.
fn advance_row(state: AppState, delta: i32) -> AppState {
    let row_count = state.current_row_count();
    if row_count == 0 {
        return state;
    }
    let current = state.selected_row as i32;
    let max_idx = (row_count - 1) as i32;
    let next = (current + delta).clamp(0, max_idx) as usize;
    let scroll_offset = compute_scroll_offset(next, state.scroll_offset, state.visible_rows);
    AppState {
        selected_row: next,
        scroll_offset,
        ..state
    }
}

/// Keep the cursor inside the visible window:
/// - if `selected_row < scroll_offset`, scroll up so cursor is at the top.
/// - if `selected_row >= scroll_offset + visible_rows`, scroll down so
///   cursor is at the bottom.
/// - otherwise leave scroll_offset unchanged.
fn compute_scroll_offset(selected: usize, current_offset: usize, visible: usize) -> usize {
    if visible == 0 {
        return current_offset;
    }
    if selected < current_offset {
        return selected;
    }
    if selected >= current_offset + visible {
        return selected + 1 - visible;
    }
    current_offset
}

/// Open the zap-confirm dialog snapshot for the currently-selected tool. The
/// classifier (unique-vs-shared) is computed conservatively from the local
/// `AppState` view: every model is treated as unique because the WS slice
/// has no cross-tool inventory yet (one-tool-installed scenario), which is
/// safe per ADR-002 §"Conservative deletion" — any uncertainty defaults to
/// "unique" (the more cautious estimate).
fn open_zap_dialog(state: AppState) -> AppState {
    let dialog = match state.current_tool() {
        Some(tool) => {
            let total = tool.total_bytes();
            ZapConfirmState::for_tool(tool.tool, tool.model_ids.len(), total, total, 0)
        }
        // No tool selected (pathological — `tools` is empty). Open a benign
        // empty-mode dialog so the user can dismiss with Esc.
        None => ZapConfirmState::for_tool(ToolId(""), 0, 0, 0, 0),
    };
    AppState {
        zap_dialog: Some(dialog),
        ..state
    }
}

/// Apply an in-place mutation to the open zap dialog (if any). When no
/// dialog is open, the message is silently ignored (defense in depth — the
/// keymap routes dialog keys only when a dialog is open, but a stray test
/// `Msg::DialogTextInput` would otherwise produce a confusing panic).
fn mutate_dialog<F>(mut state: AppState, f: F) -> AppState
where
    F: FnOnce(&mut ZapConfirmState),
{
    if let Some(dialog) = state.zap_dialog.as_mut() {
        f(dialog);
    }
    state
}

/// Which key triggered the dialog decision. Determines whether `decide_on_enter`
/// or `decide_on_esc` is called.
enum DialogKey {
    Enter,
    Esc,
}

/// Resolve a dialog Confirm/Cancel decision. The unify dialog takes
/// precedence over the zap dialog because it's the most recently opened (a
/// well-formed app never opens both at once, but defense-in-depth picks
/// one). On Confirm with the destructive path, emit the matching trigger
/// so the composition root invokes `link()` (unify) or `delete_all` (zap).
/// In every case, close whichever dialog was open.
fn decide_dialog(state: AppState, key: DialogKey) -> (AppState, UpdateEffect) {
    if state.unify_dialog.is_some() {
        return decide_unify_dialog(state, key);
    }
    if state.zap_dialog.is_some() {
        return decide_zap_dialog(state, key);
    }
    (state, UpdateEffect::default())
}

fn decide_zap_dialog(state: AppState, key: DialogKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.zap_dialog else {
        return (state, UpdateEffect::default());
    };
    let decision = match key {
        DialogKey::Enter => dialog.decide_on_enter(),
        DialogKey::Esc => dialog.decide_on_esc(),
    };
    let trigger_zap = match decision {
        ZapDecision::Confirm if !dialog.is_empty_tool() => Some(dialog.tool),
        _ => None,
    };
    let next_state = AppState {
        zap_dialog: None,
        ..state
    };
    (
        next_state,
        UpdateEffect {
            trigger_zap,
            ..UpdateEffect::default()
        },
    )
}

fn decide_unify_dialog(state: AppState, key: DialogKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.unify_dialog else {
        return (state, UpdateEffect::default());
    };
    let decision = match key {
        DialogKey::Enter => dialog.decide_on_enter(),
        DialogKey::Esc => dialog.decide_on_esc(),
    };
    match decision {
        UnifyDecision::Confirm => {
            let plan = dialog.plan.clone();
            let next_state = AppState {
                unify_dialog: None,
                ..state
            };
            (
                next_state,
                UpdateEffect {
                    trigger_unify: Some(plan),
                    ..UpdateEffect::default()
                },
            )
        }
        UnifyDecision::Cancel => {
            let next_state = AppState {
                unify_dialog: None,
                ..state
            };
            (next_state, UpdateEffect::default())
        }
        UnifyDecision::BackToConfirm => {
            // From DryRunPreview, Esc returns the dialog to Confirm mode
            // with the same plan. The dialog stays open.
            let mut next_state = state;
            if let Some(d) = next_state.unify_dialog.as_mut() {
                d.back_to_confirm();
            }
            (next_state, UpdateEffect::default())
        }
        UnifyDecision::DryRun => {
            // Defensive: decide_on_enter / decide_on_esc never return DryRun.
            // A future change might wire dry-run through DialogKey; until
            // then, no-op.
            (state, UpdateEffect::default())
        }
    }
}

/// US-14: handle `Msg::UnifyDryRun` (the `[n]` key while the unify dialog is
/// open). Reads the dialog mode; when Confirm, emits
/// `UpdateEffect::trigger_dry_run = Some(plan)` so the composition root can
/// invoke `actions::unify::dry_run`. The dialog STAYS OPEN — the dry-run
/// effect later dispatches `Msg::UnifyDryRunCompleted(lines)` which
/// transitions the dialog to `UnifyMode::DryRunPreview { lines }`.
fn decide_dry_run(state: AppState) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.unify_dialog else {
        return (state, UpdateEffect::default());
    };
    if !matches!(dialog.decide_on_dry_run_key(), UnifyDecision::DryRun) {
        // [n] outside Confirm mode is a no-op (AlreadyUnified, DryRunPreview).
        return (state, UpdateEffect::default());
    }
    let plan = dialog.plan.clone();
    (
        state,
        UpdateEffect {
            trigger_dry_run: Some(plan),
            ..UpdateEffect::default()
        },
    )
}

/// US-14: handle `Msg::UnifyDryRunCompleted(lines)`. Transition the open
/// unify dialog into `UnifyMode::DryRunPreview { lines }` so the render
/// layer shows the formatted "(dry-run) Would..." lines. If the dialog has
/// closed for any reason between the dry-run dispatch and its completion,
/// the message is silently dropped (defense in depth).
fn apply_dry_run_lines(mut state: AppState, lines: Vec<String>) -> (AppState, UpdateEffect) {
    if let Some(dialog) = state.unify_dialog.as_mut() {
        dialog.enter_dry_run_preview(lines);
    }
    (state, UpdateEffect::default())
}

/// Which key drove the cross-fs dialog decision (US-19).
enum CrossFsKey {
    Skip,
    Copy,
    Cancel,
}

/// Resolve a cross-fs choice (Skip/Copy/Cancel). On Skip or Copy, close the
/// dialog and emit `trigger_unify = Some(plan)` plus `cross_fs_choice =
/// Some(...)` so the orchestrator routes through the cross-fs aware path. On
/// Cancel, close the dialog with no destructive side-effect (refuse default).
fn decide_cross_fs(state: AppState, key: CrossFsKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.cross_fs_dialog else {
        return (state, UpdateEffect::default());
    };
    let plan = dialog.plan.clone();
    let (trigger_unify, cross_fs_choice) = match key {
        CrossFsKey::Skip => (Some(plan), Some(CrossFsChoice::Skip)),
        CrossFsKey::Copy => (Some(plan), Some(CrossFsChoice::Copy)),
        CrossFsKey::Cancel => (None, None),
    };
    let next_state = AppState {
        cross_fs_dialog: None,
        ..state
    };
    (
        next_state,
        UpdateEffect {
            trigger_unify,
            cross_fs_choice,
            ..UpdateEffect::default()
        },
    )
}
