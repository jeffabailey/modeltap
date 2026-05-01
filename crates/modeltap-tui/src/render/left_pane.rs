//! Left pane: list of slots (real tools + future synthetic entries) with
//! per-slot model count, total size, and status.
//!
//! Per ADR-014, `state.left_pane_slots` is a heterogeneous list:
//! `LeftPaneSlot::Real(ToolView)` rows render the existing tool row; the
//! `LeftPaneSlot::Synthetic(_)` arm renders a placeholder that the future
//! step 04-02 will replace with the `[All Unified]` row. For step 01-03 the
//! synthetic slot is NOT yet appended, so the placeholder code path is dead
//! until 04-02 wires the synthesis.

use modeltap_core::domain::synthetic_slot::{LeftPaneSlot, SyntheticSlot};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

use crate::app_state::{AppState, FocusPane, ToolView};

/// Render the left pane. The currently-selected slot is shown with a
/// highlighted style. Each Real row reads:
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
    let total_slots = state.left_pane_slots.len();
    let visible = state.left_visible_rows.max(1);
    let start = state.left_scroll_offset.min(total_slots.saturating_sub(1));
    let end = (start + visible).min(total_slots);
    let items: Vec<ListItem<'_>> = if total_slots == 0 {
        Vec::new()
    } else {
        (start..end)
            .map(|idx| {
                let row_text = match &state.left_pane_slots[idx] {
                    LeftPaneSlot::Real(tool) => format_real_row(tool),
                    LeftPaneSlot::Synthetic(syn) => format_synthetic_row(syn),
                };
                let mut style = Style::default();
                if idx == state.selected_tool {
                    style = style.add_modifier(Modifier::REVERSED);
                    if state.focus == FocusPane::Left {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                }
                ListItem::new(Line::styled(row_text, style))
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

/// Format the row text for a real `ToolView` slot. Pre-existing logic, lifted
/// out so the slot-dispatch in `render` stays a single match arm.
fn format_real_row(tool: &ToolView) -> String {
    let status = match &tool.status {
        modeltap_core::ToolStatus::Ok => String::new(),
        modeltap_core::ToolStatus::NotInstalled => " (not installed)".to_string(),
        modeltap_core::ToolStatus::Error { .. } => " (error)".to_string(),
    };
    format!(
        "{}  {}  {}{}",
        tool.tool.0,
        tool.model_ids.len(),
        format_size(tool.total_bytes()),
        status,
    )
}

/// Format a placeholder row for the synthetic slot. Step 01-03 leaves this as
/// a dead code path (the synthetic slot is not yet appended to
/// `left_pane_slots`); step 04-02 lands the real `[All Unified]` rendering
/// (count + saved-bytes badge). Until then this is a defensive placeholder so
/// a future caller that injects a synthetic slot does not panic on render.
fn format_synthetic_row(syn: &SyntheticSlot) -> String {
    match syn {
        SyntheticSlot::AllUnified { count, .. } => {
            let badge = match count {
                Some(n) => format!("({n})"),
                None => "(?)".to_string(),
            };
            format!("[All Unified] {}", badge)
        }
    }
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
