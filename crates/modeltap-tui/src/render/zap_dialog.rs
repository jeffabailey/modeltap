//! Zap-confirmation modal overlay (US-05). Pure render; reads
//! `state.zap_dialog` and centers a modal box over the main area.
//!
//! Empty-tool branch (per AC-5) shows "Nothing to zap." with `[Esc] close`.
//! The destructive branch shows the metrics + a typed-input prompt.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::dialogs::zap_confirm::ZapConfirmState;

/// Render the zap dialog modal centered in `parent_area`. Caller is the
/// top-level `view()`; gates rendering on `state.zap_dialog.is_some()`.
pub fn render(frame: &mut Frame<'_>, parent_area: Rect, dialog: &ZapConfirmState) {
    let modal = centered_rect(60, 50, parent_area);
    frame.render_widget(Clear, modal);

    let lines = if dialog.is_empty_tool() {
        vec![
            Line::from(format!("Zap {} — nothing to zap.", dialog.tool.0)),
            Line::from(""),
            Line::from("This tool has no models registered. Nothing will be removed."),
            Line::from(""),
            Line::from(Span::styled(
                "[Esc] close",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ]
    } else {
        let total = format_bytes(dialog.total_bytes);
        let unique = format_bytes(dialog.unique_bytes);
        let shared = format_bytes(dialog.shared_bytes);
        vec![
            Line::from(format!(
                "Zap {}: remove ALL {} models?",
                dialog.tool.0, dialog.model_count
            )),
            Line::from(""),
            Line::from(format!("Total apparent size: {total}")),
            Line::from(format!(
                "Unique to {}: {} models, {} (will be freed)",
                dialog.tool.0, dialog.unique_count, unique
            )),
            Line::from(format!(
                "Shared with other tools: {} models, {} (registration only)",
                dialog.shared_count, shared
            )),
            Line::from(""),
            Line::from(format!(
                "Type the tool name exactly to confirm: {}_",
                dialog.typed_input()
            )),
            Line::from(""),
            Line::from(Span::styled(
                "[Enter] confirm   [Esc] cancel   [Backspace] delete",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ]
    };

    let title = if dialog.is_empty_tool() {
        " Zap Tool "
    } else {
        " Confirm Zap "
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
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

fn format_bytes(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const MB: u64 = 1_000_000;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} B", bytes)
    }
}
