//! `Msg` — the Elm-style message type for the TUI (per ADR-006).
//!
//! Every keystroke (and every async-arriving event in later steps) becomes
//! one variant of this enum. The pure `update::update()` function consumes
//! a `Msg` and returns the next `AppState` plus a description of any side
//! effects (`UpdateEffect`).

use modeltap_core::domain::last_action::LastAction;
use modeltap_core::logic::plan::UnifyPlan;

use crate::app_state::ToolView;
use crate::screens::detail::DetailScreenState;

/// All the messages that can drive `update()`. Step 01-03 covers keyboard
/// navigation; later steps add discovery-progress, action-completion, and
/// tick variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// User pressed `q`. Clean shutdown, exit 0.
    Quit,
    /// User pressed Ctrl+C. Shutdown with POSIX SIGINT exit code (130).
    CtrlC,
    /// Right Arrow — advance to the next tool slot (cycles).
    SelectNextTool,
    /// Left Arrow — regress to the previous tool slot (cycles).
    SelectPrevTool,
    /// Down Arrow — advance to the next row in the current tool.
    SelectNextRow,
    /// Up Arrow — regress to the previous row in the current tool.
    SelectPrevRow,
    /// Tab — toggle focus between left and right panes.
    ToggleFocus,

    // -----------------------------------------------------------------------
    // US-05 zap-all dialog. The dialog state machine lives in
    // `dialogs::zap_confirm`; these variants are how the keymap drives it
    // through `update()`.
    // -----------------------------------------------------------------------
    /// User pressed `z`. Open the zap-confirmation dialog for the
    /// currently-selected tool. No payload — `update()` reads the selection
    /// from `AppState`.
    ZapTool,
    /// One printable character pressed while a typed-input dialog is open.
    /// Appended to the dialog's input buffer.
    DialogTextInput(char),
    /// Backspace pressed while a typed-input dialog is open. Removes the
    /// last character from the dialog's input buffer (no-op when empty).
    DialogBackspace,
    /// Enter pressed while a dialog is open. The dialog's `decide_on_enter`
    /// determines confirm-vs-cancel.
    DialogConfirm,
    /// Esc pressed while a dialog is open. Always cancels per US-05 AC-3.
    DialogCancel,

    // -----------------------------------------------------------------------
    // US-06 post-action banner (in-memory only, per intake Q7).
    // -----------------------------------------------------------------------
    /// Composition root dispatches this after an action effect completes
    /// (zap in WS scope; unify/delete in 03-02/03-06). `update()` writes the
    /// payload into `AppState.last_action`.
    SetLastAction(LastAction),
    /// Composition root dispatches this after re-running discovery for a
    /// single tool (per US-06.AC-4 / US-11.AC-1: incremental refresh stays
    /// under 500 ms by NOT re-running every plugin's discover()). `update()`
    /// replaces the matching `ToolView` slot in `AppState.tools`.
    RefreshTool(ToolView),

    // -----------------------------------------------------------------------
    // US-13 per-model detail screen.
    // -----------------------------------------------------------------------
    /// User pressed Enter on a model row in the main view. The composition
    /// root has assembled the `DetailScreenState` (with cross-tool registrations
    /// and an optional already-cached SHA-256). `update()` writes
    /// `current_screen = Screen::Detail(...)`.
    OpenDetail(DetailScreenState),
    /// User pressed Esc while on the detail screen. `update()` writes
    /// `current_screen = Screen::Main` and preserves the prior selection.
    CloseDetail,

    // -----------------------------------------------------------------------
    // US-08 help overlay + cross-step shortcut placeholders.
    // -----------------------------------------------------------------------
    /// User pressed `?`. Layer the help overlay on top of the current screen
    /// (via `Screen::Help { previous }`); pressing `?` again or Esc restores
    /// the previous screen with selection state intact.
    ToggleHelp,
    /// User pressed `u` (unify). Bound here so the bottom-bar INT-6 invariant
    /// holds (every visible shortcut maps to a non-noop Msg). On the main
    /// screen this is a no-op (unify needs the per-model context the detail
    /// screen provides) — see `Msg::OpenUnifyDialog` for the productive path.
    Unify,
    /// Composition root dispatches this when the user presses `u` on a
    /// multi-tool model in the detail screen. Carries the `UnifyPlan` the
    /// orchestrator built from the cross-tool registrations. `update()`
    /// writes `unify_dialog = Some(UnifyDialogState::from_plan(plan))`.
    OpenUnifyDialog(UnifyPlan),

    // -----------------------------------------------------------------------
    // US-19 cross-filesystem fallback dialog (ADR-008, step 03-03).
    //
    // When the unify planner detects 1+ cross-fs target, the orchestrator
    // dispatches `OpenCrossFsDialog` instead of `OpenUnifyDialog`. The dialog
    // accepts [s] / [c] / [x] keys (mapped to the three Msgs below) and
    // Enter/Esc default to refuse (Cancel) per ADR-008's no-silent-copy rule.
    // -----------------------------------------------------------------------
    /// Composition root dispatches this when `u` is pressed and the plan has
    /// 1+ cross-fs target. Carries the `UnifyPlan` so the orchestrator has
    /// the per-target same-fs flags + canonical path on confirmation.
    OpenCrossFsDialog(UnifyPlan),
    /// User pressed `s` while the cross-fs dialog is open — proceed with
    /// skip semantics (cross-fs targets untouched; same-fs targets linked).
    CrossFsSkip,
    /// User pressed `c` while the cross-fs dialog is open — proceed with
    /// copy semantics (cross-fs targets duplicated; same-fs targets linked).
    CrossFsCopy,
    /// User pressed `x` (or Enter / Esc per ADR-008 refuse default) while
    /// the cross-fs dialog is open — abort the unify entirely.
    CrossFsCancel,

    /// User pressed `d` (delete-from-one). Bound here so the bottom-bar INT-6
    /// invariant holds. Wired to the delete-from-one effect in 03-06.
    DeleteFromOne,

    /// Any unrecognized key. No-op per US-03 AC-6 (silently ignored).
    UnboundKey,
}
