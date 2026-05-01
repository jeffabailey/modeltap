//! Unit tests for the step 01-03 AppState refactor.
//!
//! Per ADR-014 the left pane is now a heterogeneous list of slots
//! (`LeftPaneSlot::Real(ToolView)` for real tools, `LeftPaneSlot::Synthetic(_)`
//! for the future `[All Unified]` synthetic). This file pins the shape:
//!
//!   B1: `AppState::new_with_default_selection(Vec<ToolView>)` produces
//!       `left_pane_slots` containing exactly one `LeftPaneSlot::Real(_)`
//!       per input `ToolView`.
//!   B2: New AppState fields (`hash_state`, `dedup_summary`,
//!       `summary_delta`) exist with sensible defaults.
//!   B3: Selection navigation (`Msg::SelectNextTool`) over the new
//!       `left_pane_slots` is unchanged in observable behavior — selected
//!       index stays at 0 with a 1-tool inventory.
//!   B4: `state.real_tool_at(idx)` accessor returns the inner `ToolView`
//!       for a `Real` slot at the given index.
//!
//! Test budget: 4 behaviors × 2 = 8 tests max. We use 4.
//!
//! Each test enters through `update()` (driving port for navigation) or via
//! `AppState`'s public accessors (driving port for the data model).

use modeltap_core::domain::synthetic_slot::LeftPaneSlot;
use modeltap_core::{ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, ToolView};
use modeltap_tui::msg::Msg;
use modeltap_tui::update::update;

fn tool_view(name: &'static str) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status: ToolStatus::Ok,
        model_ids: vec![format!("{name}:m0")],
        model_sizes_bytes: vec![1_000_000],
    }
}

// ---------------------------------------------------------------------------
// B1 — left_pane_slots wraps each input ToolView in LeftPaneSlot::Real.
// ---------------------------------------------------------------------------

#[test]
fn new_with_default_selection_wraps_each_tool_in_real_slot() {
    let state = AppState::new_with_default_selection(vec![tool_view("ollama")]);
    assert_eq!(
        state.left_pane_slots.len(),
        1,
        "exactly one slot for one input tool"
    );
    match &state.left_pane_slots[0] {
        LeftPaneSlot::Real(view) => {
            assert_eq!(view.tool, ToolId("ollama"));
            assert_eq!(view.model_ids.len(), 1);
        }
        LeftPaneSlot::Synthetic(_) => panic!("expected Real, got Synthetic"),
    }
}

// ---------------------------------------------------------------------------
// B2 — new fields exist with sensible defaults.
// ---------------------------------------------------------------------------

#[test]
fn new_with_default_selection_has_default_hash_dedup_and_delta_fields() {
    let state = AppState::new_with_default_selection(vec![tool_view("ollama")]);
    // hash_state defaults to empty (no hashing yet).
    assert_eq!(state.hash_state.total, 0);
    assert_eq!(state.hash_state.completed, 0);
    assert!(state.hash_state.in_progress.is_empty());
    assert!(state.hash_state.failed.is_empty());
    // dedup_summary defaults to None / 0 (no classification yet).
    assert_eq!(state.dedup_summary.dedup_able_bytes, None);
    assert_eq!(state.dedup_summary.unified_count, None);
    assert_eq!(state.dedup_summary.total_saved_by_unification, None);
    // summary_delta defaults to None (no transient delta).
    assert!(state.summary_delta.is_none());
}

// ---------------------------------------------------------------------------
// B3 — Msg::SelectNextTool navigation works on the new left_pane_slots.
// With a 1-tool inventory (1 slot total), Next wraps back to index 0.
// ---------------------------------------------------------------------------

#[test]
fn select_next_tool_with_single_slot_stays_at_zero() {
    let state = AppState::new_with_default_selection(vec![tool_view("ollama")]);
    assert_eq!(state.selected_tool, 0);
    let (next, _effect) = update(state, Msg::SelectNextTool);
    assert_eq!(
        next.selected_tool, 0,
        "single-slot inventory wraps SelectNextTool back to 0"
    );
}

// ---------------------------------------------------------------------------
// B4 — `real_tool_at(idx)` accessor returns the inner ToolView for Real slots.
// ---------------------------------------------------------------------------

#[test]
fn real_tool_at_returns_view_for_real_slot() {
    let state = AppState::new_with_default_selection(vec![tool_view("ollama")]);
    let view = state.real_tool_at(0).expect("real slot at idx 0");
    assert_eq!(view.tool, ToolId("ollama"));
}
