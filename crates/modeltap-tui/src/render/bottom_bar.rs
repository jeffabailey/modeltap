//! Bottom bar: shortcut line driven by `keymap::SHORTCUT_TABLE`. Per the
//! @walking-skeleton @us-01 scenario, the bottom bar must always display
//! the canonical `MAIN_BOTTOM_BAR` text so the existing acceptance contract
//! holds. Step 01-03 keeps that contract by emitting MAIN_BOTTOM_BAR
//! verbatim; the SHORTCUT_TABLE-driven dynamic bar (US-08) lands in a
//! later step. The `SHORTCUT_TABLE` is still the source of truth for
//! dispatch — only the rendered text is currently a static string for
//! backward-compatibility with the WS acceptance contract.

use modeltap_core::MAIN_BOTTOM_BAR;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn render(frame: &mut Frame<'_>, area: Rect) {
    let line = Line::from(vec![Span::styled(
        MAIN_BOTTOM_BAR,
        Style::default().add_modifier(Modifier::DIM),
    )]);
    frame.render_widget(Paragraph::new(line), area);
}
