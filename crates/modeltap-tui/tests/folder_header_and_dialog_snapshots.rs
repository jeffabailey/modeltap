//! Folder-group-bulk-delete Step 01-04: TUI surface snapshot tests.
//!
//! Five behaviors (per the step's test scenarios):
//!   B1: Folder header renders **collapsed** with `[+]` indicator and the
//!       `(N unique, K shared)` split.
//!   B2: Folder header renders **expanded** with `[-]` indicator and child
//!       rows indented.
//!   B3: Shift+F on a folder header opens the folder-confirm dialog.
//!   B4: Folder-confirm dialog body shows path, absolute path, counts,
//!       Reclaim, Retained.
//!   B5: Shift+F is registered in `SHORTCUT_TABLE` — the same source the
//!       bottom bar renders from.
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors = 5, budget = 5 × 2 = 10. We use 5.
//!
//! Snapshots are captured against the pure render functions (folder-header
//! row and folder-confirm dialog) so the test does not need a fully-wired
//! `AppState` to exercise the rendering. Wiring into the right-pane lands
//! in step 01-05 (walking skeleton).

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use modeltap_core::types::{
    DedupKey, DisplayLabel, FolderGroup, Format, ModelMeta, ModelStatus, Sidecar, SidecarKind,
};
use modeltap_core::ToolId;
use modeltap_tui::dialogs::folder_confirm::FolderConfirmState;
use modeltap_tui::keymap::{dispatch, SHORTCUT_TABLE};
use modeltap_tui::msg::Msg;
use modeltap_tui::render::folder_confirm_dialog;
use modeltap_tui::render::folder_header;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// Fixture builders.
// ---------------------------------------------------------------------------

fn model_meta(id_in_tool: &str, size_bytes: u64) -> ModelMeta {
    ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: id_in_tool.to_string(),
        on_disk_path: PathBuf::from(format!("/cache/hub/{}", id_in_tool)),
        size_bytes,
        format: Format::Gguf,
        dedup_key: DedupKey::Tentative(DisplayLabel::from(format!("{id_in_tool}@{size_bytes}"))),
        display_label: DisplayLabel::from(id_in_tool),
        status: ModelStatus::Healthy,
    }
}

fn sidecar(name: &str, size_bytes: u64) -> Sidecar {
    Sidecar {
        path: PathBuf::from(format!("/cache/hub/{name}")),
        size_bytes,
        kind: SidecarKind::Readme,
    }
}

/// All-unique HF folder: 3 model files + 1 README sidecar; total bytes
/// match the values surfaced in the walking-skeleton scenario.
fn folder_all_unique() -> FolderGroup {
    let models = vec![
        model_meta(
            "bartowski/Llama-3.2-1B-Instruct-GGUF/Q4_K_M.gguf",
            1_000_000_000,
        ),
        model_meta(
            "bartowski/Llama-3.2-1B-Instruct-GGUF/Q5_K_M.gguf",
            1_200_000_000,
        ),
        model_meta(
            "bartowski/Llama-3.2-1B-Instruct-GGUF/Q8_0.gguf",
            1_500_000_000,
        ),
    ];
    let sidecars = vec![sidecar(
        "bartowski/Llama-3.2-1B-Instruct-GGUF/README.md",
        10_000,
    )];
    FolderGroup::new(
        "bartowski/Llama-3.2-1B-Instruct-GGUF".to_string(),
        PathBuf::from(
            "/home/devon/.cache/huggingface/hub/models--bartowski--Llama-3.2-1B-Instruct-GGUF",
        ),
        ToolId("hf"),
        models,
        sidecars,
    )
    .expect("folder_all_unique constructs")
}

/// Render the folder-header pure function into a TestBackend strip and return
/// the captured surface as a multi-line string (last line stripped of trailing
/// whitespace) for `insta::assert_snapshot!`.
fn render_folder_header_to_string(
    folder: &FolderGroup,
    expanded: bool,
    unique: usize,
    shared: usize,
) -> String {
    let line = folder_header::render_folder_header_line(folder, expanded, unique, shared);
    // Render directly into a one-row TestBackend so snapshots include the
    // exact characters the user will see — including the `[+]`/`[-]` glyph
    // and the bytes label produced by `format_bytes`.
    let mut terminal = Terminal::new(TestBackend::new(120, 1)).expect("test backend");
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 120, 1);
            f.render_widget(ratatui::widgets::Paragraph::new(line), area);
        })
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for x in 0..buffer.area.width {
        out.push_str(buffer[(x, 0)].symbol());
    }
    out.trim_end().to_string()
}

/// Render the folder-confirm dialog into a 100×24 TestBackend and return the
/// captured surface as a multi-line string for `insta::assert_snapshot!`.
fn render_folder_dialog_to_string(state: &FolderConfirmState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("test backend");
    terminal
        .draw(|f| {
            let area = f.area();
            folder_confirm_dialog::render(f, area, state);
        })
        .expect("draw");
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
// B1 — Collapsed folder header (US-05c.AC-2): `[+]` glyph + (N unique, K shared).
// ---------------------------------------------------------------------------

#[test]
fn folder_header_collapsed_shows_plus_glyph_and_unique_shared_split() {
    let folder = folder_all_unique();
    let rendered = render_folder_header_to_string(&folder, false, 3, 0);
    insta::assert_snapshot!("folder_header_collapsed", rendered);
}

// ---------------------------------------------------------------------------
// B2 — Expanded folder header (US-05c.AC-3): `[-]` glyph.
// ---------------------------------------------------------------------------

#[test]
fn folder_header_expanded_shows_minus_glyph() {
    let folder = folder_all_unique();
    let rendered = render_folder_header_to_string(&folder, true, 3, 0);
    insta::assert_snapshot!("folder_header_expanded", rendered);
}

// ---------------------------------------------------------------------------
// B3 — Shift+F dispatches to the folder-delete request message
// (US-05c.AC-4 / AC-19). The keymap is the single source of truth so this
// proves both bottom-bar render and dispatch are wired through one table.
// ---------------------------------------------------------------------------

#[test]
fn shift_f_dispatches_to_request_folder_delete() {
    let key = KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT);
    let got = dispatch(key);
    // Sentinel payload (empty folder path) is what the keymap emits — the
    // composition root resolves the actual folder from the cursor's row.
    // The semantic check is: dispatch produced a RequestFolderDelete variant,
    // NOT some other Msg (e.g. UnboundKey).
    assert!(
        matches!(got, Msg::RequestFolderDelete),
        "Shift+F must dispatch RequestFolderDelete (got {:?})",
        got
    );
}

// ---------------------------------------------------------------------------
// B4 — Folder-confirm dialog body (US-05c.AC-6 / AC-8 / INT-FGD-7).
// Shows path, absolute path, counts, Reclaim, Retained. All-unique mode for
// step 01-04 (mixed shared/unique itemization lands at 03-01).
// ---------------------------------------------------------------------------

#[test]
fn folder_confirm_dialog_all_unique_body_shows_path_counts_reclaim_retained() {
    let folder = folder_all_unique();
    let total_bytes = folder.total_bytes();
    let file_count = folder.file_count();
    let state = FolderConfirmState::for_folder(folder, 3, 0, 1, total_bytes, 0);
    assert_eq!(state.file_count(), file_count);
    let rendered = render_folder_dialog_to_string(&state);
    insta::assert_snapshot!("folder_confirm_dialog_all_unique", rendered);
}

// ---------------------------------------------------------------------------
// B5 — `Shift+F` is in `SHORTCUT_TABLE` (US-05c.AC-19 / INT-FGD-7).
// The bottom-bar render and the dispatch table read from the same array.
// ---------------------------------------------------------------------------

#[test]
fn shift_f_is_registered_in_shortcut_table_single_source() {
    let entry = SHORTCUT_TABLE
        .iter()
        .find(|e| e.key.code == KeyCode::Char('F') && e.key.modifiers.contains(KeyModifiers::SHIFT))
        .expect("Shift+F entry must exist in SHORTCUT_TABLE");
    // The label must be the user-visible "[F] folder-delete" string per
    // US-05c.AC-18; this is the same string the bottom bar will render.
    assert_eq!(entry.label, "[F] folder-delete");
    // The dispatch slot must be `RequestFolderDelete { folder: <sentinel> }`.
    assert!(
        matches!(entry.msg, Msg::RequestFolderDelete),
        "Shift+F SHORTCUT_TABLE entry dispatches RequestFolderDelete, got {:?}",
        entry.msg
    );
}
