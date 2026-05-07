//! Render snapshot regression for the US-05b `delete_one_dialog` overlay
//! (RCA: `docs/feature/fix-delete-one-hang/troubleshoot/rca.md` Root Cause A).
//!
//! Pre-fix, `state.delete_one_dialog = Some(...)` produced a frame that
//! did not contain the dialog at all — the render layer simply never read
//! the field. Headless acceptance tests asserted on filesystem + JSONL side
//! effects, so the missing modal slipped through.
//!
//! This test drives `modeltap_tui::view` directly with a populated
//! `delete_one_dialog` and snapshots the resulting `TestBackend`. The
//! captured frame MUST contain the dialog title and key affordance text;
//! when the layout overlay is missing, the snapshot is byte-identical to a
//! frame with `delete_one_dialog = None` and `insta::assert_snapshot!` flags
//! the regression.
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: Shared mode (`was_shared = true`) → `[y/n]` confirmation footer.
//!     B2: Unique mode (`was_shared = false`) → typed-input echo + Enter footer.
//!   budget = 2 × 2 = 4 unit tests max. We use 2 (one per mode).

use modeltap_core::ToolId;
use modeltap_tui::app_state::AppState;
use modeltap_tui::dialogs::delete_one_confirm::DeleteOneConfirmState;
use modeltap_tui::view;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Render `view(&state, frame)` into a `TestBackend(120, 40)` and return
/// the buffer as a multi-line string suitable for `insta::assert_snapshot!`.
fn render_to_string(state: &AppState) -> String {
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

// ---------------------------------------------------------------------------
// B1 — Shared mode: dialog renders title + [y]/[n]/[Esc] affordances.
// ---------------------------------------------------------------------------

#[test]
fn delete_one_dialog_renders_in_shared_mode() {
    let state = AppState {
        delete_one_dialog: Some(DeleteOneConfirmState::for_model(
            ToolId("ollama"),
            "llama3:8b",
            4_700_000_000,
            true,
        )),
        ..AppState::default()
    };

    let frame = render_to_string(&state);

    insta::assert_snapshot!("delete_one_dialog_shared_mode", frame);
}

// ---------------------------------------------------------------------------
// B2 — Unique mode: dialog renders typed-input echo + Enter/Esc footer.
// ---------------------------------------------------------------------------

#[test]
fn delete_one_dialog_renders_in_unique_mode() {
    let state = AppState {
        delete_one_dialog: Some(DeleteOneConfirmState::for_model(
            ToolId("ollama"),
            "llama3:8b",
            4_700_000_000,
            false,
        )),
        ..AppState::default()
    };

    let frame = render_to_string(&state);

    insta::assert_snapshot!("delete_one_dialog_unique_mode", frame);
}
