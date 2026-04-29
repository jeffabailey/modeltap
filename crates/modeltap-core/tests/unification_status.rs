//! Unit tests for the per-model unification-status engine (US-13).
//!
//! Pure-domain functions — port-to-port at domain scope (the function
//! signature IS the public interface, per `nw-tdd-methodology`).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: Single registration → SingleTool
//!     B2: All registrations share one inode → Unified
//!     B3: Registrations have N distinct inodes (none shared) → NotUnified
//!     B4: Registrations have a mix (some shared, some distinct) → PartiallyUnified
//!     B5: Reclaim for SingleTool/Unified → 0
//!     B6: Reclaim for NotUnified → (count - 1) * canonical_size
//!     B7: Reclaim for PartiallyUnified → (distinct_inodes - 1) * canonical_size
//!     B8: Hardlink count for Unified → number of registrations
//!   budget = 8 × 2 = 16 tests max. We use ~10.

use std::path::PathBuf;

use modeltap_core::logic::unification_status::{
    compute_reclaim_estimate, compute_unification_status, DetailRegistration, UnificationStatus,
};
use modeltap_core::ToolId;

fn reg(tool: &'static str, path: &str, inode: Option<u64>) -> DetailRegistration {
    DetailRegistration {
        tool: ToolId(tool),
        path: PathBuf::from(path),
        inode,
    }
}

// ---------------------------------------------------------------------------
// B1 — One registration → SingleTool
// ---------------------------------------------------------------------------

#[test]
fn one_registration_yields_single_tool_status() {
    let regs = vec![reg("hf", "/hf/foo", Some(1))];
    let status = compute_unification_status(&regs);
    assert!(
        matches!(status, UnificationStatus::SingleTool),
        "1 registration must yield SingleTool, got {:?}",
        status
    );
}

// ---------------------------------------------------------------------------
// B2 — All registrations share one inode → Unified
// ---------------------------------------------------------------------------

#[test]
fn all_registrations_with_same_inode_yield_unified_status() {
    let regs = vec![
        reg("hf", "/hf/foo", Some(7777)),
        reg("llama-cli", "/llms/foo", Some(7777)),
        reg("ollama", "/ollama/blob", Some(7777)),
    ];
    let status = compute_unification_status(&regs);
    match status {
        UnificationStatus::Unified { hardlink_count } => {
            assert_eq!(
                hardlink_count, 3,
                "Unified must report hardlink_count = number of registrations"
            );
        }
        other => panic!("expected Unified, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// B3 — Distinct inodes (no sharing) → NotUnified
// ---------------------------------------------------------------------------

#[test]
fn distinct_inodes_yield_not_unified_status() {
    let regs = vec![
        reg("hf", "/hf/foo", Some(1001)),
        reg("llama-cli", "/llms/foo", Some(1002)),
        reg("ollama", "/ollama/blob", Some(1003)),
    ];
    let status = compute_unification_status(&regs);
    match status {
        UnificationStatus::NotUnified { copy_count } => {
            assert_eq!(
                copy_count, 3,
                "NotUnified must report copy_count = number of registrations"
            );
        }
        other => panic!("expected NotUnified, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// B4 — Mix (2 share, 1 distinct) → PartiallyUnified
// ---------------------------------------------------------------------------

#[test]
fn mixed_inodes_yield_partially_unified_status() {
    let regs = vec![
        reg("hf", "/hf/foo", Some(7777)),
        reg("llama-cli", "/llms/foo", Some(7777)),
        reg("ollama", "/ollama/blob", Some(1003)), // distinct
    ];
    let status = compute_unification_status(&regs);
    match status {
        UnificationStatus::PartiallyUnified {
            shared_count,
            total_count,
            distinct_inodes,
        } => {
            assert_eq!(shared_count, 2, "2 paths share an inode");
            assert_eq!(total_count, 3, "3 registrations total");
            assert_eq!(distinct_inodes, 2, "2 distinct inodes");
        }
        other => panic!("expected PartiallyUnified, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// B5a — Reclaim for SingleTool is 0
// ---------------------------------------------------------------------------

#[test]
fn reclaim_estimate_for_single_tool_is_zero() {
    let bytes = compute_reclaim_estimate(&UnificationStatus::SingleTool, 4_400_000_000);
    assert_eq!(bytes, 0, "SingleTool has no duplicates → reclaim must be 0");
}

// ---------------------------------------------------------------------------
// B5b — Reclaim for Unified is 0 (already unified)
// ---------------------------------------------------------------------------

#[test]
fn reclaim_estimate_for_unified_is_zero() {
    let bytes = compute_reclaim_estimate(
        &UnificationStatus::Unified { hardlink_count: 3 },
        4_400_000_000,
    );
    assert_eq!(
        bytes, 0,
        "Unified has shared inode → no further reclaim possible"
    );
}

// ---------------------------------------------------------------------------
// B6 — Reclaim for NotUnified = (copy_count - 1) * canonical_size
// ---------------------------------------------------------------------------

#[test]
fn reclaim_estimate_for_not_unified_is_count_minus_one_times_canonical() {
    let bytes = compute_reclaim_estimate(
        &UnificationStatus::NotUnified { copy_count: 3 },
        4_400_000_000,
    );
    assert_eq!(
        bytes,
        2 * 4_400_000_000,
        "NotUnified: (3 copies - 1) * 4.4 GB = 8.8 GB"
    );
}

// ---------------------------------------------------------------------------
// B7 — Reclaim for PartiallyUnified = (distinct_inodes - 1) * canonical_size
// ---------------------------------------------------------------------------

#[test]
fn reclaim_estimate_for_partially_unified_uses_distinct_inodes() {
    // 3 paths, 2 distinct inodes → could collapse to 1 → reclaim 1 * size.
    let bytes = compute_reclaim_estimate(
        &UnificationStatus::PartiallyUnified {
            shared_count: 2,
            total_count: 3,
            distinct_inodes: 2,
        },
        4_400_000_000,
    );
    assert_eq!(
        bytes, 4_400_000_000,
        "PartiallyUnified: (2 distinct - 1) * 4.4 GB = 4.4 GB"
    );
}

// ---------------------------------------------------------------------------
// B8 — Status variant carries the data the render layer needs.
// ---------------------------------------------------------------------------

#[test]
fn unified_carries_hardlink_count_for_render() {
    let regs = vec![
        reg("a", "/a", Some(42)),
        reg("b", "/b", Some(42)),
        reg("c", "/c", Some(42)),
        reg("d", "/d", Some(42)),
    ];
    let status = compute_unification_status(&regs);
    assert!(
        matches!(status, UnificationStatus::Unified { hardlink_count: 4 }),
        "Unified must carry hardlink_count for the render layer to display"
    );
}

// ---------------------------------------------------------------------------
// B3-edge — All inodes None (unknown) → conservative NotUnified
// Per ADR-002 conservative-when-uncertain: missing inode info means we cannot
// confirm sharing → treat as separate copies (preserve data).
// ---------------------------------------------------------------------------

#[test]
fn missing_inodes_default_to_not_unified_conservative() {
    let regs = vec![
        reg("hf", "/hf/foo", None),
        reg("llama-cli", "/llms/foo", None),
    ];
    let status = compute_unification_status(&regs);
    assert!(
        matches!(status, UnificationStatus::NotUnified { copy_count: 2 }),
        "ADR-002 conservative rule: missing inodes → NotUnified, got {:?}",
        status
    );
}
