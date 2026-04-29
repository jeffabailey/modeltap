//! Unit tests for `modeltap_core::domain::last_action::LastAction`.
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: `for_zap_success` constructor populates verb/target/status/bytes
//!     B2: `for_zap_success` with bytes_retained == 0 omits retained suffix
//!         from `body_with_retain` (US-06.AC-2 schema)
//!     B3: `for_zap_failed` constructor populates Failed status
//!     B4: INT-5 invariant: bytes_reclaimed sums correctly with retain
//!   budget = 4 × 2 = 8 tests max. We use 4.
//!
//! These exercise the pure data type — no I/O, no rendering. The render
//! functions live in `modeltap-tui::render::last_action`.

use modeltap_core::domain::last_action::{ActionStatus, ActionVerb, LastAction};
use modeltap_core::ToolId;

// B1 — for_zap_success populates all fields.
#[test]
fn for_zap_success_populates_verb_target_status_and_bytes() {
    let action = LastAction::for_zap_success(ToolId("ollama"), 14_600_000_000, 6_800_000_000);
    assert_eq!(action.verb, ActionVerb::Zap);
    assert_eq!(action.target, "ollama");
    assert_eq!(action.status, ActionStatus::Success);
    assert_eq!(action.bytes_reclaimed, 14_600_000_000);
    assert_eq!(action.bytes_retained, 6_800_000_000);
}

// B2 — bytes_retained == 0 still produces a valid LastAction; the retained
// suffix is suppressed at the render layer (tested in modeltap-tui crate).
#[test]
fn for_zap_success_with_zero_retained_is_valid() {
    let action = LastAction::for_zap_success(ToolId("ollama"), 12_800_000_000, 0);
    assert_eq!(action.bytes_retained, 0);
    assert_eq!(action.status, ActionStatus::Success);
}

// B3 — for_zap_failed populates Failed status with zero bytes.
#[test]
fn for_zap_failed_uses_failed_status_with_zero_bytes() {
    let action = LastAction::for_zap_failed(ToolId("ollama"));
    assert_eq!(action.verb, ActionVerb::Zap);
    assert_eq!(action.status, ActionStatus::Failed);
    assert_eq!(action.bytes_reclaimed, 0);
    assert_eq!(action.bytes_retained, 0);
}

// B4 — INT-5 property check: a LastAction's bytes_reclaimed is consistent
// across many byte values. This is the algebraic property the integration
// invariant relies on (the actual disk-usage delta check happens in the
// modeltap-app integration test).
#[test]
fn property_bytes_reclaimed_round_trips_for_many_values() {
    // Sample a range of byte sizes including edge values; each must
    // construct a LastAction whose bytes_reclaimed equals the input.
    for &bytes in &[
        0u64,
        1,
        1024,
        1_000_000,
        1_000_000_000,
        14_600_000_000,
        u64::MAX / 2,
    ] {
        let action = LastAction::for_zap_success(ToolId("ollama"), bytes, 0);
        assert_eq!(
            action.bytes_reclaimed, bytes,
            "bytes_reclaimed must round-trip for {}",
            bytes
        );
    }
}
