//! Right pane: model list for the currently-selected tool, with a header
//! "Models in <tool> (<count>, <total> GB)" and a scroll-position indicator
//! "<selected+1>/<total>" rendered in the bottom-right corner.

use modeltap_core::domain::{classify_by_presence, other_tools_by_presence, ToolPresence};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app_state::{AppState, FocusPane};
use crate::render::colors::no_color_active;
use crate::render::last_action;
use crate::render::row::render_row_basic;

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let Some(tool) = state.current_tool() else {
        let block = Block::default().borders(Borders::ALL).title("Models");
        frame.render_widget(block, area);
        return;
    };

    let header = format!(
        "Models in {} ({}, {})",
        tool.tool.0,
        tool.model_ids.len(),
        format_size(tool.total_bytes()),
    );

    // Build the cross-tool presence-view once per render (US-04.AC-4).
    // Cheap to construct: clones each tool's id-string list.
    let inventory: Vec<ToolPresence> = state
        .tools
        .iter()
        .map(|t| ToolPresence {
            tool: t.tool,
            model_ids: t.model_ids.clone(),
        })
        .collect();
    let no_color = no_color_active();

    // Visible window for rows.
    let visible = state.visible_rows.max(1);
    let total_rows = tool.model_ids.len();
    let start = state.scroll_offset.min(total_rows.saturating_sub(1));
    let end = (start + visible).min(total_rows);
    let rows = if total_rows == 0 {
        Vec::new()
    } else {
        (start..end)
            .map(|i| {
                let id = &tool.model_ids[i];
                let size = tool.model_sizes_bytes.get(i).copied().unwrap_or(0);
                let indicator = classify_by_presence(id, &inventory);
                let also_in = other_tools_by_presence(id, tool.tool, &inventory);
                let mut line = render_row_basic(id, size, indicator, &also_in, no_color);
                if i == state.selected_row {
                    let mut style = Style::default().add_modifier(Modifier::REVERSED);
                    if state.focus == FocusPane::Right {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    // Apply the selection style on top of any per-span style
                    // (e.g., the colored indicator glyph) so the highlight is
                    // visible without losing the indicator's color cue.
                    line = line.patch_style(style);
                }
                ListItem::new(line)
            })
            .collect::<Vec<_>>()
    };

    let title = if matches!(state.focus, FocusPane::Right) {
        format!("{header} [focused]")
    } else {
        header
    };

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner_area = block.inner(area);
    frame.render_widget(block, area);
    let widget = List::new(rows);
    frame.render_widget(widget, inner_area);

    // Scroll-position indicator in the bottom-right corner. Format:
    // "<selected+1>/<total>". Only rendered when there are rows.
    if total_rows > 0 {
        let label = format!("{}/{}", state.selected_row + 1, total_rows);
        let label_w = label.len() as u16;
        if inner_area.width >= label_w && inner_area.height >= 1 {
            let x = inner_area.x + inner_area.width - label_w;
            let y = inner_area.y + inner_area.height - 1;
            let indicator_area = Rect::new(x, y, label_w, 1);
            frame.render_widget(Paragraph::new(label), indicator_area);
        }
    }

    // US-06 post-action banner: structured 2-line header + body, drawn at
    // the TOP of the inner area so it appears above the model list (per
    // the Step-5 mockup in journey-cleanup-and-unify-visual.md). Pure-
    // structured rendering via `render::last_action`; the right pane only
    // owns the layout slice.
    if let Some(action) = &state.last_action {
        if inner_area.height >= 2 {
            let banner_area = Rect::new(inner_area.x, inner_area.y, inner_area.width, 2);
            last_action::render(frame, banner_area, action);
        }
    }
}

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
