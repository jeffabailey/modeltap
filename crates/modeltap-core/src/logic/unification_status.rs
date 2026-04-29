//! Pure-domain unification status engine for the per-model detail screen
//! (US-13).
//!
//! Inputs: a list of `DetailRegistration` (one per tool the model is
//! registered with). Each registration carries the on-disk path and the
//! inode (when known). Output: one of four `UnificationStatus` variants.
//!
//! ## Decision rules
//!
//! 1. 1 registration → `SingleTool`
//! 2. All registrations share ONE inode → `Unified { hardlink_count = N }`
//! 3. All registrations have DISTINCT inodes (or any inode is `None`) →
//!    `NotUnified { copy_count = N }`. **ADR-002 conservative-when-uncertain
//!    rule**: missing inode info means we cannot confirm sharing → treat as
//!    separate copies (preserve data).
//! 4. Otherwise (some share, some distinct) →
//!    `PartiallyUnified { shared_count, total_count, distinct_inodes }`
//!
//! Reclaim estimate is computed by `compute_reclaim_estimate(status, size)`:
//!
//! - `SingleTool` / `Unified` → 0 (no further deduplication possible)
//! - `NotUnified { copy_count }` → `(copy_count - 1) * canonical_size`
//! - `PartiallyUnified { distinct_inodes }` →
//!   `(distinct_inodes - 1) * canonical_size`
//!
//! ## Purity contract
//!
//! No I/O, no global state, no panics. Inputs in → status out. The inode
//! comparison happens at the `modeltap-app` boundary (real `std::fs::metadata`
//! lookup); this module consumes the resulting structured input.

use std::collections::HashSet;
use std::path::PathBuf;

use serde::Serialize;

use crate::types::{DisplayLabel, Format, ModelStatus, ToolId};

// ---------------------------------------------------------------------------
// Input types — what the orchestrator hands to the detail screen.
// ---------------------------------------------------------------------------

/// One tool's registration of a model. The `inode` is `Some(_)` when the
/// orchestrator could `stat` the path successfully; `None` when stat failed
/// or the file is on a filesystem that does not expose inodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct DetailRegistration {
    pub tool: ToolId,
    pub path: PathBuf,
    /// Filesystem inode number, when available. Per ADR-002 conservative
    /// rule, missing inodes are treated as "definitely not shared" rather
    /// than guessed.
    pub inode: Option<u64>,
}

/// The model identity slice the detail screen needs. Constructed by the
/// orchestrator from the cross-tool inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetailModelView {
    pub id: String,
    pub format: Format,
    /// Quantization label parsed out of the format / filename, when available
    /// (e.g. "q4_K_M"). `None` for formats where it does not apply.
    pub format_quant: Option<String>,
    pub canonical_size_bytes: u64,
    pub display_label: DisplayLabel,
    pub status: ModelStatus,
}

// ---------------------------------------------------------------------------
// Output type
// ---------------------------------------------------------------------------

/// One of four mutually-exclusive states. Variants carry the data the render
/// layer needs to format the status header (no separate query against the
/// underlying `&[DetailRegistration]` is needed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum UnificationStatus {
    /// Model registered with exactly one tool. Unify is not applicable.
    SingleTool,
    /// All N registrations share one inode (the model is hardlinked across
    /// every tool). `hardlink_count == N`.
    Unified { hardlink_count: usize },
    /// All N registrations are separate files (no shared inodes). The user
    /// can reclaim `(N - 1) * size` by unifying.
    NotUnified { copy_count: usize },
    /// Some registrations share an inode, some don't. The user can still
    /// reclaim `(distinct_inodes - 1) * size`.
    PartiallyUnified {
        shared_count: usize,
        total_count: usize,
        distinct_inodes: usize,
    },
}

// ---------------------------------------------------------------------------
// Pure functions
// ---------------------------------------------------------------------------

/// Compute the unification status from a list of registrations. Pure. ADR-002
/// conservative-when-uncertain rule: missing inodes → treat as distinct
/// copies (preserve data — never overstate sharing).
pub fn compute_unification_status(regs: &[DetailRegistration]) -> UnificationStatus {
    let n = regs.len();
    match n {
        0 => UnificationStatus::SingleTool, // pathological — render as if 1
        1 => UnificationStatus::SingleTool,
        _ => classify_multi(regs),
    }
}

/// Classify when there are 2+ registrations.
fn classify_multi(regs: &[DetailRegistration]) -> UnificationStatus {
    // Collect known inodes; treat any None as a distinct "ghost" inode (per
    // ADR-002 conservative rule — preserve data when uncertain).
    let inodes: Vec<Option<u64>> = regs.iter().map(|r| r.inode).collect();

    // If ALL inodes are Some and equal → Unified.
    if inodes.iter().all(|i| i.is_some()) {
        let unique: HashSet<u64> = inodes.iter().filter_map(|i| *i).collect();
        if unique.len() == 1 {
            return UnificationStatus::Unified {
                hardlink_count: regs.len(),
            };
        }
        if unique.len() == regs.len() {
            return UnificationStatus::NotUnified {
                copy_count: regs.len(),
            };
        }
        // Mixed: some share, some don't.
        let shared_count = compute_shared_count(&inodes);
        return UnificationStatus::PartiallyUnified {
            shared_count,
            total_count: regs.len(),
            distinct_inodes: unique.len(),
        };
    }

    // At least one inode is None → conservative NotUnified.
    UnificationStatus::NotUnified {
        copy_count: regs.len(),
    }
}

/// Number of registrations whose inode is shared with at least one other
/// registration.
fn compute_shared_count(inodes: &[Option<u64>]) -> usize {
    let mut count = 0;
    for (i, ino) in inodes.iter().enumerate() {
        let Some(this) = ino else { continue };
        let shared_with_another = inodes
            .iter()
            .enumerate()
            .any(|(j, other)| j != i && other.as_ref() == Some(this));
        if shared_with_another {
            count += 1;
        }
    }
    count
}

/// Compute the reclaim estimate in bytes for a given status + canonical size.
/// Pure. See module docstring for the formula table.
pub fn compute_reclaim_estimate(status: &UnificationStatus, canonical_size_bytes: u64) -> u64 {
    match status {
        UnificationStatus::SingleTool => 0,
        UnificationStatus::Unified { .. } => 0,
        UnificationStatus::NotUnified { copy_count } => {
            (copy_count.saturating_sub(1) as u64).saturating_mul(canonical_size_bytes)
        }
        UnificationStatus::PartiallyUnified {
            distinct_inodes, ..
        } => (distinct_inodes.saturating_sub(1) as u64).saturating_mul(canonical_size_bytes),
    }
}
