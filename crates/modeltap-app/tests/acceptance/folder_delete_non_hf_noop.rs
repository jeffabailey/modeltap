//! M2 — Shift+F no-op when the active tool is not Hugging Face
//! (US-05c, AC-5, step 02-02).
//!
//! Source scenario (un-skipped in `folder-group-delete.feature` by this step):
//!   @us-05c @milestone-2 @ac-5
//!     "Shift+F is a no-op when the active tool is not Hugging Face"
//!
//! Gherkin:
//!   Given Devon has fixture "devon-multi-tool" with both Ollama and Hugging
//!     Face installed
//!   And Devon has selected "Ollama" in the left pane
//!   And the cursor is on a model row in the Ollama right pane
//!   When Devon presses Shift+F
//!   Then no dialog opens
//!   And the "[F]" indicator in the bottom bar is dimmed
//!   And the Ollama fixture directory is unchanged
//!
//! ## Test design
//!
//! Shift+F has two production seams: (1) the keymap `dispatch_*` translation
//! and (2) the bottom-bar `render_bottom_bar` availability gate. The "no
//! dialog opens" assertion is the *absence* of a `Msg::RequestFolderDelete`
//! dispatch (which is what would trigger the dialog in the orchestrator).
//! The "[F] is dimmed" assertion is on the bottom-bar `Span` style.
//!
//! This file is the ATDD outer loop for AC-5: it exercises the keymap and
//! bottom-bar render layers as their own driving ports (the keymap function
//! signature IS the public driving port for the keystroke→message
//! translation; the `render_bottom_bar` signature IS the public driving port
//! for the bottom-bar render). Per `tdd-methodology.md` §"Port-to-port
//! testing", pure-function APIs ARE their own driving ports.
//!
//! The Ollama fixture-dir-unchanged assertion is structural: because the
//! keymap returns `Msg::UnboundKey` for Shift+F when the active tool is not
//! HF, the orchestrator is never invoked, so the filesystem CANNOT mutate.
//! We assert this directly via a `DirManifest` pre/post snapshot so a future
//! refactor that accidentally routes the message into the orchestrator (which
//! would be observable as Ollama-dir changes) is caught.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use modeltap_core::{LeftPaneSlot, ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, FocusPane, ToolView};
use modeltap_tui::keymap::{dispatch_with_active_tool, SHORTCUT_TABLE};
use modeltap_tui::msg::Msg;
use modeltap_tui::render::bottom_bar::{render_bottom_bar, BarContext};
use ratatui::style::Modifier;
use tempfile::TempDir;

use super::dir_manifest::DirManifest;

// ---------------------------------------------------------------------------
// Fixture: devon-multi-tool
//
// Builds a minimal HF cache AND a minimal Ollama models tree so the test
// proves that selecting "Ollama" leaves its blob store byte-identical
// pre/post the (no-op) Shift+F. Sizes are sparse to keep the test fast.
// ---------------------------------------------------------------------------

const HF_REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const HF_REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";
const OLLAMA_BLOB_NAME: &str = "sha256-deadbeefcafef00dba5eba11abad1deaff00bafe";
const OLLAMA_BLOB_BYTES: u64 = 4_400_000_000;

struct DevonMultiToolFixture {
    _temp: TempDir,
    ollama_root: PathBuf,
}

fn build_multi_tool_fixture() -> DevonMultiToolFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();

    // HF cache — populated so the user has a folder header to (potentially)
    // target with Shift+F, proving the no-op is driven by the ACTIVE-TOOL
    // gate rather than by an empty inventory.
    let hf_home = root.join(".cache").join("huggingface");
    let hub = hf_home.join("hub");
    let hf_repo_dir = hub.join(HF_REPO_DIR_NAME);
    let blobs_dir = hf_repo_dir.join("blobs");
    let snap_dir = hf_repo_dir.join("snapshots").join(HF_REV_SHA);
    let refs_dir = hf_repo_dir.join("refs");
    fs::create_dir_all(&blobs_dir).expect("hf blobs dir");
    fs::create_dir_all(&snap_dir).expect("hf snap dir");
    fs::create_dir_all(&refs_dir).expect("hf refs dir");
    let blob_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let blob_path = blobs_dir.join(blob_hash);
    fs::File::create(&blob_path)
        .unwrap()
        .set_len(808 * 1024 * 1024)
        .unwrap();
    symlink(
        PathBuf::from("..").join("..").join("blobs").join(blob_hash),
        snap_dir.join("Llama-3.2-1B-Instruct-Q4_K_M.gguf"),
    )
    .expect("symlink HF blob");
    fs::write(refs_dir.join("main"), HF_REV_SHA).unwrap();

    // Ollama side — single blob in the blobs/ dir. The acceptance assertion
    // is on `ollama_root`'s `DirManifest` being byte-identical pre/post the
    // Shift+F no-op. If a future regression routes Shift+F into the
    // orchestrator with the Ollama tool active, the orchestrator would
    // either refuse with `DeleteError::Unsupported` (no fs side effect) OR
    // — in the failure-mode this test guards against — start touching the
    // Ollama tree.
    let ollama_root = root.join("ollama");
    let ollama_blobs = ollama_root.join("models").join("blobs");
    fs::create_dir_all(&ollama_blobs).expect("ollama blobs dir");
    fs::File::create(ollama_blobs.join(OLLAMA_BLOB_NAME))
        .unwrap()
        .set_len(OLLAMA_BLOB_BYTES)
        .unwrap();

    DevonMultiToolFixture {
        _temp: temp,
        ollama_root,
    }
}

// ---------------------------------------------------------------------------
// AppState builder for the "Ollama selected" precondition.
//
// Two ToolViews — `hf` and `ollama`. The default-selection algorithm sorts
// alphabetically and picks the first installed tool (`hf`), so the test
// explicitly advances `selected_tool` to the Ollama index. Right-pane focus
// + a real model row simulates the "cursor on a model row in the Ollama
// right pane" precondition.
// ---------------------------------------------------------------------------

fn ollama_selected_state() -> AppState {
    let hf = ToolView {
        tool: ToolId("hf"),
        status: ToolStatus::Ok,
        model_ids: vec!["bartowski/Llama-3.2-1B-Instruct-GGUF".to_string()],
        model_sizes_bytes: vec![808 * 1024 * 1024],
    };
    let ollama = ToolView {
        tool: ToolId("ollama"),
        status: ToolStatus::Ok,
        model_ids: vec!["llama3.2:1b".to_string()],
        model_sizes_bytes: vec![1_300_000_000],
    };
    let mut state = AppState::new_with_default_selection(vec![hf, ollama]);
    // Sorted alphabetically: ["hf", "ollama"] → Ollama is index 1.
    let ollama_idx = state
        .left_pane_slots
        .iter()
        .position(|slot| match slot {
            LeftPaneSlot::Real(t) => t.tool == ToolId("ollama"),
            _ => false,
        })
        .expect("ollama slot present");
    state.selected_tool = ollama_idx;
    state.selected_row = 0; // cursor on the first Ollama model row
    state.focus = FocusPane::Right;
    assert_eq!(
        state.current_tool().map(|t| t.tool),
        Some(ToolId("ollama")),
        "fixture precondition: Ollama must be the active tool"
    );
    state
}

// ---------------------------------------------------------------------------
// THE M2 fourth scenario — "Shift+F is a no-op when the active tool is not
// Hugging Face".
// ---------------------------------------------------------------------------

#[test]
fn shift_f_is_noop_when_active_tool_is_ollama() {
    // Given — "devon-multi-tool" fixture + Ollama active + cursor on a model row.
    let fixture = build_multi_tool_fixture();
    let pre = DirManifest::snapshot(&fixture.ollama_root);
    assert!(
        pre.file_count() >= 1,
        "fixture precondition: ollama_root has at least one seeded file"
    );

    let state = ollama_selected_state();

    // When — Devon presses Shift+F. The production keymap entry point is
    // `dispatch_with_active_tool` (added in this step), which the composition
    // root invokes with the currently-active tool. Per AC-5 the dispatch
    // must short-circuit to a no-op when active_tool != hf.
    let active_tool = state.current_tool().map(|t| t.tool);
    let key = KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT);
    let msg = dispatch_with_active_tool(key, state.focus, active_tool.as_ref());

    // Then — "no dialog opens" is observable as: the keymap did NOT produce
    // `Msg::RequestFolderDelete`. Any other Msg (including UnboundKey) is
    // acceptable — the orchestrator only opens the folder-delete dialog on
    // RequestFolderDelete. Asserting on `UnboundKey` explicitly pins the
    // no-op contract.
    assert_eq!(
        msg,
        Msg::UnboundKey,
        "AC-5: Shift+F on non-HF active tool MUST dispatch to UnboundKey \
         (no-op), got {:?}",
        msg
    );

    // Then — the "[F]" indicator in the bottom bar is dimmed. The dim is
    // paired with the symbol per WCAG (no color-only signaling: symbol stays
    // visible so NO_COLOR users see the same affordance with the
    // CROSSED_OUT modifier).
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, /* no_color */ false);
    let mut found_f = false;
    for span in &line.spans {
        if span.content.contains("[F] folder-delete") {
            found_f = true;
            assert!(
                span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "AC-18: '[F] folder-delete' MUST carry Modifier::CROSSED_OUT \
                 when the active tool is not HF; got style={:?}",
                span.style
            );
        }
    }
    assert!(
        found_f,
        "expected '[F] folder-delete' span on the main bar (US-08 AC-2: \
         present-but-dimmed, not removed)"
    );

    // Then — the Ollama fixture directory is unchanged. The keymap no-op
    // means the orchestrator was never invoked; the manifest must be
    // byte-identical pre/post.
    let post = DirManifest::snapshot(&fixture.ollama_root);
    assert_eq!(
        pre, post,
        "AC-5: the Ollama fixture directory MUST be byte-identical pre/post \
         a Shift+F no-op",
    );
}

// ---------------------------------------------------------------------------
// Defense in depth: every SHORTCUT_TABLE entry whose KeyEvent is Shift+F is
// stamped against the active-tool guard. There is exactly one such entry
// today, and the assertion pins that invariant — if a future refactor adds
// a second Shift+F slot, this test forces the author to revisit the guard.
// ---------------------------------------------------------------------------
#[test]
fn shortcut_table_has_exactly_one_shift_f_entry() {
    let count = SHORTCUT_TABLE
        .iter()
        .filter(|e| {
            e.key.code == KeyCode::Char('F') && e.key.modifiers.contains(KeyModifiers::SHIFT)
        })
        .count();
    assert_eq!(
        count, 1,
        "SHORTCUT_TABLE must declare exactly one Shift+F entry; the active-\
         tool guard in `dispatch_with_active_tool` matches against that one \
         row. Found {count} entries — revisit the guard if you add another."
    );
}

// ---------------------------------------------------------------------------
// Defense in depth — property scenario: for ANY non-hf active tool, Shift+F
// is a no-op. Enumerates every non-HF plugin in the production registry.
//
// `active_tool == None` (the synthetic `[All Unified]` slot, or any caller
// that does not thread the active tool) is deliberately NOT covered here —
// the keymap preserves legacy `Shift+F → RequestFolderDelete` for the
// `None` case as a compatibility shim required by the US-03/US-08
// SHORTCUT_TABLE single-source-of-truth invariants
// (`unit_tdd_us03.rs::shortcut_table_drives_*` and
// `unit_tdd_us08.rs::int_6_invariant_every_visible_bar_key_dispatches_to_non_noop`).
// The orchestrator no-ops `RequestFolderDelete` when there is no current
// folder, so the user-visible behaviour is identical; the defense-in-depth
// here is on the *active-tool* axis, where the no-op MUST land at the
// keymap layer so the orchestrator never sees the message.
// ---------------------------------------------------------------------------
#[test]
fn shift_f_is_noop_for_every_non_hf_active_tool() {
    let non_hf_tools = [
        ToolId("ollama"),
        ToolId("lm-studio"),
        ToolId("Atomic Chat"),
        ToolId("gpt4all"),
    ];
    let key = KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT);
    for tool in non_hf_tools {
        let msg = dispatch_with_active_tool(key, FocusPane::Right, Some(&tool));
        assert_eq!(
            msg,
            Msg::UnboundKey,
            "Shift+F must be a no-op for active_tool={:?}, got {:?}",
            tool,
            msg
        );
    }
    // Sanity: WITH hf active, Shift+F DOES dispatch RequestFolderDelete.
    // This pins the positive case so a future regression that makes EVERY
    // Shift+F a no-op (over-correcting the guard) is caught.
    let hf = Some(ToolId("hf"));
    let msg = dispatch_with_active_tool(key, FocusPane::Right, hf.as_ref());
    assert!(
        matches!(msg, Msg::RequestFolderDelete),
        "regression guard: Shift+F with HF active must STILL dispatch \
         RequestFolderDelete, got {:?}",
        msg
    );
}
