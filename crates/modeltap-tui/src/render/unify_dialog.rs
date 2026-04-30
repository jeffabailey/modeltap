//! Unify-confirmation modal overlay (US-10). Pure render; reads
//! `state.unify_dialog` and centers a modal box over the main area.
//!
//! AlreadyUnified branch (per AC-5) shows a benign informational message;
//! the destructive branch shows the canonical, the link list, and the disk
//! reclaim BEFORE any action is taken.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::dialogs::unify_confirm::{UnifyDialogState, UnifyMode};
use crate::render::bytes::format_bytes;

/// Render the unify dialog modal centered in `parent_area`. Caller is the
/// top-level `view()`; gates rendering on `state.unify_dialog.is_some()`.
pub fn render(frame: &mut Frame<'_>, parent_area: Rect, dialog: &UnifyDialogState) {
    let modal = centered_rect(70, 60, parent_area);
    frame.render_widget(Clear, modal);

    let lines = match &dialog.mode {
        UnifyMode::AlreadyUnified => build_already_unified_lines(dialog),
        UnifyMode::Confirm => build_confirm_lines(dialog),
        UnifyMode::DryRunPreview { lines } => build_dry_run_preview_lines(lines),
    };

    let title = match &dialog.mode {
        UnifyMode::AlreadyUnified => " Unify (already unified) ",
        UnifyMode::Confirm => " Confirm Unify ",
        UnifyMode::DryRunPreview { .. } => " Unify (dry-run preview) ",
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Build the lines shown in the destructive Confirm path. Per US-10.AC-1 the
/// modal MUST display the chosen canonical, the per-tool target list, and
/// the disk reclaim before the user confirms.
fn build_confirm_lines(dialog: &UnifyDialogState) -> Vec<Line<'static>> {
    let plan = &dialog.plan;
    let canonical_path = plan.canonical.path.display().to_string();
    let canonical_tool = plan.canonical.tool.0.to_string();
    let reclaim = format_bytes(plan.bytes_reclaimed_estimate);

    let mut lines = vec![
        Line::from(format!(
            "Unify: hardlink {} target(s) to {}'s canonical copy?",
            plan.links.len(),
            canonical_tool
        )),
        Line::from(""),
        Line::from(format!("Canonical:  {}", canonical_path)),
        Line::from(format!("Reclaim:    {} (after unify)", reclaim)),
        Line::from(""),
        Line::from("Targets to hardlink:"),
    ];

    for link in &plan.links {
        let suffix = if link.already_linked {
            "  (already linked, no-op)"
        } else if link.cross_filesystem {
            "  (cross-filesystem — will fail on this step)"
        } else {
            ""
        };
        lines.push(Line::from(format!(
            "  {}: {}{}",
            link.tool.0,
            link.target.display(),
            suffix
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[Enter] confirm   [n] dry-run   [Esc] cancel",
        Style::default().add_modifier(Modifier::DIM),
    )));
    lines
}

/// Build the lines for the US-14 DryRunPreview mode. Renders the
/// pre-formatted "(dry-run) Would..." lines from the `DryRunOutcome` plus
/// a footer hint. The lines are produced by `actions::unify::dry_run` and
/// carried in the dialog state by `Msg::UnifyDryRunCompleted(lines)`.
fn build_dry_run_preview_lines(preview_lines: &[String]) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = preview_lines
        .iter()
        .map(|s| Line::from(s.clone()))
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[Enter] proceed   [Esc] return to confirm",
        Style::default().add_modifier(Modifier::DIM),
    )));
    lines
}

/// Build the lines shown in the benign AlreadyUnified path (US-10.AC-5). The
/// model has multiple registrations but they all share an inode already, so
/// nothing is to be done.
fn build_already_unified_lines(dialog: &UnifyDialogState) -> Vec<Line<'static>> {
    let plan = &dialog.plan;
    let canonical_path = plan.canonical.path.display().to_string();
    let mut lines = vec![
        Line::from("All registrations already share an inode — already unified."),
        Line::from(""),
        Line::from(format!("Canonical: {}", canonical_path)),
        Line::from(format!(
            "{} hardlink(s) currently point at this inode.",
            plan.links.len()
        )),
        Line::from(""),
        Line::from("No action required."),
        Line::from(""),
        Line::from(Span::styled(
            "[Esc] close",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ];
    // Avoid `unused` warnings if there are zero links (defensive).
    if plan.links.is_empty() {
        lines.push(Line::from("(0 hardlinks recorded)"));
    }
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
