//! Unit tests for `summary_bar::summary_text` — Dedup-able branch wiring
//! (step 01-04 of cross-tool-model-unify).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: hashing in progress → "Dedup-able: computing..."
//!     B2: hashing complete + Some(0) → "Dedup-able: 0 B"
//!     B3: hashing complete + Some(n>0) → "Dedup-able: <formatted>"
//!     B4: dedup_summary.dedup_able_bytes == None (default, pre-paint) →
//!         "Dedup-able: computing..."
//!     B5: refresh-failed suffix preserved alongside dedup branches.
//!   budget = 5 × 2 = 10 unit tests max. We use 6.
//!
//! Each test enters through:
//!   - `render::summary_bar::summary_text` — pure summary fn (driving port).
//!
//! Per AC-NFR-5: the bar reads `state.dedup_summary.dedup_able_bytes`,
//! NOT a separate computation. `state.hash_state.is_hashing()` selects
//! the computing branch when work is in flight.

use std::collections::BTreeSet;

use modeltap_core::{DedupSummary, ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, HashPoolState, ToolView};
use modeltap_tui::render::summary_bar;

fn tool_view(name: &'static str, status: ToolStatus, sizes: &[u64]) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status,
        model_ids: (0..sizes.len()).map(|i| format!("{name}:m{i}")).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

/// Construct a state with one installed tool so the totals portion of the
/// summary is non-trivial. Dedup state is set by each individual test.
fn base_state() -> AppState {
    AppState::new_with_default_selection(vec![tool_view(
        "ollama",
        ToolStatus::Ok,
        &[1_000_000_000, 2_000_000_000],
    )])
}

// B1: hashing in progress → "Dedup-able: computing..."
#[test]
fn dedup_able_renders_computing_while_hashing_in_flight() {
    let mut state = base_state();
    state.hash_state = HashPoolState {
        total: 5,
        completed: 2,
        in_progress: BTreeSet::new(),
        failed: BTreeSet::new(),
    };
    // Even if a stale Some(_) value were carried, the in-flight branch wins.
    state.dedup_summary = DedupSummary {
        dedup_able_bytes: Some(123_456),
        ..DedupSummary::default()
    };

    let text = summary_bar::summary_text(&state);

    assert!(
        text.contains("Dedup-able: computing..."),
        "expected `Dedup-able: computing...` while hashing, got: {text}"
    );
}

// B2: hashing complete + Some(0) → honest zero per AC-U2.5
#[test]
fn dedup_able_renders_zero_bytes_when_some_zero_and_complete() {
    let mut state = base_state();
    state.hash_state = HashPoolState {
        total: 3,
        completed: 3,
        in_progress: BTreeSet::new(),
        failed: BTreeSet::new(),
    };
    state.dedup_summary = DedupSummary {
        dedup_able_bytes: Some(0),
        ..DedupSummary::default()
    };

    let text = summary_bar::summary_text(&state);

    assert!(
        text.contains("Dedup-able: 0 B"),
        "expected `Dedup-able: 0 B` when complete and Some(0), got: {text}"
    );
    assert!(
        !text.contains("computing..."),
        "should NOT show computing once hashing complete with Some(0): {text}"
    );
}

// B3: hashing complete + Some(n>0) → formatted size
#[test]
fn dedup_able_renders_formatted_bytes_when_some_nonzero() {
    let mut state = base_state();
    state.hash_state = HashPoolState {
        total: 4,
        completed: 4,
        in_progress: BTreeSet::new(),
        failed: BTreeSet::new(),
    };
    state.dedup_summary = DedupSummary {
        dedup_able_bytes: Some(2_500_000_000),
        ..DedupSummary::default()
    };

    let text = summary_bar::summary_text(&state);

    assert!(
        text.contains("Dedup-able: 2.5 GB"),
        "expected `Dedup-able: 2.5 GB` for 2_500_000_000, got: {text}"
    );
}

// B4: dedup_summary == None (default) AND no hashing → "computing..." pre-paint default
#[test]
fn dedup_able_renders_computing_when_none_and_idle_pre_paint_default() {
    let state = base_state();
    // Default state: hash_state.total == 0 (so is_hashing() == false because
    // completed (0) < total (0) is false), dedup_summary.dedup_able_bytes == None.
    assert!(!state.hash_state.is_hashing(), "fixture invariant: idle");
    assert!(
        state.dedup_summary.dedup_able_bytes.is_none(),
        "fixture invariant: None"
    );

    let text = summary_bar::summary_text(&state);

    assert!(
        text.contains("Dedup-able: computing..."),
        "expected `Dedup-able: computing...` for None pre-paint default, got: {text}"
    );
}

// B5: refresh-failed suffix preserved alongside the dedup branch.
#[test]
fn refresh_failed_suffix_preserved_with_computing_branch() {
    let mut state = base_state();
    state.hash_state = HashPoolState {
        total: 5,
        completed: 1,
        in_progress: BTreeSet::new(),
        failed: BTreeSet::new(),
    };
    state.refresh_failed_tools.insert(ToolId("ollama"));

    let text = summary_bar::summary_text(&state);

    assert!(
        text.contains("Dedup-able: computing..."),
        "computing branch must be present: {text}"
    );
    assert!(
        text.ends_with("(refresh failed)"),
        "(refresh failed) suffix must be preserved: {text}"
    );
}

// B5b: refresh-failed suffix preserved alongside the formatted dedup branch.
#[test]
fn refresh_failed_suffix_preserved_with_formatted_dedup() {
    let mut state = base_state();
    state.hash_state = HashPoolState {
        total: 2,
        completed: 2,
        in_progress: BTreeSet::new(),
        failed: BTreeSet::new(),
    };
    state.dedup_summary = DedupSummary {
        dedup_able_bytes: Some(750_000_000),
        ..DedupSummary::default()
    };
    state.refresh_failed_tools.insert(ToolId("hf"));

    let text = summary_bar::summary_text(&state);

    assert!(
        text.contains("Dedup-able: 750.0 MB"),
        "formatted dedup must be present: {text}"
    );
    assert!(
        text.ends_with("(refresh failed)"),
        "(refresh failed) suffix must be preserved: {text}"
    );
}
