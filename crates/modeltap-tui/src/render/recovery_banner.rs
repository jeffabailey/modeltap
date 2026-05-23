//! Pure render fn for the cache-recovery banner (US-23 / AC-23-7 / AC-23-11).
//!
//! Painted at row 0 of the main view when `AppState.recovery_reason` is
//! `Some(_)`. Dismissable with `[Esc]` (the keymap dispatches
//! `Msg::DismissRecoveryBanner` which the update handler implements by
//! setting `recovery_reason = None`).
//!
//! AC-23-11 invariant: the banner NEVER blocks the inventory view. The
//! summary bar + left/right panes paint below it via the normal cold-start
//! fallback. If `recovery_reason` is `None`, this renderer is a no-op and
//! consumes zero rows.
//!
//! Per architecture-design.md §7.4, the banner text is:
//!
//!   "Previous cache reset (corrupted or schema mismatch). Renamed to <path>.
//!    Cold-start discovery in progress. See ~/.modeltap/diagnostics.log."
//!
//! The renderer returns the number of rows consumed (0 or 1) so the
//! enclosing layout can adjust downstream pane heights.

use std::path::Path;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app_state::RecoveryReason;

/// Compose the banner message string for the given reason + renamed path.
/// Pure — separated from `render` so unit tests can assert the exact text
/// without a ratatui Frame.
pub fn banner_text(reason: &RecoveryReason, renamed_to: &Path) -> String {
    let cause = match reason {
        RecoveryReason::Corrupted => "corrupted",
        RecoveryReason::Downgrade { .. } => "schema mismatch",
        RecoveryReason::MigrationFailed { .. } => "migration failure",
    };
    format!(
        "Previous cache reset ({cause}). Renamed to {}. \
         Cold-start discovery in progress. See ~/.modeltap/diagnostics.log. \
         [Esc] dismiss",
        renamed_to.display()
    )
}

/// Render the banner into `area` if `recovery` is `Some(_)`. Returns the
/// number of rows the banner consumed (1 when painted, 0 when skipped).
///
/// `area` is the full screen rect — the renderer carves out the top row
/// only. The caller is responsible for shrinking the downstream pane rects
/// by the returned row count.
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    recovery: Option<(&RecoveryReason, &Path)>,
) -> u16 {
    let Some((reason, renamed_to)) = recovery else {
        return 0;
    };
    if area.height == 0 {
        return 0;
    }
    let banner_rect = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    let text = banner_text(reason, renamed_to);
    let style = Style::default()
        .fg(Color::Black)
        .bg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let paragraph = Paragraph::new(text).style(style);
    frame.render_widget(paragraph, banner_rect);
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn banner_text_includes_corrupted_cause_and_renamed_path() {
        let path = PathBuf::from("/home/devon/.local/share/modeltap/cache.sqlite.corrupt-2026-05-22T120000");
        let text = banner_text(&RecoveryReason::Corrupted, &path);
        assert!(text.contains("(corrupted)"), "banner must mention `corrupted` cause: {text}");
        assert!(
            text.contains("cache.sqlite.corrupt-2026-05-22T120000"),
            "banner must contain the renamed path: {text}"
        );
        assert!(
            text.contains("[Esc] dismiss"),
            "banner must advertise the Esc dismissal: {text}"
        );
    }

    #[test]
    fn banner_text_uses_schema_mismatch_for_downgrade() {
        let path = PathBuf::from("/x/cache.sqlite.future-version-99");
        let text = banner_text(
            &RecoveryReason::Downgrade {
                found: 99,
                expected: 1,
            },
            &path,
        );
        assert!(
            text.contains("(schema mismatch)"),
            "downgrade banner must mention `schema mismatch`: {text}"
        );
    }

    #[test]
    fn banner_text_uses_migration_failure_phrasing() {
        let path = PathBuf::from("/x/cache.sqlite.corrupt-2026-05-22T120000");
        let text =
            banner_text(&RecoveryReason::MigrationFailed { from: 0, to: 1 }, &path);
        assert!(
            text.contains("(migration failure)"),
            "migration-failed banner must mention `migration failure`: {text}"
        );
    }
}
