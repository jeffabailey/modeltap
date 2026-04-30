//! `UnifyDialogState` — the confirmation dialog for the unify action (US-10).
//!
//! Pure state machine. The dialog opens with a snapshot of the unify plan
//! (canonical path, list of targets, bytes-reclaimed estimate). On Enter the
//! orchestrator fires `Effect::trigger_unify` and the action runner calls
//! each plugin's `link()` per the plan. On Esc the dialog closes with no
//! destructive side-effect.
//!
//! Per US-10 the dialog has TWO modes:
//!
//! 1. **Confirm** — the destructive path. The plan has at least one
//!    `PlannedLink` whose `already_linked == false`. Render shows the
//!    canonical, the link list, and the disk reclaim BEFORE any action.
//!    Enter dispatches `UnifyDecision::Confirm`; Esc cancels.
//! 2. **AlreadyUnified** — informational path (US-10.AC-5). All targets
//!    already share the canonical's inode. Render shows a benign "already
//!    unified" message; the dialog accepts only Esc.
//!
//! ADR cites:
//! - ADR-002 §"Conservative deletion" — `bytes_reclaimed` comes from
//!   `UnifyPlan::bytes_reclaimed_estimate` (counted per unique inode, not
//!   per target).
//! - ADR-003 §"No central store" — the canonical is always an EXISTING
//!   tool-owned path; the dialog displays its on-disk path directly.
//! - ADR-008 §"Atomic-or-revert" — the dialog only emits a decision; the
//!   orchestrator in `actions::unify::run` enforces the all-or-nothing
//!   contract.

use modeltap_core::logic::plan::UnifyPlan;

/// Decision returned by `decide_on_enter` / `decide_on_esc`. The `update()`
/// function maps `Confirm` to a `UpdateEffect::trigger_unify` and `Cancel` to
/// closing the dialog with no destructive side-effect. `DryRun` is the
/// US-14 pre-confirmation preview path: `update()` dispatches the dry-run
/// effect (orchestrator calls `actions::unify::dry_run`) and parks the
/// dialog in `UnifyMode::DryRunPreview`. `BackToConfirm` returns from the
/// preview to the destructive Confirm dialog so the user can press Enter or
/// Esc.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UnifyDecision {
    /// User pressed Enter on the destructive path → run the unify.
    Confirm,
    /// User pressed Esc, OR pressed Enter on the AlreadyUnified informational
    /// path → close dialog, no-op.
    Cancel,
    /// User pressed `n` on the destructive path → run the dry-run preview
    /// (no fs mutation; parks the dialog in DryRunPreview mode).
    DryRun,
    /// User pressed Esc from the DryRunPreview path → return to the prior
    /// Confirm dialog with the same plan.
    BackToConfirm,
}

/// Which mode the dialog is in. `AlreadyUnified` is the benign US-10.AC-5
/// path: the model has multiple registrations but they all already share an
/// inode, so there is nothing for unify to do. `DryRunPreview` is the US-14
/// pre-confirmation preview path: pressing `[n]` from `Confirm` walks the
/// plan descriptively without mutating disk and parks the dialog in this
/// mode showing the formatted "(dry-run) Would..." lines. Pressing
/// `[Enter]` from `DryRunPreview` proceeds to the real run with the same
/// plan; `[Esc]` returns to `Confirm`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UnifyMode {
    /// The destructive path. The plan has at least one link to perform.
    Confirm,
    /// All targets already share the canonical's inode. No action required.
    AlreadyUnified,
    /// US-14 dry-run preview. The dialog shows the formatted "(dry-run)
    /// Would..." lines produced by `actions::unify::dry_run`. Pressing
    /// `[Enter]` proceeds to real run; `[Esc]` returns to `Confirm`.
    DryRunPreview {
        /// Pre-formatted lines from the orchestrator's `DryRunOutcome`.
        /// Owned by the dialog so the render layer is decoupled from the
        /// dry_run function itself (ADR-006: state stays in the TUI crate).
        lines: Vec<String>,
    },
}

/// Pure state for the unify confirmation dialog. The full `UnifyPlan` is
/// carried so the render layer has the canonical path, the list of targets
/// to link, and the disk reclaim estimate without re-querying the inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifyDialogState {
    /// The plan the dialog is showing. Carried to the orchestrator on
    /// confirm so the action runner has everything it needs.
    pub plan: UnifyPlan,
    pub mode: UnifyMode,
}

impl UnifyDialogState {
    /// Construct a dialog state from a `UnifyPlan`. The mode is derived from
    /// the plan: if every link is `already_linked`, mode is `AlreadyUnified`;
    /// otherwise `Confirm`.
    pub fn from_plan(plan: UnifyPlan) -> Self {
        let mode = if plan.links.iter().all(|l| l.already_linked) && !plan.links.is_empty() {
            UnifyMode::AlreadyUnified
        } else {
            UnifyMode::Confirm
        };
        Self { plan, mode }
    }

    /// True when the dialog is in the benign "already unified" path.
    pub fn is_already_unified(&self) -> bool {
        matches!(self.mode, UnifyMode::AlreadyUnified)
    }

    /// True when the dialog is in the US-14 dry-run preview mode.
    pub fn is_dry_run_preview(&self) -> bool {
        matches!(self.mode, UnifyMode::DryRunPreview { .. })
    }

    /// Transition this dialog into the US-14 DryRunPreview mode carrying the
    /// pre-formatted "(dry-run) Would..." lines. Called by `update()` after
    /// the orchestrator's dry_run effect produces a `DryRunOutcome` and the
    /// `Msg::UnifyDryRunCompleted(...)` Msg arrives. The plan is preserved
    /// unchanged so pressing Enter from the preview proceeds to real run
    /// with the SAME plan value (ADR-006).
    pub fn enter_dry_run_preview(&mut self, lines: Vec<String>) {
        self.mode = UnifyMode::DryRunPreview { lines };
    }

    /// Decide what Enter does given the current mode. AlreadyUnified is
    /// informational only — Enter cancels just like Esc. From DryRunPreview,
    /// Enter proceeds to real run with the same plan.
    pub fn decide_on_enter(&self) -> UnifyDecision {
        match self.mode {
            UnifyMode::Confirm => UnifyDecision::Confirm,
            UnifyMode::AlreadyUnified => UnifyDecision::Cancel,
            UnifyMode::DryRunPreview { .. } => UnifyDecision::Confirm,
        }
    }

    /// Esc always cancels — closes the dialog with no destructive
    /// side-effect. From DryRunPreview this also exits cleanly per US-14
    /// AC: "After dry-run, [Enter] proceeds with the same plan, [Esc]
    /// cancels."
    pub fn decide_on_esc(&self) -> UnifyDecision {
        UnifyDecision::Cancel
    }

    /// Decide what `[n]` does. Only meaningful from the destructive Confirm
    /// path; from any other mode (`AlreadyUnified`, `DryRunPreview`) it is a
    /// no-op (Cancel — the dialog ignores the keystroke). The orchestrator
    /// uses this branch to gate `Effect::trigger_dry_run` emission.
    pub fn decide_on_dry_run_key(&self) -> UnifyDecision {
        match self.mode {
            UnifyMode::Confirm => UnifyDecision::DryRun,
            // From AlreadyUnified or DryRunPreview, the [n] key is ignored.
            _ => UnifyDecision::Cancel,
        }
    }

    /// Transition back from DryRunPreview to Confirm — used by `update()`
    /// when the user presses Esc from the preview. Defensive: if not in
    /// preview, no-op.
    pub fn back_to_confirm(&mut self) {
        if matches!(self.mode, UnifyMode::DryRunPreview { .. }) {
            self.mode = UnifyMode::Confirm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::logic::plan::{PlanCandidate, PlannedLink};
    use modeltap_core::ToolId;
    use std::path::PathBuf;

    fn cand(tool: &'static str, path: &str, exists: bool) -> PlanCandidate {
        PlanCandidate {
            tool: ToolId(tool),
            path: PathBuf::from(path),
            exists,
            device: 1,
            inode: 100,
            size_bytes: 1024,
        }
    }

    fn plan_with_links(links: Vec<PlannedLink>, bytes_reclaimed: u64) -> UnifyPlan {
        UnifyPlan {
            canonical: cand("ollama", "/c", true),
            links,
            bytes_reclaimed_estimate: bytes_reclaimed,
        }
    }

    #[test]
    fn from_plan_with_unlinked_targets_is_confirm_mode() {
        let plan = plan_with_links(
            vec![PlannedLink {
                tool: ToolId("hf"),
                target: PathBuf::from("/h"),
                cross_filesystem: false,
                already_linked: false,
            }],
            1024,
        );
        let dialog = UnifyDialogState::from_plan(plan);
        assert_eq!(dialog.mode, UnifyMode::Confirm);
        assert!(!dialog.is_already_unified());
    }

    #[test]
    fn from_plan_where_all_targets_already_linked_is_already_unified_mode() {
        let plan = plan_with_links(
            vec![
                PlannedLink {
                    tool: ToolId("hf"),
                    target: PathBuf::from("/h"),
                    cross_filesystem: false,
                    already_linked: true,
                },
                PlannedLink {
                    tool: ToolId("llama-cli"),
                    target: PathBuf::from("/l"),
                    cross_filesystem: false,
                    already_linked: true,
                },
            ],
            0,
        );
        let dialog = UnifyDialogState::from_plan(plan);
        assert_eq!(dialog.mode, UnifyMode::AlreadyUnified);
        assert!(dialog.is_already_unified());
    }

    #[test]
    fn empty_links_is_confirm_mode_so_dialog_is_not_silently_dismissed() {
        // Defensive: a plan with zero links shouldn't masquerade as
        // AlreadyUnified — the user should still see SOMETHING.
        let plan = plan_with_links(vec![], 0);
        let dialog = UnifyDialogState::from_plan(plan);
        assert_eq!(dialog.mode, UnifyMode::Confirm);
    }

    #[test]
    fn enter_in_confirm_mode_returns_confirm_decision() {
        let plan = plan_with_links(
            vec![PlannedLink {
                tool: ToolId("hf"),
                target: PathBuf::from("/h"),
                cross_filesystem: false,
                already_linked: false,
            }],
            1024,
        );
        let dialog = UnifyDialogState::from_plan(plan);
        assert_eq!(dialog.decide_on_enter(), UnifyDecision::Confirm);
    }

    #[test]
    fn enter_in_already_unified_mode_returns_cancel_decision() {
        let plan = plan_with_links(
            vec![PlannedLink {
                tool: ToolId("hf"),
                target: PathBuf::from("/h"),
                cross_filesystem: false,
                already_linked: true,
            }],
            0,
        );
        let dialog = UnifyDialogState::from_plan(plan);
        assert_eq!(dialog.decide_on_enter(), UnifyDecision::Cancel);
    }

    #[test]
    fn esc_always_cancels_regardless_of_mode() {
        let p1 = plan_with_links(
            vec![PlannedLink {
                tool: ToolId("hf"),
                target: PathBuf::from("/h"),
                cross_filesystem: false,
                already_linked: false,
            }],
            1024,
        );
        let p2 = plan_with_links(
            vec![PlannedLink {
                tool: ToolId("hf"),
                target: PathBuf::from("/h"),
                cross_filesystem: false,
                already_linked: true,
            }],
            0,
        );
        assert_eq!(
            UnifyDialogState::from_plan(p1).decide_on_esc(),
            UnifyDecision::Cancel
        );
        assert_eq!(
            UnifyDialogState::from_plan(p2).decide_on_esc(),
            UnifyDecision::Cancel
        );
    }

    // ---- US-14 dry-run preview state machine -----------------------------

    fn make_confirm_dialog() -> UnifyDialogState {
        let plan = plan_with_links(
            vec![PlannedLink {
                tool: ToolId("hf"),
                target: PathBuf::from("/h/a.bin"),
                cross_filesystem: false,
                already_linked: false,
            }],
            4096,
        );
        UnifyDialogState::from_plan(plan)
    }

    #[test]
    fn dry_run_key_in_confirm_mode_returns_dry_run_decision() {
        let dialog = make_confirm_dialog();
        assert_eq!(dialog.decide_on_dry_run_key(), UnifyDecision::DryRun);
    }

    #[test]
    fn enter_dry_run_preview_transitions_mode_and_carries_lines() {
        let mut dialog = make_confirm_dialog();
        let lines = vec![
            "(dry-run) Would create canonical at /c".to_string(),
            "(dry-run) Reclaim: 4 KB".to_string(),
        ];
        dialog.enter_dry_run_preview(lines.clone());
        assert!(dialog.is_dry_run_preview());
        if let UnifyMode::DryRunPreview { lines: stored } = &dialog.mode {
            assert_eq!(stored, &lines);
        } else {
            panic!("expected DryRunPreview mode");
        }
    }

    #[test]
    fn enter_in_dry_run_preview_proceeds_to_real_run() {
        // From DryRunPreview, Enter proceeds with the SAME plan to real run.
        let mut dialog = make_confirm_dialog();
        dialog.enter_dry_run_preview(vec!["(dry-run) preview".to_string()]);
        assert_eq!(dialog.decide_on_enter(), UnifyDecision::Confirm);
    }

    #[test]
    fn esc_in_dry_run_preview_cancels_per_us_14_ac() {
        // Per US-14 AC: "After dry-run, [Enter] proceeds with the same plan,
        // [Esc] cancels." Esc from DryRunPreview closes the dialog with no
        // destructive side-effect.
        let mut dialog = make_confirm_dialog();
        dialog.enter_dry_run_preview(vec!["(dry-run) preview".to_string()]);
        assert_eq!(dialog.decide_on_esc(), UnifyDecision::Cancel);
    }

    #[test]
    fn dry_run_key_in_already_unified_is_ignored() {
        let plan = plan_with_links(
            vec![PlannedLink {
                tool: ToolId("hf"),
                target: PathBuf::from("/h"),
                cross_filesystem: false,
                already_linked: true,
            }],
            0,
        );
        let dialog = UnifyDialogState::from_plan(plan);
        assert!(dialog.is_already_unified());
        // [n] in AlreadyUnified is a no-op (Cancel sentinel).
        assert_eq!(dialog.decide_on_dry_run_key(), UnifyDecision::Cancel);
    }
}
