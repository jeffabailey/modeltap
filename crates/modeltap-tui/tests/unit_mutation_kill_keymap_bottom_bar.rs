//! Mutation-kill unit tests for `keymap.rs` and `render/bottom_bar.rs`.
//!
//! These tests close gaps surfaced by `cargo-mutants` during Phase 5 of the
//! `arrow-keys-navigate-tools` feature delivery. They are NOT new feature
//! tests — they assert observable behaviors of pre-existing functions in the
//! two files that ended up in the mutation scope (`dispatch_in_dialog`,
//! `BarContext::for_state`, `is_available_main`, `is_available_detail`) so
//! that arithmetic / boolean / match-arm mutations are detected by the
//! test suite.
//!
//! Driving ports:
//!   - `keymap::dispatch_in_dialog(key, unify_dialog_open)` — pure dialog
//!     keymap dispatcher.
//!   - `render::bottom_bar::{render_bottom_bar, BarContext::for_state}` —
//!     pure bar-render driving port + state-derived context.
//!
//! Each test asserts an observable outcome at the driving port:
//!   - Msg returned for a given key, OR
//!   - styled `Span` carrying `Modifier::CROSSED_OUT` (the bar's
//!     "unavailable" affordance — visible to the user as a strike-through
//!     on the shortcut label).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     M1: dispatch_in_dialog Ctrl+C is the unique CtrlC trigger
//!         (key='c' alone OR Ctrl alone must NOT produce CtrlC)
//!     M2: dispatch_in_dialog Esc/Enter/Backspace/Char(c) routing in the
//!         no-dialog branch
//!     M3: dispatch_in_dialog space inside unify dialog produces ToggleTarget
//!     M4: BarContext::for_state on Detail screen captures the detail state
//!     M5: is_available_main `[u] unify` is unavailable when current_tool has
//!         no models (CROSSED_OUT styling)
//!     M6: is_available_main `[z] zap tool` is unavailable when current_tool
//!         has no models (CROSSED_OUT styling)
//!     M7: is_available_detail `[u] unify` is unavailable on SingleTool detail
//!         (CROSSED_OUT styling) AND available on multi-tool detail
//!     M8: is_available_detail `[d] delete-from-one` is unavailable on
//!         SingleTool detail (CROSSED_OUT styling)
//!   budget = 8 × 2 = 16 unit tests max. We use 9.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use modeltap_core::logic::unification_status::{DetailModelView, DetailRegistration};
use modeltap_core::{ContentHash, DisplayLabel, Format, ModelStatus, ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, Screen, ToolView};
use modeltap_tui::keymap::dispatch_in_dialog;
use modeltap_tui::msg::Msg;
use modeltap_tui::render::bottom_bar::{render_bottom_bar, BarContext};
use modeltap_tui::screens::detail::DetailScreenState;
use ratatui::style::Modifier;
use std::path::PathBuf;

const HASH_A: ContentHash = ContentHash([0xAA; 32]);

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

fn tool_view(name: &'static str, model_ids: &[&str], sizes: &[u64]) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status: ToolStatus::Ok,
        model_ids: model_ids.iter().map(|s| s.to_string()).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

fn detail_single_tool() -> DetailScreenState {
    DetailScreenState::new(
        DetailModelView {
            id: "TheBloke/foo-AWQ".to_string(),
            format: Format::Awq,
            format_quant: None,
            canonical_size_bytes: 7_000_000_000,
            display_label: DisplayLabel::from("TheBloke/foo-AWQ"),
            status: ModelStatus::Healthy,
        },
        vec![DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hub/TheBloke/foo-AWQ/model.safetensors"),
            inode: Some(2001),
        }],
        Some(HASH_A),
    )
}

fn detail_not_unified() -> DetailScreenState {
    DetailScreenState::new(
        DetailModelView {
            id: "mistralai/Mistral-7B-v0.3".to_string(),
            format: Format::Gguf,
            format_quant: Some("q4_K_M".to_string()),
            canonical_size_bytes: 4_400_000_000,
            display_label: DisplayLabel::from("mistralai/Mistral-7B-v0.3"),
            status: ModelStatus::Healthy,
        },
        vec![
            DetailRegistration {
                tool: ToolId("hf"),
                path: PathBuf::from("/hub/mistralai/Mistral-7B-v0.3/model.safetensors"),
                inode: Some(1001),
            },
            DetailRegistration {
                tool: ToolId("Loose GGUFs"),
                path: PathBuf::from("/llms/mistral-7b.gguf"),
                inode: Some(1002),
            },
        ],
        Some(HASH_A),
    )
}

// ---------------------------------------------------------------------------
// M1 — dispatch_in_dialog Ctrl+C uniqueness
//
// Kills mutations:
//   - keymap.rs:226:39 `&& -> ||` (would treat 'c' alone OR Ctrl alone as Ctrl+C)
//   - keymap.rs:226:17 `== -> !=` (would invert the 'c' check)
// ---------------------------------------------------------------------------

#[test]
fn dispatch_in_dialog_lowercase_c_without_ctrl_is_text_input_not_ctrlc() {
    // Plain 'c' (no Ctrl modifier) must NOT produce Msg::CtrlC. With
    // `&& -> ||` the function would treat plain 'c' as Ctrl+C; with
    // `== -> !=` it would treat anything-not-'c'+Ctrl as Ctrl+C.
    let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE);
    assert_eq!(
        dispatch_in_dialog(key, /* unify_dialog_open */ false),
        Msg::DialogTextInput('c'),
        "plain 'c' (no Ctrl) inside a dialog must be DialogTextInput, not CtrlC"
    );
}

#[test]
fn dispatch_in_dialog_ctrl_other_letter_is_not_ctrlc() {
    // Ctrl+X (or any other letter with Ctrl) must NOT produce Msg::CtrlC.
    // With `&& -> ||` Ctrl+anything would route to CtrlC.
    let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    let got = dispatch_in_dialog(key, /* unify_dialog_open */ false);
    assert_ne!(
        got,
        Msg::CtrlC,
        "Ctrl+X must NOT route to CtrlC; got {:?}",
        got
    );
}

#[test]
fn dispatch_in_dialog_ctrl_c_routes_to_ctrlc() {
    // Positive-direction baseline: Ctrl+C IS Msg::CtrlC. Combined with the
    // two negatives above, the conjunction guard is fully exercised.
    let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(
        dispatch_in_dialog(key, /* unify_dialog_open */ false),
        Msg::CtrlC,
        "Ctrl+C must always route to Msg::CtrlC, even with a dialog open"
    );
}

// ---------------------------------------------------------------------------
// M2 — dispatch_in_dialog no-dialog branch routes Esc/Enter/Backspace/Char
//
// Kills mutations:
//   - keymap.rs:238 delete `KeyCode::Esc` arm → DialogCancel
//   - keymap.rs:239 delete `KeyCode::Enter` arm → DialogConfirm
//   - keymap.rs:240 delete `KeyCode::Backspace` arm → DialogBackspace
//   - keymap.rs:241 delete `KeyCode::Char(c)` arm → DialogTextInput
// ---------------------------------------------------------------------------

#[test]
fn dispatch_in_dialog_no_unify_routes_each_dialog_key_distinctly() {
    let cases: &[(KeyCode, Msg)] = &[
        (KeyCode::Esc, Msg::DialogCancel),
        (KeyCode::Enter, Msg::DialogConfirm),
        (KeyCode::Backspace, Msg::DialogBackspace),
        (KeyCode::Char('a'), Msg::DialogTextInput('a')),
        (KeyCode::Char('Z'), Msg::DialogTextInput('Z')),
    ];
    for (code, expected) in cases {
        let key = KeyEvent::new(*code, KeyModifiers::NONE);
        let got = dispatch_in_dialog(key, /* unify_dialog_open */ false);
        assert_eq!(
            got, *expected,
            "dispatch_in_dialog({:?}, false) → {:?}, expected {:?}",
            code, got, expected
        );
    }
}

// ---------------------------------------------------------------------------
// M3 — dispatch_in_dialog unify-dialog branch routes space distinctly
//
// Kills mutation:
//   - keymap.rs:231 delete `KeyCode::Char(' ')` arm → ToggleTarget(0)
// ---------------------------------------------------------------------------

#[test]
fn dispatch_in_dialog_with_unify_open_routes_space_to_toggle_target() {
    // US-U5: space inside unify dialog toggles the targeted row.
    let key = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
    assert_eq!(
        dispatch_in_dialog(key, /* unify_dialog_open */ true),
        Msg::ToggleTarget(0),
        "space inside unify dialog must dispatch ToggleTarget(0)"
    );
    // And in the no-unify branch, space falls through to text input — proving
    // the two arms are genuinely distinct (not collapsible to one).
    assert_eq!(
        dispatch_in_dialog(key, /* unify_dialog_open */ false),
        Msg::DialogTextInput(' '),
        "space outside unify dialog must dispatch DialogTextInput(' ')"
    );
}

// ---------------------------------------------------------------------------
// M4 — BarContext::for_state captures Detail screen state
//
// Kills mutation:
//   - bottom_bar.rs:71 delete `Screen::Detail(d) => Some(d)` arm
//
// Without that arm, `ctx.detail` is None even on Detail screens, which
// silently downgrades availability gating to "always true" (the None branch
// returns true) — observable as `[u] unify` NOT being CROSSED_OUT on a
// SingleTool detail screen. We assert the negative (`[u]` IS CROSSED_OUT)
// via the M7 test below; this test asserts the positive observation that
// the Detail bar replaces the Main bar (which only happens when `section ==
// Detail`, populated from the same match) AND that `ctx.detail.is_some()`.
// ---------------------------------------------------------------------------

#[test]
fn bar_context_for_state_on_detail_screen_populates_detail() {
    let mut state = AppState::new_with_default_selection(vec![tool_view(
        "hf",
        &["mistralai/Mistral-7B-v0.3"],
        &[4_400_000_000],
    )]);
    state.current_screen = Screen::Detail(detail_not_unified());
    let ctx = BarContext::for_state(&state);
    assert!(
        ctx.detail.is_some(),
        "BarContext on Detail screen must carry Some(detail); got None — \
         is_available_detail would silently fall through to true and dim \
         nothing"
    );
}

// ---------------------------------------------------------------------------
// M5/M6 — is_available_main `[u]`/`[z]` strikethrough when no models
//
// Kills mutations:
//   - bottom_bar.rs:171 delete `KeyCode::Char('z')` arm in is_available_main
//   - bottom_bar.rs:175 delete `KeyCode::Char('u')` arm in is_available_main
//
// The existing `unit_tdd_us08::main_bar_dims_zap_when_no_models_in_current_tool`
// asserted Modifier::DIM (which active shortcuts also carry), so it could not
// kill the arm-deletion mutants. CROSSED_OUT is the discriminating modifier.
// ---------------------------------------------------------------------------

#[test]
fn main_bar_strikes_through_zap_when_current_tool_has_no_models() {
    let state = AppState::new_with_default_selection(vec![tool_view("hf", &[], &[])]);
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, /* no_color */ false);
    let mut found_z = false;
    for span in &line.spans {
        if span.content.contains("[z] zap tool") {
            found_z = true;
            assert!(
                span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "'[z] zap tool' on empty-tool Main bar must be CROSSED_OUT \
                 (unavailable affordance); got style={:?}",
                span.style
            );
        }
    }
    assert!(found_z, "expected '[z] zap tool' span in main bar");
}

#[test]
fn main_bar_strikes_through_unify_when_current_tool_has_no_models() {
    let state = AppState::new_with_default_selection(vec![tool_view("hf", &[], &[])]);
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, /* no_color */ false);
    let mut found_u = false;
    for span in &line.spans {
        if span.content.contains("[u] unify") {
            found_u = true;
            assert!(
                span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "'[u] unify' on empty-tool Main bar must be CROSSED_OUT \
                 (unavailable affordance); got style={:?}",
                span.style
            );
        }
    }
    assert!(found_u, "expected '[u] unify' span in main bar");
}

// ---------------------------------------------------------------------------
// M7/M8 — is_available_detail `[u]`/`[d]` strikethrough on SingleTool
//
// Kills mutations:
//   - bottom_bar.rs:188 replace is_available_detail with `true` (would
//     leave [u]/[d] active on SingleTool — no strikethrough)
//   - bottom_bar.rs:188 replace is_available_detail with `false` (would
//     strike through Esc/?/r as well — caught via the multi-tool case)
//   - bottom_bar.rs:195 delete `KeyCode::Char('u')` arm
//   - bottom_bar.rs:198 delete `KeyCode::Char('d')` arm
//   - bottom_bar.rs:195:31 delete `!` in `Char('u')` arm (would invert — [u]
//     active on SingleTool, dimmed on multi-tool)
//   - bottom_bar.rs:198:31 delete `!` in `Char('d')` arm (same shape)
// ---------------------------------------------------------------------------

#[test]
fn detail_bar_strikes_through_unify_and_delete_on_single_tool() {
    // SingleTool detail: both [u] unify and [d] delete-from-one are
    // unavailable (nothing to unify; deleting orphans the model). Both
    // must carry CROSSED_OUT.
    let mut state = AppState::new_with_default_selection(vec![tool_view(
        "hf",
        &["TheBloke/foo-AWQ"],
        &[7_000_000_000],
    )]);
    state.current_screen = Screen::Detail(detail_single_tool());
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, /* no_color */ false);

    let (mut found_u, mut found_d) = (false, false);
    for span in &line.spans {
        if span.content.contains("[u] unify") {
            found_u = true;
            assert!(
                span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "SingleTool detail: '[u] unify' must be CROSSED_OUT; got style={:?}",
                span.style
            );
        }
        if span.content.contains("[d] delete-from-one") {
            found_d = true;
            assert!(
                span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "SingleTool detail: '[d] delete-from-one' must be CROSSED_OUT; got style={:?}",
                span.style
            );
        }
    }
    assert!(
        found_u,
        "expected '[u] unify' span in SingleTool detail bar"
    );
    assert!(
        found_d,
        "expected '[d] delete-from-one' span in SingleTool detail bar"
    );
}

#[test]
fn detail_bar_does_not_strike_through_unify_and_delete_on_multi_tool() {
    // Multi-tool detail (NotUnified status): both [u] and [d] are
    // applicable. Neither may carry CROSSED_OUT. Catches the `is_available_
    // detail -> false` mutation AND the `delete !` mutations (which would
    // INVERT the SingleTool / multi-tool gating).
    let mut state = AppState::new_with_default_selection(vec![tool_view(
        "hf",
        &["mistralai/Mistral-7B-v0.3"],
        &[4_400_000_000],
    )]);
    state.current_screen = Screen::Detail(detail_not_unified());
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, /* no_color */ false);

    let (mut found_u, mut found_d, mut found_esc) = (false, false, false);
    for span in &line.spans {
        if span.content.contains("[u] unify") {
            found_u = true;
            assert!(
                !span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "multi-tool detail: '[u] unify' must NOT be CROSSED_OUT; got style={:?}",
                span.style
            );
        }
        if span.content.contains("[d] delete-from-one") {
            found_d = true;
            assert!(
                !span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "multi-tool detail: '[d] delete-from-one' must NOT be CROSSED_OUT; got style={:?}",
                span.style
            );
        }
        // [Esc] back must always be applicable on detail; catches the
        // `is_available_detail -> false` mutation that would dim every
        // detail-bar entry.
        if span.content.contains("[Esc] back") {
            found_esc = true;
            assert!(
                !span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "multi-tool detail: '[Esc] back' must NOT be CROSSED_OUT; got style={:?}",
                span.style
            );
        }
    }
    assert!(
        found_u,
        "expected '[u] unify' span in multi-tool detail bar"
    );
    assert!(
        found_d,
        "expected '[d] delete-from-one' span in multi-tool detail bar"
    );
    assert!(
        found_esc,
        "expected '[Esc] back' span in multi-tool detail bar"
    );
}
