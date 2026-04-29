//! Layered help overlay (US-08). Pure render that groups
//! `keymap::SHORTCUT_TABLE` shortcuts by `BarSection` and draws a centered
//! modal listing every shortcut with its label.
//!
//! `Screen::Help { previous: Box<Screen> }` layers this over any underlying
//! screen so closing returns to the exact prior state. Per ADR-006 the view
//! layer is pure: this module reads inputs and writes ratatui widgets; no
//! I/O, no mutation, no env reads.
//!
//! ## Source of truth
//!
//! All shortcut labels come from `keymap::SHORTCUT_TABLE`. The
//! architecture-lint test in `tests/architecture.rs` enforces that no
//! hardcoded shortcut tokens leak into other render modules.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::keymap::{BarSection, SHORTCUT_TABLE};

/// Render the help overlay centered in `parent_area`. Caller is the
/// top-level `view()`; gates rendering on `Screen::Help`.
pub fn render(frame: &mut Frame<'_>, parent_area: Rect) {
    let modal = centered_rect(60, 60, parent_area);
    frame.render_widget(Clear, modal);

    let title = " Help — All Shortcuts ";
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(modal);
    frame.render_widget(block, modal);

    let lines = render_help_lines();
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Pure: build the body lines for the help overlay grouped by BarSection.
/// Each section is preceded by a header (Main / Detail / Dialogs); shortcuts
/// inside a section are pulled from SHORTCUT_TABLE in declaration order.
pub fn render_help_lines() -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(""));

    push_section(&mut lines, "Main", BarSection::Main);
    push_section(&mut lines, "Detail", BarSection::Detail);
    push_section(&mut lines, "Dialogs", BarSection::Dialog);

    // Closing footer.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press [?] or [Esc] to close",
        Style::default().add_modifier(Modifier::DIM),
    )));

    lines
}

fn push_section(lines: &mut Vec<Line<'static>>, header: &'static str, section: BarSection) {
    lines.push(Line::from(Span::styled(
        header,
        Style::default().add_modifier(Modifier::BOLD),
    )));
    let mut any = false;
    for entry in SHORTCUT_TABLE {
        if !entry.sections.contains(&section) {
            continue;
        }
        any = true;
        lines.push(Line::from(format!("  {}", entry.label)));
    }
    if !any {
        // Section has no shortcuts in SHORTCUT_TABLE yet (e.g., Dialog before
        // 03-04 wires its shortcuts). Show a placeholder line so the section
        // header still appears in the overlay.
        lines.push(Line::from(Span::styled(
            "  (none)",
            Style::default().add_modifier(Modifier::DIM),
        )));
    }
    lines.push(Line::from(""));
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_w = r.width * percent_x / 100;
    let popup_h = r.height * percent_y / 100;
    Rect {
        x: r.x + (r.width.saturating_sub(popup_w)) / 2,
        y: r.y + (r.height.saturating_sub(popup_h)) / 2,
        width: popup_w,
        height: popup_h,
    }
}
