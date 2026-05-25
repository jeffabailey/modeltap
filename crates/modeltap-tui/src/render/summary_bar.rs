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

use std::time::{Instant, SystemTime};

use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app_state::AppState;
use crate::render::bytes::format_bytes;
use crate::view::provenance::format_provenance;

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
///
/// Per AC-U6.5 (step 05-01): when `state.summary_delta` is `Some(delta)` AND
/// `delta.expires_at > Instant::now()`, append `(was <previous>)` immediately
/// after the Dedup-able segment so the user sees the transient delta after
/// a successful unify. The orchestrator separately schedules a
/// `Msg::SummaryDeltaExpired` dispatch ~5s later that clears the field; the
/// renderer also honours expiry locally so a stale field never produces
/// stale visual output between dispatch and the next paint.
pub fn summary_text(state: &AppState) -> String {
    summary_text_at(state, SystemTime::now())
}

/// Test-friendly variant of [`summary_text`] that takes `now` as a parameter.
///
/// Step 05-03 (US-24 AC-24-1 / AC-24-2 / AC-24-7 + US-25): the summary line
/// gains an `" | as of <Z>"` provenance segment and — while
/// `state.reconciling` is non-empty — a `", refreshing <tool>..."` (single)
/// or `", reconciling..."` (multiple) suffix per AC-24-2 / AC-24-7. The
/// production [`render`] call site passes `SystemTime::now()`; tests pass
/// synthetic instants for deterministic assertions on the suffix
/// transitions.
///
/// `state.last_refreshed_tool` (set by `Msg::ReconcileCompleted` and cleared
/// on any nav Msg) appends `" (<tool> refreshed)"` to the provenance suffix
/// per AC-24-7 — surfaces the post-action narrative without occupying the
/// `LastAction` banner slot.
pub fn summary_text_at(state: &AppState, now: SystemTime) -> String {
    let dedup_segment = dedup_able_segment(state);
    let dedup_with_delta = match &state.summary_delta {
        Some(delta) if delta.expires_at > Instant::now() => {
            format!(
                "{dedup_segment} (was {})",
                format_bytes(delta.previous_dedup_able_bytes)
            )
        }
        _ => dedup_segment,
    };
    // AC-U6.4 (step 05-03): the summary bar surfaces the cross-tool unified
    // group count so post-unify state is observable in the same single line
    // the user already reads for Total/Disk/Dedup-able. The value is read
    // from `state.dedup_summary.unified_count` — the same source the
    // `[All Unified]` synthetic-slot badge uses (AC-CONS-2 single source of
    // truth). Pre-hash (None) the segment is omitted to avoid showing a
    // misleading `Unified: 0` while hashing is still in flight.
    let unified_segment = state
        .dedup_summary
        .unified_count
        .map(|n| format!(" | Unified: {}", n));
    let base = format!(
        "Total: {} models | Disk: {} | {}{}",
        total_models(state),
        format_bytes(total_disk_bytes(state)),
        dedup_with_delta,
        unified_segment.unwrap_or_default(),
    );
    let with_refresh_failed = if state.refresh_failed_tools.is_empty() {
        base
    } else {
        format!("{base} (refresh failed)")
    };
    // US-25 provenance: always-visible "as of <freshness>" segment driven by
    // the pure `format_provenance(now, last_scan_at)` helper.
    let provenance = format_provenance(now, state.last_scan_at);
    let with_provenance = format!("{with_refresh_failed} | as of {provenance}");
    // AC-24-7: post-completion "(<tool> refreshed)" annotation. Surfaces the
    // most-recent reconcile target inline with the provenance segment so the
    // user sees both freshness AND which tool just refreshed in one glance.
    let with_refreshed_tool = match &state.last_refreshed_tool {
        Some(tool) => format!("{with_provenance} ({} refreshed)", tool.0),
        None => with_provenance,
    };
    // AC-24-2 / AC-24-7: in-flight reconciling suffix. Single tool ->
    // ", refreshing <tool>..."; multiple -> ", reconciling...". BTreeSet
    // iteration is deterministic for the single-tool case so the suffix is
    // stable under repeated paints.
    match state.reconciling.len() {
        0 => with_refreshed_tool,
        1 => {
            let only = state
                .reconciling
                .iter()
                .next()
                .map(|t| t.0.to_string())
                .unwrap_or_default();
            format!("{with_refreshed_tool}, refreshing {only}...")
        }
        _ => format!("{with_refreshed_tool}, reconciling..."),
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
        Some(n) => format!("Dedup-able: {}", format_bytes(n)),
        None => "Dedup-able: computing...".to_string(),
    }
}

/// Render the summary line into `area`. Single-row paragraph; the caller
/// (`layout::view`) reserves the row above the shortcut bar.
///
/// Step 03-01 (US-U4): when `state.status_line` is `Some(_)`, the transient
/// status-line hint REPLACES the totals line in this slot so the user sees
/// the per-row "u" feedback (per AC-U4.4 / AC-U4.5: "no copies in other
/// tools" / "still computing"). The hint is cleared by any nav Msg via
/// `clear_last_action` in `update.rs`, so the totals line returns as soon as
/// the user moves on.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let text = match &state.status_line {
        Some(hint) => hint.clone(),
        None => summary_text(state),
    };
    let max_w = area.width as usize;
    let trimmed: String = text.chars().take(max_w).collect();
    let row_w = trimmed.chars().count() as u16;
    let row = Rect::new(area.x, area.y, row_w.min(area.width), 1);
    frame.render_widget(Paragraph::new(trimmed), row);
}

#[cfg(test)]
mod tests {
    //! Step 05-03 (US-24 / US-25) suffix-transition coverage. Asserts the pure
    //! `summary_text_at` behaviour across the three reconciling-set states
    //! (empty / one tool / many tools) and the post-completion
    //! `(<tool> refreshed)` annotation. The pure `format_provenance` helper
    //! has its own table-driven tests in `view::provenance::tests`.
    use std::time::{Duration, SystemTime};

    use modeltap_core::ToolId;

    use super::*;
    use crate::app_state::{AppState, ToolView};

    /// Anchor instant so every assertion is deterministic. The exact value
    /// is irrelevant — only `now - last_scan_at` matters.
    fn anchor_now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    fn tool_view(id: &'static str) -> ToolView {
        ToolView {
            tool: ToolId(id),
            status: modeltap_core::ToolStatus::Ok,
            model_ids: vec![],
            model_sizes_bytes: vec![],
        }
    }

    fn state_with_two_tools() -> AppState {
        AppState::new_with_default_selection(vec![tool_view("ollama"), tool_view("hf")])
    }

    #[test]
    fn summary_text_includes_provenance_suffix_when_reconciling_is_empty() {
        let mut state = state_with_two_tools();
        state.last_scan_at = Some(anchor_now()); // "just now"
        let text = summary_text_at(&state, anchor_now());
        assert!(
            text.contains("| as of just now"),
            "missing provenance suffix; got: {text}"
        );
        assert!(
            !text.contains("refreshing"),
            "must NOT show refreshing suffix when reconciling is empty; got: {text}"
        );
        assert!(
            !text.contains("reconciling..."),
            "must NOT show reconciling suffix when set is empty; got: {text}"
        );
    }

    #[test]
    fn summary_text_appends_single_tool_refreshing_suffix() {
        let mut state = state_with_two_tools();
        state.last_scan_at = Some(anchor_now());
        state.reconciling.insert(ToolId("ollama"));
        let text = summary_text_at(&state, anchor_now());
        assert!(
            text.contains(", refreshing ollama..."),
            "expected single-tool suffix; got: {text}"
        );
    }

    #[test]
    fn summary_text_appends_reconciling_plural_for_multiple_tools() {
        let mut state = state_with_two_tools();
        state.last_scan_at = Some(anchor_now());
        state.reconciling.insert(ToolId("ollama"));
        state.reconciling.insert(ToolId("hf"));
        let text = summary_text_at(&state, anchor_now());
        assert!(
            text.contains(", reconciling..."),
            "expected plural suffix; got: {text}"
        );
        assert!(
            !text.contains("refreshing ollama"),
            "must NOT inline tool name in plural case; got: {text}"
        );
    }

    #[test]
    fn summary_text_inlines_last_refreshed_tool_annotation() {
        let mut state = state_with_two_tools();
        state.last_scan_at = Some(anchor_now());
        state.last_refreshed_tool = Some(ToolId("hf"));
        let text = summary_text_at(&state, anchor_now());
        assert!(
            text.contains("just now (hf refreshed)"),
            "expected (<tool> refreshed) annotation after provenance; got: {text}"
        );
    }

    #[test]
    fn summary_text_provenance_renders_never_when_last_scan_at_is_none() {
        let state = state_with_two_tools();
        // `last_scan_at` defaults to None.
        let text = summary_text_at(&state, anchor_now());
        assert!(
            text.contains("| as of never reconciled"),
            "expected never-reconciled provenance; got: {text}"
        );
    }
}
