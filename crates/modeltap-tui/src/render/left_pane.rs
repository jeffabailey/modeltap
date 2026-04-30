//! Left pane: list of tools with model count, total size, and status.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app_state::{AppState, FocusPane};

/// Render the left pane. The currently-selected tool is shown with a
/// highlighted style. Each row reads:
///
///   <name>   <count>   <size>   <status?>
///
/// Status annotation is shown only when the tool is not installed or in
/// error state.
///
/// Renders only the [left_scroll_offset, left_scroll_offset +
/// left_visible_rows) window so on small terminals or with many plugins the
/// highlighted row stays visible (the `update` path keeps the offset in
/// sync via `compute_scroll_offset`). Today the registry has 4 tools so
/// the window is the full list; this keeps the invariant ready for plugin
/// growth.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let total_tools = state.tools.len();
    let visible = state.left_visible_rows.max(1);
    let start = state.left_scroll_offset.min(total_tools.saturating_sub(1));
    let end = (start + visible).min(total_tools);
    let items: Vec<ListItem<'_>> = if total_tools == 0 {
        Vec::new()
    } else {
        (start..end)
            .map(|idx| {
                let tool = &state.tools[idx];
                let status = match &tool.status {
                    modeltap_core::ToolStatus::Ok => String::new(),
                    modeltap_core::ToolStatus::NotInstalled => " (not installed)".to_string(),
                    modeltap_core::ToolStatus::Error { .. } => " (error)".to_string(),
                };
                let row = format!(
                    "{}  {}  {}{}",
                    tool.tool.0,
                    tool.model_ids.len(),
                    format_size(tool.total_bytes()),
                    status,
                );
                let mut style = Style::default();
                if idx == state.selected_tool {
                    style = style.add_modifier(Modifier::REVERSED);
                    if state.focus == FocusPane::Left {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                }
                ListItem::new(Line::styled(row, style))
            })
            .collect()
    };

    let title = match state.focus {
        FocusPane::Left => "Tools (focused)",
        FocusPane::Right => "Tools",
    };
    let widget = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    frame.render_widget(widget, area);
}

/// Display-formatter for a byte count: GB if >= 1 GB, MB if >= 1 MB, else
/// "<N B". Step 01-03 keeps this minimal; richer formatting (TB / EB,
/// thousands separators) is not needed for the @us-03 scenarios.
fn format_size(bytes: u64) -> String {
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
