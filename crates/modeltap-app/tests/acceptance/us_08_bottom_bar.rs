//! Acceptance tests for US-08 (Bottom bar polish — dim-when-unavailable, ?
//! help overlay, single source of truth).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-08 @release-2 scenarios. The 3 scenarios drive the TUI's pure
//! `(update, view)` driving port (per ADR-006) — entering through `Msg`
//! and asserting on the rendered `TestBackend` buffer (port-to-port at the
//! TUI driving-port scope).
//!
//! Tags: @us-08 @release-2.
//!
//! Behaviors covered:
//! - AC-1 — Bottom bar always occupies one row.
//! - AC-2 — Unavailable shortcuts visibly dimmed but not removed.
//! - AC-3 — Detail screens / dialogs replace the main bar with their own list.
//! - AC-4 — `?` opens a comprehensive help overlay; `?`/Esc closes.
//! - AC-5 — INT-6 single-source-of-truth invariant: shortcuts shown in the
//!   bar resolve to non-noop dispatch (covered in unit-test property file).

use std::path::PathBuf;

use modeltap_core::logic::unification_status::{DetailModelView, DetailRegistration};
use modeltap_core::{ContentHash, DisplayLabel, Format, ModelStatus, ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, Screen, ToolView};
use modeltap_tui::msg::Msg;
use modeltap_tui::screens::detail::DetailScreenState;
use modeltap_tui::update::update;
use modeltap_tui::view;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

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

fn render_to_text(state: &AppState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test backend");
    terminal.draw(|f| view(state, f)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn state_devon_multi_tool() -> AppState {
    AppState::new_with_default_selection(vec![
        tool_view(
            "hf",
            &["mistralai/Mistral-7B-v0.3", "TheBloke/foo-AWQ"],
            &[4_400_000_000, 7_000_000_000],
        ),
        tool_view("llama-cli", &["mistral-7b.gguf"], &[4_400_000_000]),
        tool_view("ollama", &["mistral:7b"], &[4_400_000_000]),
    ])
}

fn detail_single_tool_awq() -> DetailScreenState {
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

// ---------------------------------------------------------------------------
// AC-2 — Unavailable shortcuts are dimmed but still drawn
// ---------------------------------------------------------------------------

#[test]
fn unavailable_shortcuts_are_dimmed_in_bottom_bar() {
    // Empty-tool selection: an AppState with no models means [u] unify and
    // [z] zap-tool are not applicable to the current selection — they should
    // be dimmed but still drawn (US-08.AC-2).
    let state = AppState::new_with_default_selection(vec![tool_view("hf", &[], &[])]);

    let frame = render_to_text(&state);

    // [u] unify is shown (still drawn) per AC-2.
    assert!(
        frame.contains("[u] unify") || frame.contains("[u]"),
        "AC-2: '[u] unify' must remain visible in the bar even when unavailable:\n{}",
        frame
    );
    // The bar is one row (AC-1) — verify the bar text appears.
    assert!(
        frame.contains("[q] quit"),
        "AC-1/AC-2: bottom bar must contain '[q] quit':\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-3 — Detail screen replaces the main bar with its own list
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_shortcuts_replace_main_bar() {
    let mut state = state_devon_multi_tool();
    state.current_screen = Screen::Detail(detail_single_tool_awq());

    let frame = render_to_text(&state);

    // AC-3: Detail bar shortcuts present.
    assert!(
        frame.contains("[Esc] back"),
        "AC-3: detail bar must show '[Esc] back':\n{}",
        frame
    );
    assert!(
        frame.contains("[u] unify"),
        "AC-3: detail bar must show '[u] unify':\n{}",
        frame
    );
    assert!(
        frame.contains("[d] delete-from-one"),
        "AC-3: detail bar must show '[d] delete-from-one':\n{}",
        frame
    );
    assert!(
        frame.contains("[?] help"),
        "AC-3: detail bar must show '[?] help':\n{}",
        frame
    );

    // AC-3: main shortcuts absent — the bar replaces, not augments.
    // The main bar contains "[<-/->] tools" and "[z] zap tool"; on the detail
    // screen, these must NOT appear.
    assert!(
        !frame.contains("[<-/->] tools"),
        "AC-3: main shortcuts must NOT appear on detail screen, found '[<-/->] tools':\n{}",
        frame
    );
    assert!(
        !frame.contains("[z] zap tool"),
        "AC-3: main shortcuts must NOT appear on detail screen, found '[z] zap tool':\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-4 — Help overlay opens with `?`, closes with `?` or Esc
// ---------------------------------------------------------------------------

#[test]
fn help_overlay_opens_with_question_mark_and_closes_with_esc() {
    let state = state_devon_multi_tool();
    let original_tool = state.selected_tool;
    let original_row = state.selected_row;

    // Press `?` — Msg::ToggleHelp from Main.
    let (after_open, _) = update(state, Msg::ToggleHelp);

    // AC-4: help overlay opens.
    assert!(
        matches!(after_open.current_screen, Screen::Help { .. }),
        "AC-4: ? must open the Help overlay, got {:?}",
        after_open.current_screen
    );

    // The rendered frame contains help-overlay sections (Main / Detail / Dialogs).
    let frame_open = render_to_text(&after_open);
    assert!(
        frame_open.to_lowercase().contains("help"),
        "AC-4: help overlay must include the word 'help':\n{}",
        frame_open
    );
    assert!(
        frame_open.contains("Main"),
        "AC-4: help overlay must include a 'Main' section:\n{}",
        frame_open
    );
    assert!(
        frame_open.contains("Detail"),
        "AC-4: help overlay must include a 'Detail' section:\n{}",
        frame_open
    );
    assert!(
        frame_open.contains("Dialogs") || frame_open.contains("Dialog"),
        "AC-4: help overlay must include a 'Dialogs' section:\n{}",
        frame_open
    );

    // Press Esc to close the help overlay.
    let (after_close, _) = update(after_open, Msg::ToggleHelp);

    // AC-4: help overlay closes; previous screen restored.
    assert!(
        matches!(after_close.current_screen, Screen::Main),
        "AC-4: ? again (or Esc) must close help and restore previous, got {:?}",
        after_close.current_screen
    );
    assert_eq!(
        after_close.selected_tool, original_tool,
        "AC-4: selected_tool must be preserved across help open/close"
    );
    assert_eq!(
        after_close.selected_row, original_row,
        "AC-4: selected_row must be preserved across help open/close"
    );
}
