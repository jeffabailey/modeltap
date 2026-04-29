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
    pub mod us_05_zap_all;
    pub mod us_06_post_action_message;
}
