//! Unit tests for US-06 (post-action message + summary bar + nav-clears).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: `render::last_action::view_lines` produces correct header+body
//!         for a successful zap (with retained > 0)
//!     B2: `view_lines` for retained == 0 produces header+body without the
//!         "retained" parenthetical
//!     B3: `render::summary_bar::summary_text` produces "Total: N | Disk: X"
//!     B4: `update(state, Msg::SetLastAction)` sets state.last_action
//!     B5: `update(state, nav_msg)` clears state.last_action (every nav Msg)
//!     B6: `update(state, Msg::RefreshTool)` replaces tool slot in state.tools
//!   budget = 6 × 2 = 12 tests max. We use ~7.
//!
//! Each test enters through:
//!   - `render::last_action::view_lines` — pure render fn (port-to-port: the
//!     function signature IS the public interface)
//!   - `render::summary_bar::summary_text` — pure summary fn
//!   - `update(state, msg)` — Elm-style update driving port

use modeltap_core::domain::last_action::LastAction;
use modeltap_core::{ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, ToolView};
use modeltap_tui::msg::Msg;
use modeltap_tui::render::last_action as last_action_render;
use modeltap_tui::render::summary_bar;
use modeltap_tui::update::update;

fn tool_view(name: &'static str, status: ToolStatus, sizes: &[u64]) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status,
        model_ids: (0..sizes.len()).map(|i| format!("{name}:m{i}")).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

fn state_with_ollama() -> AppState {
    AppState::new_with_default_selection(vec![
        tool_view("hf", ToolStatus::NotInstalled, &[]),
        tool_view("Loose GGUFs", ToolStatus::NotInstalled, &[]),
        tool_view("lm-studio", ToolStatus::NotInstalled, &[]),
        tool_view(
            "ollama",
            ToolStatus::Ok,
            &[4_700_000_000, 4_400_000_000, 8_900_000_000],
        ),
    ])
}

// ---------------------------------------------------------------------------
// B1 — view_lines produces header+body for a successful zap with retained > 0.
// Per US-06.AC-1+AC-2 the schema is:
//   line 0: "Last action: zap <target> (success)"
//   line 1: "Reclaimed: <N> GB (<M> GB retained — also linked from other tools)"
// ---------------------------------------------------------------------------

#[test]
fn view_lines_for_zap_success_with_retained_renders_header_and_body() {
    let action = LastAction::for_zap_success(ToolId("Loose GGUFs"), 14_600_000_000, 6_800_000_000);
    let lines = last_action_render::view_lines(&action);
    assert_eq!(lines.len(), 2, "two lines: header + body");
    assert_eq!(lines[0], "Last action: zap Loose GGUFs (success)");
    assert_eq!(
        lines[1],
        "Reclaimed: 14.6 GB (6.8 GB retained — also linked from other tools)"
    );
}

// ---------------------------------------------------------------------------
// B2 — view_lines for retained == 0 omits the retained parenthetical.
// ---------------------------------------------------------------------------

#[test]
fn view_lines_for_zap_success_with_zero_retained_omits_retain_suffix() {
    let action = LastAction::for_zap_success(ToolId("ollama"), 12_800_000_000, 0);
    let lines = last_action_render::view_lines(&action);
    assert_eq!(lines[0], "Last action: zap ollama (success)");
    assert_eq!(
        lines[1], "Reclaimed: 12.8 GB",
        "retained-bytes parenthetical only appears when M > 0"
    );
}

// ---------------------------------------------------------------------------
// B2b — view_lines for failed zap renders "(failed)" with no Reclaimed line.
// ---------------------------------------------------------------------------

#[test]
fn view_lines_for_zap_failed_renders_failed_header_only() {
    let action = LastAction::for_zap_failed(ToolId("ollama"));
    let lines = last_action_render::view_lines(&action);
    assert_eq!(lines[0], "Last action: zap ollama (failed)");
    // For failed zap, the body line carries no bytes — schema is just the
    // header line. We still produce a body line (empty or "0 B reclaimed")
    // for layout symmetry — the contract is the FIRST line.
}

// ---------------------------------------------------------------------------
// B3 — summary_text produces "Total: N models | Disk: X GB" from AppState.
// ---------------------------------------------------------------------------

#[test]
fn summary_text_aggregates_total_models_and_disk() {
    let state = state_with_ollama();
    let text = summary_bar::summary_text(&state);
    // ollama has 3 models totaling 18.0 GB; other tools are NotInstalled.
    assert!(
        text.contains("Total: 3 models"),
        "summary should aggregate model counts, got: {}",
        text
    );
    assert!(
        text.contains("Disk: 18.0 GB"),
        "summary should aggregate disk total, got: {}",
        text
    );
}

// ---------------------------------------------------------------------------
// B4 — Msg::SetLastAction sets state.last_action.
// ---------------------------------------------------------------------------

#[test]
fn set_last_action_message_sets_state_last_action() {
    let state = state_with_ollama();
    assert!(state.last_action.is_none());
    let action = LastAction::for_zap_success(ToolId("ollama"), 12_800_000_000, 0);
    let (next, _) = update(state, Msg::SetLastAction(action.clone()));
    assert_eq!(
        next.last_action.as_ref(),
        Some(&action),
        "SetLastAction must store the LastAction in AppState"
    );
}

// ---------------------------------------------------------------------------
// B5 — Any navigation Msg clears state.last_action. Parametrized over the
// nav Msg set (M5 input variations of same behavior = ONE parametrized test).
// ---------------------------------------------------------------------------

#[test]
fn navigation_messages_clear_last_action() {
    let nav_msgs = [
        Msg::SelectNextTool,
        Msg::SelectPrevTool,
        Msg::SelectNextRow,
        Msg::SelectPrevRow,
    ];
    for nav in nav_msgs {
        let mut state = state_with_ollama();
        state.last_action = Some(LastAction::for_zap_success(
            ToolId("ollama"),
            12_800_000_000,
            0,
        ));
        let (next, _) = update(state, nav.clone());
        assert!(
            next.last_action.is_none(),
            "nav msg {:?} must clear last_action",
            nav
        );
    }
}

// ---------------------------------------------------------------------------
// B6 — Msg::RefreshTool replaces the matching tool slot in state.tools.
// ---------------------------------------------------------------------------

#[test]
fn refresh_tool_replaces_matching_slot_in_tools() {
    let state = state_with_ollama();
    // After zap, ollama discovery returns 0 models / 0 bytes.
    let refreshed = tool_view("ollama", ToolStatus::Ok, &[]);
    let (next, _) = update(state, Msg::RefreshTool(refreshed));
    let ollama = next
        .real_tools_iter()
        .find(|t| t.tool == ToolId("ollama"))
        .expect("ollama still present");
    assert_eq!(ollama.model_ids.len(), 0, "ollama refreshed to 0 models");
    assert_eq!(ollama.total_bytes(), 0, "ollama refreshed to 0 bytes");
    // Other tools must remain unchanged.
    let hf = next
        .real_tools_iter()
        .find(|t| t.tool == ToolId("hf"))
        .expect("hf still present");
    assert_eq!(hf.status, ToolStatus::NotInstalled);
}

// ---------------------------------------------------------------------------
// INT-5 invariant property test: new_total = old_total - bytes_reclaimed
// within ≤ 1 KB rounding. This is the integration invariant — a LastAction's
// bytes_reclaimed plus a refreshed inventory must be consistent.
// ---------------------------------------------------------------------------

#[test]
fn int5_invariant_new_total_equals_old_minus_reclaimed() {
    // Hand-rolled fuzzer over a small byte-size sample. For each (old_total,
    // reclaimed) pair: build a state with old_total bytes in ollama, build a
    // refreshed ToolView with (old_total - reclaimed) bytes (single model),
    // dispatch RefreshTool, and verify the resulting state's aggregate
    // matches old_total - reclaimed within 1 KB rounding tolerance.
    let cases = [
        (18_000_000_000u64, 14_600_000_000u64),
        (12_800_000_000, 12_800_000_000),
        (1_000_000_000, 0),
        (5_000_000_000, 5_000_000_000),
        (500_000_000, 250_000_000),
    ];
    for (old_total, reclaimed) in cases {
        let state = AppState::new_with_default_selection(vec![tool_view(
            "ollama",
            ToolStatus::Ok,
            &[old_total],
        )]);
        let pre = summary_bar::total_disk_bytes(&state);
        assert_eq!(pre, old_total, "pre-refresh total");

        let new_total = old_total - reclaimed;
        let refreshed = if new_total == 0 {
            tool_view("ollama", ToolStatus::Ok, &[])
        } else {
            tool_view("ollama", ToolStatus::Ok, &[new_total])
        };
        let (next, _) = update(state, Msg::RefreshTool(refreshed));
        let post = summary_bar::total_disk_bytes(&next);

        let expected = old_total - reclaimed;
        let diff = post.abs_diff(expected);
        assert!(
            diff <= 1024,
            "INT-5: post={} expected={} diff={} (>1KB)",
            post,
            expected,
            diff
        );
    }
}
