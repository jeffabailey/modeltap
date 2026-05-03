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
/// that appears in `outcome.failures`, plus the `canonical_tool` (the tool
/// that owns the canonical inode — `actions::unify::run` does NOT include
/// it in `tools_unified` because no link was performed for it; the canonical
/// IS the inode every other tool's blob got linked to). Without including
/// the canonical, the lib-side reclassify would only rewrite the LINKED
/// tools' entries onto themselves (a no-op), leaving the canonical's entry
/// pointing at its old, distinct inode. The dedup_summary recompute would
/// then still see N distinct inodes and the row glyph would stay `=`
/// instead of flipping to `#` (AC-U6.2 violation).
pub fn reclassify_after_unify(
    state: AppState,
    outcome: &UnifyOutcome,
    canonical_tool: ToolId,
) -> AppState {
    let mut succeeded_tools: Vec<ToolId> = outcome
        .tools_unified
        .iter()
        .filter(|tool| !outcome.failures.iter().any(|f| f.tool == **tool))
        .copied()
        .collect();
    if !succeeded_tools.contains(&canonical_tool) {
        succeeded_tools.push(canonical_tool);
    }
    let summary = UnifyReclassifySummary {
        succeeded_tools,
        already_unified: matches!(outcome.outcome, UnifyResult::AlreadyUnified),
    };
    lib_reclassify(state, &summary)
}

#[cfg(test)]
mod tests {
    //! Step 05-02 unit tests for the bin-side `reclassify_after_unify`
    //! adapter. Covers the partial-success path (AC-U6.3, AC-U6.6) and the
    //! AC-CONS-4 invariant (`previous_dedup_able_bytes - new_dedup_able_bytes
    //! == bytes_reclaimed`).
    //!
    //! These tests focus on the ADAPTER's translation of `UnifyOutcome` into
    //! `UnifyReclassifySummary` — specifically that failed tools are filtered
    //! out of `succeeded_tools` so the lib-side reclassify only rewrites the
    //! successful tools' inodes onto the canonical. The lib-side recompute
    //! (dedup_summary, summary_delta) is exercised end-to-end here as well so
    //! the AC-CONS-4 invariant can be asserted against actual recomputed state.
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use modeltap_core::{ContentHash, ToolId, ToolStatus};
    use modeltap_tui::app_state::HashPoolState;
    use modeltap_tui::{AppState, ToolView};

    use super::*;
    use crate::actions::unify::{UnifyFailure, UnifyOutcome, UnifyResult};

    fn h(byte: u8) -> ContentHash {
        ContentHash([byte; 32])
    }

    /// Build an AppState seeded with the given tools/models/hashes/inodes and
    /// a populated dedup_summary (via a noop reclassify pass). Mirrors the
    /// helper in tests/reclassify_test.rs but kept private to this module.
    fn make_state(
        tools: Vec<(ToolId, Vec<(&str, u64)>)>,
        hashes: BTreeMap<(ToolId, String), ContentHash>,
        inodes: BTreeMap<(ToolId, String), (u64, u64)>,
    ) -> AppState {
        let mut tool_views: Vec<ToolView> = Vec::new();
        let mut total_jobs: u64 = 0;
        for (tool, models) in &tools {
            let mut ids = Vec::new();
            let mut sizes = Vec::new();
            for (id, size) in models {
                ids.push((*id).to_string());
                sizes.push(*size);
                total_jobs += 1;
            }
            tool_views.push(ToolView {
                tool: *tool,
                status: ToolStatus::Ok,
                model_ids: ids,
                model_sizes_bytes: sizes,
            });
        }
        let mut state = AppState::new_with_default_selection(tool_views);
        state.hash_state = HashPoolState {
            total: total_jobs,
            completed: total_jobs,
            in_progress: BTreeSet::new(),
            failed: BTreeSet::new(),
            completed_hashes: hashes,
            inodes,
        };
        // Seed dedup_summary by running a noop reclassify. This drives the
        // canonical recompute so the pre-condition reflects the seeded
        // topology. The seed canonical is the first tool in `tools` — its
        // identity does not matter because `tools_unified` is empty (no inode
        // rewrite is performed).
        let seed_canonical = tools.first().map(|(t, _)| *t).unwrap_or(ToolId("ollama"));
        let seed_outcome = UnifyOutcome {
            tools_unified: Vec::new(),
            bytes_reclaimed: 0,
            outcome: UnifyResult::Failed,
            failures: Vec::new(),
            cross_fs_targets_skipped: 0,
            cross_fs_targets_copied: 0,
        };
        let mut seeded = reclassify_after_unify(state, &seed_outcome, seed_canonical);
        seeded.summary_delta = None;
        seeded
    }

    fn three_tool_state() -> (AppState, ToolId, ToolId, ToolId) {
        let ollama = ToolId("ollama");
        let hf = ToolId("hf");
        let lm = ToolId("lm-studio");
        let mut hashes = BTreeMap::new();
        hashes.insert((ollama, "m".to_string()), h(0x42));
        hashes.insert((hf, "m".to_string()), h(0x42));
        hashes.insert((lm, "m".to_string()), h(0x42));
        let mut inodes = BTreeMap::new();
        inodes.insert((ollama, "m".to_string()), (1, 100));
        inodes.insert((hf, "m".to_string()), (1, 200));
        inodes.insert((lm, "m".to_string()), (1, 300));
        let state = make_state(
            vec![
                (ollama, vec![("m", 4096)]),
                (hf, vec![("m", 4096)]),
                (lm, vec![("m", 4096)]),
            ],
            hashes,
            inodes,
        );
        (state, ollama, hf, lm)
    }

    // ---------- T1: Success outcome — all tools_unified pass through --------

    #[test]
    fn success_outcome_rewrites_all_tools_unified_inodes_onto_canonical() {
        let (state, ollama, hf, lm) = three_tool_state();
        let pre = state.dedup_summary.dedup_able_bytes.unwrap_or(0);
        assert!(pre >= 4096, "precondition: dedup_able_bytes >= 4096");

        let outcome = UnifyOutcome {
            tools_unified: vec![ollama, hf, lm],
            bytes_reclaimed: 8192, // 2 inodes worth converged
            outcome: UnifyResult::Success,
            failures: Vec::new(),
            cross_fs_targets_skipped: 0,
            cross_fs_targets_copied: 0,
        };
        let after = reclassify_after_unify(state, &outcome, ollama);

        // All three tools must have converged onto the same (device, inode).
        let inode_ol = after.hash_state.inodes.get(&(ollama, "m".to_string()));
        let inode_hf = after.hash_state.inodes.get(&(hf, "m".to_string()));
        let inode_lm = after.hash_state.inodes.get(&(lm, "m".to_string()));
        assert_eq!(
            inode_ol, inode_hf,
            "Success: ollama+hf must share inode after unify"
        );
        assert_eq!(
            inode_ol, inode_lm,
            "Success: ollama+lm must share inode after unify"
        );
        // dedup_able collapses to 0 (single inode shared by all three).
        assert_eq!(
            after.dedup_summary.dedup_able_bytes,
            Some(0),
            "Success: dedup_able_bytes must collapse to 0 when all targets converge"
        );
        assert_eq!(
            after.dedup_summary.unified_count,
            Some(1),
            "Success: unified_count must be 1 for the single converged group"
        );
    }

    // ---------- T2: Partial outcome — failures filtered (AC-U6.3) -----------

    #[test]
    fn partial_outcome_filters_failed_tool_from_succeeded_inode_rewrite() {
        let (state, ollama, hf, lm) = three_tool_state();
        // Note: per actions::unify::run semantics, `tools_unified` already
        // excludes failures — but the adapter MUST defend against the
        // theoretical case where a caller includes a tool in BOTH
        // `tools_unified` and `failures`. The adapter's filter must still
        // exclude that tool.
        let outcome = UnifyOutcome {
            tools_unified: vec![ollama, hf, lm], // lm appears here
            bytes_reclaimed: 4096,               // only one inode reclaimed
            outcome: UnifyResult::Partial,
            failures: vec![UnifyFailure {
                tool: lm, // ...but is also a failure
                target: PathBuf::from("/lm/m"),
                reason: "permission-denied".to_string(),
            }],
            cross_fs_targets_skipped: 0,
            cross_fs_targets_copied: 0,
        };
        let after = reclassify_after_unify(state, &outcome, ollama);

        let inode_ol = after.hash_state.inodes.get(&(ollama, "m".to_string()));
        let inode_hf = after.hash_state.inodes.get(&(hf, "m".to_string()));
        let inode_lm = after.hash_state.inodes.get(&(lm, "m".to_string()));
        assert_eq!(inode_ol, inode_hf, "Partial: ollama+hf must converge");
        assert_eq!(
            inode_lm,
            Some(&(1, 300)),
            "AC-U6.3: lm-studio inode must NOT move on partial success (failed target)"
        );
        // Two distinct inodes remain ((ollama,hf) shared + lm-studio
        // separate) so the group is still DedupAble — glyph stays '='.
        assert!(
            after.dedup_summary.dedup_able_bytes.unwrap_or(0) > 0,
            "AC-U6.3: partial success must leave dedup_able_bytes > 0, got {:?}",
            after.dedup_summary.dedup_able_bytes
        );
        assert_eq!(
            after.dedup_summary.unified_count.unwrap_or(99),
            0,
            "AC-U6.6: partial success must NOT increment unified_count"
        );
    }

    // ---------- T3: Failed outcome — no inode rewrite, summary_delta still set

    #[test]
    fn failed_outcome_performs_no_inode_rewrite_but_sets_summary_delta() {
        let (state, ollama, hf, lm) = three_tool_state();
        let pre_inodes = state.hash_state.inodes.clone();
        let pre_dedup_able = state.dedup_summary.dedup_able_bytes.unwrap_or(0);

        // Every target failed — `tools_unified` is empty per actions::unify::run
        // semantics, but defensively: even if a caller put tools in
        // `tools_unified` AND in `failures`, the filter must zero out
        // succeeded_tools.
        let outcome = UnifyOutcome {
            tools_unified: vec![ollama, hf, lm],
            bytes_reclaimed: 0,
            outcome: UnifyResult::Failed,
            failures: vec![
                UnifyFailure {
                    tool: ollama,
                    target: PathBuf::from("/o/m"),
                    reason: "io-error".to_string(),
                },
                UnifyFailure {
                    tool: hf,
                    target: PathBuf::from("/h/m"),
                    reason: "io-error".to_string(),
                },
                UnifyFailure {
                    tool: lm,
                    target: PathBuf::from("/lm/m"),
                    reason: "io-error".to_string(),
                },
            ],
            cross_fs_targets_skipped: 0,
            cross_fs_targets_copied: 0,
        };
        let after = reclassify_after_unify(state, &outcome, ollama);

        assert_eq!(
            after.hash_state.inodes, pre_inodes,
            "Failed: NO inode entry may be rewritten when every target failed"
        );
        assert_eq!(
            after.dedup_summary.dedup_able_bytes.unwrap_or(0),
            pre_dedup_able,
            "Failed: dedup_able_bytes must NOT change when no inode rewrite happens"
        );
        // summary_delta is still set so the renderer can show the action was
        // acknowledged — but the delta is zero (previous == current).
        let delta = after
            .summary_delta
            .as_ref()
            .expect("Failed outcome still acknowledges via summary_delta");
        assert_eq!(
            delta.previous_dedup_able_bytes, pre_dedup_able,
            "Failed: summary_delta.previous must equal pre-state dedup_able_bytes \
             (delta is zero, but the acknowledgement window opens)"
        );
    }

    // ---------- T4: AC-CONS-4 invariant (full-success case) ----------------

    #[test]
    fn ac_cons_4_success_delta_equals_bytes_reclaimed() {
        // AC-CONS-4: `summary_bar.dedup_able_bytes_delta == toast.reclaimed_bytes`
        //
        // Per `crates/modeltap-core/src/logic/dedup.rs:228`, dedup_able_bytes
        // is counted ONCE per hash-group ("one inode-worth per group"). The
        // invariant `delta == bytes_reclaimed` therefore holds cleanly only
        // for the full-success branch (group collapses from N≥2 distinct
        // inodes to 1, dedup_able for that group goes from `size` → 0). For
        // partial success the invariant is satisfied DIFFERENTLY: dedup_able
        // remains at `size` (≥2 distinct inodes still), and bytes_reclaimed
        // reflects the inodes ACTUALLY collapsed by the action. This test
        // locks the full-success invariant and the summary_delta single-
        // source-of-truth contract; the partial-success case is locked by
        // T2 (`partial_outcome_filters_failed_tool_from_succeeded_inode_rewrite`).
        //
        // Setup: 2 tools, 1 model, 4096 bytes each, 2 distinct inodes →
        // pre dedup_able = 4096. After both succeed: 1 inode → 0.
        // Delta = 4096 == bytes_reclaimed.
        let ollama = ToolId("ollama");
        let hf = ToolId("hf");
        let mut hashes = BTreeMap::new();
        hashes.insert((ollama, "m".to_string()), h(0x42));
        hashes.insert((hf, "m".to_string()), h(0x42));
        let mut inodes = BTreeMap::new();
        inodes.insert((ollama, "m".to_string()), (1, 100));
        inodes.insert((hf, "m".to_string()), (1, 200));
        let state = make_state(
            vec![(ollama, vec![("m", 4096)]), (hf, vec![("m", 4096)])],
            hashes,
            inodes,
        );
        let pre_dedup_able = state.dedup_summary.dedup_able_bytes.unwrap_or(0);
        assert_eq!(
            pre_dedup_able, 4096,
            "precondition: 2-tool group with distinct inodes → dedup_able=4096"
        );

        let bytes_reclaimed: u64 = 4096;
        let outcome = UnifyOutcome {
            tools_unified: vec![ollama, hf],
            bytes_reclaimed,
            outcome: UnifyResult::Success,
            failures: Vec::new(),
            cross_fs_targets_skipped: 0,
            cross_fs_targets_copied: 0,
        };
        let after = reclassify_after_unify(state, &outcome, ollama);

        let new_dedup_able = after.dedup_summary.dedup_able_bytes.unwrap_or(0);
        let delta = pre_dedup_able.saturating_sub(new_dedup_able);
        assert_eq!(
            delta, bytes_reclaimed,
            "AC-CONS-4: full-success delta must equal bytes_reclaimed. \
             previous={pre_dedup_able}, new={new_dedup_able}, delta={delta}, \
             bytes_reclaimed={bytes_reclaimed}"
        );
        // summary_delta carries the same `previous` value so the renderer
        // and the toast cite the same source.
        let summary_delta = after
            .summary_delta
            .as_ref()
            .expect("AC-CONS-4: summary_delta must be set");
        assert_eq!(
            summary_delta.previous_dedup_able_bytes, pre_dedup_able,
            "AC-CONS-4: summary_delta.previous must agree with toast's \
             `previous` reading (single source of truth)"
        );
    }
}
