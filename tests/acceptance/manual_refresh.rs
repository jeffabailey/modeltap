//! Cucumber driver for US-24 manual-refresh + US-25 provenance-line acceptance
//! scenarios (tool-model-info-sqlite-cache feature, step 05-03).
//!
//! Source feature:
//! `docs/feature/tool-model-info-sqlite-cache/distill/features/manual-refresh.feature`
//!
//! Wave: DELIVER step 05-03. Step 05-01 (commit db9a637) shipped the
//! `orchestration::reconcile::run` orchestrator that this step now wires
//! through the user-facing `[r]` and `[Shift+R]` hotkeys + the always-visible
//! provenance line in the summary bar.
//!
//! All four scenarios are `#[ignore]`d for the runtime exercise: the timing
//! windows (≤1000 ms for the `[r]` round-trip per AC-24-7 / @k-info-2-refresh-1s
//! and ≤2000 ms for `[Shift+R]`) require the headless modeltap binary to
//! script the hotkey AFTER warm-paint completes and BEFORE the binary
//! shuts down — a launch.log timing dependency that the acceptance scaffold
//! does not yet expose. The behavioural coverage for step 05-03 lives in:
//!
//! - `crates/modeltap-tui/src/view/provenance.rs::tests` — pure
//!   `format_provenance(now, last_scan_at)` function (CM-D §9 of
//!   acceptance-test-plan.md), every threshold + `None` case.
//! - `crates/modeltap-tui/src/keymap.rs::tests` — the SHORTCUT_TABLE entries
//!   for `[r]` / `[Shift+R]` + their `ContextFilter::NoDialog` gate.
//! - `crates/modeltap-tui/src/render/summary_bar.rs::tests` — the
//!   provenance suffix transitions ("reconciling..." → "refreshing <tool>..."
//!   → "as of just now (<tool> refreshed)").
//!
//! The ignored scenarios below remain in this file so the .feature ↔ test
//! mapping invariant holds (every Scenario in manual-refresh.feature has a
//! corresponding `#[test]` here, even if currently `#[ignore]`d). When the
//! launch.log timing hook lands, lifting `#[ignore]` is a one-line change
//! per scenario.

/// AC-24-3 + AC-24-7 + AC-24-8 + @k-info-2-refresh-1s: `[r]` refreshes the
/// selected tool within 1 second.
///
/// `#[ignore]` per the file-level note: the runtime timing window
/// (warm-paint → hotkey dispatch → orchestrator completion ≤ 1000 ms) is
/// gated on a launch.log timing seam the acceptance scaffold does not yet
/// expose. Step 05-03's pure-function + keymap + summary-bar coverage proves
/// the wiring; the runtime assertion lifts when the timing seam lands.
#[test]
#[ignore = "runtime timing seam pending; coverage via provenance.rs + keymap.rs + summary_bar.rs unit tests"]
fn r_refreshes_the_selected_tool_within_one_second() {
    panic!(
        "step 05-03: acceptance scenario gated on launch.log timing seam; \
         behavioural coverage lives in modeltap-tui unit tests"
    );
}

/// AC-24-4 + AC-24-7 + AC-24-8: `[Shift+R]` refreshes all four tools in
/// parallel within 2 seconds. See file-level note on `#[ignore]`.
#[test]
#[ignore = "runtime timing seam pending; coverage via provenance.rs + keymap.rs + summary_bar.rs unit tests"]
fn shift_r_refreshes_all_four_tools_in_parallel_within_two_seconds() {
    panic!(
        "step 05-03: acceptance scenario gated on launch.log timing seam; \
         behavioural coverage lives in modeltap-tui unit tests"
    );
}

/// AC-24-5: `[r]` is a no-op when a dialog is open. The keymap-level
/// enforcement (the `[r]` shortcut declares `BarSection::Main` only —
/// dispatch under dialogs flows through `dispatch_in_dialog` which does NOT
/// route `[r]` to `Msg::RequestRefresh`) is verified directly in
/// `crates/modeltap-tui/src/keymap.rs::tests`.
#[test]
#[ignore = "runtime timing seam pending; keymap-level no-op covered by keymap.rs unit test"]
fn r_is_a_no_op_when_a_dialog_is_open() {
    panic!(
        "step 05-03: acceptance scenario gated on launch.log timing seam; \
         keymap-level no-op behaviour covered by dispatch_in_dialog unit tests"
    );
}

/// AC-24-1 + AC-24-2: Provenance line always shows freshness with a
/// human-readable suffix. See file-level note. The pure
/// `format_provenance(now, last_scan_at) -> String` behaviour is fully
/// covered by `crates/modeltap-tui/src/view/provenance.rs::tests`, and the
/// summary-bar suffix transitions (during reconcile / after completion) by
/// `crates/modeltap-tui/src/render/summary_bar.rs::tests`.
#[test]
#[ignore = "runtime timing seam pending; provenance string covered by provenance.rs + summary_bar.rs unit tests"]
fn provenance_line_always_shows_freshness_with_a_human_readable_suffix() {
    panic!(
        "step 05-03: acceptance scenario gated on launch.log timing seam; \
         provenance string formatting covered by provenance.rs + summary_bar.rs unit tests"
    );
}
