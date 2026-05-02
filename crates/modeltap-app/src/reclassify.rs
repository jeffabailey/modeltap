//! `reclassify_after_unify` — pure recomputation of the dedup view-model
//! after a confirmed unify action (US-U6).
//!
//! Triggered by the orchestrator (headless.rs / interactive.rs) immediately
//! after `actions::unify::run` returns. Walks the just-touched
//! `(tool, model_id)` pairs in `state.hash_state.inodes`, refreshes them so
//! every successfully-linked target shares the canonical inode, recomputes
//! `state.dedup_summary` via the canonical `logic::dedup::dedup_summary`,
//! and sets `state.summary_delta` so the renderer can display the transient
//! "(was X)" annotation for ~5 seconds (the orchestrator schedules the
//! `Msg::SummaryDeltaExpired` dispatch separately).
//!
//! This function is **pure** (no I/O, no async). It is the lib-side
//! counterpart of the bin's `actions::reclassify::reclassify_after_unify`
//! adapter — exposing it from the library lets integration tests construct
//! `AppState` snapshots and assert post-unify invariants without spawning
//! the binary.
//!
//! ## Why dispatch `Msg::UnifyApplied` rather than reach into update internals?
//!
//! The pure update handler `apply_unify_outcome` (in modeltap-tui's
//! `update.rs`) already implements the exact reclassification logic this
//! function wants:
//!   1. Capture previous `dedup_able_bytes`.
//!   2. Pick a canonical `(device, inode)` from the first affected entry
//!      with a recorded inode.
//!   3. Rewrite every affected pair to that canonical inode.
//!   4. Recompute `state.dedup_summary` via the canonical
//!      `logic::dedup::dedup_summary`.
//!   5. Set `state.summary_delta = Some { previous, expires_at }`.
//!
//! Re-implementing it here would couple us to the same private `state_inventory`
//! / `recompute_dedup_summary` helpers in update.rs. Dispatching the existing
//! `Msg::UnifyApplied(UnifyOutcome)` instead routes through the pure
//! `update()` function, which is the canonical state-transition seam (per
//! ADR-006 the Elm-style `update()` is the only place that mutates AppState).

use modeltap_core::ToolId;
use modeltap_tui::effects::unify_outcome::UnifyOutcome as TuiUnifyOutcome;
use modeltap_tui::{update, AppState, Msg};

/// Compact summary of an `actions::unify::run` outcome — just the fields the
/// pure reclassify pass needs. Decoupled from `crate::actions::unify::UnifyOutcome`
/// (which lives in the bin module tree) so the library function can be
/// constructed directly by integration tests and by the bin adapter.
#[derive(Debug, Clone)]
pub struct UnifyReclassifySummary {
    /// Tools whose link succeeded. Used to look up affected
    /// `(tool, model_id)` pairs in `state.hash_state.inodes` so they can be
    /// rewritten onto the canonical inode.
    pub succeeded_tools: Vec<ToolId>,
    /// True iff `UnifyResult::AlreadyUnified` (no fs-level work happened
    /// because every target was already hardlinked into the canonical
    /// inode). The reclassify pass still sets `summary_delta` so the user
    /// gets visual acknowledgement, but no inode movement is required —
    /// the existing entries already share a single `(device, inode)`.
    pub already_unified: bool,
}

/// Recompute the dedup view-model after a confirmed unify action.
///
/// Pre-conditions:
/// - `state.hash_state.inodes` contains entries for every model that
///   participated in the unify (populated by the hash pool earlier).
/// - `summary.succeeded_tools` lists ToolIds whose link returned success.
///   For partial-success, ONLY the successful tools are included; failed
///   tools' inodes are left untouched.
///
/// Post-conditions:
/// - Every `(tool, model_id)` pair in `state.hash_state.inodes` whose tool
///   appears in `succeeded_tools` AND whose model_id has a peer in another
///   succeeded tool now points at the canonical `(device, inode)` (chosen
///   as the first such pair's existing inode, mirroring the planner's
///   canonical-selection in ADR-002).
/// - `state.dedup_summary` is recomputed via the canonical
///   `logic::dedup::dedup_summary` so the row glyphs and summary bar agree.
/// - `state.summary_delta` is `Some { previous_dedup_able_bytes, expires_at }`
///   with a 5-second window. The orchestrator must schedule a
///   `Msg::SummaryDeltaExpired` dispatch at the expiry time to clear it.
///
/// Pure: no I/O, no async. Completes in well under 200 ms for inventories
/// up to ~50 models (perf gate per step 01-11).
pub fn reclassify_after_unify(state: AppState, summary: &UnifyReclassifySummary) -> AppState {
    let affected = collect_affected_pairs(&state, &summary.succeeded_tools);
    // Even when there are no affected pairs (empty succeeded_tools, or a
    // succeeded tool with no recorded inode entries), we still dispatch
    // Msg::UnifyApplied so that `summary_delta` is set — this matches
    // AC-U6.7 (AlreadyUnified case acknowledges the action even though
    // there are no inode changes). The existing apply_unify_outcome handler
    // is robust to an empty `affected` list: it simply skips the canonical
    // inode rewrite and proceeds to the dedup_summary recompute + delta set.
    let _ = summary.already_unified; // documented intentionally for clarity
    let outcome = TuiUnifyOutcome {
        affected,
        bytes_reclaimed: 0,
    };
    let (next, _eff) = update(state, Msg::UnifyApplied(outcome));
    next
}

/// Walk `state.hash_state.inodes` and collect every `(tool, model_id)` pair
/// whose `tool` is in `succeeded_tools`. The `apply_unify_outcome` handler
/// picks the canonical inode from the FIRST entry with a recorded inode —
/// for our case every entry has one (we just walked the inode map), so the
/// lookup is deterministic on iteration order. `BTreeMap` iteration is
/// sorted by key, which gives us a stable canonical pick across runs.
fn collect_affected_pairs(state: &AppState, succeeded_tools: &[ToolId]) -> Vec<(ToolId, String)> {
    if succeeded_tools.is_empty() {
        return Vec::new();
    }
    state
        .hash_state
        .inodes
        .keys()
        .filter(|(tool, _)| succeeded_tools.contains(tool))
        .cloned()
        .collect()
}
