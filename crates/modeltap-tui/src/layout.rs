//! Top-level view function and terminal-size guard.
//!
//! Per ADR-006 the view is pure: it reads `&AppState` and writes into a
//! ratatui `Frame`. No I/O, no mutation. The actual pane rendering lives
//! in the `render::*` submodules; this module only owns the layout splitting
//! and the terminal-size guard.

use modeltap_core::MIN_TERMINAL_COLUMNS;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app_state::{AppState, Screen};
use crate::render::{
    bottom_bar, delete_one_dialog, left_pane, right_pane, running_tool_dialog, summary_bar,
    unify_dialog, zap_dialog,
};
use crate::screens::detail::render_detail;
use crate::screens::help_overlay;

/// Returned when the terminal is narrower than the minimum width on startup
/// (US-01 AC-4). The composition root prints this to stderr and exits 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSizeError {
    pub required: u16,
    pub actual: u16,
}

impl std::fmt::Display for TerminalSizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Terminal too narrow: need at least {} columns, found {}",
            self.required, self.actual
        )
    }
}

impl std::error::Error for TerminalSizeError {}

/// Validate the terminal width before any TUI initialization. Returns
/// `Err(TerminalSizeError)` when too narrow; the composition root pattern-
/// matches on this and exits cleanly without ever entering raw mode.
pub fn check_terminal_width(actual: u16) -> Result<(), TerminalSizeError> {
    if actual < MIN_TERMINAL_COLUMNS {
        return Err(TerminalSizeError {
            required: MIN_TERMINAL_COLUMNS,
            actual,
        });
    }
    Ok(())
}

/// Compute the outer Main-screen layout for a given terminal `size`. Mirrors
/// `view_main`'s vertical and horizontal split exactly so the production
/// interactive loop can read the same per-pane `Rect`s used by render.
///
/// Returned chunks: `[left_pane, right_pane, summary_bar, bottom_bar]`.
/// When the terminal is too small for the constraints (e.g. height < 3),
/// ratatui shrinks the rects but does not panic; the helper returns whatever
/// ratatui produces.
fn outer_chunks(size: Rect) -> [Rect; 4] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(size);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(chunks[0]);
    [panes[0], panes[1], chunks[1], chunks[2]]
}

/// Number of right-pane content rows visible in a terminal of size `size`.
///
/// Mirrors the right-pane render's effective body height: the right pane is
/// the wider 70% horizontal split of the top "Min(1)" vertical chunk; the
/// outer block consumes 1 row at the top and 1 row at the bottom for the
/// border. This matches `state.visible_rows`'s semantics — the count of
/// model rows the user can see at once.
///
/// Always returns at least 1 so the production event loop never writes a
/// degenerate `visible_rows = 0` (which would freeze scrolling).
pub fn right_pane_body_rows(size: Rect) -> usize {
    let [_, right, _, _] = outer_chunks(size);
    let h = right.height as usize;
    h.saturating_sub(2).max(1)
}

/// Number of left-pane content rows visible in a terminal of size `size`.
///
/// Same semantics as `right_pane_body_rows` but for the 30% horizontal
/// split. Returns at least 1.
pub fn left_pane_body_rows(size: Rect) -> usize {
    let [left, _, _, _] = outer_chunks(size);
    let h = left.height as usize;
    h.saturating_sub(2).max(1)
}

/// Top-level pure view function. Dispatches on `state.current_screen`:
///
/// - `Screen::Main` — two-pane discovery view + summary bar + shortcut bar.
/// - `Screen::Detail(...)` — per-model detail screen (US-13). The detail
///   screen owns its own bottom bar; we do not render the main shortcut bar
///   while the detail screen is active.
/// - `Screen::Help { previous }` — the layered help overlay (US-08). Renders
///   the underlying screen first (so closing reveals the same state), then
///   overlays the centered help modal on top.
pub fn view(state: &AppState, frame: &mut Frame<'_>) {
    let area = frame.area();

    match &state.current_screen {
        Screen::Main => view_main(state, frame, area),
        Screen::Detail(detail) => {
            render_detail(frame, area, detail, state);
            // The unify dialog (US-10) is opened from the detail screen via
            // `Msg::OpenUnifyDialog`, so we must overlay it on top of the
            // detail view as well. Same `Option` gate as in `view_main`.
            if let Some(dialog) = state.unify_dialog.as_ref() {
                unify_dialog::render(frame, area, dialog);
            }
            // US-05b single-model delete dialog (ADR-009). Rendered BEFORE
            // `running_tool_dialog` so the running-tool gate wins layering
            // when both are somehow open at once (defense-in-depth — the
            // well-formed workflow only opens one at a time).
            if let Some(dialog) = state.delete_one_dialog.as_ref() {
                delete_one_dialog::render(frame, area, dialog);
            }
            // US-17 running-tool prompt overlays both unify and delete-one
            // gates on the detail screen. Render LAST so it wins over any
            // simultaneously-open dialog (defense-in-depth — well-formed
            // workflow only opens one at a time).
            if let Some(dialog) = state.running_tool_dialog.as_ref() {
                running_tool_dialog::render(frame, area, dialog);
            }
        }
        Screen::Help { previous } => {
            // Render the underlying screen first so the help-close transition
            // is visually instant (no flash of empty terminal).
            let underlay = AppState {
                current_screen: (**previous).clone(),
                ..state.clone()
            };
            match &underlay.current_screen {
                Screen::Main => view_main(&underlay, frame, area),
                Screen::Detail(d) => render_detail(frame, area, d, &underlay),
                Screen::Help { .. } => {
                    // Pathological double-help; render an empty main so the
                    // user can dismiss with `?`/Esc to escape the recursion.
                    view_main(&underlay, frame, area);
                }
            }
            help_overlay::render(frame, area);
        }
    }
}

fn view_main(state: &AppState, frame: &mut Frame<'_>, area: ratatui::layout::Rect) {
    // Vertical split: main panes (Min 1) | summary bar (1 row) | shortcut bar (1 row).
    // Identical rects as `outer_chunks` — the helper exists so the production
    // event loop can compute pane heights without re-rendering.
    let [left, right, summary, bottom] = outer_chunks(area);

    left_pane::render(frame, left, state);
    right_pane::render(frame, right, state);
    summary_bar::render(frame, summary, state);
    bottom_bar::render(frame, bottom, state);

    // Modal dialogs render LAST so they overlay the panes (US-05/US-10). Each
    // dialog reads its `Option` field on `state` — when None, the call is a
    // no-op. The unify dialog (US-10) wins layering over the zap dialog when
    // both are somehow open at once (defense-in-depth; the well-formed
    // workflow only ever opens one at a time).
    if let Some(dialog) = state.zap_dialog.as_ref() {
        zap_dialog::render(frame, area, dialog);
    }
    if let Some(dialog) = state.unify_dialog.as_ref() {
        unify_dialog::render(frame, area, dialog);
    }
    // US-05b single-model delete dialog (ADR-009). Rendered BEFORE
    // `running_tool_dialog` so the running-tool gate wins layering when both
    // are somehow open at once (defense-in-depth — the well-formed workflow
    // only opens one at a time).
    if let Some(dialog) = state.delete_one_dialog.as_ref() {
        delete_one_dialog::render(frame, area, dialog);
    }
    // US-17 running-tool prompt — overlays unify/delete-one and zap-all gates
    // on the main screen. Render LAST so it wins over any simultaneously-open
    // dialog.
    if let Some(dialog) = state.running_tool_dialog.as_ref() {
        running_tool_dialog::render(frame, area, dialog);
    }
}
