//! `actions::reclassify::reclassify_after_unify` — bin-side adapter for the
//! lib's `modeltap_app::reclassify::reclassify_after_unify`.
//!
//! Triggered by the orchestrator (headless.rs / interactive.rs) immediately
//! after `actions::unify::run` returns. Translates the canonical
//! `crate::actions::unify::UnifyOutcome` into the lib's
//! `UnifyReclassifySummary` shape and delegates the pure recomputation.
//!
//! The lib function does the actual work (inode rewrite + dedup_summary
//! recompute + summary_delta set); this adapter exists so the bin's
//! `actions::*` namespace presents a single unified seam — `actions::unify::run`
//! followed by `actions::reclassify::reclassify_after_unify` follows the same
//! shape as `actions::zap::run` followed by an incremental refresh.
//!
//! Pure: no I/O. The orchestrator separately schedules the
//! `Msg::SummaryDeltaExpired` dispatch via `tokio::spawn(sleep(5s); send(...))`.

use modeltap_app::reclassify::{reclassify_after_unify as lib_reclassify, UnifyReclassifySummary};
use modeltap_core::ToolId;
use modeltap_tui::AppState;

use crate::actions::unify::{UnifyOutcome, UnifyResult};

/// Recompute the dedup view-model after a confirmed unify action.
///
/// Translates the canonical `UnifyOutcome` into a `UnifyReclassifySummary`:
/// the set of "succeeded tools" is `outcome.tools_unified` minus any tool
/// that appears in `outcome.failures`. (The canonical orchestrator already
/// records partial-success failures separately; this lets the reclassify
/// pass move ONLY the successful tools' inodes onto the canonical.)
pub fn reclassify_after_unify(state: AppState, outcome: &UnifyOutcome) -> AppState {
    let succeeded_tools: Vec<ToolId> = outcome
        .tools_unified
        .iter()
        .filter(|tool| !outcome.failures.iter().any(|f| f.tool == **tool))
        .copied()
        .collect();
    let summary = UnifyReclassifySummary {
        succeeded_tools,
        already_unified: matches!(outcome.outcome, UnifyResult::AlreadyUnified),
    };
    lib_reclassify(state, &summary)
}
