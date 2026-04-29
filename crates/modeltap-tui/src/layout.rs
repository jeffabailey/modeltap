//! View function and terminal-size guard.
//!
//! Per ADR-006 the view is pure: it reads `&AppState` and writes into a
//! ratatui `Frame`. No I/O, no mutation.

use modeltap_core::{MAIN_BOTTOM_BAR, MIN_TERMINAL_COLUMNS};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::event_loop::AppState;

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

/// Render the two-pane scaffold. Step 01-01 only needs the empty layout —
/// step 01-02 fills the right pane with discovered models. The bottom bar
/// is always present (US-08 AC-1) and matches the canonical `MAIN_BOTTOM_BAR`
/// (US-01 AC-6).
pub fn view(state: &AppState, frame: &mut Frame<'_>) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    render_panes(frame, chunks[0]);
    render_bottom_bar(frame, chunks[1]);

    // Reference `state` so the signature is honored; future steps will branch
    // on `state.should_quit` to render farewell screens.
    let _ = state;
}

fn render_panes(frame: &mut Frame<'_>, area: Rect) {
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let left = Paragraph::new("Tools").block(Block::default().borders(Borders::ALL).title("Tools"));
    let right = Paragraph::new("discovering...")
        .block(Block::default().borders(Borders::ALL).title("Models"));

    frame.render_widget(left, split[0]);
    frame.render_widget(right, split[1]);
}

fn render_bottom_bar(frame: &mut Frame<'_>, area: Rect) {
    // Plain text — colors arrive in later steps. The exact text is asserted
    // by the US-01 acceptance scenario "the bottom bar shows ...".
    let line = Line::from(vec![Span::styled(
        MAIN_BOTTOM_BAR,
        Style::default().add_modifier(Modifier::DIM),
    )]);
    frame.render_widget(Paragraph::new(line), area);
}
