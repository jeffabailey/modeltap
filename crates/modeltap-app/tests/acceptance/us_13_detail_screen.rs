//! Acceptance tests for US-13 (Per-model detail screen — the "aha 8.8 GB"
//! moment).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-13 @release-1 scenarios. The 4 scenarios drive the TUI's pure
//! `(update, view)` driving port (per ADR-006) — entering through
//! `Msg::OpenDetail` / `Msg::CloseDetail` and asserting on the rendered
//! `TestBackend` buffer. This is port-to-port testing at the TUI driving-port
//! scope: the driving port is the (update, view) pair, the driven boundaries
//! (Hasher port for SHA256 streaming) are mocked at the port boundary.
//!
//! Tags: @us-13 @release-1.
//!
//! Behaviors covered:
//! - AC-1 — Detail screen shows id, format, size, dedup key, per-tool paths.
//! - AC-2 — Status is one of UNIFIED / NOT UNIFIED / PARTIALLY UNIFIED /
//!   SINGLE TOOL.
//! - AC-3 — Reclaim estimate computed correctly per status.
//! - AC-4 — Esc returns to main view; previously-selected row remains.
//! - AC-7 — SINGLE TOOL: [u] dimmed + "single tool — unify not applicable".
//! - AC-8 — UNIFIED: shows "1 inode, N hardlinks" with reclaim 0.

use std::path::PathBuf;

use modeltap_core::logic::unification_status::{DetailModelView, DetailRegistration};
use modeltap_core::{ContentHash, DisplayLabel, Format, ModelStatus, ToolId};
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
        status: modeltap_core::ToolStatus::Ok,
        model_ids: model_ids.iter().map(|s| s.to_string()).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

fn render_to_text(state: &AppState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test backend");
    terminal.draw(|f| view(state, f)).expect("draw");
    let backend = terminal.backend();
    let buffer = backend.buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

/// Build an AppState selected on the multi-tool model with three separate
/// copies (NOT UNIFIED). Each registration has the SAME ContentHash but a
/// different inode → 3 separate copies.
fn state_with_not_unified_mistral() -> AppState {
    let mut state = AppState::new_with_default_selection(vec![
        tool_view("hf", &["mistralai/Mistral-7B-v0.3"], &[4_400_000_000]),
        tool_view("Loose GGUFs", &["mistral-7b.gguf"], &[4_400_000_000]),
        tool_view("ollama", &["mistral:7b"], &[4_400_000_000]),
    ]);
    // Select the hf row whose model id is "mistralai/Mistral-7B-v0.3" — the
    // canonical id this scenario exercises in the detail screen. Tools are
    // sorted alphabetically by ToolId; "Loose GGUFs" (capital L) sorts before
    // lowercase "hf"/"ollama", so we find hf's slot by name rather than index.
    state.selected_tool = state
        .tools
        .iter()
        .position(|t| t.tool.0 == "hf")
        .expect("hf must be in fixture");
    state.selected_row = 0;
    state
}

/// Build a detail screen state with three separate file copies (NOT UNIFIED).
fn detail_not_unified_3_copies() -> DetailScreenState {
    let registrations = vec![
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
        DetailRegistration {
            tool: ToolId("ollama"),
            path: PathBuf::from("/ollama/blobs/sha256-aaa"),
            inode: Some(1003),
        },
    ];
    DetailScreenState::new(
        DetailModelView {
            id: "mistralai/Mistral-7B-v0.3".to_string(),
            format: Format::Gguf,
            format_quant: Some("q4_K_M".to_string()),
            canonical_size_bytes: 4_400_000_000,
            display_label: DisplayLabel::from("mistralai/Mistral-7B-v0.3"),
            status: ModelStatus::Healthy,
        },
        registrations,
        Some(HASH_A),
    )
}

/// Build a detail screen state for a model in only one tool (SINGLE TOOL).
fn detail_single_tool_awq() -> DetailScreenState {
    let registrations = vec![DetailRegistration {
        tool: ToolId("hf"),
        path: PathBuf::from("/hub/TheBloke/foo-AWQ/model.safetensors"),
        inode: Some(2001),
    }];
    DetailScreenState::new(
        DetailModelView {
            id: "TheBloke/foo-AWQ".to_string(),
            format: Format::Awq,
            format_quant: None,
            canonical_size_bytes: 7_000_000_000,
            display_label: DisplayLabel::from("TheBloke/foo-AWQ"),
            status: ModelStatus::Healthy,
        },
        registrations,
        Some(HASH_A),
    )
}

/// Build a detail screen state with three paths sharing the same inode
/// (UNIFIED).
fn detail_unified_3_hardlinks() -> DetailScreenState {
    let registrations = vec![
        DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hub/mistralai/Mistral-7B-v0.3/model.safetensors"),
            inode: Some(7777),
        },
        DetailRegistration {
            tool: ToolId("Loose GGUFs"),
            path: PathBuf::from("/llms/mistral-7b.gguf"),
            inode: Some(7777), // SAME inode → hardlink
        },
        DetailRegistration {
            tool: ToolId("ollama"),
            path: PathBuf::from("/ollama/blobs/sha256-aaa"),
            inode: Some(7777), // SAME inode → hardlink
        },
    ];
    DetailScreenState::new(
        DetailModelView {
            id: "mistralai/Mistral-7B-v0.3".to_string(),
            format: Format::Gguf,
            format_quant: Some("q4_K_M".to_string()),
            canonical_size_bytes: 4_400_000_000,
            display_label: DisplayLabel::from("mistralai/Mistral-7B-v0.3"),
            status: ModelStatus::Healthy,
        },
        registrations,
        Some(HASH_A),
    )
}

// ---------------------------------------------------------------------------
// Scenario 1 (AC-1, AC-2, AC-3): NOT UNIFIED — 3 copies, reclaim 8.8 GB
// "Detail screen shows duplicate paths and reclaim estimate"
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_shows_duplicate_paths_and_reclaim_estimate() {
    let mut state = state_with_not_unified_mistral();
    state.current_screen = Screen::Detail(detail_not_unified_3_copies());

    let frame = render_to_text(&state);

    // AC-1: id, format, size, dedup key, per-tool paths shown.
    assert!(
        frame.contains("mistralai/Mistral-7B-v0.3"),
        "AC-1: model id missing from detail screen:\n{}",
        frame
    );
    assert!(
        frame.contains("hf"),
        "AC-1: hf registration tool missing:\n{}",
        frame
    );
    assert!(
        frame.contains("Loose GGUFs"),
        "AC-1: llama-cli registration tool missing:\n{}",
        frame
    );
    assert!(
        frame.contains("ollama"),
        "AC-1: ollama registration tool missing:\n{}",
        frame
    );
    assert!(
        frame.contains("/hub/mistralai/Mistral-7B-v0.3/model.safetensors")
            || frame.contains("Mistral-7B-v0.3/model.safetensors"),
        "AC-1: hf path missing:\n{}",
        frame
    );
    assert!(
        frame.contains("GGUF"),
        "AC-1: format label missing:\n{}",
        frame
    );

    // AC-2: status reads NOT UNIFIED with N copies.
    assert!(
        frame.contains("NOT UNIFIED"),
        "AC-2: 'NOT UNIFIED' status missing:\n{}",
        frame
    );
    assert!(
        frame.contains("3 separate copies"),
        "AC-2: '3 separate copies' missing:\n{}",
        frame
    );
    assert!(
        frame.contains("13.2 GB"),
        "AC-2: total bytes (13.2 GB) missing:\n{}",
        frame
    );

    // AC-3: reclaim estimate = (3-1) * 4.4 GB = 8.8 GB.
    assert!(
        frame.contains("8.8 GB"),
        "AC-3: reclaim estimate (8.8 GB) missing:\n{}",
        frame
    );
    assert!(
        frame.contains("would reclaim"),
        "AC-3: reclaim estimate phrasing missing:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 (AC-7): SINGLE TOOL — [u] dimmed
// "Single-tool model detail dims [u]"
// ---------------------------------------------------------------------------

#[test]
fn single_tool_model_detail_dims_unify_shortcut() {
    let mut state = state_with_not_unified_mistral();
    state.current_screen = Screen::Detail(detail_single_tool_awq());

    let frame = render_to_text(&state);

    // 1 path shown.
    assert!(
        frame.contains("/hub/TheBloke/foo-AWQ/model.safetensors")
            || frame.contains("TheBloke/foo-AWQ/model.safetensors"),
        "expected hf-only path on detail screen:\n{}",
        frame
    );

    // AC-2: SINGLE TOOL status.
    assert!(
        frame.contains("SINGLE TOOL"),
        "AC-2: 'SINGLE TOOL' status missing:\n{}",
        frame
    );

    // AC-7: "single tool — unify not applicable" annotation.
    assert!(
        frame.contains("single tool") && frame.contains("unify not applicable"),
        "AC-7: 'single tool — unify not applicable' annotation missing:\n{}",
        frame
    );

    // Bottom-bar shortcut for [u] is present (the bar is rendered) — full
    // dim styling is exercised in the unit test snapshot suite.
    assert!(
        frame.contains("[u]"),
        "bottom bar should still display [u] (dimmed):\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 (AC-8): UNIFIED — "1 inode, 3 hardlinks"
// "Already-unified model detail shows hardlink count"
// ---------------------------------------------------------------------------

#[test]
fn already_unified_model_detail_shows_hardlink_count() {
    let mut state = state_with_not_unified_mistral();
    state.current_screen = Screen::Detail(detail_unified_3_hardlinks());

    let frame = render_to_text(&state);

    // AC-2: UNIFIED — 1 inode, 3 hardlinks.
    assert!(
        frame.contains("UNIFIED"),
        "AC-2: 'UNIFIED' status missing:\n{}",
        frame
    );
    assert!(
        frame.contains("1 inode"),
        "AC-8: '1 inode' missing:\n{}",
        frame
    );
    assert!(
        frame.contains("3 hardlinks"),
        "AC-8: '3 hardlinks' missing:\n{}",
        frame
    );

    // Reclaim = 0 because already unified.
    assert!(
        frame.contains("Reclaimed: 8.8 GB") || frame.contains("already reclaimed"),
        "AC-3: already-unified must show reclaimed bytes (0 to reclaim, M already reclaimed):\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 4 (AC-4): Esc returns from detail to main view, row preserved.
// ---------------------------------------------------------------------------

#[test]
fn esc_returns_from_detail_to_main_view_with_row_preserved() {
    let mut state = state_with_not_unified_mistral();
    // Highlight a specific row before opening detail.
    state.selected_row = 0;
    let original_row = state.selected_row;
    let original_tool = state.selected_tool;

    // Open detail.
    state.current_screen = Screen::Detail(detail_not_unified_3_copies());
    assert!(
        matches!(state.current_screen, Screen::Detail(_)),
        "precondition: detail screen open"
    );

    // Press Esc — Msg::CloseDetail.
    let (after_esc, _) = update(state, Msg::CloseDetail);

    // AC-4: main view restored.
    assert!(
        matches!(after_esc.current_screen, Screen::Main),
        "AC-4: Esc must return to Main screen, got {:?}",
        after_esc.current_screen
    );

    // AC-4: previously-selected row + tool preserved.
    assert_eq!(
        after_esc.selected_tool, original_tool,
        "AC-4: selected_tool must be preserved across detail close"
    );
    assert_eq!(
        after_esc.selected_row, original_row,
        "AC-4: selected_row must be preserved across detail close"
    );

    // The main view rendered after Esc shows the model row again.
    let frame = render_to_text(&after_esc);
    assert!(
        frame.contains("mistralai/Mistral-7B-v0.3"),
        "AC-4: main view must show the previously-selected model row:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-5: Bottom bar on detail screen shows [Esc] back / [u] unify / [d] /[?].
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_bottom_bar_shows_us_08_contract() {
    let mut state = state_with_not_unified_mistral();
    state.current_screen = Screen::Detail(detail_not_unified_3_copies());

    let frame = render_to_text(&state);

    assert!(
        frame.contains("[Esc] back"),
        "AC-5: detail-screen bottom bar must show '[Esc] back':\n{}",
        frame
    );
    assert!(
        frame.contains("[u] unify"),
        "AC-5: detail-screen bottom bar must show '[u] unify':\n{}",
        frame
    );
    assert!(
        frame.contains("[d] delete-from-one"),
        "AC-5: detail-screen bottom bar must show '[d] delete-from-one':\n{}",
        frame
    );
    assert!(
        frame.contains("[?] help"),
        "AC-5: detail-screen bottom bar must show '[?] help':\n{}",
        frame
    );
}
