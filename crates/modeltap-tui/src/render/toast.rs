//! Pure render fn for the US-U10 partial-success toast.
//!
//! Replaces the v1 non-specific success banner for partial / total-failure
//! unify outcomes. Layout per the master-acceptance @us-u10 schema:
//!
//!   Line 0: header — "Unified <model> into <K> of <N>"
//!   Lines 1..N: per-target outcome lines:
//!     "  OK   <tool>: <bytes saved>"
//!     "  FAIL <tool>: <reason>"
//!   Line -2: "Reclaimed: <total>"
//!   Line -1: "[r] Retry-failed-only   [Enter] Continue"
//!
//! Per ADR-006 the view layer is pure. `view_lines` returns `Vec<String>`
//! so the right-pane layout can position them anywhere; the widget code
//! (paragraph + position) lives in `right_pane`.
//!
//! For Success / AlreadyUnified / Failed (no per-target detail yet) the
//! toast falls back to the v1 single-banner format from `render::last_action`.
//! Only `ActionStatus::Partial` AND total-failure (Failed verb=Unify with
//! recorded per-target failures via the orchestrator) flow through this
//! richer toast.

use modeltap_core::domain::last_action::{ActionStatus, ActionVerb, LastAction, TargetError};
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Format the partial-success toast into header + per-target + footer lines.
///
/// `total_targets` is the total number of targets the unify attempted (the
/// "N" in "K of N"). For partial-success this is `successes + failures.len()`.
/// For total-failure this is `failures.len()` and `successes == 0`.
///
/// Returns at least 4 lines: header + 1 target + reclaim + footer. The
/// caller (right_pane) is responsible for clipping to the available area.
pub fn view_lines(action: &LastAction) -> Vec<String> {
    match &action.status {
        ActionStatus::Partial {
            successes,
            failures,
        } => format_partial(action, *successes, failures),
        ActionStatus::Failed if matches!(action.verb, ActionVerb::Unify) => {
            // Total-failure with no per-target detail (the orchestrator
            // collapsed the outcome to Failed when nothing succeeded). The
            // toast still wants to communicate "0 of N" — but we don't know
            // N here. Render a minimal "Unified <model> into 0 of 0." line
            // and the standard footer; the richer total-failure path goes
            // through Partial { successes: 0, failures } below when the
            // orchestrator preserves the failure list.
            vec![
                format!("Unified {} into 0 of 0.", action.target),
                "Reclaimed: 0 B".to_string(),
                "[Enter] Continue".to_string(),
            ]
        }
        _ => {
            // Non-partial status: defer to last_action's banner format. Toast
            // renderer is a no-op pass-through for Success / AlreadyUnified /
            // non-unify actions.
            crate::render::last_action::view_lines(action)
        }
    }
}

fn format_partial(action: &LastAction, successes: u64, failures: &[TargetError]) -> Vec<String> {
    let total = successes + failures.len() as u64;
    let mut lines = Vec::with_capacity(4 + failures.len() + successes as usize);

    // Header: "Unified <model> into <K> of <N>"
    lines.push(format!(
        "Unified {} into {} of {}.",
        action.target, successes, total,
    ));

    // Per-target lines. We don't have the per-success target names on the
    // banner data type — we render an OK summary line instead so the toast
    // still differentiates OK vs FAIL counts. Once `LastAction` carries
    // per-success detail this can expand to one-line-per-target.
    if successes > 0 {
        lines.push(format!(
            "  OK   {} target{} linked",
            successes,
            if successes == 1 { "" } else { "s" }
        ));
    }
    for f in failures {
        // Render as "  FAIL <basename>: <reason>" — the basename is enough
        // for the user to identify the target, and we avoid leaking the
        // full path (privacy parity with the JSONL events per C5).
        let target_name = std::path::Path::new(&f.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(f.path.as_str());
        lines.push(format!("  FAIL {}: {}", target_name, f.reason));
    }

    // Total reclaim line.
    lines.push(format!(
        "Reclaimed: {}",
        format_size(action.bytes_reclaimed)
    ));

    // Footer — retry-failed-only is offered ONLY when there is at least one
    // failed target to retry; total-failure also offers it.
    if !failures.is_empty() {
        lines.push("[r] Retry-failed-only   [Enter] Continue".to_string());
    } else {
        lines.push("[Enter] Continue".to_string());
    }

    lines
}

/// Render the toast into the given area. Top line = header; subsequent lines
/// = per-target outcomes; last two lines = reclaim + footer. If `area` is too
/// small to fit every line, the bottom (footer) is preserved by truncating
/// the per-target middle.
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

/// Display-formatter for byte counts. Mirrors `render::last_action::format_size`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::domain::last_action::{LastAction, TargetError};

    #[test]
    fn partial_success_header_shows_k_of_n_and_per_target_lines() {
        // 2 successes + 1 failure = "2 of 3".
        let action = LastAction::for_unify_partial(
            "dup/Dup-7B".to_string(),
            8192,
            2,
            vec![TargetError {
                path: "/cache/lm-studio/models/dup/Dup-7B/model.gguf".to_string(),
                reason: "permission-denied".to_string(),
            }],
        );
        let lines = view_lines(&action);
        let joined = lines.join("\n");

        assert!(
            joined.contains("Unified dup/Dup-7B into 2 of 3"),
            "header must read 'Unified <model> into K of N', got:\n{}",
            joined
        );
        assert!(
            joined.contains("OK") && joined.contains("2 target"),
            "OK line must summarise successful target count, got:\n{}",
            joined
        );
        assert!(
            joined.contains("FAIL") && joined.contains("model.gguf"),
            "FAIL line must show the target's basename, got:\n{}",
            joined
        );
        assert!(
            joined.contains("permission-denied"),
            "FAIL line must show the reason, got:\n{}",
            joined
        );
        assert!(
            joined.contains("Reclaimed:"),
            "toast must show total Reclaimed line, got:\n{}",
            joined
        );
        assert!(
            joined.contains("[r]") && joined.contains("Retry-failed-only"),
            "footer must offer [r] Retry-failed-only when failures exist, got:\n{}",
            joined
        );
    }

    #[test]
    fn total_failure_header_shows_zero_of_n_and_no_retry_when_no_successes_but_offers_retry_for_partial(
    ) {
        // Total-failure case via Partial { successes: 0, failures: [..] } — the
        // orchestrator preserves the failures so the toast can show "0 of N"
        // and still offer retry-failed-only (every target is "failed", so
        // retrying retries all of them).
        let action = LastAction::for_unify_partial(
            "dup/Dup-7B".to_string(),
            0,
            0,
            vec![
                TargetError {
                    path: "/a.bin".to_string(),
                    reason: "permission-denied".to_string(),
                },
                TargetError {
                    path: "/b.bin".to_string(),
                    reason: "io-error".to_string(),
                },
            ],
        );
        let lines = view_lines(&action);
        let joined = lines.join("\n");
        assert!(
            joined.contains("Unified dup/Dup-7B into 0 of 2"),
            "total-failure header must read '0 of N', got:\n{}",
            joined
        );
        assert!(
            joined.contains("Reclaimed: 0 B"),
            "total-failure must show Reclaimed: 0 B, got:\n{}",
            joined
        );
        assert!(
            joined.contains("[r]") && joined.contains("Retry-failed-only"),
            "total-failure footer must offer [r] Retry-failed-only (every target failed = retry-able), got:\n{}",
            joined
        );
    }

    #[test]
    fn full_success_falls_back_to_v1_banner_format() {
        // Full success has no per-target detail — defer to the existing
        // `render::last_action::view_lines` banner so we don't double-render.
        let action = LastAction::for_unify_success("dup/Dup-7B".to_string(), 8192, 3);
        let toast_lines = view_lines(&action);
        let banner_lines = crate::render::last_action::view_lines(&action);
        assert_eq!(
            toast_lines, banner_lines,
            "non-partial outcomes must defer to the v1 banner format"
        );
    }
}
