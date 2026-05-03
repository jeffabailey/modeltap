//! Pure render fn for the US-06 post-action banner.
//!
//! Two-line layout per the master-acceptance @us-06 schema:
//!   Line 0: "Last action: <verb> <target> (<status>)"
//!   Line 1: "Reclaimed: <N> GB" + optional retain or extra suffix.
//!
//! Per ADR-006 the view layer is pure. `view_lines` returns `Vec<String>`
//! so the right-pane renderer can lay them out wherever the design calls
//! for. The widget code (paragraph + position) lives in `right_pane`.

use modeltap_core::domain::last_action::{ActionStatus, LastAction};
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::render::bytes::format_bytes;

/// Format the post-action banner into header + body lines per the US-06
/// master-acceptance schema. Returns at least one line (header); a second
/// "Reclaimed: ..." body line is included for Success and Partial outcomes.
pub fn view_lines(action: &LastAction) -> Vec<String> {
    let header = format!(
        "Last action: {} {} ({})",
        action.verb.as_str(),
        action.target,
        action.status.header_label(),
    );
    let mut lines = vec![header];

    match &action.status {
        ActionStatus::Failed => {
            // Failure has no body — header is sufficient. Push a blank
            // body line so the right-pane layout can reserve two rows
            // consistently.
            lines.push(String::new());
        }
        _ => {
            let mut body = format!("Reclaimed: {}", format_bytes(action.bytes_reclaimed));
            if let Some(extra) = &action.extra {
                body.push_str(" (");
                body.push_str(extra);
                body.push(')');
            } else if action.bytes_retained > 0 {
                body.push_str(&format!(
                    " ({} retained — also linked from other tools)",
                    format_bytes(action.bytes_retained)
                ));
            }
            lines.push(body);
        }
    }
    lines
}

/// Render the post-action banner into the given area. Top line = header,
/// next line = body. If `area` is too small to fit both lines, the body is
/// elided. Pure widget code; called by `right_pane::render` when
/// `state.last_action.is_some()`.
pub fn render(frame: &mut Frame<'_>, area: Rect, action: &LastAction) {
    let lines = view_lines(action);
    if area.height == 0 || area.width == 0 {
        return;
    }
    for (i, line) in lines.iter().enumerate() {
        if (i as u16) >= area.height {
            break;
        }
        let max_w = area.width as usize;
        let trimmed: String = line.chars().take(max_w).collect();
        let row_w = trimmed.chars().count() as u16;
        let row_w = row_w.min(area.width);
        if row_w == 0 {
            continue;
        }
        let row = Rect::new(area.x, area.y + i as u16, row_w, 1);
        frame.render_widget(Paragraph::new(trimmed), row);
    }
}
