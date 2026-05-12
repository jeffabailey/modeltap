//! Folder-delete confirmation modal overlay (US-05c.AC-6 / AC-8).
//! Pure render; takes a `FolderConfirmState` and centers a modal box over
//! the parent area. Mirrors the `delete_one_dialog` pattern.
//!
//! Per US-05c.AC-6, the dialog body shows:
//! - folder path (the canonical `<author>/<repo>`)
//! - absolute on-disk path
//! - count of unique files
//! - count of shared files
//! - count of sidecars
//! - Reclaim bytes
//! - Retain bytes
//! - optional running-tool warning slot
//!
//! Plus the typed-input echo + `[Enter]/[Esc]` footer (AC-8, AC-9).
//!
//! Step 01-04 renders the **all-unique** happy-path body only — when
//! `shared_count == 0` the dialog body suppresses the per-tool itemization
//! that mixed shared/unique cases need. The mixed-mode itemization lands at
//! step 03-01.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::dialogs::folder_confirm::FolderConfirmState;
use crate::render::bytes::{format_bytes, format_gb};

/// Render the folder-confirm dialog modal centered in `parent_area`. Caller
/// is the top-level `view()` (wiring lands at step 01-05); gates rendering
/// on `state.folder_confirm_dialog.is_some()`.
pub fn render(frame: &mut Frame<'_>, parent_area: Rect, dialog: &FolderConfirmState) {
    let modal = centered_rect(70, 70, parent_area);
    frame.render_widget(Clear, modal);

    let title = " Delete folder (HF repo) ";
    let lines = build_lines(dialog);

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Build the lines shown in the dialog body. All-unique mode keeps the
/// step-01-04 schema (`Reclaim:` / `Retain:` lines + indented per-bucket
/// counts). Mixed shared/unique mode (step 03-01, when `shared_count > 0`)
/// swaps in the itemised "N unique + M shared + K sidecars" summary line,
/// names each shared file alongside the tools whose hardlinks keep its
/// inode alive, and uses the always-GB `Retained:` line so the user reads
/// reclaim and retain in the same unit.
fn build_lines(dialog: &FolderConfirmState) -> Vec<Line<'static>> {
    let abs_path = dialog.folder.absolute_path.display().to_string();
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(format!("Delete folder '{}'?", dialog.folder.path)),
        Line::from(""),
        Line::from(format!("Folder:      {}", dialog.folder.path)),
        Line::from(format!("On disk:     {}", abs_path)),
    ];

    if dialog.shared_count > 0 {
        // Mixed mode (03-01) — itemised counts, per-shared-file detail,
        // separate Reclaim/Retained lines.
        lines.push(Line::from(format!(
            "Files:       {} unique + {} shared + {} sidecars",
            dialog.unique_count, dialog.shared_count, dialog.sidecar_count
        )));
        for shared in &dialog.shared_models {
            let file_name = shared
                .model
                .on_disk_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| shared.model.id_in_tool.clone());
            let other_tools = shared
                .other_tools
                .iter()
                .map(|t| t.0)
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(Line::from(format!(
                "  {} — also linked in {}",
                file_name, other_tools
            )));
        }
        lines.push(Line::from(format!(
            "Reclaim:     {}",
            format_gb(dialog.bytes_to_reclaim)
        )));
        lines.push(Line::from(format!(
            "Retained:    {}",
            format_gb(dialog.bytes_to_retain)
        )));
    } else {
        // All-unique mode (01-04) — original schema preserved verbatim so
        // the `folder_confirm_dialog_all_unique` snapshot stays green.
        let reclaim = format_bytes(dialog.bytes_to_reclaim);
        let retain = format_bytes(dialog.bytes_to_retain);
        lines.push(Line::from(format!("Files:       {}", dialog.file_count())));
        lines.push(Line::from(format!("  Unique:    {}", dialog.unique_count)));
        lines.push(Line::from(format!("  Shared:    {}", dialog.shared_count)));
        lines.push(Line::from(format!("  Sidecars:  {}", dialog.sidecar_count)));
        lines.push(Line::from(format!("Reclaim:     {}", reclaim)));
        lines.push(Line::from(format!("Retain:      {}", retain)));
    }

    lines.push(Line::from(""));
    if let Some(warning) = dialog.running_tool_warning.as_ref() {
        lines.push(Line::from(Span::styled(
            warning.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(
        "Type the folder path exactly, then [Enter] to confirm.",
    ));
    lines.push(Line::from(format!("> {}_", dialog.typed_input())));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Type the folder path, then [Enter]   [Esc] cancel",
        Style::default().add_modifier(Modifier::DIM),
    )));
    lines
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
