//! Top-level view function and terminal-size guard.
//!
//! Per ADR-006 the view is pure: it reads `&AppState` and writes into a
//! ratatui `Frame`. No I/O, no mutation. The actual pane rendering lives
//! in the `render::*` submodules; this module only owns the layout splitting
//! and the terminal-size guard.

use modeltap_core::MIN_TERMINAL_COLUMNS;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use crate::app_state::{AppState, Screen};
use crate::render::{bottom_bar, left_pane, right_pane, summary_bar, zap_dialog};
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
        Screen::Detail(detail) => render_detail(frame, area, detail, state),
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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(chunks[0]);

    left_pane::render(frame, panes[0], state);
    right_pane::render(frame, panes[1], state);
    summary_bar::render(frame, chunks[1], state);
    bottom_bar::render(frame, chunks[2], state);

    // Modal dialogs render LAST so they overlay the panes (US-05). The
    // dialog reads `state.zap_dialog` (Option) — when None, this is a no-op.
    if let Some(dialog) = state.zap_dialog.as_ref() {
        zap_dialog::render(frame, area, dialog);
    }
}
