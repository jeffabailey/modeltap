//! Unit tests for folder collapse/expand state (Step 01-07).
//!
//! Per quality-framework test-budget calculation:
//!   distinct behaviors:
//!     B1: `AppState::default()` initializes `expanded_folders` empty.
//!     B2: `Msg::ToggleFolderExpansion` toggles the cursor's folder in
//!         `expanded_folders` (insert when absent, remove when present).
//!     B3: Keymap dispatches `KeyCode::Enter` on main view to
//!         `Msg::ToggleFolderExpansion` (cursor-aware resolution happens
//!         later in `update`).
//!   budget = 3 × 2 = 6 unit tests.
//!
//! All tests enter through `update::update(state, msg)` or the
//! `keymap::dispatch_focus_aware` driving port.

use modeltap_core::{ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, FocusPane, ToolView};
use modeltap_tui::keymap::dispatch_focus_aware;
use modeltap_tui::msg::Msg;
use modeltap_tui::update::update;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const REPO_A: &str = "alice/foo";
const REPO_B: &str = "bob/bar";

fn hf_view_with_two_folders() -> ToolView {
    // Five files per folder, alphabetically ordered.
    let mut ids: Vec<String> = Vec::new();
    let mut sizes: Vec<u64> = Vec::new();
    for variant in ["Q2_K", "Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0"] {
        ids.push(format!("{REPO_A}/file-{variant}.gguf"));
        sizes.push(100 * 1024 * 1024);
    }
    for variant in ["Q2_K", "Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0"] {
        ids.push(format!("{REPO_B}/file-{variant}.gguf"));
        sizes.push(100 * 1024 * 1024);
    }
    ToolView {
        tool: ToolId("hf"),
        status: ToolStatus::Ok,
        model_ids: ids,
        model_sizes_bytes: sizes,
    }
}

fn state_with_hf_focused() -> AppState {
    let mut state = AppState::new_with_default_selection(vec![hf_view_with_two_folders()]);
    state.focus = FocusPane::Right;
    state
}

// ---------------------------------------------------------------------------
// B1 — default state: all folders collapsed.
// ---------------------------------------------------------------------------

#[test]
fn default_app_state_has_empty_expanded_folders() {
    let state = AppState::default();
    assert!(
        state.expanded_folders.is_empty(),
        "Default state must start with all folders collapsed; got {:?}",
        state.expanded_folders
    );
}

#[test]
fn new_with_default_selection_has_empty_expanded_folders() {
    let state = state_with_hf_focused();
    assert!(
        state.expanded_folders.is_empty(),
        "Constructor must start with all folders collapsed; got {:?}",
        state.expanded_folders
    );
}

// ---------------------------------------------------------------------------
// B2 — ToggleFolderExpansion inserts the cursor's folder when absent
//      and removes it when present.
// ---------------------------------------------------------------------------

#[test]
fn toggle_folder_expansion_inserts_cursor_folder_when_absent() {
    let mut state = state_with_hf_focused();
    state.selected_row = 0; // cursor on first model -> folder REPO_A
    let (next, _eff) = update(state, Msg::ToggleFolderExpansion);
    assert!(
        next.expanded_folders.contains(REPO_A),
        "Toggle on REPO_A's first file must insert `{REPO_A}` into expanded_folders; \
         got {:?}",
        next.expanded_folders
    );
}

#[test]
fn toggle_folder_expansion_removes_cursor_folder_when_present() {
    let mut state = state_with_hf_focused();
    state.selected_row = 0;
    state.expanded_folders.insert(REPO_A.to_string());
    let (next, _eff) = update(state, Msg::ToggleFolderExpansion);
    assert!(
        !next.expanded_folders.contains(REPO_A),
        "Second toggle must remove `{REPO_A}` from expanded_folders; got {:?}",
        next.expanded_folders
    );
}

#[test]
fn toggle_folder_expansion_only_affects_cursor_folder() {
    let mut state = state_with_hf_focused();
    // Cursor lands on REPO_B's first file (5 files of REPO_A precede it).
    state.selected_row = 5;
    let (next, _eff) = update(state, Msg::ToggleFolderExpansion);
    assert!(
        next.expanded_folders.contains(REPO_B),
        "Toggle on REPO_B's first file must insert `{REPO_B}`; got {:?}",
        next.expanded_folders
    );
    assert!(
        !next.expanded_folders.contains(REPO_A),
        "Toggle on REPO_B must NOT touch REPO_A; got {:?}",
        next.expanded_folders
    );
}

// ---------------------------------------------------------------------------
// B3 — Keymap: Enter on main view dispatches Msg::ToggleFolderExpansion.
// ---------------------------------------------------------------------------

#[test]
fn enter_dispatches_to_toggle_folder_expansion_under_right_focus() {
    let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    let msg = dispatch_focus_aware(key, FocusPane::Right);
    assert_eq!(
        msg,
        Msg::ToggleFolderExpansion,
        "Enter on main view (Right pane focused) must dispatch ToggleFolderExpansion"
    );
}
