//! Unit tests for US-13 (Detail screen state machine + key dispatch).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: Msg::OpenDetail sets current_screen = Detail(...)
//!     B2: Msg::CloseDetail returns current_screen = Main, restores selection
//!     B3: Detail screen renders id, format, size, dedup key, paths (snapshot)
//!     B4: Detail screen renders status header per UnificationStatus variant
//!         (4 snapshots: NotUnified / Unified / PartiallyUnified / SingleTool)
//!     B5: Detail screen bottom bar matches US-08 contract
//!     B6: Detail screen progress UX (lazy hashing) renders intermediate
//!         "computing dedup key... N%" while hash is in-flight.
//!   budget = 6 × 2 = 12 tests max. We use ~9.

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

fn tool_view(name: &'static str, model_ids: &[&str], sizes: &[u64]) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status: ToolStatus::Ok,
        model_ids: model_ids.iter().map(|s| s.to_string()).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

fn render(state: &AppState) -> String {
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

fn detail_for(model_id: &str, regs: Vec<DetailRegistration>, fmt: Format) -> DetailScreenState {
    DetailScreenState::new(
        DetailModelView {
            id: model_id.to_string(),
            format: fmt,
            format_quant: Some("q4_K_M".to_string()),
            canonical_size_bytes: 4_400_000_000,
            display_label: DisplayLabel::from(model_id),
            status: ModelStatus::Healthy,
        },
        regs,
        Some(HASH_A),
    )
}

// ---------------------------------------------------------------------------
// B1 — Msg::OpenDetail sets current_screen = Detail(...)
// ---------------------------------------------------------------------------

#[test]
fn open_detail_msg_sets_current_screen_to_detail() {
    let state = AppState::new_with_default_selection(vec![tool_view(
        "hf",
        &["mistralai/Mistral-7B-v0.3"],
        &[4_400_000_000],
    )]);
    let detail = detail_for(
        "mistralai/Mistral-7B-v0.3",
        vec![DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hf/mistral.safetensors"),
            inode: Some(1),
        }],
        Format::Safetensors,
    );

    let (next, _) = update(state, Msg::OpenDetail(detail));

    assert!(
        matches!(next.current_screen, Screen::Detail(_)),
        "Msg::OpenDetail must set current_screen = Detail, got {:?}",
        next.current_screen
    );
}

// ---------------------------------------------------------------------------
// B2 — Msg::CloseDetail returns current_screen = Main, preserves selection.
// ---------------------------------------------------------------------------

#[test]
fn close_detail_msg_returns_to_main_with_row_preserved() {
    let mut state =
        AppState::new_with_default_selection(vec![tool_view("hf", &["a", "b", "c"], &[1, 2, 3])]);
    state.selected_tool = 0;
    state.selected_row = 2;
    state.current_screen = Screen::Detail(detail_for(
        "c",
        vec![DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hf/c"),
            inode: Some(1),
        }],
        Format::Gguf,
    ));

    let (next, _) = update(state, Msg::CloseDetail);

    assert!(
        matches!(next.current_screen, Screen::Main),
        "Msg::CloseDetail must restore current_screen = Main"
    );
    assert_eq!(next.selected_row, 2, "row selection must be preserved");
    assert_eq!(next.selected_tool, 0, "tool selection must be preserved");
}

// ---------------------------------------------------------------------------
// B3 — Detail screen renders id, format, size, dedup key, paths.
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_renders_model_identity_and_paths() {
    let mut state = AppState::new_with_default_selection(vec![tool_view(
        "hf",
        &["mistralai/Mistral-7B-v0.3"],
        &[4_400_000_000],
    )]);
    let regs = vec![DetailRegistration {
        tool: ToolId("hf"),
        path: PathBuf::from("/hub/mistral/model.safetensors"),
        inode: Some(1001),
    }];
    state.current_screen =
        Screen::Detail(detail_for("mistralai/Mistral-7B-v0.3", regs, Format::Gguf));

    let frame = render(&state);

    assert!(
        frame.contains("mistralai/Mistral-7B-v0.3"),
        "id missing:\n{}",
        frame
    );
    assert!(frame.contains("GGUF"), "format missing:\n{}", frame);
    assert!(frame.contains("4.4 GB"), "size missing:\n{}", frame);
    assert!(
        frame.contains("aaaaaa") || frame.contains("AAAAAA"),
        "dedup-key hex prefix missing:\n{}",
        frame
    );
    assert!(
        frame.contains("/hub/mistral/model.safetensors"),
        "path missing:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// B4a — NotUnified status snapshot.
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_renders_not_unified_status_with_reclaim() {
    let mut state =
        AppState::new_with_default_selection(vec![tool_view("hf", &["mistral"], &[4_400_000_000])]);
    let regs = vec![
        DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hf/m"),
            inode: Some(1),
        },
        DetailRegistration {
            tool: ToolId("llama-cli"),
            path: PathBuf::from("/llms/m"),
            inode: Some(2),
        },
        DetailRegistration {
            tool: ToolId("ollama"),
            path: PathBuf::from("/ollama/m"),
            inode: Some(3),
        },
    ];
    state.current_screen = Screen::Detail(detail_for("mistral", regs, Format::Gguf));

    let frame = render(&state);

    assert!(
        frame.contains("NOT UNIFIED"),
        "NOT UNIFIED status missing:\n{}",
        frame
    );
    assert!(
        frame.contains("3 separate copies"),
        "copy count narrative missing:\n{}",
        frame
    );
    assert!(
        frame.contains("13.2 GB"),
        "13.2 GB total missing:\n{}",
        frame
    );
    assert!(
        frame.contains("8.8 GB") && frame.contains("would reclaim"),
        "reclaim narrative missing:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// B4b — Unified status snapshot.
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_renders_unified_status_with_hardlink_count() {
    let mut state =
        AppState::new_with_default_selection(vec![tool_view("hf", &["mistral"], &[4_400_000_000])]);
    let regs = vec![
        DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hf/m"),
            inode: Some(7777),
        },
        DetailRegistration {
            tool: ToolId("llama-cli"),
            path: PathBuf::from("/llms/m"),
            inode: Some(7777),
        },
        DetailRegistration {
            tool: ToolId("ollama"),
            path: PathBuf::from("/ollama/m"),
            inode: Some(7777),
        },
    ];
    state.current_screen = Screen::Detail(detail_for("mistral", regs, Format::Gguf));

    let frame = render(&state);

    assert!(frame.contains("UNIFIED"), "UNIFIED missing:\n{}", frame);
    assert!(
        frame.contains("1 inode") && frame.contains("3 hardlinks"),
        "hardlink narrative missing:\n{}",
        frame
    );
    assert!(
        frame.contains("Reclaimed: 8.8 GB") || frame.contains("already reclaimed"),
        "already-reclaimed narrative missing:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// B4c — PartiallyUnified status snapshot.
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_renders_partially_unified_status() {
    let mut state =
        AppState::new_with_default_selection(vec![tool_view("hf", &["mistral"], &[4_400_000_000])]);
    let regs = vec![
        DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hf/m"),
            inode: Some(7777),
        },
        DetailRegistration {
            tool: ToolId("llama-cli"),
            path: PathBuf::from("/llms/m"),
            inode: Some(7777),
        },
        DetailRegistration {
            tool: ToolId("ollama"),
            path: PathBuf::from("/ollama/m"),
            inode: Some(9999), // distinct
        },
    ];
    state.current_screen = Screen::Detail(detail_for("mistral", regs, Format::Gguf));

    let frame = render(&state);

    assert!(
        frame.contains("PARTIALLY UNIFIED"),
        "PARTIALLY UNIFIED status missing:\n{}",
        frame
    );
    assert!(
        frame.contains("2 of 3"),
        "shared/total narrative '2 of 3' missing:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// B4d — SingleTool status snapshot, [u] dimmed annotation.
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_renders_single_tool_status_with_dimmed_unify() {
    let mut state =
        AppState::new_with_default_selection(vec![tool_view("hf", &["foo"], &[7_000_000_000])]);
    let regs = vec![DetailRegistration {
        tool: ToolId("hf"),
        path: PathBuf::from("/hf/foo-awq"),
        inode: Some(2001),
    }];
    state.current_screen = Screen::Detail(detail_for("foo", regs, Format::Awq));

    let frame = render(&state);

    assert!(
        frame.contains("SINGLE TOOL"),
        "SINGLE TOOL status missing:\n{}",
        frame
    );
    assert!(
        frame.contains("single tool") && frame.contains("unify not applicable"),
        "annotation missing:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// B5 — Detail-screen bottom bar matches US-08 contract.
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_bottom_bar_matches_us_08_contract() {
    let mut state =
        AppState::new_with_default_selection(vec![tool_view("hf", &["foo"], &[1_000_000_000])]);
    state.current_screen = Screen::Detail(detail_for(
        "foo",
        vec![DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hf/foo"),
            inode: Some(1),
        }],
        Format::Gguf,
    ));

    let frame = render(&state);

    for shortcut in ["[Esc] back", "[u] unify", "[d] delete-from-one", "[?] help"] {
        assert!(
            frame.contains(shortcut),
            "detail bottom bar must contain {:?}:\n{}",
            shortcut,
            frame
        );
    }
}

// ---------------------------------------------------------------------------
// B6 — Lazy-hash progress UX: when hash is in-flight (not yet computed), the
// screen renders "computing dedup key... N%" instead of the final hash.
// ---------------------------------------------------------------------------

#[test]
fn detail_screen_renders_progress_while_hash_in_flight() {
    let mut state = AppState::new_with_default_selection(vec![tool_view(
        "hf",
        &["mistral"],
        &[50_000_000_000],
    )]);
    // Construct a detail screen WITHOUT a final hash — represents the
    // lazy-hash window where the cache miss is in progress.
    let mut detail = DetailScreenState::new(
        DetailModelView {
            id: "mistral".to_string(),
            format: Format::Gguf,
            format_quant: None,
            canonical_size_bytes: 50_000_000_000,
            display_label: DisplayLabel::from("mistral"),
            status: ModelStatus::Healthy,
        },
        vec![DetailRegistration {
            tool: ToolId("hf"),
            path: PathBuf::from("/hf/big.gguf"),
            inode: Some(1),
        }],
        None, // hash not yet computed
    );
    detail.set_hash_progress(50);
    state.current_screen = Screen::Detail(detail);

    let frame = render(&state);

    assert!(
        frame.contains("computing dedup key") && frame.contains("50%"),
        "progress UX must render 'computing dedup key... 50%':\n{}",
        frame
    );
}
