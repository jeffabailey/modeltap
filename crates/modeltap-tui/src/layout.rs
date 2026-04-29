//! Top-level view function and terminal-size guard.
//!
//! Per ADR-006 the view is pure: it reads `&AppState` and writes into a
//! ratatui `Frame`. No I/O, no mutation. The actual pane rendering lives
//! in the `render::*` submodules; this module only owns the layout splitting
//! and the terminal-size guard.

use modeltap_core::MIN_TERMINAL_COLUMNS;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Frame;

use crate::app_state::AppState;
use crate::render::{bottom_bar, left_pane, right_pane, zap_dialog};

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

/// Top-level pure view function. Splits the screen into the two-pane main
/// area + the one-row bottom bar; delegates each pane to its render module.
pub fn view(state: &AppState, frame: &mut Frame<'_>) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(chunks[0]);

    left_pane::render(frame, panes[0], state);
    right_pane::render(frame, panes[1], state);
    bottom_bar::render(frame, chunks[1]);

    // Modal dialogs render LAST so they overlay the panes (US-05). The
    // dialog reads `state.zap_dialog` (Option) — when None, this is a no-op.
    if let Some(dialog) = state.zap_dialog.as_ref() {
        zap_dialog::render(frame, area, dialog);
    }
}
