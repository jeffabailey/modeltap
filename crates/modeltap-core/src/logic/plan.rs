//! Pure-domain unify-plan builder (US-10, ADR-002, ADR-003).
//!
//! Given a chosen `canonical` path and a list of `target_paths` (one per
//! other tool that registers the same content), produce a `UnifyPlan` —
//! the data the action orchestrator (deferred to step 03-02b) will execute
//! by calling `Tool::link` on each plugin.
//!
//! ## Decision rules
//!
//! 1. The canonical path is excluded from the link list — you do not link
//!    the canonical to itself.
//! 2. A target whose `inode == canonical_inode` is excluded as a no-op (the
//!    file is already hardlinked into the canonical's chain).
//! 3. Targets whose `device != canonical_device` are flagged as
//!    `cross_filesystem` — the planner does NOT skip them; the orchestrator
//!    handles the [s/c/x] dialog (deferred to 03-03).
//! 4. Total `bytes_reclaimed_estimate` = `(unique_inodes_to_replace) * size`.
//!
//! ## Purity contract
//!
//! No I/O, no async. Inputs in → plan out. The orchestrator is responsible
//! for invoking `FsProbe::dev_and_inode` to produce the input slice.

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Serialize;

use crate::types::ToolId;

/// One target the unify action would link to the canonical. Constructed by
/// the orchestrator after `stat`-ing each tool's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanCandidate {
    pub tool: ToolId,
    pub path: PathBuf,
    pub exists: bool,
    /// Filesystem device id from `stat`. Compared against the canonical's
    /// device to detect `EXDEV`-prone links.
    pub device: u64,
    pub inode: u64,
    pub size_bytes: u64,
}

/// What the planner returns for one tool's path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedLink {
    pub tool: ToolId,
    pub target: PathBuf,
    /// True if this link would cross filesystems. The orchestrator surfaces
    /// the [s/c/x] dialog (skip / copy / cancel) per US-19.
    pub cross_filesystem: bool,
    /// True if the target already shares the canonical's inode and the link
    /// is a no-op. Surfaced so the action emits `LinkResult::AlreadyLinked`
    /// rather than performing a redundant fs op.
    pub already_linked: bool,
}

/// Output of `build_plan`: the canonical, the list of links to perform,
/// and the bytes-reclaimed estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnifyPlan {
    pub canonical: PlanCandidate,
    pub links: Vec<PlannedLink>,
    pub bytes_reclaimed_estimate: u64,
}

/// Construct a unify plan. `canonical` is the source; `targets` is the slice
/// of all candidates (the canonical may appear in `targets`; it will be
/// filtered out).
///
/// Returns `None` if `canonical` does not exist on disk — the unify action
/// is not applicable in that case.
pub fn build_plan(canonical: &PlanCandidate, targets: &[PlanCandidate]) -> Option<UnifyPlan> {
    if !canonical.exists {
        return None;
    }

    let mut links: Vec<PlannedLink> = Vec::new();
    let mut inodes_replaced: HashSet<(u64, u64)> = HashSet::new();

    for t in targets {
        if t.path == canonical.path {
            continue; // canonical itself
        }
        if !t.exists {
            // Missing target: skip silently. The acceptance contract says
            // unify operates on currently-registered models; a vanished
            // file is the orchestrator's concern, not the planner's.
            continue;
        }
        let already_linked = t.device == canonical.device && t.inode == canonical.inode;
        let cross_filesystem = t.device != canonical.device;
        if !already_linked {
            inodes_replaced.insert((t.device, t.inode));
        }
        links.push(PlannedLink {
            tool: t.tool,
            target: t.path.clone(),
            cross_filesystem,
            already_linked,
        });
    }

    let bytes_reclaimed_estimate =
        (inodes_replaced.len() as u64).saturating_mul(canonical.size_bytes);

    Some(UnifyPlan {
        canonical: canonical.clone(),
        links,
        bytes_reclaimed_estimate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(
        tool: &'static str,
        path: &str,
        exists: bool,
        device: u64,
        inode: u64,
        size_bytes: u64,
    ) -> PlanCandidate {
        PlanCandidate {
            tool: ToolId(tool),
            path: PathBuf::from(path),
            exists,
            device,
            inode,
            size_bytes,
        }
    }

    #[test]
    fn returns_none_when_canonical_missing() {
        let canonical = cand("ollama", "/c", false, 1, 100, 1024);
        let targets = vec![cand("hf", "/h", true, 1, 200, 1024)];
        assert!(build_plan(&canonical, &targets).is_none());
    }

    #[test]
    fn excludes_canonical_from_link_list() {
        let canonical = cand("ollama", "/c", true, 1, 100, 1024);
        let targets = vec![
            cand("ollama", "/c", true, 1, 100, 1024), // self
            cand("hf", "/h", true, 1, 200, 1024),
        ];
        let plan = build_plan(&canonical, &targets).unwrap();
        assert_eq!(plan.links.len(), 1, "self-link must be filtered out");
        assert_eq!(plan.links[0].tool, ToolId("hf"));
    }

    #[test]
    fn flags_already_linked_when_inodes_match() {
        let canonical = cand("ollama", "/c", true, 1, 100, 1024);
        let targets = vec![cand("hf", "/h", true, 1, 100, 1024)]; // same inode
        let plan = build_plan(&canonical, &targets).unwrap();
        assert_eq!(plan.links.len(), 1);
        assert!(plan.links[0].already_linked);
        assert!(!plan.links[0].cross_filesystem);
        assert_eq!(
            plan.bytes_reclaimed_estimate, 0,
            "no replacement work to do"
        );
    }

    #[test]
    fn flags_cross_filesystem_when_devices_differ() {
        let canonical = cand("ollama", "/c", true, 1, 100, 1024);
        let targets = vec![cand("hf", "/h", true, 2, 200, 1024)]; // different device
        let plan = build_plan(&canonical, &targets).unwrap();
        assert_eq!(plan.links.len(), 1);
        assert!(plan.links[0].cross_filesystem);
        assert!(!plan.links[0].already_linked);
    }

    #[test]
    fn computes_bytes_reclaimed_per_unique_inode() {
        let canonical = cand("ollama", "/c", true, 1, 100, 1024);
        let targets = vec![
            cand("hf", "/h", true, 1, 200, 1024), // distinct inode → reclaim
            cand("llama-cli", "/l", true, 1, 200, 1024), // SAME inode as hf → already shared
            cand("lm-studio", "/m", true, 1, 300, 1024), // distinct inode → reclaim
        ];
        let plan = build_plan(&canonical, &targets).unwrap();
        assert_eq!(plan.links.len(), 3);
        // 2 unique inodes to replace ((1,200) + (1,300)) → 2 * 1024
        assert_eq!(plan.bytes_reclaimed_estimate, 2 * 1024);
    }

    #[test]
    fn skips_missing_targets() {
        let canonical = cand("ollama", "/c", true, 1, 100, 1024);
        let targets = vec![
            cand("hf", "/missing", false, 1, 0, 1024),
            cand("llama-cli", "/l", true, 1, 200, 1024),
        ];
        let plan = build_plan(&canonical, &targets).unwrap();
        assert_eq!(
            plan.links.len(),
            1,
            "missing target must be skipped silently"
        );
        assert_eq!(plan.links[0].tool, ToolId("llama-cli"));
    }

    #[test]
    fn empty_targets_yields_empty_plan_with_zero_reclaim() {
        let canonical = cand("ollama", "/c", true, 1, 100, 1024);
        let plan = build_plan(&canonical, &[]).unwrap();
        assert!(plan.links.is_empty());
        assert_eq!(plan.bytes_reclaimed_estimate, 0);
    }
}
