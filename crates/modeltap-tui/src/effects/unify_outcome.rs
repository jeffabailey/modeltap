//! Minimal stub for the unify action outcome.
//!
//! TODO(01-08): replace this stub with the canonical `UnifyOutcome` from
//! `modeltap_app::actions::unify` once the composition-root wiring lands.
//! `modeltap-tui` cannot depend on `modeltap-app` directly (dep direction
//! inversion), so the wiring step will either:
//!   - move the canonical type into `modeltap-core` and have both crates
//!     re-export it; OR
//!   - introduce a small `From<modeltap_app::actions::unify::UnifyOutcome>`
//!     impl in the composition root that constructs this stub.
//!
//! For 01-06 the stub keeps the step self-contained: the new
//! `Msg::UnifyApplied` variant has a real type to carry, the unit tests
//! construct it directly, and the GREEN handler reads only the fields the
//! task spec listed (`affected`, `bytes_reclaimed`).

use modeltap_core::ToolId;

/// Outcome of a confirmed unify action surfaced to the TUI via
/// `Msg::UnifyApplied`. The pure update handler reads `affected` to refresh
/// the inode map and `bytes_reclaimed` for diagnostics; the canonical
/// upstream type carries additional per-target failure detail not needed
/// for the pure state transition tested here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifyOutcome {
    /// `(tool, model_id_in_tool)` pairs whose targets were touched by the
    /// unify (linked or already-linked). After a successful unify, every
    /// pair points at the canonical inode; the handler refreshes
    /// `state.hash_state.inodes` for each pair so a follow-up
    /// `dedup_summary` recompute classifies them as `AlreadyUnified`.
    pub affected: Vec<(ToolId, String)>,
    /// Total bytes reclaimed by the action. Equal to
    /// `(unique_inodes_replaced) * canonical.size_bytes` per ADR-002.
    /// Carried for diagnostics; the pure handler does not consume it
    /// (the summary delta is computed from the previous
    /// `dedup_summary.dedup_able_bytes`, not from this field).
    pub bytes_reclaimed: u64,
}
