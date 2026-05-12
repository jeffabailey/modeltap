//! M3 — Mixed shared/unique acceptance tests for folder-group-bulk-delete
//! (US-05c, step 03-01).
//!
//! Source scenarios (un-skipped in `folder-group-delete.feature` by this step):
//!   @milestone-3 @ac-6 @ac-7 —
//!     "Dialog itemises unique, shared, and sidecar counts for a mixed folder"
//!   @milestone-3 @ac-7 @ac-16 @destructive —
//!     "Post-action summary reports bytes reclaimed and retained separately for a mixed folder"
//!
//! Strategy: pure-render assertions against the public APIs of `modeltap-tui`.
//! The dialog body is rendered via `folder_confirm_dialog::render` into a
//! `TestBackend`; the post-action banner is rendered via
//! `render::last_action::view_lines` from a hand-built `LastAction`. Both are
//! the exact code paths the production composition root drives; neither
//! requires the headless binary or a real HF cache fixture (the
//! mixed-classification orchestration is already covered by step 01-02's
//! `classify_unique_vs_shared` unit tests — this step asserts the rendered
//! surface).
//!
//! AC-7 / INT-FGD-3 invariant (Reclaim + Retained == folder.total_bytes
//! within 1 byte) is asserted DIRECTLY on the `FolderConfirmState` /
//! `FolderGroup` constructed for the dialog snapshot.

use std::path::PathBuf;

use modeltap_core::domain::last_action::LastAction;
use modeltap_core::types::{
    DedupKey, DisplayLabel, FolderGroup, Format, ModelMeta, ModelStatus, SharedModel, Sidecar,
    SidecarKind, ToolId,
};
use modeltap_tui::dialogs::folder_confirm::FolderConfirmState;
use modeltap_tui::render::{folder_confirm_dialog, last_action};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

// ---------------------------------------------------------------------------
// devon-hf-mixed fixture (pure value-objects; no real HF cache needed for
// the rendering scenarios — the orchestrator's classification engine is
// proven separately in `crates/modeltap-core/tests/folder_group_logic.rs`).
//
// Layout per the M3 feature scenario:
//   - 19 unique HF-only model files, totaling 13.2 GB (each = 13.2 GB / 19,
//     rounded so the sum is exactly 13_200_000_000 bytes)
//   - 1 shared file "Llama-3.2-1B-Instruct-Q4_K_M.gguf" (808 MB) also
//     linked in Ollama
//   - 3 sidecars totaling 1.3 MB
// ---------------------------------------------------------------------------

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";

const UNIQUE_COUNT: usize = 19;
const SHARED_COUNT: usize = 1;
const SIDECAR_COUNT: usize = 3;

const SHARED_FILE_NAME: &str = "Llama-3.2-1B-Instruct-Q4_K_M.gguf";
const SHARED_BYTES: u64 = 808 * 1_000_000; // 808 MB (1 GB == 1_000_000_000 to match format_bytes display)
const UNIQUE_TOTAL_BYTES: u64 = 13_200_000_000; // 13.2 GB
const SIDECAR_TOTAL_BYTES: u64 = 1_300_000; // 1.3 MB

fn model_meta(id_in_tool: &str, size_bytes: u64) -> ModelMeta {
    ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: id_in_tool.to_string(),
        on_disk_path: PathBuf::from(format!("/cache/hub/{id_in_tool}")),
        size_bytes,
        format: Format::Gguf,
        dedup_key: DedupKey::Tentative(DisplayLabel::from(format!("{id_in_tool}@{size_bytes}"))),
        display_label: DisplayLabel::from(id_in_tool),
        status: ModelStatus::Healthy,
    }
}

fn sidecar(name: &str, size_bytes: u64, kind: SidecarKind) -> Sidecar {
    Sidecar {
        path: PathBuf::from(format!("/cache/hub/{name}")),
        size_bytes,
        kind,
    }
}

/// Build the `devon-hf-mixed` `FolderGroup` + `Vec<SharedModel>` pair the
/// dialog and the post-action banner snapshot scenarios consume.
fn devon_hf_mixed() -> (FolderGroup, Vec<SharedModel>) {
    // 19 unique files. Distribute 13.2 GB across them with the first 18
    // taking `floor(13_200_000_000 / 19)` bytes and the last absorbing the
    // remainder so the sum is exact.
    let per_unique = UNIQUE_TOTAL_BYTES / UNIQUE_COUNT as u64;
    let last_remainder = UNIQUE_TOTAL_BYTES - per_unique * (UNIQUE_COUNT as u64 - 1);
    let mut models: Vec<ModelMeta> = (0..(UNIQUE_COUNT - 1))
        .map(|i| model_meta(&format!("{REPO_PATH}/unique-{i:02}.gguf"), per_unique))
        .collect();
    models.push(model_meta(
        &format!("{REPO_PATH}/unique-{:02}.gguf", UNIQUE_COUNT - 1),
        last_remainder,
    ));

    // 1 shared file — also linked in Ollama.
    let shared_model = model_meta(&format!("{REPO_PATH}/{SHARED_FILE_NAME}"), SHARED_BYTES);
    models.push(shared_model.clone());
    let shared = vec![SharedModel {
        model: shared_model,
        other_tools: vec![ToolId("ollama")],
    }];

    // 3 sidecars totaling 1.3 MB. README dominates; .imatrix and .urls are
    // small.
    let sidecars = vec![
        sidecar(
            &format!("{REPO_PATH}/README.md"),
            1_280_000,
            SidecarKind::Readme,
        ),
        sidecar(
            &format!("{REPO_PATH}/Llama-3.2-1B-Instruct.imatrix"),
            16_000,
            SidecarKind::Imatrix,
        ),
        sidecar(
            &format!("{REPO_PATH}/Llama-3.2-1B-Instruct-Q4_K_M.gguf.urls"),
            4_000,
            SidecarKind::Urls,
        ),
    ];

    let folder = FolderGroup::new(
        REPO_PATH.to_string(),
        PathBuf::from(
            "/home/devon/.cache/huggingface/hub/models--bartowski--Llama-3.2-1B-Instruct-GGUF",
        ),
        ToolId("hf"),
        models,
        sidecars,
    )
    .expect("devon-hf-mixed folder constructs");

    (folder, shared)
}

/// Render the folder-confirm dialog into a 100×28 TestBackend and return the
/// captured surface as a multi-line string for `insta::assert_snapshot!`.
fn render_dialog_to_string(state: &FolderConfirmState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 28)).expect("test backend");
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 100, 28);
            folder_confirm_dialog::render(f, area, state);
        })
        .expect("draw dialog");
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
// M3.1 — Dialog itemises unique, shared, and sidecar counts for a mixed folder.
//
// Feature assertions:
//   - dialog itemises "19 unique + 1 shared + 3 sidecars"
//   - dialog identifies the shared file as "also linked in Ollama"
//   - dialog shows "Reclaim: 13.2 GB"
//   - dialog shows "Retained: 0.8 GB"
//
// AC-7 / INT-FGD-3: Reclaim + Retained == folder.total_bytes within 1 byte.
// ---------------------------------------------------------------------------

#[test]
fn devon_sees_mixed_folder_dialog_itemising_unique_shared_and_sidecars() {
    let (folder, shared) = devon_hf_mixed();
    let bytes_to_reclaim = UNIQUE_TOTAL_BYTES + SIDECAR_TOTAL_BYTES;
    let bytes_to_retain = SHARED_BYTES;

    // AC-7 / INT-FGD-3 invariant: reclaim + retain == folder.total_bytes
    // within 1 byte. The assertion lives here, in the rendering test, because
    // the dialog body shows these numbers and the user reads them as a
    // contract.
    let total = folder.total_bytes();
    let sum = bytes_to_reclaim + bytes_to_retain;
    assert!(
        sum.abs_diff(total) <= 1,
        "AC-7 / INT-FGD-3: reclaim ({bytes_to_reclaim}) + retain ({bytes_to_retain}) = {sum} \
         must equal folder.total_bytes ({total}) within 1 byte",
    );

    let state = FolderConfirmState::for_folder_with_shared(
        folder,
        UNIQUE_COUNT,
        SIDECAR_COUNT,
        bytes_to_reclaim,
        bytes_to_retain,
        shared,
    );

    let rendered = render_dialog_to_string(&state);

    // Per-line assertions before the snapshot so a failure points at the
    // missing piece, not a 28-line diff.
    assert!(
        rendered.contains("19 unique + 1 shared + 3 sidecars"),
        "AC-6: dialog must itemise '19 unique + 1 shared + 3 sidecars', got:\n{rendered}"
    );
    assert!(
        rendered.contains(SHARED_FILE_NAME),
        "AC-6: dialog must name the shared file '{SHARED_FILE_NAME}', got:\n{rendered}"
    );
    assert!(
        rendered.contains("also linked in ollama"),
        "AC-6: dialog must say 'also linked in ollama' for the shared file, got:\n{rendered}"
    );
    assert!(
        rendered.contains("Reclaim:"),
        "AC-6: dialog must show 'Reclaim:' line, got:\n{rendered}"
    );
    assert!(
        rendered.contains("Retained:"),
        "AC-6: dialog must show 'Retained:' line, got:\n{rendered}"
    );

    insta::assert_snapshot!("folder_confirm_dialog_mixed", rendered);
}

// ---------------------------------------------------------------------------
// M3.2 — Post-action summary reports bytes reclaimed and retained separately
//        for a mixed folder.
//
// Feature assertions:
//   - "Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF (success)"
//   - "23 of 23 files removed"
//   - "Reclaimed: 13.2 GB"
//   - "Retained: 0.8 GB (1 file also linked in Ollama)"
// ---------------------------------------------------------------------------

#[test]
fn devon_sees_post_action_summary_reporting_reclaim_and_retain_separately() {
    let bytes_reclaimed = UNIQUE_TOTAL_BYTES + SIDECAR_TOTAL_BYTES;
    let bytes_retained = SHARED_BYTES;
    let files_total: u64 = (UNIQUE_COUNT + SHARED_COUNT + SIDECAR_COUNT) as u64;
    let files_removed: u64 = files_total;

    // The composition root builds `LastAction` from `FolderDeleteOutcome` via
    // `for_folder_delete_success` (step 01-05) and then plumbs the mixed-
    // retain detail through the public `extra` field (step 03-01). The
    // headless wiring sets `extra = "1 file also linked in ollama"` for the
    // mixed scenario; this test constructs the same shape directly so it
    // exercises ONLY the render layer.
    let mut last_action = LastAction::for_folder_delete_success(
        REPO_PATH.to_string(),
        bytes_reclaimed,
        bytes_retained,
        files_total,
        files_removed,
    );
    last_action.extra = Some("1 file also linked in ollama".to_string());

    let lines = last_action::view_lines(&last_action);

    let frame = lines.join("\n");
    assert!(
        frame.contains(&format!("Last action: folder-delete {REPO_PATH} (success)")),
        "AC-16: header line must say 'Last action: folder-delete {REPO_PATH} (success)', got:\n{frame}"
    );
    assert!(
        frame.contains("23 of 23 files removed"),
        "AC-16: must show '23 of 23 files removed', got:\n{frame}"
    );
    // Separate Reclaimed and Retained lines (M3 contract — distinct from
    // the WS / M1 banner which folded retain into the Reclaimed parenthetical).
    let reclaimed_line = lines
        .iter()
        .find(|l| l.starts_with("Reclaimed:"))
        .unwrap_or_else(|| panic!("AC-16: expected a 'Reclaimed:' line, got:\n{frame}"));
    let retained_line = lines
        .iter()
        .find(|l| l.starts_with("Retained:"))
        .unwrap_or_else(|| {
            panic!("AC-16: expected a 'Retained:' line distinct from Reclaimed, got:\n{frame}")
        });
    assert!(
        reclaimed_line.contains("13.2 GB"),
        "AC-16: Reclaimed line must contain '13.2 GB', got: {reclaimed_line}"
    );
    assert!(
        retained_line.contains("0.8 GB"),
        "AC-16: Retained line must contain '0.8 GB', got: {retained_line}"
    );
    assert!(
        retained_line.contains("1 file also linked in ollama"),
        "AC-16: Retained line must include '1 file also linked in ollama' parenthetical, got: {retained_line}"
    );

    insta::assert_snapshot!("folder_delete_post_action_mixed", frame);
}
