//! `CrossFsChoiceDialog` — the per-target cross-filesystem fallback prompt
//! for unify (US-19, ADR-008).
//!
//! Pure state machine. Opened by the headless / production event loop when
//! the unify planner detects 1+ target whose `cross_filesystem == true`.
//! Per ADR-008 OQ-4, the default policy is **refuse-and-ask**: there is no
//! silent default that mutates disk. The dialog presents three options:
//!
//! - `[s] skip` — leave each cross-fs target untouched at its original path.
//!   Same-filesystem targets are still linked normally.
//! - `[c] copy` — duplicate the canonical's bytes to each cross-fs target
//!   (atomic write+rename). No reclaim for the cross-fs target. Same-fs
//!   targets are still linked normally.
//! - `[x] cancel` — abort the entire unify. Nothing is changed: even
//!   same-fs targets that haven't been linked yet are NOT linked
//!   (transactional intent).
//!
//! The default-on-Enter is `Cancel` (refuse). This is the cardinal contract
//! from ADR-008 — **never silent copy that wastes disk**. The user has to
//! type `s` or `c` explicitly to opt in to the destructive paths.
//!
//! The dialog has two modes:
//!
//! - `Mixed` — some targets cross-fs, some same-fs. The three options apply
//!   to the cross-fs subset; same-fs targets are linked silently regardless.
//! - `AllCrossFs` — every target is cross-fs. The dialog shows a refusal
//!   message: "all targets on different filesystems — unify cannot proceed"
//!   per US-19 example 3. Skip and Copy are still selectable (skip = no-op,
//!   copy = duplicate bytes everywhere); Cancel is the natural default.

use modeltap_core::logic::plan::UnifyPlan;

/// One per-target choice for the cross-fs fallback. Mirrors the [s/c/x]
/// option labels in the UI exactly.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CrossFsChoice {
    /// Leave each cross-fs target untouched at its original path. No disk
    /// reclaim for the cross-fs subset; same-fs targets still link.
    Skip,
    /// Duplicate the canonical's bytes to each cross-fs target. No reclaim
    /// for the cross-fs subset; same-fs targets still link.
    Copy,
}

/// Dispatch outcome from a key press. The composition root maps `Skip` /
/// `Copy` to `UpdateEffect::trigger_unify_with_cross_fs` and `Cancel` to
/// closing the dialog with no destructive side-effect.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CrossFsDecision {
    /// User pressed `s` — proceed with the unify, skipping cross-fs targets.
    Confirm(CrossFsChoice),
    /// User pressed `x` OR Enter (default) OR Esc — abort the unify entirely.
    Cancel,
}

/// Which mode the dialog is in. `AllCrossFs` is the US-19 example-3 path:
/// every target is on a different filesystem from the canonical, so the
/// dialog shows a refusal message but still accepts the per-target choice
/// keys (skip = no-op, copy = duplicate bytes for every target).
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum CrossFsMode {
    /// Some targets are cross-fs, some are same-fs.
    Mixed,
    /// Every target is cross-fs.
    AllCrossFs,
}

/// Pure state for the cross-fs choice dialog. The full `UnifyPlan` is
/// carried so the orchestrator has everything it needs on confirmation
/// (canonical path, per-target same-fs flags, bytes-reclaim estimate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossFsChoiceDialog {
    pub plan: UnifyPlan,
    pub mode: CrossFsMode,
}

impl CrossFsChoiceDialog {
    /// Construct the dialog from a plan. The mode is derived from the plan:
    /// if EVERY non-already-linked target has `cross_filesystem == true`,
    /// the mode is `AllCrossFs`; otherwise `Mixed`. Caller is responsible
    /// for only constructing this dialog when the plan has 1+ cross-fs
    /// target — `from_plan_for_cross_fs` panics otherwise (defense in depth).
    pub fn from_plan(plan: UnifyPlan) -> Self {
        let active: Vec<_> = plan.links.iter().filter(|l| !l.already_linked).collect();
        debug_assert!(
            !active.is_empty() && active.iter().any(|l| l.cross_filesystem),
            "CrossFsChoiceDialog::from_plan called with no cross-fs targets"
        );
        let all_cross_fs = !active.is_empty() && active.iter().all(|l| l.cross_filesystem);
        let mode = if all_cross_fs {
            CrossFsMode::AllCrossFs
        } else {
            CrossFsMode::Mixed
        };
        Self { plan, mode }
    }

    /// Number of cross-fs targets among the active (non-already-linked) links.
    /// Used by the render layer for the "N of M targets on different
    /// filesystem" header.
    pub fn cross_fs_count(&self) -> usize {
        self.plan
            .links
            .iter()
            .filter(|l| !l.already_linked && l.cross_filesystem)
            .count()
    }

    /// Total active (non-already-linked) target count. Denominator for the
    /// "N of M targets" header.
    pub fn active_count(&self) -> usize {
        self.plan.links.iter().filter(|l| !l.already_linked).count()
    }

    /// True iff this is the all-cross-fs case (US-19 example 3 — refusal).
    pub fn is_all_cross_fs(&self) -> bool {
        matches!(self.mode, CrossFsMode::AllCrossFs)
    }

    /// Decide what `[s]` does — proceed with skip semantics.
    pub fn decide_on_skip(&self) -> CrossFsDecision {
        CrossFsDecision::Confirm(CrossFsChoice::Skip)
    }

    /// Decide what `[c]` does — proceed with copy semantics.
    pub fn decide_on_copy(&self) -> CrossFsDecision {
        CrossFsDecision::Confirm(CrossFsChoice::Copy)
    }

    /// Decide what `[x]` does — abort the unify entirely.
    pub fn decide_on_cancel(&self) -> CrossFsDecision {
        CrossFsDecision::Cancel
    }

    /// Decide what Enter does. Per ADR-008 OQ-4 the default is **refuse**:
    /// pressing Enter at the cross-fs prompt cancels rather than silently
    /// copying. The user must explicitly type `s` or `c` to opt in.
    pub fn decide_on_enter(&self) -> CrossFsDecision {
        CrossFsDecision::Cancel
    }

    /// Esc always cancels — same as Enter at this dialog.
    pub fn decide_on_esc(&self) -> CrossFsDecision {
        CrossFsDecision::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::logic::plan::{PlanCandidate, PlannedLink};
    use modeltap_core::ToolId;
    use std::path::PathBuf;

    fn cand(tool: &'static str, path: &str) -> PlanCandidate {
        PlanCandidate {
            tool: ToolId(tool),
            path: PathBuf::from(path),
            exists: true,
            device: 1,
            inode: 100,
            size_bytes: 1024,
        }
    }

    fn link(tool: &'static str, path: &str, cross_fs: bool, already: bool) -> PlannedLink {
        PlannedLink {
            tool: ToolId(tool),
            target: PathBuf::from(path),
            cross_filesystem: cross_fs,
            already_linked: already,
        }
    }

    fn plan_with(links: Vec<PlannedLink>) -> UnifyPlan {
        UnifyPlan {
            canonical: cand("ollama", "/c"),
            links,
            bytes_reclaimed_estimate: 1024,
        }
    }

    #[test]
    fn mixed_mode_when_some_targets_cross_fs() {
        let p = plan_with(vec![
            link("hf", "/h", false, false),
            link("llama-cli", "/l", true, false),
        ]);
        let dlg = CrossFsChoiceDialog::from_plan(p);
        assert_eq!(dlg.mode, CrossFsMode::Mixed);
        assert!(!dlg.is_all_cross_fs());
        assert_eq!(dlg.cross_fs_count(), 1);
        assert_eq!(dlg.active_count(), 2);
    }

    #[test]
    fn all_cross_fs_mode_when_every_active_target_cross_fs() {
        let p = plan_with(vec![
            link("hf", "/h", true, false),
            link("llama-cli", "/l", true, false),
        ]);
        let dlg = CrossFsChoiceDialog::from_plan(p);
        assert_eq!(dlg.mode, CrossFsMode::AllCrossFs);
        assert!(dlg.is_all_cross_fs());
        assert_eq!(dlg.cross_fs_count(), 2);
        assert_eq!(dlg.active_count(), 2);
    }

    #[test]
    fn already_linked_targets_excluded_from_counts() {
        let p = plan_with(vec![
            link("hf", "/h", true, false),
            link("llama-cli", "/l", false, true), // pre-unified, ignored
        ]);
        let dlg = CrossFsChoiceDialog::from_plan(p);
        // Only one active target, and it's cross-fs → AllCrossFs mode.
        assert_eq!(dlg.mode, CrossFsMode::AllCrossFs);
        assert_eq!(dlg.cross_fs_count(), 1);
        assert_eq!(dlg.active_count(), 1);
    }

    #[test]
    fn skip_returns_confirm_skip_decision() {
        let p = plan_with(vec![
            link("hf", "/h", false, false),
            link("llama-cli", "/l", true, false),
        ]);
        let dlg = CrossFsChoiceDialog::from_plan(p);
        assert_eq!(
            dlg.decide_on_skip(),
            CrossFsDecision::Confirm(CrossFsChoice::Skip)
        );
    }

    #[test]
    fn copy_returns_confirm_copy_decision() {
        let p = plan_with(vec![
            link("hf", "/h", false, false),
            link("llama-cli", "/l", true, false),
        ]);
        let dlg = CrossFsChoiceDialog::from_plan(p);
        assert_eq!(
            dlg.decide_on_copy(),
            CrossFsDecision::Confirm(CrossFsChoice::Copy)
        );
    }

    #[test]
    fn cancel_x_returns_cancel_decision() {
        let p = plan_with(vec![link("llama-cli", "/l", true, false)]);
        let dlg = CrossFsChoiceDialog::from_plan(p);
        assert_eq!(dlg.decide_on_cancel(), CrossFsDecision::Cancel);
    }

    /// ADR-008 default-on-Enter contract: Enter must REFUSE the unify when a
    /// cross-fs target is present. No silent default.
    #[test]
    fn enter_default_is_cancel_per_adr_008_refuse_policy() {
        let p = plan_with(vec![
            link("hf", "/h", false, false),
            link("llama-cli", "/l", true, false),
        ]);
        let dlg = CrossFsChoiceDialog::from_plan(p);
        assert_eq!(
            dlg.decide_on_enter(),
            CrossFsDecision::Cancel,
            "ADR-008: default on Enter must be REFUSE — never silent copy"
        );
    }

    #[test]
    fn esc_always_cancels() {
        let p = plan_with(vec![link("llama-cli", "/l", true, false)]);
        let dlg = CrossFsChoiceDialog::from_plan(p);
        assert_eq!(dlg.decide_on_esc(), CrossFsDecision::Cancel);
    }
}
