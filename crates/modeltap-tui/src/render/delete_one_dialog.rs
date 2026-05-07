//! Single-model delete confirmation modal overlay (US-05b; ADR-009).
//! Pure render; reads `state.delete_one_dialog` and centers a modal box
//! over the active screen.
//!
//! Per `dialogs::delete_one_confirm::DeleteOneMode`:
//! - **Shared** (`was_shared = true`): single-key `[y/n]` confirmation. Title
//!   reads "Delete from one tool"; footer advertises `[y]/[n]/[Esc]`.
//! - **Unique** (`was_shared = false`): typed-id confirmation. Title reads
//!   "Delete (UNIQUE — only copy)"; footer echoes the typed buffer plus
//!   `[Enter]/[Esc]`.
//!
//! Wired into BOTH `layout::view_main` and the Detail branch of `layout::view`,
//! mirroring `running_tool_dialog` placement so the running-tool dialog always
//! wins layering when somehow open simultaneously (defense in depth — the
//! well-formed workflow only opens one at a time).

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::dialogs::delete_one_confirm::DeleteOneConfirmState;
use crate::render::bytes::format_bytes;

/// Render the delete-one dialog modal centered in `parent_area`. Caller is
/// the top-level `view()`; gates rendering on
/// `state.delete_one_dialog.is_some()`.
pub fn render(frame: &mut Frame<'_>, parent_area: Rect, dialog: &DeleteOneConfirmState) {
    let modal = centered_rect(60, 50, parent_area);
    frame.render_widget(Clear, modal);

    let (title, lines) = if dialog.is_shared() {
        (" Delete from one tool ", build_shared_lines(dialog))
    } else {
        (" Delete (UNIQUE — only copy) ", build_unique_lines(dialog))
    };

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Build the lines shown in Shared mode (single-key `[y/n]` confirmation).
/// The model's content is preserved on disk under another tool's tree, so
/// the footer matches the small blast radius.
fn build_shared_lines(dialog: &DeleteOneConfirmState) -> Vec<Line<'static>> {
    let size = format_bytes(dialog.size_bytes);
    vec![
        Line::from(format!(
            "Delete '{}' from {}?",
            dialog.model_id, dialog.tool.0
        )),
        Line::from(""),
        Line::from(format!("Tool:     {}", dialog.tool.0)),
        Line::from(format!("Model:    {}", dialog.model_id)),
        Line::from(format!("Size:     {}", size)),
        Line::from(""),
        Line::from("Other tools still have this model. Only this tool's registration"),
        Line::from("(and, where applicable, this tool's standalone copy) is removed."),
        Line::from(""),
        Line::from(Span::styled(
            "[y] confirm   [n] cancel   [Esc] cancel",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ]
}

/// Build the lines shown in Unique mode (typed-id confirmation). The model's
/// content vanishes from this machine; the footer mirrors US-05's typed-input
/// safety bar with a live echo of the buffer.
fn build_unique_lines(dialog: &DeleteOneConfirmState) -> Vec<Line<'static>> {
    let size = format_bytes(dialog.size_bytes);
    vec![
        Line::from(format!(
            "Delete '{}' from {} (UNIQUE)?",
            dialog.model_id, dialog.tool.0
        )),
        Line::from(""),
        Line::from(format!("Tool:     {}", dialog.tool.0)),
        Line::from(format!("Model:    {}", dialog.model_id)),
        Line::from(format!("Size:     {}", size)),
        Line::from(""),
        Line::from("This is the ONLY copy on this machine. Deletion frees the disk"),
        Line::from("space but the model content is gone."),
        Line::from(""),
        Line::from(format!("> {}_", dialog.typed_input())),
        Line::from(""),
        Line::from(Span::styled(
            "Type the model id, then [Enter]   [Esc] cancel",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ]
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
