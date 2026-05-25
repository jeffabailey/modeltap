//! Pre-mutate revalidation acceptance scenarios (US-26 AC-26-5 / AC-26-6 /
//! AC-26-7) — step 05-04 of tool-model-info-sqlite-cache.
//!
//! Source feature:
//! `docs/feature/tool-model-info-sqlite-cache/distill/features/cache-state-model.feature`
//! lines 211-237 (the two pre-mutate scenarios).
//!
//! Wave: DELIVER step 05-04. Step 05-02 (commit 3255116) landed the
//! pre-mutate K5 gate (`orchestration::revalidate::pre_mutate`) and wired it
//! through the four destructive entry points. This step layers the
//! UX-visible behavior on top:
//!
//!   - Drift → re-introspect via the plugin, write recomputed size +
//!     metadata back to cache_models, emit `inspect.invoked
//!     source=pre_mutate_drift` to launch.log (AC-26-6).
//!   - Gone → enqueue a per-tool refresh for the affected tool, emit
//!     `refresh.tool source=pre_mutate_gone` to launch.log, leave the
//!     fixture filesystem byte-identical (AC-26-7).
//!
//! ## Strategy A — in-process orchestrator drive
//!
//! Per step-definitions-skeleton.md §F, the assertions are on the JSONL +
//! CACHE + FS seams (NOT SNAPSHOT). The TUI-visible "Re-introspecting
//! before proceeding..." progress line and the dialog re-confirm flow are
//! gated on the same launch.log timing seam that `manual_refresh.rs`
//! currently `#[ignore]`s — that surface lands in a follow-up TUI step
//! once the timing seam is exposed. The behavioural core (orchestrator
//! emits the right JSONL, updates the cache, refuses destructive action)
//! is fully covered here against `actions::unify::run` with `Some(&Cache)`
//! and the new `orchestration::revalidate::re_introspect_after_drift` +
//! `auto_refresh_after_gone` helpers.
//!
//! The step phrases live in `steps/revalidate.rs`. This driver wires the
//! Given / When / Then phrases per scenario.

#[path = "steps/revalidate.rs"]
mod revalidate_steps;

use revalidate_steps::*;

// ---------------------------------------------------------------------------
// Scenario A — drift (cache-state-model.feature:211-221).
//
// @us-26 @ac-26-5 @ac-26-6 @adr-015 @release-2 @real-io
//
//   Given Devon has fixture "devon-cache-mtime-drift"
//     And "<model>" is registered in the test-tool per the cache
//     And the test-tool copy's mtime has changed since the last cache write
//   When the destructive flow's pre-mutate gate fires and the orchestrator
//        re-introspects before proceeding
//   Then a `revalidate.invoked outcome=drift` event appears in launch.log
//     And an `inspect.invoked source=pre_mutate_drift` event appears in
//         launch.log
//     And the dedup-key / size for the drifted file is recomputed in
//         cache_models (`metadata_introspected_at` set; `size_bytes` reflects
//         the post-drift on-disk size)
//     And the cache_model_files row's (mtime, size, inode, dev) quad matches
//         the post-drift filesystem (subsequent verify_against_fs would
//         observe Match, not Drift)
//     And the destructive action did NOT mutate the filesystem (DirManifest
//         equality on the test-tool root pre/post)
// ---------------------------------------------------------------------------
#[test]
fn pre_mutate_gate_fires_drift_and_orchestrator_re_introspects_before_proceeding() {
    let mut world = RevalidateWorld::new_for_drift();

    given_devon_has_fixture_mtime_drift(&world);
    given_model_is_registered_in_tool_per_the_cache(&world);
    given_the_test_tool_copys_mtime_has_changed(&world);

    when_pre_mutate_gate_fires_and_orchestrator_re_introspects_before_proceeding(&mut world);

    then_revalidate_invoked_outcome_drift_event_is_emitted(&world);
    then_inspect_invoked_source_pre_mutate_drift_event_is_emitted(&world);
    then_dedup_key_and_size_for_drifted_file_is_recomputed_in_cache_models(&world);
    then_cache_model_files_quad_matches_post_drift_filesystem(&world);
    then_destructive_action_did_not_mutate_the_filesystem(&world);
}

// ---------------------------------------------------------------------------
// Scenario B — gone (cache-state-model.feature:227-236).
//
// @us-26 @ac-26-5 @ac-26-7 @adr-015 @release-2 @real-io
//
//   Given Devon has fixture "devon-cache-file-gone"
//     And "<model>" is registered in the test-tool per the cache
//     And the model file has been deleted out-of-band between launch and
//         Devon's action
//   When Devon attempts to unify
//   Then a `revalidate.invoked outcome=gone` event appears in launch.log
//     And a `refresh.tool source=pre_mutate_gone` event appears in launch.log
//     And the UnifyOutcome reports CacheStale (no plugin link() call)
//     And no destructive action occurred (DirManifest equality on the
//         test-tool root pre/post; the gone file is still gone but no other
//         files were touched)
// ---------------------------------------------------------------------------
#[test]
fn pre_mutate_gate_fires_gone_and_orchestrator_triggers_auto_refresh() {
    let mut world = RevalidateWorld::new_for_gone();

    given_devon_has_fixture_file_gone(&world);
    given_model_is_registered_in_tool_per_the_cache(&world);
    given_one_file_has_been_deleted_out_of_band(&world);

    when_devon_attempts_to_unify(&mut world);

    then_revalidate_invoked_outcome_gone_event_is_emitted(&world);
    then_refresh_tool_source_pre_mutate_gone_event_is_emitted(&world);
    then_unify_outcome_reports_cache_stale(&world);
    then_no_destructive_action_occurred(&world);
}
