//! Unit tests for US-05 (Zap-all dialog state machine + key dispatch).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: Pressing 'z' opens zap dialog (Msg::ZapTool)
//!     B2: Typing characters appends to dialog input buffer
//!     B3: Backspace removes last char from input
//!     B4: Enter with input == tool name → DialogConfirm
//!     B5: Enter with input != tool name → DialogCancel
//!     B6: Esc → DialogCancel
//!     B7: Empty-tool dialog: Enter has no destructive path; only Esc closes
//!     B8: Property: typed_input != tool_name → state stays in Cancel branch
//!         (no transition to Confirm)
//!   budget = 8 × 2 = 16 tests max. We use ~10.
//!
//! Each test enters through:
//!   - `keymap::dispatch(KeyEvent) -> Msg` — translation
//!   - `update(state, msg) -> (state, effect)` — state transition
//!   - `dialogs::zap_confirm::*` — pure dialog state functions

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use modeltap_core::{ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, ToolView};
use modeltap_tui::dialogs::zap_confirm::{ZapConfirmState, ZapDecision};
use modeltap_tui::keymap::dispatch;
use modeltap_tui::msg::Msg;
use modeltap_tui::update::update;

fn tool_view(name: &'static str, status: ToolStatus, model_count: usize) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status,
        model_ids: (0..model_count).map(|i| format!("{name}:m{i}")).collect(),
        model_sizes_bytes: (0..model_count).map(|i| 1024 * (i as u64 + 1)).collect(),
    }
}

fn state_with_only_ollama_installed() -> AppState {
    AppState::new_with_default_selection(vec![
        tool_view("hf", ToolStatus::NotInstalled, 0),
        tool_view("llama-cli", ToolStatus::NotInstalled, 0),
        tool_view("lm-studio", ToolStatus::NotInstalled, 0),
        tool_view("ollama", ToolStatus::Ok, 4),
    ])
}

// ---------------------------------------------------------------------------
// B1 — Pressing 'z' produces Msg::ZapTool
// ---------------------------------------------------------------------------

#[test]
fn z_keypress_dispatches_zap_tool_message() {
    let key = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE);
    assert_eq!(
        dispatch(key),
        Msg::ZapTool,
        "pressing 'z' must dispatch Msg::ZapTool"
    );
}

// ---------------------------------------------------------------------------
// B1 (state) — Msg::ZapTool opens the zap dialog with the selected tool's
// metrics shown.
// ---------------------------------------------------------------------------

#[test]
fn zap_tool_message_opens_dialog_with_tool_metrics() {
    let state = state_with_only_ollama_installed();
    let (next, _effect) = update(state, Msg::ZapTool);
    let dialog = next
        .zap_dialog
        .expect("zap dialog must be open after ZapTool");
    assert_eq!(
        dialog.tool,
        ToolId("ollama"),
        "dialog must reference the currently-selected tool"
    );
    assert_eq!(
        dialog.model_count, 4,
        "dialog must show ollama's model count (4)"
    );
    assert!(
        !dialog.is_empty_tool(),
        "ollama has 4 models — not the empty path"
    );
}

// ---------------------------------------------------------------------------
// B7 — Zap on empty tool opens dialog in benign mode.
// ---------------------------------------------------------------------------

#[test]
fn zap_on_empty_tool_opens_dialog_in_benign_mode() {
    let state = AppState::new_with_default_selection(vec![tool_view("ollama", ToolStatus::Ok, 0)]);
    let (next, _effect) = update(state, Msg::ZapTool);
    let dialog = next
        .zap_dialog
        .expect("zap dialog must open even on empty tool");
    assert_eq!(dialog.model_count, 0);
    assert!(
        dialog.is_empty_tool(),
        "0-model tool must mark dialog as benign (no destructive path)"
    );
}

// ---------------------------------------------------------------------------
// B2, B3 — Typing characters and backspace mutate the dialog input buffer.
// ---------------------------------------------------------------------------

#[test]
fn typing_characters_appends_to_dialog_input() {
    let mut dialog = ZapConfirmState::for_tool(ToolId("ollama"), 4, 1000, 1000, 0);
    dialog.handle_char('o');
    dialog.handle_char('l');
    dialog.handle_char('l');
    assert_eq!(dialog.typed_input(), "oll");
}

#[test]
fn backspace_removes_last_character() {
    let mut dialog = ZapConfirmState::for_tool(ToolId("ollama"), 4, 1000, 1000, 0);
    dialog.handle_char('o');
    dialog.handle_char('l');
    dialog.handle_backspace();
    assert_eq!(dialog.typed_input(), "o");
    // Backspace on empty buffer is a no-op (does not panic).
    dialog.handle_backspace();
    dialog.handle_backspace();
    assert_eq!(dialog.typed_input(), "");
}

// ---------------------------------------------------------------------------
// B4 — Enter with input == tool name → ZapDecision::Confirm.
// ---------------------------------------------------------------------------

#[test]
fn enter_with_exact_tool_name_match_confirms() {
    let mut dialog = ZapConfirmState::for_tool(ToolId("ollama"), 4, 1000, 1000, 0);
    for c in "ollama".chars() {
        dialog.handle_char(c);
    }
    assert_eq!(
        dialog.decide_on_enter(),
        ZapDecision::Confirm,
        "exact match must confirm"
    );
}

// ---------------------------------------------------------------------------
// B5 — Enter with input != tool name → ZapDecision::Cancel.
// Case-sensitive: "OLLAMA" must NOT confirm.
// ---------------------------------------------------------------------------

#[test]
fn enter_with_wrong_case_cancels() {
    let mut dialog = ZapConfirmState::for_tool(ToolId("ollama"), 4, 1000, 1000, 0);
    for c in "OLLAMA".chars() {
        dialog.handle_char(c);
    }
    assert_eq!(
        dialog.decide_on_enter(),
        ZapDecision::Cancel,
        "case-sensitive: 'OLLAMA' must cancel"
    );
}

#[test]
fn enter_with_partial_match_cancels() {
    let mut dialog = ZapConfirmState::for_tool(ToolId("ollama"), 4, 1000, 1000, 0);
    for c in "olla".chars() {
        dialog.handle_char(c);
    }
    assert_eq!(dialog.decide_on_enter(), ZapDecision::Cancel);
}

// ---------------------------------------------------------------------------
// B6 — Esc closes dialog (handled by update; here we assert decide_on_esc()).
// ---------------------------------------------------------------------------

#[test]
fn esc_decision_is_always_cancel() {
    let dialog = ZapConfirmState::for_tool(ToolId("ollama"), 4, 1000, 1000, 0);
    assert_eq!(
        dialog.decide_on_esc(),
        ZapDecision::Cancel,
        "Esc must always cancel"
    );

    // Even after typing the exact name, Esc still cancels.
    let mut dialog2 = ZapConfirmState::for_tool(ToolId("ollama"), 4, 1000, 1000, 0);
    for c in "ollama".chars() {
        dialog2.handle_char(c);
    }
    assert_eq!(
        dialog2.decide_on_esc(),
        ZapDecision::Cancel,
        "Esc cancels even when input matches name"
    );
}

// ---------------------------------------------------------------------------
// B8 — Property: ANY typed input that is not byte-equal to the tool name
// produces ZapDecision::Cancel. This is the property guard.
// ---------------------------------------------------------------------------

#[test]
fn property_any_input_other_than_exact_tool_name_cancels() {
    let candidates = [
        "",
        " ",
        "ollam",          // missing last char
        "ollamaa",        // extra char
        "OLLAMA",         // wrong case
        "Ollama",         // mixed case
        " ollama",        // leading space
        "ollama ",        // trailing space
        "hf",             // different tool
        "OLLAma",         // mixed case
        "oll4ma",         // typo
        "ollama\n",       // trailing newline
        "ollama\toollam", // extra after tab
    ];
    for input in candidates {
        let mut dialog = ZapConfirmState::for_tool(ToolId("ollama"), 4, 1000, 1000, 0);
        for c in input.chars() {
            dialog.handle_char(c);
        }
        assert_eq!(
            dialog.decide_on_enter(),
            ZapDecision::Cancel,
            "input {:?} must cancel (not equal to 'ollama')",
            input
        );
    }
}

// ---------------------------------------------------------------------------
// State machine via update(): the dialog Msgs flow through update.
// ---------------------------------------------------------------------------

#[test]
fn dialog_text_input_messages_append_to_buffer() {
    let state = state_with_only_ollama_installed();
    let (state, _) = update(state, Msg::ZapTool);
    let (state, _) = update(state, Msg::DialogTextInput('o'));
    let (state, _) = update(state, Msg::DialogTextInput('l'));
    let dialog = state.zap_dialog.as_ref().expect("dialog still open");
    assert_eq!(dialog.typed_input(), "ol");
}

#[test]
fn dialog_cancel_message_closes_dialog() {
    let state = state_with_only_ollama_installed();
    let (state, _) = update(state, Msg::ZapTool);
    assert!(state.zap_dialog.is_some());
    let (state, _) = update(state, Msg::DialogCancel);
    assert!(
        state.zap_dialog.is_none(),
        "DialogCancel must close the dialog"
    );
}
