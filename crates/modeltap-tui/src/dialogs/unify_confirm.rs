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
/// closing the dialog with no destructive side-effect.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum UnifyDecision {
    /// User pressed Enter on the destructive path → run the unify.
    Confirm,
    /// User pressed Esc, OR pressed Enter on the AlreadyUnified informational
    /// path → close dialog, no-op.
    Cancel,
}

/// Which mode the dialog is in. `AlreadyUnified` is the benign US-10.AC-5
/// path: the model has multiple registrations but they all already share an
/// inode, so there is nothing for unify to do.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum UnifyMode {
    /// The destructive path. The plan has at least one link to perform.
    Confirm,
    /// All targets already share the canonical's inode. No action required.
    AlreadyUnified,
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

    /// Decide what Enter does given the current mode. AlreadyUnified is
    /// informational only — Enter cancels just like Esc.
    pub fn decide_on_enter(&self) -> UnifyDecision {
        match self.mode {
            UnifyMode::Confirm => UnifyDecision::Confirm,
            UnifyMode::AlreadyUnified => UnifyDecision::Cancel,
        }
    }

    /// Esc always cancels.
    pub fn decide_on_esc(&self) -> UnifyDecision {
        UnifyDecision::Cancel
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
}
