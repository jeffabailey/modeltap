//! Pure Elm-style `update()` (per ADR-006).
//!
//! `update(state, msg) -> (state, effect)` is a pure function — no I/O, no
//! mutation of inputs, no clocks. The composition root interprets the
//! returned `UpdateEffect` (write JSONL events, exit, etc.).

use crate::app_state::{AppState, FocusPane};
use crate::msg::Msg;

/// Side-effects the composition root must perform after this update. The
/// pure update function only describes effects; it does not execute them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateEffect {
    /// When set, the composition root should emit a `launch.ended` JSONL
    /// event before exiting. True ONLY for `Msg::Quit` (NOT for Ctrl+C —
    /// per the master-acceptance KPI invariant).
    pub emit_launch_ended: bool,
}

/// Pure transition. Takes ownership of `state` and returns the next state.
pub fn update(state: AppState, msg: Msg) -> (AppState, UpdateEffect) {
    match msg {
        Msg::Quit => (
            AppState {
                should_quit: true,
                exit_code: 0,
                ..state
            },
            UpdateEffect {
                emit_launch_ended: true,
            },
        ),
        Msg::CtrlC => (
            AppState {
                should_quit: true,
                exit_code: 130,
                ..state
            },
            UpdateEffect {
                emit_launch_ended: false,
            },
        ),
        Msg::SelectNextTool => (advance_tool(state, 1), UpdateEffect::default()),
        Msg::SelectPrevTool => (advance_tool(state, -1), UpdateEffect::default()),
        Msg::SelectNextRow => (advance_row(state, 1), UpdateEffect::default()),
        Msg::SelectPrevRow => (advance_row(state, -1), UpdateEffect::default()),
        Msg::ToggleFocus => (
            AppState {
                focus: match state.focus {
                    FocusPane::Left => FocusPane::Right,
                    FocusPane::Right => FocusPane::Left,
                },
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::UnboundKey => (state, UpdateEffect::default()),
    }
}

/// Move the tool selection forward or backward (cyclic). Resets the row
/// selection and scroll offset because the new tool has its own row list.
fn advance_tool(state: AppState, delta: i32) -> AppState {
    let n = state.tools.len();
    if n == 0 {
        return state;
    }
    let current = state.selected_tool as i32;
    let next = ((current + delta).rem_euclid(n as i32)) as usize;
    AppState {
        selected_tool: next,
        selected_row: 0,
        scroll_offset: 0,
        ..state
    }
}

/// Move the row selection within the current tool. Clamps at boundaries
/// (no wrap; long lists scroll instead of wrapping). Updates `scroll_offset`
/// so the cursor stays inside the visible window.
fn advance_row(state: AppState, delta: i32) -> AppState {
    let row_count = state.current_row_count();
    if row_count == 0 {
        return state;
    }
    let current = state.selected_row as i32;
    let max_idx = (row_count - 1) as i32;
    let next = (current + delta).clamp(0, max_idx) as usize;
    let scroll_offset = compute_scroll_offset(next, state.scroll_offset, state.visible_rows);
    AppState {
        selected_row: next,
        scroll_offset,
        ..state
    }
}

/// Keep the cursor inside the visible window:
/// - if `selected_row < scroll_offset`, scroll up so cursor is at the top.
/// - if `selected_row >= scroll_offset + visible_rows`, scroll down so
///   cursor is at the bottom.
/// - otherwise leave scroll_offset unchanged.
fn compute_scroll_offset(selected: usize, current_offset: usize, visible: usize) -> usize {
    if visible == 0 {
        return current_offset;
    }
    if selected < current_offset {
        return selected;
    }
    if selected >= current_offset + visible {
        return selected + 1 - visible;
    }
    current_offset
}
