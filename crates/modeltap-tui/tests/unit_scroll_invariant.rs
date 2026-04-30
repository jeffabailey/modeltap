//! Scroll-invariant tests for both panes.
//!
//! Invariant under test (both panes):
//!   ∀ N items, viewport V > 0, and any sequence of Up/Down keypresses,
//!   the `selected` index is ALWAYS inside the rendered window
//!   `[scroll_offset, scroll_offset + V)`.
//!
//! This is the property that was silently broken on terminals shorter than
//! 28 rows before the production interactive loop started syncing
//! `visible_rows` / `left_visible_rows` from the real terminal layout.
//!
//! Test-budget calculation:
//!   distinct behaviors:
//!     B1: After Down, right-pane selected ∈ visible window
//!     B2: After Up, right-pane selected ∈ visible window
//!     B3: After cycling tools, left-pane selected ∈ visible window
//!     B4: visible_rows respected when shorter than item count (small viewport)
//!     B5: visible_rows == 1 still keeps cursor visible (degenerate viewport)
//!   budget = 5 × 2 = 10 tests max. We use 7.
//!
//! Each test enters through the pure `update(state, msg)` driving port and
//! the pure `compute_scroll_offset(...)` driving port. No I/O, no terminal.

use modeltap_core::{ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, ToolView};
use modeltap_tui::msg::Msg;
use modeltap_tui::update::{compute_scroll_offset, update};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn tool(name: &'static str, model_count: usize) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status: ToolStatus::Ok,
        model_ids: (0..model_count).map(|i| format!("{name}:m{i}")).collect(),
        model_sizes_bytes: (0..model_count).map(|i| 1024 * (i as u64 + 1)).collect(),
    }
}

/// Build a single-tool state with `n_models` and the specified
/// right-pane viewport size. Selection starts at row 0.
fn right_pane_state(n_models: usize, visible_rows: usize) -> AppState {
    let mut state = AppState::new_with_default_selection(vec![tool("hf", n_models)]);
    state.visible_rows = visible_rows;
    state
}

/// Assert the right-pane scroll invariant on `state`.
fn assert_right_invariant(state: &AppState, ctx: &str) {
    let visible = state.visible_rows.max(1);
    let lo = state.scroll_offset;
    let hi = lo + visible;
    assert!(
        state.selected_row >= lo && state.selected_row < hi,
        "[{ctx}] right-pane invariant violated: selected={} not in [{}, {})",
        state.selected_row,
        lo,
        hi
    );
}

/// Assert the left-pane scroll invariant on `state`.
fn assert_left_invariant(state: &AppState, ctx: &str) {
    let visible = state.left_visible_rows.max(1);
    let lo = state.left_scroll_offset;
    let hi = lo + visible;
    assert!(
        state.selected_tool >= lo && state.selected_tool < hi,
        "[{ctx}] left-pane invariant violated: selected_tool={} not in [{}, {})",
        state.selected_tool,
        lo,
        hi
    );
}

// ---------------------------------------------------------------------------
// B1 — After every Down keypress, right-pane selected stays in viewport
// ---------------------------------------------------------------------------

#[test]
fn right_pane_selected_visible_after_every_down_on_small_viewport() {
    // Reproduces the original bug: 31 models, 5-row viewport (e.g. 80×10
    // terminal). Without the viewport sync, scroll_offset stayed at 0 and
    // selected_row marched off-screen.
    let mut state = right_pane_state(31, 5);
    assert_right_invariant(&state, "initial");
    for i in 0..30 {
        let (next, _) = update(state, Msg::SelectNextRow);
        state = next;
        assert_right_invariant(&state, &format!("after Down #{}", i + 1));
    }
    assert_eq!(state.selected_row, 30);
}

#[test]
fn right_pane_selected_visible_across_visible_row_extremes() {
    // Try several viewport sizes: 1 (degenerate), 3 (very small), 28 (default).
    for visible in [1usize, 3, 28] {
        let mut state = right_pane_state(31, visible);
        for i in 0..30 {
            let (next, _) = update(state, Msg::SelectNextRow);
            state = next;
            assert_right_invariant(&state, &format!("visible={visible} step={}", i + 1));
        }
    }
}

// ---------------------------------------------------------------------------
// B2 — After every Up keypress, right-pane selected stays in viewport
// ---------------------------------------------------------------------------

#[test]
fn right_pane_selected_visible_after_every_up_on_small_viewport() {
    // Start at the bottom (selected_row = 30, scroll_offset already advanced)
    // then walk Up; invariant must hold at every step.
    let mut state = right_pane_state(31, 5);
    for _ in 0..30 {
        let (next, _) = update(state, Msg::SelectNextRow);
        state = next;
    }
    assert_right_invariant(&state, "before Up walk");
    for i in 0..30 {
        let (next, _) = update(state, Msg::SelectPrevRow);
        state = next;
        assert_right_invariant(&state, &format!("after Up #{}", i + 1));
    }
    assert_eq!(state.selected_row, 0);
    assert_eq!(state.scroll_offset, 0, "scrolled all the way back to top");
}

// ---------------------------------------------------------------------------
// B3 — Left-pane selected stays in viewport across tool cycling
// ---------------------------------------------------------------------------

#[test]
fn left_pane_selected_visible_after_cycling_with_small_left_viewport() {
    // 4 tools, but pretend the left pane only fits 2 (e.g. very short
    // terminal). Cycling Right/Left through all of them must keep
    // selected_tool inside [left_scroll_offset, left_scroll_offset + 2).
    let mut state = AppState::new_with_default_selection(vec![
        tool("hf", 1),
        tool("Loose GGUFs", 1),
        tool("lm-studio", 1),
        tool("ollama", 1),
    ]);
    state.left_visible_rows = 2;
    // The constructor put selected_tool = 0 but left_scroll_offset is also
    // 0 by default — invariant holds initially.
    assert_left_invariant(&state, "initial");

    // Cycle Right through all 4 tools (with wrap-around). Invariant at every step.
    for i in 0..8 {
        let (next, _) = update(state, Msg::SelectNextTool);
        state = next;
        assert_left_invariant(&state, &format!("after Right #{}", i + 1));
    }
    // And cycle Left.
    for i in 0..8 {
        let (next, _) = update(state, Msg::SelectPrevTool);
        state = next;
        assert_left_invariant(&state, &format!("after Left #{}", i + 1));
    }
}

// ---------------------------------------------------------------------------
// B4, B5 — compute_scroll_offset pure-fn invariant (both panes share the fn)
// ---------------------------------------------------------------------------

#[test]
fn compute_scroll_offset_keeps_selected_within_window() {
    // Exhaustive table over small (selected, current_offset, visible) tuples.
    // For every input the returned offset must satisfy the window invariant:
    //     offset <= selected < offset + visible
    for n_items in 1..=20usize {
        for visible in 1..=8usize {
            for selected in 0..n_items {
                for current_offset in 0..n_items {
                    let new_offset = compute_scroll_offset(selected, current_offset, visible);
                    assert!(
                        selected >= new_offset && selected < new_offset + visible,
                        "compute_scroll_offset({selected}, {current_offset}, {visible}) = {new_offset} \
                         puts selected outside window [{new_offset}, {})",
                        new_offset + visible
                    );
                }
            }
        }
    }
}

#[test]
fn compute_scroll_offset_visible_one_tracks_selected_exactly() {
    // Degenerate viewport: only one row visible. The cursor must be
    // exactly at scroll_offset after every move.
    for selected in 0..50usize {
        for current in 0..50usize {
            let next = compute_scroll_offset(selected, current, 1);
            assert_eq!(
                next, selected,
                "visible=1 ⇒ scroll_offset must equal selected (was {current}, now {next} for sel {selected})"
            );
        }
    }
}

#[test]
fn compute_scroll_offset_visible_zero_returns_unchanged_offset() {
    // Defense-in-depth: visible == 0 cannot satisfy the invariant (no
    // window to be in), so the pure fn returns the input unchanged. The
    // production code path uses `.max(1)` on the renderer side; this test
    // pins the pure-fn contract.
    let next = compute_scroll_offset(7, 3, 0);
    assert_eq!(next, 3, "visible=0 leaves scroll_offset unchanged");
}
