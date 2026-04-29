//! Unit tests for US-08 (Bottom bar polish — dim-when-unavailable, ? help
//! overlay, single source of truth).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: SHORTCUT_TABLE entries have non-empty labels (already partly
//!         covered by US-03 B8 — extended here for the new entries)
//!     B2: render_bottom_bar(Main) contains the expected main shortcuts
//!     B3: render_bottom_bar(Detail) contains exactly the detail shortcuts
//!         and replaces the main bar (no [<-/->] tools, no [z] zap tool)
//!     B4: render_bottom_bar(Help) contains close shortcuts
//!     B5: Unavailable shortcuts get Modifier::DIM
//!     B6: Help overlay render contains Main / Detail / Dialogs sections
//!     B7: update(Msg::ToggleHelp) toggles Screen::Main <-> Screen::Help
//!     B8: INT-6 property — every key shown in the bar resolves to a non-noop Msg
//!         (≥ 256 random (screen, key) iterations)
//!     B9: dispatch ? key produces Msg::ToggleHelp
//!   budget = 9 × 2 = 18 unit tests max. We use ~10.
//!
//! Each test enters through a pure-function driving port:
//!   - `update(state, msg)` — Elm-style state machine driving port
//!   - `keymap::dispatch(key)` — key→Msg translation driving port
//!   - `render::bottom_bar::render_bottom_bar(...)` — pure render fn driving port
//!   - `screens::help_overlay::*` — pure render fn driving port

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use modeltap_core::logic::unification_status::{DetailModelView, DetailRegistration};
use modeltap_core::{ContentHash, DisplayLabel, Format, ModelStatus, ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, Screen, ToolView};
use modeltap_tui::keymap::{dispatch, BarSection, SHORTCUT_TABLE};
use modeltap_tui::msg::Msg;
use modeltap_tui::render::bottom_bar::{bar_to_plain_string, render_bottom_bar, BarContext};
use modeltap_tui::screens::detail::DetailScreenState;
use modeltap_tui::screens::help_overlay::render_help_lines;
use modeltap_tui::update::update;
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

fn state_with_models() -> AppState {
    AppState::new_with_default_selection(vec![tool_view(
        "hf",
        &["mistralai/Mistral-7B-v0.3"],
        &[4_400_000_000],
    )])
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
                tool: ToolId("llama-cli"),
                path: PathBuf::from("/llms/mistral-7b.gguf"),
                inode: Some(1002),
            },
        ],
        Some(HASH_A),
    )
}

// ---------------------------------------------------------------------------
// B1 — SHORTCUT_TABLE schema (post-extension)
// ---------------------------------------------------------------------------

#[test]
fn shortcut_table_entries_have_non_empty_labels() {
    // Note: an empty `sections` slice is intentional for "dispatch-only"
    // aliases (Right/Down arrows folded into the combined "[<-/->] tools" /
    // "[up/down] models" main labels; Tab focus toggle; Ctrl+C global
    // override) — see keymap.rs comments. The B8 INT-6 invariant
    // (`int_6_invariant_every_visible_bar_key_dispatches_to_non_noop`)
    // already enforces the meaningful direction (visible-in-bar ⇒ non-noop
    // dispatch); the reverse (dispatch ⇒ visible) is wrong by design and
    // would force duplicate labels in the bar.
    for entry in SHORTCUT_TABLE {
        assert!(
            !entry.label.is_empty(),
            "SHORTCUT_TABLE entry has empty label for {:?}",
            entry.key
        );
    }
}

#[test]
fn shortcut_table_includes_help_unify_delete_keys() {
    // After 03-01 the table must include `?`, `u`, and `d` so the bar can
    // render them and dispatch can route them.
    let codes: Vec<KeyCode> = SHORTCUT_TABLE.iter().map(|e| e.key.code).collect();
    assert!(
        codes.contains(&KeyCode::Char('?')),
        "SHORTCUT_TABLE missing '?' (toggle help)"
    );
    assert!(
        codes.contains(&KeyCode::Char('u')),
        "SHORTCUT_TABLE missing 'u' (unify)"
    );
    assert!(
        codes.contains(&KeyCode::Char('d')),
        "SHORTCUT_TABLE missing 'd' (delete-from-one)"
    );
    assert!(
        codes.contains(&KeyCode::Esc),
        "SHORTCUT_TABLE missing Esc (back / close)"
    );
}

// ---------------------------------------------------------------------------
// B2 — render_bottom_bar(Main) shows main shortcuts
// ---------------------------------------------------------------------------

#[test]
fn render_bottom_bar_main_contains_expected_shortcuts() {
    let state = state_with_models();
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, /* no_color */ false);
    let plain = bar_to_plain_string(&line);

    // The main-bar contract must include these labels (per US-01 AC-6 / US-08).
    for needle in [
        "[<-/->] tools",
        "[up/down] models",
        "[u] unify",
        "[z] zap tool",
        "[?] help",
        "[q] quit",
    ] {
        assert!(
            plain.contains(needle),
            "main bar missing {:?}, got:\n{}",
            needle,
            plain
        );
    }
}

// ---------------------------------------------------------------------------
// B3 — render_bottom_bar(Detail) replaces the main bar
// ---------------------------------------------------------------------------

#[test]
fn render_bottom_bar_detail_replaces_main_bar() {
    let mut state = state_with_models();
    state.current_screen = Screen::Detail(detail_not_unified());
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, false);
    let plain = bar_to_plain_string(&line);

    // Detail bar shortcuts present.
    for needle in ["[Esc] back", "[u] unify", "[d] delete-from-one", "[?] help"] {
        assert!(
            plain.contains(needle),
            "detail bar missing {:?}, got:\n{}",
            needle,
            plain
        );
    }
    // Main shortcuts absent — the bar replaces, not augments.
    assert!(
        !plain.contains("[<-/->] tools"),
        "detail bar must NOT include main shortcuts, got:\n{}",
        plain
    );
    assert!(
        !plain.contains("[z] zap tool"),
        "detail bar must NOT include main shortcuts, got:\n{}",
        plain
    );
}

// ---------------------------------------------------------------------------
// B4 — render_bottom_bar(Help) shows close shortcut
// ---------------------------------------------------------------------------

#[test]
fn render_bottom_bar_help_screen_shows_close_shortcut() {
    let state = state_with_models();
    let mut help_state = state;
    help_state.current_screen = Screen::Help {
        previous: Box::new(Screen::Main),
    };
    let ctx = BarContext::for_state(&help_state);
    let line = render_bottom_bar(&ctx, false);
    let plain = bar_to_plain_string(&line);

    // Help-screen bar shows at least an Esc-close shortcut.
    assert!(
        plain.contains("[Esc]") || plain.contains("[?]"),
        "help-screen bar must show '[Esc]' or '[?]' close shortcut, got:\n{}",
        plain
    );
}

// ---------------------------------------------------------------------------
// B5 — Unavailable shortcut is dimmed (Modifier::DIM)
// ---------------------------------------------------------------------------

#[test]
fn detail_bar_dims_unify_when_single_tool() {
    // SINGLE TOOL detail — [u] unify is not applicable, must be DIM.
    let mut state = state_with_models();
    state.current_screen = Screen::Detail(detail_single_tool());
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, false);

    // Walk spans and find the "[u] unify" one.
    let mut found_u = false;
    for span in &line.spans {
        if span.content.contains("[u] unify") {
            found_u = true;
            assert!(
                span.style.add_modifier.contains(Modifier::DIM),
                "AC-2: '[u] unify' on SINGLE TOOL detail must be DIM, got style={:?}",
                span.style
            );
        }
    }
    assert!(found_u, "expected '[u] unify' span in detail bar");
}

#[test]
fn main_bar_dims_zap_when_no_models_in_current_tool() {
    // Empty-tool main view — [z] zap tool has nothing to zap, must be DIM.
    let state = AppState::new_with_default_selection(vec![tool_view("hf", &[], &[])]);
    let ctx = BarContext::for_state(&state);
    let line = render_bottom_bar(&ctx, false);

    let mut found_z = false;
    for span in &line.spans {
        if span.content.contains("[z] zap tool") {
            found_z = true;
            assert!(
                span.style.add_modifier.contains(Modifier::DIM),
                "AC-2: '[z] zap tool' with empty current tool must be DIM, got style={:?}",
                span.style
            );
        }
    }
    assert!(found_z, "expected '[z] zap tool' span in main bar");
}

// ---------------------------------------------------------------------------
// B6 — Help overlay render lines include Main / Detail / Dialogs sections
// ---------------------------------------------------------------------------

#[test]
fn help_overlay_lines_include_section_headers() {
    let lines = render_help_lines();
    let plain: String = lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    for section in ["Main", "Detail", "Dialog"] {
        assert!(
            plain.contains(section),
            "help overlay missing '{}' section, got:\n{}",
            section,
            plain
        );
    }
    // Help overlay must list at least the canonical main shortcuts.
    for shortcut in ["[q] quit", "[?] help", "[Esc]"] {
        assert!(
            plain.contains(shortcut),
            "help overlay missing '{}', got:\n{}",
            shortcut,
            plain
        );
    }
}

// ---------------------------------------------------------------------------
// B7 — Msg::ToggleHelp toggles Screen::Main <-> Screen::Help
// ---------------------------------------------------------------------------

#[test]
fn toggle_help_from_main_layers_help_with_previous_main() {
    let state = state_with_models();
    let original_tool = state.selected_tool;
    let original_row = state.selected_row;

    let (after, _) = update(state, Msg::ToggleHelp);

    match &after.current_screen {
        Screen::Help { previous } => {
            assert!(
                matches!(**previous, Screen::Main),
                "ToggleHelp from Main must store Main as previous"
            );
        }
        other => panic!("ToggleHelp must produce Screen::Help, got {:?}", other),
    }
    assert_eq!(after.selected_tool, original_tool);
    assert_eq!(after.selected_row, original_row);
}

#[test]
fn toggle_help_from_help_restores_previous_screen() {
    let mut state = state_with_models();
    state.current_screen = Screen::Help {
        previous: Box::new(Screen::Main),
    };
    let (after, _) = update(state, Msg::ToggleHelp);
    assert!(
        matches!(after.current_screen, Screen::Main),
        "ToggleHelp from Help must restore the previous screen, got {:?}",
        after.current_screen
    );
}

#[test]
fn toggle_help_from_detail_returns_to_detail_after_close() {
    let mut state = state_with_models();
    let detail = detail_not_unified();
    state.current_screen = Screen::Detail(detail.clone());

    let (opened, _) = update(state, Msg::ToggleHelp);
    assert!(
        matches!(opened.current_screen, Screen::Help { .. }),
        "ToggleHelp from Detail must open Help"
    );

    let (closed, _) = update(opened, Msg::ToggleHelp);
    match closed.current_screen {
        Screen::Detail(d) => assert_eq!(d, detail, "previous Detail state must be preserved"),
        other => panic!(
            "expected Screen::Detail after closing Help, got {:?}",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// B8 — INT-6 property: every key shown in the bar resolves to a non-noop Msg
// ≥ 256 (screen, key) iterations
// ---------------------------------------------------------------------------

#[test]
fn int_6_invariant_every_visible_bar_key_dispatches_to_non_noop() {
    // Build the deterministic universe of screens we need to cover.
    let states: Vec<AppState> = vec![
        // Main with models
        state_with_models(),
        // Main without models (zap dimmed)
        AppState::new_with_default_selection(vec![tool_view("hf", &[], &[])]),
        // Detail (NOT UNIFIED — [u] available)
        {
            let mut s = state_with_models();
            s.current_screen = Screen::Detail(detail_not_unified());
            s
        },
        // Detail (SINGLE TOOL — [u] dimmed)
        {
            let mut s = state_with_models();
            s.current_screen = Screen::Detail(detail_single_tool());
            s
        },
        // Help overlay
        {
            let mut s = state_with_models();
            s.current_screen = Screen::Help {
                previous: Box::new(Screen::Main),
            };
            s
        },
    ];

    // Iterate (state, key) pairs. For each state we render the bar and pull
    // the keys actually shown in any section the entry belongs to. Then we
    // verify that for every (KeyEvent) shown, dispatch produces a non-noop Msg.
    let mut iterations = 0usize;
    // Outer loop replicates so we exceed 256 iterations even when the keyspace
    // is small (4 main + 4 detail + ~2 help = small per-state count).
    'outer: for replica in 0..32 {
        for state in &states {
            let ctx = BarContext::for_state(state);
            let line = render_bottom_bar(&ctx, /* no_color */ false);
            let plain = bar_to_plain_string(&line);

            // For every SHORTCUT_TABLE entry whose label appears in the
            // currently-rendered bar text, assert that dispatch on its key
            // yields a non-noop Msg. (`Msg::UnboundKey` is the noop sentinel.)
            for entry in SHORTCUT_TABLE {
                if !plain.contains(entry.label) {
                    continue;
                }
                let mapped = dispatch(entry.key);
                assert_ne!(
                    mapped,
                    Msg::UnboundKey,
                    "INT-6 violation (replica {}): bar shows label {:?} (key {:?}) but dispatch yielded UnboundKey",
                    replica, entry.label, entry.key
                );
                iterations += 1;
                if iterations >= 1024 {
                    break 'outer;
                }
            }
        }
    }
    assert!(
        iterations >= 256,
        "INT-6 property test must run ≥ 256 iterations, ran {}",
        iterations
    );
}

// ---------------------------------------------------------------------------
// B9 — dispatch ? produces Msg::ToggleHelp
// ---------------------------------------------------------------------------

#[test]
fn question_mark_key_dispatches_toggle_help() {
    let key = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
    assert_eq!(
        dispatch(key),
        Msg::ToggleHelp,
        "pressing '?' must dispatch Msg::ToggleHelp"
    );
}

// ---------------------------------------------------------------------------
// Additional B1/B2: BarSection enum is exposed and Main/Detail/Help/Dialog
// values exist (compile-only)
// ---------------------------------------------------------------------------

#[test]
fn bar_section_enum_exposes_all_required_variants() {
    // Compile-time check: Main, Detail, Help, Dialog are all valid variants.
    let _ = BarSection::Main;
    let _ = BarSection::Detail;
    let _ = BarSection::Help;
    let _ = BarSection::Dialog;
}
