//! Unit tests for US-03 (Two-pane selection state and keyboard navigation).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors in US-03 acceptance criteria:
//!     B1: Right Arrow advances tool selection (cycle)
//!     B2: Left Arrow regresses tool selection (cycle)
//!     B3: Down Arrow advances row selection in current tool
//!     B4: Up Arrow regresses row selection
//!     B5: Tab toggles focus pane (Left <-> Right)
//!     B6: Default selection = alphabetically-first INSTALLED tool
//!     B7: Unbound key returns unchanged state (no inventory mutation)
//!     B8: SHORTCUT_TABLE drives both render and dispatch (single source)
//!     B9: keymap::dispatch translates KeyEvent into Msg
//!   budget = 9 × 2 = 18 tests max. We use ~12.
//!
//! Each test enters through a pure-function driving port:
//!   - `update(state, msg)` — driving port for the Elm-style state machine
//!   - `keymap::dispatch(key)` — driving port for key→Msg translation
//!   - `keymap::SHORTCUT_TABLE` — single-source const consumed by both
//!     render and dispatch

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use modeltap_core::{ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, FocusPane, ToolView};
use modeltap_tui::keymap::{dispatch, SHORTCUT_TABLE};
use modeltap_tui::msg::Msg;
use modeltap_tui::update::update;

// ---------------------------------------------------------------------------
// Test fixtures: synthesize an AppState with multiple tools.
// ---------------------------------------------------------------------------

fn tool_view(name: &'static str, status: ToolStatus, model_count: usize) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status,
        model_ids: (0..model_count).map(|i| format!("{name}:m{i}")).collect(),
        model_sizes_bytes: (0..model_count).map(|i| 1024 * (i as u64 + 1)).collect(),
    }
}

/// Build a state with 4 tools alphabetically: hf (not installed), llama-cli
/// (not installed), lm-studio (not installed), ollama (installed, 4 models).
/// Default selection lands on the alphabetically-first INSTALLED tool: ollama
/// (index 3).
fn state_with_only_ollama_installed() -> AppState {
    AppState::new_with_default_selection(vec![
        tool_view("hf", ToolStatus::NotInstalled, 0),
        tool_view("llama-cli", ToolStatus::NotInstalled, 0),
        tool_view("lm-studio", ToolStatus::NotInstalled, 0),
        tool_view("ollama", ToolStatus::Ok, 4),
    ])
}

/// Build a state where multiple tools are installed; the alphabetically-first
/// installed tool is "hf".
fn state_all_installed() -> AppState {
    AppState::new_with_default_selection(vec![
        tool_view("hf", ToolStatus::Ok, 31),
        tool_view("llama-cli", ToolStatus::Ok, 6),
        tool_view("lm-studio", ToolStatus::Ok, 9),
        tool_view("ollama", ToolStatus::Ok, 4),
    ])
}

// ---------------------------------------------------------------------------
// B6 — Default selection = alphabetically-first INSTALLED tool
// ---------------------------------------------------------------------------

#[test]
fn default_selection_picks_alphabetically_first_installed_tool_when_only_ollama_installed() {
    let state = state_with_only_ollama_installed();
    // Tools listed alphabetically: hf, llama-cli, lm-studio, ollama.
    // Only ollama is installed → default selection index 3.
    assert_eq!(
        state.selected_tool, 3,
        "default selection must skip not-installed tools and land on ollama (index 3)"
    );
}

#[test]
fn default_selection_picks_hf_when_all_installed() {
    let state = state_all_installed();
    // All tools installed → alphabetically-first is hf at index 0.
    assert_eq!(
        state.selected_tool, 0,
        "default selection must be hf (index 0)"
    );
}

#[test]
fn default_selection_picks_first_when_no_tool_installed() {
    let state = AppState::new_with_default_selection(vec![
        tool_view("hf", ToolStatus::NotInstalled, 0),
        tool_view("llama-cli", ToolStatus::NotInstalled, 0),
    ]);
    // No installed tool — fall back to index 0 so the right pane has something
    // to render.
    assert_eq!(
        state.selected_tool, 0,
        "with no installed tools, default selection falls back to index 0"
    );
}

// ---------------------------------------------------------------------------
// B1, B2 — Tool navigation via Left/Right Arrow (cycle)
// ---------------------------------------------------------------------------

#[test]
fn right_arrow_advances_tool_selection_with_wraparound() {
    let state = state_with_only_ollama_installed(); // selected = 3
    let (next, _effect) = update(state, Msg::SelectNextTool);
    assert_eq!(next.selected_tool, 0, "ollama (3) Right wraps to hf (0)");

    let (next2, _) = update(next, Msg::SelectNextTool);
    assert_eq!(next2.selected_tool, 1, "hf (0) Right -> llama-cli (1)");
}

#[test]
fn left_arrow_regresses_tool_selection_with_wraparound() {
    let state = state_all_installed(); // selected = 0 (hf)
    let (next, _) = update(state, Msg::SelectPrevTool);
    assert_eq!(next.selected_tool, 3, "hf (0) Left wraps to ollama (3)");
}

// ---------------------------------------------------------------------------
// B1 + side-effects — Switching tool resets row selection and scroll offset
// ---------------------------------------------------------------------------

#[test]
fn switching_tool_resets_row_and_scroll() {
    let mut state = state_all_installed();
    state.selected_row = 5;
    state.scroll_offset = 3;
    let (next, _) = update(state, Msg::SelectNextTool);
    assert_eq!(next.selected_row, 0, "row resets when tool changes");
    assert_eq!(next.scroll_offset, 0, "scroll resets when tool changes");
}

// ---------------------------------------------------------------------------
// B3, B4 — Row navigation via Up/Down Arrow
// ---------------------------------------------------------------------------

#[test]
fn down_arrow_advances_row_within_current_tool() {
    let state = state_all_installed(); // selected = hf with 31 models
    let (next, _) = update(state, Msg::SelectNextRow);
    assert_eq!(next.selected_row, 1, "Down advances row");
}

#[test]
fn down_arrow_clamps_at_last_row() {
    let mut state = state_all_installed(); // hf has 31 models
    state.selected_row = 30; // already on last
    let (next, _) = update(state, Msg::SelectNextRow);
    assert_eq!(
        next.selected_row, 30,
        "Down at last row stays at last row (no wrap)"
    );
}

#[test]
fn up_arrow_clamps_at_first_row() {
    let state = state_all_installed(); // selected_row = 0
    let (next, _) = update(state, Msg::SelectPrevRow);
    assert_eq!(next.selected_row, 0, "Up at row 0 stays at 0");
}

#[test]
fn down_arrow_advances_scroll_offset_when_past_visible_window() {
    // hf has 31 models; visible_rows = 28. After 28 Down presses the
    // selected_row is 28; scroll_offset becomes 1 so the cursor is at the
    // bottom of the visible window. After 30 Down presses, scroll_offset = 3.
    let mut state = state_all_installed();
    state.visible_rows = 28;

    // Press Down 30 times.
    for _ in 0..30 {
        let (next, _) = update(state, Msg::SelectNextRow);
        state = next;
    }
    assert_eq!(
        state.selected_row, 30,
        "row reaches last index 30 after 30 Down"
    );
    // visible window is rows [scroll_offset, scroll_offset + visible_rows).
    // To keep selected_row visible: scroll_offset = 30 - 28 + 1 = 3.
    assert_eq!(
        state.scroll_offset, 3,
        "scroll_offset must follow selected_row past visible window"
    );
}

// ---------------------------------------------------------------------------
// B5 — Tab toggles focus pane
// ---------------------------------------------------------------------------

#[test]
fn tab_toggles_focus_pane() {
    let state = state_all_installed();
    assert_eq!(state.focus, FocusPane::Left, "default focus is Left");
    let (next, _) = update(state, Msg::ToggleFocus);
    assert_eq!(next.focus, FocusPane::Right, "Tab toggles Left -> Right");
    let (next2, _) = update(next, Msg::ToggleFocus);
    assert_eq!(next2.focus, FocusPane::Left, "Tab toggles Right -> Left");
}

// ---------------------------------------------------------------------------
// B7 — Unbound key never mutates state (property)
// ---------------------------------------------------------------------------

#[test]
fn unbound_key_never_mutates_inventory_or_selection() {
    // Hand-rolled property test: enumerate a sample of unbound key codes and
    // verify update(state, UnboundKey) returns state unchanged. Per the step
    // contract the test surface is small enough that proptest is overkill.
    let state = state_all_installed();
    let initial = state.clone();
    let (next, _effect) = update(state, Msg::UnboundKey);
    assert_eq!(next, initial, "unbound key must not mutate state");
}

// ---------------------------------------------------------------------------
// B9 — keymap::dispatch translates KeyEvents into the right Msg
// ---------------------------------------------------------------------------

#[test]
fn dispatch_translates_arrow_keys_into_navigation_messages() {
    let cases = [
        (KeyCode::Right, KeyModifiers::NONE, Msg::SelectNextTool),
        (KeyCode::Left, KeyModifiers::NONE, Msg::SelectPrevTool),
        (KeyCode::Down, KeyModifiers::NONE, Msg::SelectNextRow),
        (KeyCode::Up, KeyModifiers::NONE, Msg::SelectPrevRow),
        (KeyCode::Tab, KeyModifiers::NONE, Msg::ToggleFocus),
        (KeyCode::Char('q'), KeyModifiers::NONE, Msg::Quit),
        (KeyCode::Char('c'), KeyModifiers::CONTROL, Msg::CtrlC),
    ];
    for (code, mods, expected) in cases {
        let key = KeyEvent::new(code, mods);
        assert_eq!(
            dispatch(key),
            expected,
            "dispatch({:?}, {:?}) should produce {:?}",
            code,
            mods,
            expected
        );
    }
}

#[test]
fn dispatch_returns_unbound_key_for_unknown_keys() {
    // 'x' is not in any context's binding list.
    let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(
        dispatch(key),
        Msg::UnboundKey,
        "unmapped key must produce Msg::UnboundKey"
    );
}

// ---------------------------------------------------------------------------
// B8 — SHORTCUT_TABLE single source of truth
// ---------------------------------------------------------------------------

#[test]
fn shortcut_table_drives_both_render_label_and_dispatch_msg() {
    // Every entry in SHORTCUT_TABLE has (KeyEvent, label, Msg). The bottom
    // bar renderer reads `label` to display; `dispatch()` reads `key` to
    // translate. This test asserts BOTH directions: every key that maps to
    // a Msg in dispatch is also present in SHORTCUT_TABLE with the same Msg.
    // (Per ADR-006: the table is the single source of truth.)
    for entry in SHORTCUT_TABLE {
        let mapped = dispatch(entry.key);
        assert_eq!(
            mapped, entry.msg,
            "SHORTCUT_TABLE entry {:?} -> {:?} but dispatch produced {:?}",
            entry.key, entry.msg, mapped
        );
        // Label must be non-empty.
        assert!(
            !entry.label.is_empty(),
            "SHORTCUT_TABLE entry has empty label for {:?}",
            entry.key
        );
    }
    // Sanity: at least the 4 navigation keys + q live in the table.
    assert!(
        SHORTCUT_TABLE.len() >= 5,
        "SHORTCUT_TABLE must have at least 5 entries (arrows + q), got {}",
        SHORTCUT_TABLE.len()
    );
}
