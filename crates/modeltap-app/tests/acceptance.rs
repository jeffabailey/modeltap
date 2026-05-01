//! Aggregator for acceptance tests. Cargo discovers `tests/*.rs` files as
//! separate integration test crates; per-story files live in
//! `tests/acceptance/` to keep them grouped, and this file `mod`s them in.
//!
//! Per acceptance-test-plan.md §1, scenarios are framed in business language
//! (US-NN style file names, Devon-perspective scenario names).

mod acceptance {
    pub mod us_01_launch_quit;
    pub mod us_02_discover_ollama;
    pub mod us_03_two_pane_navigation;
    pub mod us_04_row_metadata;
    pub mod us_05_zap_all;
    pub mod us_05b_delete_one;
    pub mod us_06_post_action_message;
    pub mod us_08_bottom_bar;
    pub mod us_09_compatibility_engine;
    pub mod us_10_unify_hardlinks;
    pub mod us_11_updated_totals;
    pub mod us_12_discover_hf;
    pub mod us_13_detail_screen;
    pub mod us_14_dry_run;
    pub mod us_15_discover_lm_studio;
    pub mod us_16_format_locked_indicator;
    pub mod us_17_running_tool_detect;
    pub mod us_18_plugin_trait;
    pub mod us_19_cross_fs_fallback;
    pub mod us_20_cross_platform;
    pub mod us_atomic_chat_discovery;

    // ---------------------------------------------------------------------
    // cross-tool-model-unify feature (DISTILL artifacts):
    //   docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature
    //
    // All scenarios in these modules are #[ignore]d as RED tests; the
    // DELIVER wave (software-crafter) removes the ignore one test at a
    // time as each scenario goes green. Walking-skeleton scenario lives in
    // us_u1_walking_skeleton.rs.
    // ---------------------------------------------------------------------
    pub mod us_u1_walking_skeleton;
    pub mod us_u2_dedup_able_bytes_wired;
    pub mod us_u3_row_dedup_glyphs;
    pub mod us_u4_unify_from_main_view;
    pub mod us_u5_unify_dialog_preview_and_apply;
    pub mod us_u6_post_unify_no_restart;
    pub mod us_u7_all_unified_pseudo_slot;
}
