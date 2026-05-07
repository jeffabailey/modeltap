//! Unit tests for `is_available_main` gating of `[d] delete-from-one` on the
//! Main bar (RCA: fix-delete-one-hang Root Cause C).
//!
//! Driving port: `render::bottom_bar::render_bottom_bar(ctx, no_color)`
//! (the same pure function the production frame draw uses). We assert on the
//! styled `Span` for `[d] delete-from-one` — the same observable affordance
//! the user sees on screen.
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors: 1 — `is_available_main` for `KeyCode::Char('d')`
//!   budget = 1 × 2 = 2 unit tests max. We use 2 (both arms).
//!
//! Both arms are exercised:
//!   - `current_tool_has_models = false` (Unified virtual column / empty tool)
//!     → `[d]` MUST be dimmed (Modifier::CROSSED_OUT — the "unavailable" style
//!     the production code already uses for `[u]`/`[z]`).
//!   - `current_tool_has_models = true` (real tool with at least one model)
//!     → `[d]` MUST NOT carry CROSSED_OUT (only the baseline DIM that every
//!     active main-bar shortcut wears).

use modeltap_core::{ToolId, ToolStatus};
use modeltap_tui::app_state::{AppState, ToolView};
use modeltap_tui::render::bottom_bar::{render_bottom_bar, BarContext};
use ratatui::style::Modifier;

fn tool_view_with_models(name: &'static str, model_ids: &[&str], sizes: &[u64]) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status: ToolStatus::Ok,
        model_ids: model_ids.iter().map(|s| s.to_string()).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

fn empty_tool_view(name: &'static str) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status: ToolStatus::Ok,
        model_ids: Vec::new(),
        model_sizes_bytes: Vec::new(),
    }
}

#[test]
fn dims_d_when_no_real_tool_selected() {
    // Empty tool on Main → current_tool_has_models = false. The current
    // selection has no models, so [d] delete-from-one cannot do anything
    // (mirrors lift_delete_one_in_main's early-return in interactive.rs).
    let state = AppState::new_with_default_selection(vec![empty_tool_view("hf")]);
    let ctx = BarContext::for_state(&state);
    assert!(
        !ctx.current_tool_has_models,
        "fixture invariant: empty tool must produce current_tool_has_models = false"
    );

    let line = render_bottom_bar(&ctx, /* no_color */ false);

    let mut found_d = false;
    for span in &line.spans {
        if span.content.contains("[d] delete-from-one") {
            found_d = true;
            assert!(
                span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "RCA cause C: '[d] delete-from-one' must be marked unavailable \
                 (CROSSED_OUT) when current_tool_has_models is false; got style={:?}",
                span.style
            );
        }
    }
    assert!(
        found_d,
        "expected '[d] delete-from-one' span on the main bar (it should be \
         present-but-dimmed, not removed — US-08 AC-2)"
    );
}

#[test]
fn enables_d_when_real_tool_has_models() {
    // Real tool with models on Main → current_tool_has_models = true. The
    // dialog can be opened, so [d] must render as an active shortcut.
    let state = AppState::new_with_default_selection(vec![tool_view_with_models(
        "hf",
        &["mistralai/Mistral-7B-v0.3"],
        &[4_400_000_000],
    )]);
    let ctx = BarContext::for_state(&state);
    assert!(
        ctx.current_tool_has_models,
        "fixture invariant: tool with one model must produce current_tool_has_models = true"
    );

    let line = render_bottom_bar(&ctx, /* no_color */ false);

    let mut found_d = false;
    for span in &line.spans {
        if span.content.contains("[d] delete-from-one") {
            found_d = true;
            assert!(
                !span.style.add_modifier.contains(Modifier::CROSSED_OUT),
                "'[d] delete-from-one' must NOT be marked unavailable \
                 (CROSSED_OUT) when current_tool_has_models is true; got style={:?}",
                span.style
            );
        }
    }
    assert!(
        found_d,
        "expected '[d] delete-from-one' span on the main bar"
    );
}
