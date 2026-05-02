//! Unit tests for `summary_bar::summary_text` — transient `(was X)` annotation
//! driven by `state.summary_delta` (step 05-01 of cross-tool-model-unify).
//!
//! Per AC-U6.5: after a successful unify, the renderer shows the new
//! Dedup-able total followed by `(was <previous>)` for ~5 seconds. The
//! `summary_delta.expires_at` field is an `Instant`; the renderer compares
//! against `Instant::now()` and omits the annotation once expired (the
//! orchestrator separately dispatches `Msg::SummaryDeltaExpired` to clear
//! the field, but the renderer must honour expiry locally so a stale field
//! never produces stale visual output between dispatch and the next paint).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: summary_delta == None → no `(was X)` text
//!     B2: summary_delta Some, not yet expired → `(was <formatted>)` appears
//!     B3: summary_delta Some, expired (expires_at < now) → no `(was X)`
//!     B4: summary_delta Some with previous == 0 → `(was 0 B)` (honest zero,
//!         mirroring AC-U2.5's honest-zero rendering for Dedup-able)
//!   budget = 4 × 2 = 8 unit tests max. We use 4.
//!
//! Each test enters through:
//!   - `render::summary_bar::summary_text` — pure summary fn (driving port).

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use modeltap_core::{DedupSummary, ToolStatus};
use modeltap_tui::app_state::{AppState, HashPoolState, SummaryDelta, ToolView};
use modeltap_tui::render::summary_bar;

fn tool_view(name: &'static str, status: ToolStatus, sizes: &[u64]) -> ToolView {
    ToolView {
        tool: modeltap_core::ToolId(name),
        status,
        model_ids: (0..sizes.len()).map(|i| format!("{name}:m{i}")).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

/// Construct a state with one installed tool and a complete hash pool so
/// the Dedup-able branch renders a formatted size, not "computing...".
fn base_state_with_dedup_able(dedup_able_bytes: u64) -> AppState {
    let mut state = AppState::new_with_default_selection(vec![tool_view(
        "ollama",
        ToolStatus::Ok,
        &[1_000_000_000, 2_000_000_000],
    )]);
    state.hash_state = HashPoolState {
        total: 2,
        completed: 2,
        in_progress: BTreeSet::new(),
        failed: BTreeSet::new(),
        ..HashPoolState::default()
    };
    state.dedup_summary = DedupSummary {
        dedup_able_bytes: Some(dedup_able_bytes),
        ..DedupSummary::default()
    };
    state
}

// B1: summary_delta == None → no `(was X)` annotation in the output
#[test]
fn no_was_annotation_when_summary_delta_is_none() {
    let mut state = base_state_with_dedup_able(500_000_000);
    state.summary_delta = None;

    let text = summary_bar::summary_text(&state);

    assert!(
        !text.contains("(was "),
        "no `(was ...)` expected when summary_delta is None, got: {text}"
    );
    assert!(
        text.contains("Dedup-able: 500.0 MB"),
        "Dedup-able segment should still render normally, got: {text}"
    );
}

// B2: summary_delta Some, not yet expired → `(was <formatted>)` appears
//     immediately after the Dedup-able segment.
#[test]
fn was_annotation_renders_when_delta_not_yet_expired() {
    let mut state = base_state_with_dedup_able(500_000_000);
    state.summary_delta = Some(SummaryDelta {
        previous_dedup_able_bytes: 2_500_000_000,
        expires_at: Instant::now() + Duration::from_secs(5),
    });

    let text = summary_bar::summary_text(&state);

    assert!(
        text.contains("Dedup-able: 500.0 MB (was 2.5 GB)"),
        "expected `Dedup-able: 500.0 MB (was 2.5 GB)` annotation, got: {text}"
    );
}

// B3: summary_delta Some but already expired → annotation omitted. The
//     orchestrator's Msg::SummaryDeltaExpired dispatch will clear the field
//     shortly, but the renderer must not show stale data in the meantime.
#[test]
fn no_was_annotation_when_delta_expired() {
    let mut state = base_state_with_dedup_able(500_000_000);
    state.summary_delta = Some(SummaryDelta {
        previous_dedup_able_bytes: 2_500_000_000,
        // expires_at strictly in the past
        expires_at: Instant::now() - Duration::from_millis(1),
    });

    let text = summary_bar::summary_text(&state);

    assert!(
        !text.contains("(was "),
        "no `(was ...)` expected when delta has already expired, got: {text}"
    );
    assert!(
        text.contains("Dedup-able: 500.0 MB"),
        "Dedup-able segment should still render normally, got: {text}"
    );
}

// B4: previous == 0 → honest zero rendering, mirroring AC-U2.5's
//     honest-zero policy for Dedup-able.
#[test]
fn was_annotation_renders_zero_bytes_for_zero_previous() {
    let mut state = base_state_with_dedup_able(500_000_000);
    state.summary_delta = Some(SummaryDelta {
        previous_dedup_able_bytes: 0,
        expires_at: Instant::now() + Duration::from_secs(5),
    });

    let text = summary_bar::summary_text(&state);

    assert!(
        text.contains("Dedup-able: 500.0 MB (was 0 B)"),
        "expected `(was 0 B)` for previous == 0, got: {text}"
    );
}
