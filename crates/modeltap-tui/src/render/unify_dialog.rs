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

/// Build the lines shown in the destructive Confirm path. Per US-U5 the
/// modal renders:
///   - the model name + first 8 chars of the dedup key (canonical's hash
///     proxy: device:inode is shown as the stable identifier when the
///     content hash is not on the plan itself).
///   - the canonical tool's full path.
///   - per-target rows with `[x]`/`[ ]` checkbox + tool + path + per-row
///     "X B → 0 B (saves X B)" annotation, with the cursored row prefixed
///     by `>` and `selected_target_idx`'s row reverse-styled.
///   - a **bold** "Total reclaim:" line that recomputes live from
///     `dialog.total_reclaim_bytes()`.
///   - an action footer "[Enter] Apply  [space] Toggle  [Esc] Cancel".
fn build_confirm_lines(dialog: &UnifyDialogState) -> Vec<Line<'static>> {
    let plan = &dialog.plan;
    let canonical_path = plan.canonical.path.display().to_string();
    let canonical_tool = plan.canonical.tool.0.to_string();
    let canonical_size = plan.canonical.size_bytes;
    // The plan does not carry a content hash; surface a stable per-dialog
    // identifier from the canonical's (device, inode) so the user can
    // distinguish dialog instances. Truncated to 8 chars per US-U5 spec.
    let dedup_prefix: String = format!("{:016x}", plan.canonical.inode)
        .chars()
        .take(8)
        .collect();

    let mut lines = vec![
        Line::from(format!(
            "Model: {} (#{})",
            canonical_path
                .rsplit('/')
                .next()
                .unwrap_or(canonical_path.as_str()),
            dedup_prefix
        )),
        Line::from(""),
        Line::from(format!(
            "Canonical ({}): {}",
            canonical_tool, canonical_path
        )),
        Line::from(""),
        Line::from("Targets to hardlink:"),
    ];

    for (idx, link) in plan.links.iter().enumerate() {
        let active = dialog.target_active.get(idx).copied().unwrap_or(true);
        let cursor = if idx == dialog.selected_target_idx {
            "> "
        } else {
            "  "
        };
        let checkbox = if active { "[x]" } else { "[ ]" };
        let suffix = if link.already_linked {
            "  (already linked, no-op)".to_string()
        } else if link.cross_filesystem {
            "  (cross-filesystem — will fail on this step)".to_string()
        } else if active {
            format!(
                "  {} → 0 B (saves {})",
                format_bytes(canonical_size),
                format_bytes(canonical_size)
            )
        } else {
            "  (skipped)".to_string()
        };
        let row = format!(
            "{cursor}{checkbox} {tool}: {path}{suffix}",
            cursor = cursor,
            checkbox = checkbox,
            tool = link.tool.0,
            path = link.target.display(),
            suffix = suffix
        );
        if idx == dialog.selected_target_idx {
            lines.push(Line::from(Span::styled(
                row,
                Style::default().add_modifier(Modifier::REVERSED),
            )));
        } else {
            lines.push(Line::from(row));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "Total reclaim: {}",
            format_bytes(dialog.total_reclaim_bytes())
        ),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[Enter] Apply  [space] Toggle  [Esc] Cancel",
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
