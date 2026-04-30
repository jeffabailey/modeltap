//! `RunningToolDialog` — the close-and-retry prompt for unify and delete-one
//! when a running tool process holds an in-scope file open (US-17, intake Q5).
//!
//! Pure state. Opened by the headless / production event loop after
//! `FsProbe::detect_running_tools` returns either:
//!
//! - `Ok(non_empty)` → mode = `Detected { processes }`. Dialog wording:
//!   `<tool> is running and has this file open. Close <tool> and retry.`
//!   The user MUST close the tool and press `[r]` to retry; pressing `[Esc]`
//!   cancels the action.
//!
//! - `Err(LsofUnavailable)` → mode = `LsofUnavailable`. Dialog wording:
//!   `Running-tool detection unavailable on this system`. The user can
//!   press `[r]` to proceed at own risk, or `[Esc]` to cancel.
//!
//! Per intake Q5 this is **detect-and-prompt-then-retry**, NOT soft-warning.
//! No filesystem mutation may happen while the dialog is open. The retry
//! semantics close the dialog and have the orchestrator re-run the
//! `detect_running_tools` probe; only when it returns `Ok(empty)` does the
//! action proceed.

use modeltap_core::ports::fs_probe::RunningProcess;

/// What the user pressed in response to the prompt. The composition root
/// translates `Retry` into a re-probe + (on success) re-dispatch of the
/// gated action; `Cancel` into closing the dialog with no destructive
/// side-effect; `ProceedAnyway` into bypassing the gate (only valid when
/// the dialog is in `LsofUnavailable` mode — the user is acknowledging
/// the safety check was skipped).
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RunningToolDecision {
    /// User pressed `[r]` while in `Detected` mode — retry the probe; if it
    /// now returns empty, proceed with the gated action. The orchestrator
    /// owns the re-probe + re-dispatch.
    Retry,
    /// User pressed `[r]` while in `LsofUnavailable` mode — proceed with
    /// the gated action despite the missing safety check.
    ProceedAnyway,
    /// User pressed `[Esc]` (any mode) — abort the gated action entirely.
    Cancel,
}

/// Which mode the dialog is in. Determines both the wording AND the
/// semantics of `[r]` (retry vs proceed-anyway).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunningToolMode {
    /// `lsof` returned 1+ matching process. The dialog lists each process
    /// (`<tool_name> (PID <pid>)`) so the user knows which app to close.
    Detected { processes: Vec<RunningProcess> },
    /// `lsof` is missing on this system. The dialog reads "Running-tool
    /// detection unavailable on this system" and lets the user proceed at
    /// own risk.
    LsofUnavailable,
}

/// Pure state for the running-tool prompt. Stored in
/// `AppState.running_tool_dialog` as `Option<RunningToolDialog>`; the render
/// layer overlays a centered modal when set, and the update layer routes
/// `[r]` / `[Esc]` keys to `decide_on_retry()` / `decide_on_cancel()`.
///
/// The `pending_action` field carries the action the user was attempting
/// when the gate refused. On `[r]` retry that succeeds, the orchestrator
/// re-dispatches this action; on `[Esc]` it is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningToolDialog {
    pub mode: RunningToolMode,
    pub pending_action: PendingGatedAction,
}

/// What action was gated when this dialog was raised. The composition root
/// uses this to know what to re-dispatch on `[r]` retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingGatedAction {
    /// The user pressed `[u]` to unify; the gate refused. On retry, re-run
    /// the unify gate + (if clear) the unify orchestrator.
    Unify,
    /// The user pressed `[d]` to delete-from-one; the gate refused. On
    /// retry, re-run the delete-one gate + orchestrator.
    DeleteOne,
}

impl RunningToolDialog {
    /// Construct the dialog for the `Detected` case (one or more running
    /// tools found). At least one process must be supplied — an empty list
    /// makes no sense (the gate would have proceeded). Defense in depth:
    /// caller is responsible.
    pub fn detected(processes: Vec<RunningProcess>, action: PendingGatedAction) -> Self {
        debug_assert!(
            !processes.is_empty(),
            "RunningToolDialog::detected called with no processes"
        );
        Self {
            mode: RunningToolMode::Detected { processes },
            pending_action: action,
        }
    }

    /// Construct the dialog for the `LsofUnavailable` case. The user can
    /// still proceed at own risk — `[r]` becomes "proceed anyway".
    pub fn lsof_unavailable(action: PendingGatedAction) -> Self {
        Self {
            mode: RunningToolMode::LsofUnavailable,
            pending_action: action,
        }
    }

    /// True iff the dialog is in Detected mode (running tool surfaced).
    pub fn is_detected(&self) -> bool {
        matches!(self.mode, RunningToolMode::Detected { .. })
    }

    /// True iff the dialog is in LsofUnavailable mode.
    pub fn is_lsof_unavailable(&self) -> bool {
        matches!(self.mode, RunningToolMode::LsofUnavailable)
    }

    /// Decide what `[r]` does. In `Detected` mode it retries (re-probes);
    /// in `LsofUnavailable` mode it proceeds anyway (the user has
    /// acknowledged the safety check was skipped).
    pub fn decide_on_retry(&self) -> RunningToolDecision {
        match self.mode {
            RunningToolMode::Detected { .. } => RunningToolDecision::Retry,
            RunningToolMode::LsofUnavailable => RunningToolDecision::ProceedAnyway,
        }
    }

    /// Decide what `[Esc]` does — always cancel.
    pub fn decide_on_cancel(&self) -> RunningToolDecision {
        RunningToolDecision::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn one_running_ollama() -> Vec<RunningProcess> {
        vec![RunningProcess {
            tool_name: "ollama".to_string(),
            pid: 1234,
            path: PathBuf::from("/tmp/blob"),
        }]
    }

    /// B5: Detected-mode constructor stores the process list and unify action.
    #[test]
    fn detected_mode_carries_process_list_and_action() {
        let dlg = RunningToolDialog::detected(one_running_ollama(), PendingGatedAction::Unify);
        assert!(dlg.is_detected(), "must report Detected mode");
        assert!(!dlg.is_lsof_unavailable());
        assert_eq!(dlg.pending_action, PendingGatedAction::Unify);
        match &dlg.mode {
            RunningToolMode::Detected { processes } => {
                assert_eq!(processes.len(), 1);
                assert_eq!(processes[0].tool_name, "ollama");
                assert_eq!(processes[0].pid, 1234);
            }
            other => panic!("expected Detected mode, got {:?}", other),
        }
    }

    /// B6: LsofUnavailable mode reflected in is_lsof_unavailable.
    #[test]
    fn lsof_unavailable_mode_marks_correct_branch() {
        let dlg = RunningToolDialog::lsof_unavailable(PendingGatedAction::DeleteOne);
        assert!(
            dlg.is_lsof_unavailable(),
            "must report LsofUnavailable mode"
        );
        assert!(!dlg.is_detected());
        assert_eq!(dlg.pending_action, PendingGatedAction::DeleteOne);
    }

    /// B7: [r] in Detected mode = Retry.
    #[test]
    fn r_in_detected_mode_yields_retry() {
        let dlg = RunningToolDialog::detected(one_running_ollama(), PendingGatedAction::Unify);
        assert_eq!(dlg.decide_on_retry(), RunningToolDecision::Retry);
    }

    /// B9: [r] in LsofUnavailable mode = ProceedAnyway.
    #[test]
    fn r_in_lsof_unavailable_mode_yields_proceed_anyway() {
        let dlg = RunningToolDialog::lsof_unavailable(PendingGatedAction::Unify);
        assert_eq!(
            dlg.decide_on_retry(),
            RunningToolDecision::ProceedAnyway,
            "in LsofUnavailable mode, [r] must mean ProceedAnyway"
        );
    }

    /// B8: [Esc] always cancels regardless of mode.
    #[test]
    fn esc_cancels_in_detected_mode() {
        let dlg = RunningToolDialog::detected(one_running_ollama(), PendingGatedAction::Unify);
        assert_eq!(dlg.decide_on_cancel(), RunningToolDecision::Cancel);
    }

    #[test]
    fn esc_cancels_in_lsof_unavailable_mode() {
        let dlg = RunningToolDialog::lsof_unavailable(PendingGatedAction::DeleteOne);
        assert_eq!(dlg.decide_on_cancel(), RunningToolDecision::Cancel);
    }

    /// Pending action round-trips for both kinds.
    #[test]
    fn pending_action_round_trips_unify_and_delete_one() {
        let unify_dlg =
            RunningToolDialog::detected(one_running_ollama(), PendingGatedAction::Unify);
        assert_eq!(unify_dlg.pending_action, PendingGatedAction::Unify);
        let delete_dlg =
            RunningToolDialog::detected(one_running_ollama(), PendingGatedAction::DeleteOne);
        assert_eq!(delete_dlg.pending_action, PendingGatedAction::DeleteOne);
    }
}
