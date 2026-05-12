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

use crate::render::bytes::{format_bytes, format_gb};

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
            // Folder-delete mixed mode (step 03-01): when the banner carries
            // folder-delete file counts AND the action retained bytes, emit
            // SEPARATE `Reclaimed:` and `Retained:` lines using always-GB
            // formatting so the two totals share a unit (AC-16). The
            // optional `extra` string is appended to the Retained line as a
            // parenthetical (e.g. "1 file also linked in ollama").
            // Non-folder-delete actions (zap, unify, delete-one) keep the
            // legacy single-line schema so the US-06 B1 / B2 unit tests
            // remain green.
            let is_folder_delete_with_retain =
                action.folder_delete_files.is_some() && action.bytes_retained > 0;
            if is_folder_delete_with_retain {
                lines.push(format!("Reclaimed: {}", format_gb(action.bytes_reclaimed)));
                let mut retained = format!("Retained: {}", format_gb(action.bytes_retained));
                if let Some(extra) = &action.extra {
                    retained.push_str(" (");
                    retained.push_str(extra);
                    retained.push(')');
                }
                lines.push(retained);
            } else {
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
            // US-05c (folder-group-bulk-delete, step 01-05): append a final
            // file-tally line `N of M files removed` per AC-16 when the
            // banner carries folder-delete file counts.
            if let Some(files) = action.folder_delete_files {
                lines.push(format!(
                    "{} of {} files removed",
                    files.removed, files.total
                ));
            }
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
