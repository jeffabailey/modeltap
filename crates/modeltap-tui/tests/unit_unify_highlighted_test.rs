//! Step 01-10 unit tests — `Msg::Unify` from the main view dispatches to a
//! glyph-aware response.
//!
//! Per `quality-framework` test-budget calculation:
//!   distinct behaviors:
//!     B1: `=` (DedupAble) glyph → `Msg::Unify` opens unify dialog in Confirm
//!         mode with a plan derived from the highlighted row's content_hash
//!         peers.
//!     B2: `#` (AlreadyUnified) glyph → `Msg::Unify` opens unify dialog in
//!         AlreadyUnified mode (every link reports `already_linked: true`).
//!     B3: `-` (Unique) glyph → `Msg::Unify` sets a "unique" status_line hint;
//!         no dialog opens.
//!     B4: `?` (Pending) glyph → `Msg::Unify` sets a "still computing" status
//!         hint; no dialog.
//!     B5: `~` (Hashing) glyph → same hint as Pending; no dialog.
//!     B6: `-!` (Failed) glyph → `Msg::Unify` sets a "hash failed" hint; no
//!         dialog.
//!     B7: nav messages (`SelectNextRow`, `SelectPrevRow`, `SelectNextTool`,
//!         `SelectPrevTool`) clear `state.status_line`.
//!     B8: `Msg::Unify` from `Screen::Detail` is a state-noop in pure update
//!         (the `lift_unify_in_detail` orchestrator-level lift continues to
//!         own that path).
//!   budget = 8 × 2 = 16 unit tests max. We use 8 (one per behavior).
//!
//! All tests enter through the `update::update(state, msg)` driving port and
//! assert observable state transitions. No mocks; no internal classes
//! instantiated directly except test data builders.

use modeltap_core::logic::unification_status::DetailModelView;
use modeltap_core::{ContentHash, DisplayLabel, Format, ModelStatus, ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, Screen, ToolView};
use modeltap_tui::dialogs::unify_confirm::UnifyMode;
use modeltap_tui::msg::Msg;
use modeltap_tui::screens::detail::DetailScreenState;
use modeltap_tui::update::update;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `ToolView` with one model whose id is `<tool>:m0`.
fn one_model_tool(name: &'static str, size: u64) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status: ToolStatus::Ok,
        model_ids: vec![format!("{name}:m0")],
        model_sizes_bytes: vec![size],
    }
}

fn h(byte: u8) -> ContentHash {
    ContentHash([byte; 32])
}

/// State with two tools (ollama + hf), each with one model. Both hashes
/// pre-populated to `h(7)`. Inodes provided so the rows are classifiable
/// — separate inodes by default (DedupAble).
fn state_with_two_dup_models() -> AppState {
    let mut state = AppState::new_with_default_selection(vec![
        one_model_tool("hf", 1024),
        one_model_tool("ollama", 1024),
    ]);
    state.hash_state.total = 2;
    state.hash_state.completed = 2;
    state
        .hash_state
        .completed_hashes
        .insert((ToolId("ollama"), "ollama:m0".to_string()), h(7));
    state
        .hash_state
        .completed_hashes
        .insert((ToolId("hf"), "hf:m0".to_string()), h(7));
    state
        .hash_state
        .inodes
        .insert((ToolId("ollama"), "ollama:m0".to_string()), (1, 100));
    state
        .hash_state
        .inodes
        .insert((ToolId("hf"), "hf:m0".to_string()), (1, 200));
    // Land selection on ollama:m0 (after sort, "hf" < "ollama" so ollama is
    // selected_tool == 1).
    state.selected_tool = state
        .left_pane_slots
        .iter()
        .position(|s| matches!(s, modeltap_core::domain::synthetic_slot::LeftPaneSlot::Real(t) if t.tool == ToolId("ollama")))
        .expect("ollama slot must exist");
    state.selected_row = 0;
    state
}

// ---------------------------------------------------------------------------
// B1 — `=` (DedupAble): Msg::Unify opens unify dialog in Confirm mode.
// ---------------------------------------------------------------------------

#[test]
fn unify_on_dedup_able_row_opens_dialog_in_confirm_mode() {
    let state = state_with_two_dup_models();
    assert!(
        state.unify_dialog.is_none(),
        "precondition: dialog starts closed"
    );

    let (next, _eff) = update(state, Msg::Unify);

    let dialog = next
        .unify_dialog
        .as_ref()
        .expect("AC-U4.1: Msg::Unify on '=' row must open the unify dialog");
    assert_eq!(
        dialog.mode,
        UnifyMode::Confirm,
        "AC-U4.2: dialog must open in Confirm mode (destructive path)"
    );
    // Plan must include both tools' model_ids (canonical + at least one link).
    assert!(
        !dialog.plan.links.is_empty(),
        "plan must have at least one link to perform"
    );
    assert!(
        next.status_line.is_none(),
        "no status_line hint when dialog opens"
    );
}

// ---------------------------------------------------------------------------
// B2 — `#` (AlreadyUnified): Msg::Unify opens informational dialog.
// ---------------------------------------------------------------------------

#[test]
fn unify_on_already_unified_row_opens_dialog_in_already_unified_mode() {
    let mut state = state_with_two_dup_models();
    // Both rows now share the same (device, inode) → AlreadyUnified glyph.
    state
        .hash_state
        .inodes
        .insert((ToolId("hf"), "hf:m0".to_string()), (1, 100));

    let (next, _eff) = update(state, Msg::Unify);

    let dialog = next
        .unify_dialog
        .as_ref()
        .expect("AC-U4.3: Msg::Unify on '#' row must open the dialog");
    assert_eq!(
        dialog.mode,
        UnifyMode::AlreadyUnified,
        "AC-U4.3: dialog must open in AlreadyUnified informational mode"
    );
    assert!(
        next.status_line.is_none(),
        "no status_line hint when dialog opens"
    );
}

// ---------------------------------------------------------------------------
// B3 — `-` (Unique): Msg::Unify sets status_line hint, no dialog opens.
// ---------------------------------------------------------------------------

#[test]
fn unify_on_unique_row_sets_status_line_no_dialog() {
    // Single tool, single model → no peer with matching hash → Unique.
    let mut state = AppState::new_with_default_selection(vec![one_model_tool("ollama", 1024)]);
    state.hash_state.total = 1;
    state.hash_state.completed = 1;
    state
        .hash_state
        .completed_hashes
        .insert((ToolId("ollama"), "ollama:m0".to_string()), h(9));
    state
        .hash_state
        .inodes
        .insert((ToolId("ollama"), "ollama:m0".to_string()), (1, 100));

    let (next, _eff) = update(state, Msg::Unify);

    assert!(
        next.unify_dialog.is_none(),
        "AC-U4.4: Msg::Unify on '-' row must NOT open the dialog"
    );
    let hint = next
        .status_line
        .as_ref()
        .expect("AC-U4.4: Msg::Unify on '-' row must set a status_line hint");
    assert!(
        hint.to_lowercase().contains("unique") || hint.to_lowercase().contains("nothing to unify"),
        "hint must communicate uniqueness, got: {hint:?}"
    );
}

// ---------------------------------------------------------------------------
// B4 — `?` (Pending): Msg::Unify sets "still computing" hint, no dialog.
// ---------------------------------------------------------------------------

#[test]
fn unify_on_pending_row_sets_still_computing_hint_no_dialog() {
    // Two tools both with no completed hash → Pending glyph.
    let mut state = AppState::new_with_default_selection(vec![
        one_model_tool("hf", 1024),
        one_model_tool("ollama", 1024),
    ]);
    state.hash_state.total = 2;
    state.hash_state.completed = 0;
    state.selected_tool = state
        .left_pane_slots
        .iter()
        .position(|s| matches!(s, modeltap_core::domain::synthetic_slot::LeftPaneSlot::Real(t) if t.tool == ToolId("ollama")))
        .expect("ollama slot must exist");
    state.selected_row = 0;

    let (next, _eff) = update(state, Msg::Unify);

    assert!(
        next.unify_dialog.is_none(),
        "AC-U4.5: Msg::Unify on '?' row must NOT open the dialog"
    );
    let hint = next
        .status_line
        .as_ref()
        .expect("AC-U4.5: Msg::Unify on '?' row must set a status_line hint");
    let lower = hint.to_lowercase();
    assert!(
        lower.contains("computing") || lower.contains("wait"),
        "hint must communicate hashing-in-progress, got: {hint:?}"
    );
}

// ---------------------------------------------------------------------------
// B5 — `~` (Hashing): Msg::Unify sets the same hint as Pending, no dialog.
// ---------------------------------------------------------------------------

#[test]
fn unify_on_hashing_row_sets_still_computing_hint_no_dialog() {
    let mut state = state_with_two_dup_models();
    // Mark the highlighted row's id as in-progress → Hashing glyph wins.
    state
        .hash_state
        .in_progress
        .insert("ollama:m0".to_string());

    let (next, _eff) = update(state, Msg::Unify);

    assert!(
        next.unify_dialog.is_none(),
        "Msg::Unify on '~' row must NOT open the dialog"
    );
    let hint = next
        .status_line
        .as_ref()
        .expect("Msg::Unify on '~' row must set a status_line hint");
    let lower = hint.to_lowercase();
    assert!(
        lower.contains("computing") || lower.contains("wait"),
        "hint must communicate hashing-in-progress, got: {hint:?}"
    );
}

// ---------------------------------------------------------------------------
// B6 — `-!` (Failed): Msg::Unify sets "hash failed" hint, no dialog.
// ---------------------------------------------------------------------------

#[test]
fn unify_on_failed_row_sets_hash_failed_hint_no_dialog() {
    let mut state = state_with_two_dup_models();
    // Mark the highlighted row's id as failed → Failed glyph wins.
    state.hash_state.failed.insert("ollama:m0".to_string());

    let (next, _eff) = update(state, Msg::Unify);

    assert!(
        next.unify_dialog.is_none(),
        "Msg::Unify on '-!' row must NOT open the dialog"
    );
    let hint = next
        .status_line
        .as_ref()
        .expect("Msg::Unify on '-!' row must set a status_line hint");
    let lower = hint.to_lowercase();
    assert!(
        lower.contains("hash failed") || lower.contains("re-launch"),
        "hint must communicate hash failure, got: {hint:?}"
    );
}

// ---------------------------------------------------------------------------
// B7 — Navigation messages clear status_line (mirrors last_action clearing).
// ---------------------------------------------------------------------------

#[test]
fn navigation_clears_status_line() {
    // Set a status_line, then dispatch a nav msg → status_line must be None.
    let mut state = state_with_two_dup_models();
    state.status_line = Some("This model is unique — nothing to unify".to_string());

    // Each nav msg in turn must clear the hint.
    for nav_msg in [
        Msg::SelectNextRow,
        Msg::SelectPrevRow,
        Msg::SelectNextTool,
        Msg::SelectPrevTool,
    ] {
        let mut s = state.clone();
        s.status_line = Some("hint".to_string());
        let (next, _eff) = update(s, nav_msg.clone());
        assert!(
            next.status_line.is_none(),
            "{:?} must clear status_line, got: {:?}",
            nav_msg,
            next.status_line
        );
    }
}

// ---------------------------------------------------------------------------
// B8 — Msg::Unify from Screen::Detail remains a no-op in pure update.
// ---------------------------------------------------------------------------

#[test]
fn unify_from_detail_screen_is_state_noop_in_pure_update() {
    // A Detail screen is open; the orchestrator's lift_unify_in_detail
    // handles this path. The pure update must not open a dialog or set a
    // status_line — leaving Detail-screen unify dispatch entirely to the
    // composition root (interactive.rs / headless.rs) so the existing v1
    // path (us_10_unify_hardlinks) continues to work.
    let mut state = state_with_two_dup_models();
    let model = DetailModelView {
        id: "ollama:m0".to_string(),
        format: Format::Gguf,
        format_quant: None,
        canonical_size_bytes: 1024,
        display_label: DisplayLabel::from("ollama:m0"),
        status: ModelStatus::Healthy,
    };
    let detail = DetailScreenState::new(model, vec![], Some(h(7)));
    state.current_screen = Screen::Detail(detail);
    let pre = state.clone();

    let (next, eff) = update(state, Msg::Unify);

    assert_eq!(
        next, pre,
        "Msg::Unify from Detail screen must be a state-noop in pure update"
    );
    assert_eq!(
        eff,
        modeltap_tui::update::UpdateEffect::default(),
        "Msg::Unify from Detail screen must not request side effects in pure update"
    );
}
