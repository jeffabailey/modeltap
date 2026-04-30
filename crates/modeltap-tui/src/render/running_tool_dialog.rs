//! Running-tool prompt modal overlay (US-17, intake Q5). Pure render; reads
//! `state.running_tool_dialog` and centers a modal box over the active screen.
//!
//! Per intake Q5 the wording is **detect-and-prompt-then-retry**: the dialog
//! REFUSES the gated action and the user must close the running tool and
//! press `[r]` to retry, or `[Esc]` to cancel. In `LsofUnavailable` mode the
//! dialog explicitly states the safety check was skipped and `[r]` means
//! "proceed anyway".

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::dialogs::running_tool_prompt::{RunningToolDialog, RunningToolMode};

/// Render the running-tool dialog modal centered in `parent_area`. Caller is
/// the top-level `view()`; gates rendering on
/// `state.running_tool_dialog.is_some()`.
pub fn render(frame: &mut Frame<'_>, parent_area: Rect, dialog: &RunningToolDialog) {
    let modal = centered_rect(70, 50, parent_area);
    frame.render_widget(Clear, modal);

    let (title, lines) = match &dialog.mode {
        RunningToolMode::Detected { processes } => {
            (" Running tool detected ", build_detected_lines(processes))
        }
        RunningToolMode::LsofUnavailable => (
            " Running-tool detection unavailable ",
            build_lsof_unavailable_lines(),
        ),
    };

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Build the lines shown in the Detected mode. Per intake Q5 the wording is
/// "<tool> is running and has this file open. Close <tool> and retry."
fn build_detected_lines(
    processes: &[modeltap_core::ports::fs_probe::RunningProcess],
) -> Vec<Line<'static>> {
    let primary_tool = processes
        .first()
        .map(|p| p.tool_name.clone())
        .unwrap_or_else(|| "tool".to_string());
    let mut lines = vec![
        Line::from(format!(
            "{} is running and has this file open.",
            primary_tool
        )),
        Line::from(format!("Close {} and retry.", primary_tool)),
        Line::from(""),
        Line::from("Detected processes:"),
    ];
    for p in processes {
        lines.push(Line::from(format!(
            "  {} (PID {})  -  {}",
            p.tool_name,
            p.pid,
            p.path.display()
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "[r] retry   [Esc] cancel",
        Style::default().add_modifier(Modifier::DIM),
    )));
    lines
}

/// Build the lines shown when lsof is unavailable on this system. The user
/// can press `[r]` to proceed at own risk.
fn build_lsof_unavailable_lines() -> Vec<Line<'static>> {
    vec![
        Line::from("Running-tool detection unavailable on this system."),
        Line::from(""),
        Line::from("modeltap could not check whether any tool is currently"),
        Line::from("holding the in-scope files open (lsof is missing)."),
        Line::from(""),
        Line::from("Proceed at your own risk."),
        Line::from(""),
        Line::from(Span::styled(
            "[r] proceed anyway   [Esc] cancel",
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
