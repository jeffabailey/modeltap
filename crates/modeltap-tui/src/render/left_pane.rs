//! Left pane: list of slots (real tools + future synthetic entries) with
//! per-slot icon, model count, total size, and status.
//!
//! Per ADR-014, `state.left_pane_slots` is a heterogeneous list:
//! `LeftPaneSlot::Real(ToolView)` rows render with the tool's PNG icon
//! (when available) followed by the row text; `LeftPaneSlot::Synthetic(_)`
//! rows leave the icon column blank but reserve the same width so names
//! stay vertically aligned across rows.
//!
//! # Layout per row
//!
//! ```text
//! [icon 3c][gap 1c][name  count  size  status?]
//! ```
//!
//! Icons render via `render::icons` (terminal graphics protocol when the
//! current terminal supports Kitty/iTerm2/Sixel, otherwise half-block
//! fallback). Tools without a matching asset and all synthetic slots
//! render with the icon column blank — the gap preserves alignment so
//! the eye finds names in a consistent column.
//!
//! The `List` widget that this module previously used is gone: it owned
//! its own item layout and gave us no place to embed an `Image` widget
//! per row. We render the outer `Block` plus per-row text manually via
//! the `Buffer::set_string` API — straightforward because each row is
//! exactly one line.

use modeltap_core::domain::synthetic_slot::{LeftPaneSlot, SyntheticSlot};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};
use ratatui::Frame;

use crate::app_state::{AppState, FocusPane, ToolView};
use crate::render::all_unified;
use crate::render::bytes::format_bytes;
use crate::render::icons;

/// Width in cells of the icon column (matches `icons::ICON_RECT.width`).
/// One extra column of horizontal padding separates icon from text.
const ICON_COL_WIDTH: u16 = 3;
const ICON_GAP: u16 = 1;

/// Render the left pane. The currently-selected slot is shown with a
/// highlighted style. Each Real row reads:
///
///   <icon?>   <name>   <count>   <size>   <status?>
///
/// Status annotation is shown only when the tool is not installed or in
/// error state.
///
/// Renders only the [left_scroll_offset, left_scroll_offset +
/// left_visible_rows) window so on small terminals or with many plugins the
/// highlighted row stays visible (the `update` path keeps the offset in
/// sync via `compute_scroll_offset`).
// @lat: [[tui-icons#Left pane layout]]
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let title = match state.focus {
        FocusPane::Left => "Tools (focused)",
        FocusPane::Right => "Tools",
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    // Render the block first; subsequent per-row writes land inside `inner`.
    block.render(area, frame.buffer_mut());

    let total_slots = state.left_pane_slots.len();
    if total_slots == 0 || inner.height == 0 || inner.width == 0 {
        return;
    }

    let visible = state.left_visible_rows.max(1);
    let start = state.left_scroll_offset.min(total_slots.saturating_sub(1));
    let end = (start + visible).min(total_slots);

    for (row_offset, slot_idx) in (start..end).enumerate() {
        let row_y = inner.y + row_offset as u16;
        if row_y >= inner.y + inner.height {
            break;
        }
        let row_rect = Rect {
            x: inner.x,
            y: row_y,
            width: inner.width,
            height: 1,
        };
        let is_selected = slot_idx == state.selected_tool;
        let row_style = row_style(is_selected, state.focus);

        // Paint the row's text portion with the row style first so the
        // selection highlight covers the full text width even where the
        // text itself is shorter than the row. The icon area is left
        // un-reversed: graphics-protocol output doesn't compose with
        // cell-level inversion, so a clean icon reads better than a
        // partially-inverted bitmap.
        let (icon_rect, text_rect) = split_row(row_rect);
        if is_selected {
            paint_row_background(frame, text_rect, row_style);
        }

        let row_text = match &state.left_pane_slots[slot_idx] {
            LeftPaneSlot::Real(tool) => {
                icons::render_icon(frame, icon_rect, tool.tool.0);
                format_real_row(tool)
            }
            LeftPaneSlot::Synthetic(syn) => format_synthetic_row(syn, state),
        };
        write_row_text(frame, text_rect, &row_text, row_style);
    }
}

/// Selected-row style mirrors the pre-icon List behavior: REVERSED for any
/// selected row, plus BOLD when the left pane has keyboard focus.
fn row_style(is_selected: bool, focus: FocusPane) -> Style {
    if !is_selected {
        return Style::default();
    }
    let mut style = Style::default().add_modifier(Modifier::REVERSED);
    if focus == FocusPane::Left {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// Split a row Rect into `(icon_rect, text_rect)`. The icon column is
/// always [`ICON_COL_WIDTH`] cells wide regardless of whether an icon
/// renders, so names line up vertically across rows.
///
/// On terminals so narrow that the icon + gap exceed the row width, the
/// text rect collapses to width 0 and the icon takes whatever fits — we
/// still avoid an underflow panic. The terminal-size guard in
/// `layout::check_terminal_width` makes this branch unreachable in
/// production; the saturating_sub keeps unit tests safe.
fn split_row(row: Rect) -> (Rect, Rect) {
    let icon_w = ICON_COL_WIDTH.min(row.width);
    let gap = ICON_GAP.min(row.width.saturating_sub(icon_w));
    let icon_rect = Rect {
        x: row.x,
        y: row.y,
        width: icon_w,
        height: row.height,
    };
    let text_rect = Rect {
        x: row.x + icon_w + gap,
        y: row.y,
        width: row.width.saturating_sub(icon_w + gap),
        height: row.height,
    };
    (icon_rect, text_rect)
}

/// Fill the row's text area with spaces under `style`. Required so the
/// selection highlight extends past the end of the row text — without
/// this, selected rows would show inverted text against an
/// un-highlighted background to its right.
fn paint_row_background(frame: &mut Frame<'_>, area: Rect, style: Style) {
    let buf = frame.buffer_mut();
    for x in area.x..area.x + area.width {
        for y in area.y..area.y + area.height {
            buf[(x, y)].set_char(' ').set_style(style);
        }
    }
}

/// Write the row's text into `area` with `style`. ratatui's
/// `Buffer::set_string` truncates at `area.width` for us.
fn write_row_text(frame: &mut Frame<'_>, area: Rect, text: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    frame.buffer_mut().set_string(area.x, area.y, text, style);
}

/// Format the row text for a real `ToolView` slot. The name still leads
/// the text portion (the icon now sits in the dedicated icon column to
/// its left) so screen readers and terminal-copy users still get the
/// tool identity in plain text.
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
        format_bytes(tool.total_bytes()),
        status,
    )
}

/// Format the row for a synthetic left-pane slot. The badge count for
/// `AllUnified` is derived LIVE from `collect_unified_rows(state)` rather
/// than the `count` field on the variant — this side-steps the stale-state
/// failure mode where the slot's stored count diverges from the right-pane
/// footer's count after a hash completes or a unify lands. AC-CONS-2
/// invariant: badge count == footer count == row count, all from one
/// source. When hashing is still in flight (no hashes computed yet),
/// `collect_rows_from_state` returns an empty vec, so we render `(?)` to
/// distinguish "computing" from "definitively zero".
fn format_synthetic_row(syn: &SyntheticSlot, state: &AppState) -> String {
    match syn {
        SyntheticSlot::AllUnified { .. } => {
            let badge = if state.hash_state.is_complete() {
                let count = all_unified::collect_rows_from_state(state).len();
                format!("({count})")
            } else if state.hash_state.total == 0 {
                // Pre-discovery / no jobs queued: render the live count so
                // empty-inventory states still report `(0)` honestly.
                let count = all_unified::collect_rows_from_state(state).len();
                format!("({count})")
            } else {
                "(?)".to_string()
            };
            format!("[All Unified] {}", badge)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_row_reserves_fixed_icon_column() {
        // 30-col row: 3 icon + 1 gap + 26 text. Names line up at column 4.
        let row = Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 1,
        };
        let (icon, text) = split_row(row);
        assert_eq!(icon.width, ICON_COL_WIDTH);
        assert_eq!(icon.x, 0);
        assert_eq!(text.x, ICON_COL_WIDTH + ICON_GAP);
        assert_eq!(text.width, 30 - ICON_COL_WIDTH - ICON_GAP);
    }

    #[test]
    fn split_row_does_not_panic_on_narrow_widths() {
        // Below the layout guard (MIN_TERMINAL_COLUMNS in modeltap-core),
        // production never hits this — but split_row is a pure helper
        // and must not underflow if a future test feeds it a short rect.
        for width in [0u16, 1, 2, 3, 4] {
            let row = Rect {
                x: 0,
                y: 0,
                width,
                height: 1,
            };
            let (_icon, _text) = split_row(row); // must not panic
        }
    }

    #[test]
    fn row_style_is_default_when_not_selected() {
        let style = row_style(false, FocusPane::Left);
        assert_eq!(style, Style::default());
    }

    #[test]
    fn row_style_is_reversed_when_selected_unfocused() {
        let style = row_style(true, FocusPane::Right);
        // Focused right pane → selected left row reverses but does not
        // bold (matches pre-icon-refactor behavior).
        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn row_style_is_reversed_and_bold_when_selected_and_left_focused() {
        let style = row_style(true, FocusPane::Left);
        assert!(style.add_modifier.contains(Modifier::REVERSED));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }
}
