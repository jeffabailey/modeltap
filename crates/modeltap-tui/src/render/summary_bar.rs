//! Pure render fn for the bottom-summary line.
//!
//! Per the journey mockup (`docs/feature/modeltap-tui/discuss/journey-cleanup-and-unify-visual.md`
//! Step 1):
//!   "Total: <N> models | Disk: <X> GB | Dedup-able: <Y> GB"
//!
//! Dedup-able is read from `state.dedup_summary.dedup_able_bytes` — the
//! single source of truth (NFR-5) populated by the classifier in
//! `logic::dedup::dedup_summary` on hash msgs. Branches:
//!   - `state.hash_state.is_hashing()` → "Dedup-able: computing..."
//!   - `dedup_summary.dedup_able_bytes == None` (default / pre-paint) →
//!     also "Dedup-able: computing..." (safe pre-paint default)
//!   - `Some(n)` → "Dedup-able: <formatted>" via `format_size`
//!
//! Total/Disk aggregate the real tool slots in `AppState.left_pane_slots`
//! (via `real_tools_iter()`) — every installed tool's `model_ids.len()` and
//! `total_bytes()` — and are rendered above the bottom shortcut bar.
//! Synthetic slots (when present) are skipped: their contribution is already
//! counted on the underlying real tools whose contents the synthesis aggregates.

use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app_state::AppState;

/// Aggregate the total bytes across all installed tools, deduplicating
/// nothing in the WS slice (cross-tool dedup classifier is 03-01 work).
pub fn total_disk_bytes(state: &AppState) -> u64 {
    state.real_tools_iter().map(|t| t.total_bytes()).sum()
}

/// Aggregate the total model count across all installed tools.
pub fn total_models(state: &AppState) -> u64 {
    state
        .real_tools_iter()
        .map(|t| t.model_ids.len() as u64)
        .sum()
}

/// Format the summary line. Pure string; rendered by `summary_bar::render`.
///
/// Per US-11.AC-2: when `state.refresh_failed_tools` is non-empty, append a
/// `(refresh failed)` indicator so the user sees that the displayed totals
/// are stale and can press `[r]` to retry.
///
/// Per AC-U2.1/U2.2/U2.3/U2.5/NFR-5 (step 01-04): the Dedup-able segment is
/// driven by `state.hash_state` + `state.dedup_summary.dedup_able_bytes`,
/// NOT a separate computation.
pub fn summary_text(state: &AppState) -> String {
    let base = format!(
        "Total: {} models | Disk: {} | {}",
        total_models(state),
        format_size(total_disk_bytes(state)),
        dedup_able_segment(state),
    );
    if state.refresh_failed_tools.is_empty() {
        base
    } else {
        format!("{base} (refresh failed)")
    }
}

/// Render the `Dedup-able: ...` segment. Pure helper, no side effects.
///
/// Branches per AC-U2.x:
///   - hashing in flight → "computing..."
///   - `dedup_summary.dedup_able_bytes` is `None` (default / pre-paint) →
///     "computing..." (safe pre-paint default per implementation note)
///   - `Some(n)` (incl. honest zero) → formatted `format_size(n)`
fn dedup_able_segment(state: &AppState) -> String {
    if state.hash_state.is_hashing() {
        return "Dedup-able: computing...".to_string();
    }
    match state.dedup_summary.dedup_able_bytes {
        Some(n) => format!("Dedup-able: {}", format_size(n)),
        None => "Dedup-able: computing...".to_string(),
    }
}

/// Render the summary line into `area`. Single-row paragraph; the caller
/// (`layout::view`) reserves the row above the shortcut bar.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let text = summary_text(state);
    let max_w = area.width as usize;
    let trimmed: String = text.chars().take(max_w).collect();
    let row_w = trimmed.chars().count() as u16;
    let row = Rect::new(area.x, area.y, row_w.min(area.width), 1);
    frame.render_widget(Paragraph::new(trimmed), row);
}

/// Display-formatter for byte counts. Identical formatting to the right-pane
/// renderer; kept inline to avoid a cross-module dep.
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
