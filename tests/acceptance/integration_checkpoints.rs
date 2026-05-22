//! Cucumber driver for INT-INFO-* integration scenarios — the cross-cutting
//! invariants that span US-21 + US-22.
//!
//! Source feature:
//! `docs/feature/tool-model-info-sqlite-cache/distill/features/integration-checkpoints.feature`
//!
//! Wave: DELIVER step 02-03 part 3/3 — closes the panic-isolation
//! orchestrator boundary shipped in part 2/3 (commit bd2a975). Part 1
//! (commit 12f9559) landed the in-harness `run_inspect_with_panic_isolation`
//! contract under `modeltap-core::tests::inspect`; this driver lifts that
//! contract to the real `modeltap` binary against fixture-populated tempdirs
//! (Strategy B — real I/O per
//! `docs/feature/tool-model-info-sqlite-cache/distill/wave-decisions.md`).
//!
//! Step phrases live in `steps/integration_checkpoints_steps.rs`; the driver
//! here invokes them in scenario order so the test reads like the source
//! `.feature` block.
//!
//! Two scenarios in this driver — one per orchestrator boundary:
//!
//! - INT-INFO-8 / AC-21-9: plugin panic during `inspect_tool` is caught at
//!   the orchestrator boundary
//!   (`plugin_panic_during_inspect_tool_or_inspect_model_is_caught_at_the_orchestrator_boundary`,
//!   landed in step 02-03 part 3/3).
//! - INT-INFO-8 / AC-22-7: plugin panic during `inspect_model` is caught at
//!   the orchestrator boundary
//!   (`plugin_panic_during_inspect_model_is_caught_at_the_orchestrator_boundary`,
//!   landed in step 03-03).
//!
//! Both share the INT-INFO-8 contract — the orchestrators wrap the plugin
//! call in `AssertUnwindSafe(...).catch_unwind()` so the TUI survives a
//! panicking plugin AND a diagnostics line is emitted. The pair exists as
//! two `#[test]` items rather than one parametrized test so the failure mode
//! of either boundary is independently triagable.
//!
//! Route note: the Gherkin text names "Ollama", but this driver routes the
//! panic through the in-process TestTool plugin via
//! `MODELTAP_TEST_TOOL_INSPECT_PANIC=1` (the seam landed in step 02-03 part
//! 1). The orchestrator's `AssertUnwindSafe(...).catch_unwind()` wrap is
//! plugin-agnostic — whichever plugin panics, the same boundary catches it —
//! so the routing swap leaves the AC's intent intact while keeping the
//! production Ollama plugin code untouched.

#[path = "steps/integration_checkpoints_steps.rs"]
mod integration_checkpoints_steps;

use integration_checkpoints_steps::*;

/// INT-INFO-8: plugin panic during inspect_tool or inspect_model is caught at
/// the orchestrator boundary. Drives the full panic-isolation pipeline end
/// to end through the real modeltap binary:
///
/// 1. The fixture sets up a tempdir with the TestTool's seed model file plus
///    a `.modeltap/` diagnostics directory.
/// 2. The launch helper spawns modeltap headless with
///    `MODELTAP_TEST_PLUGINS=test-tool`, `MODELTAP_TEST_TOOL_INSPECT_PANIC=1`,
///    `MODELTAP_DIAGNOSTICS_DIR=<fixture>/.modeltap`, and a scripted
///    `<enter><esc>q` input.
/// 3. The TestTool's `inspect_tool()` panics. The orchestrator catches the
///    panic via `AssertUnwindSafe(...).catch_unwind()`, surfaces
///    `INSPECT_PANIC_SENTINEL` ("(inspection failed -- see diagnostics.log)")
///    in the rendered `last_error` field, and appends
///    `inspect_panic tool=test-tool message=<sanitised>` to
///    `<diagnostics_dir>/diagnostics.log`.
/// 4. The process exits cleanly (no panic-induced abort).
///
/// Assertions verify the sentinel string is visible in the captured stdout
/// frame, the diagnostics.log line is present, and the process exited 0.
#[test]
fn plugin_panic_during_inspect_tool_or_inspect_model_is_caught_at_the_orchestrator_boundary() {
    let fixture = devon_panic_inspect_fixture();
    let result = launch_modeltap_and_navigate_to_test_tool_detail(&fixture);

    assert_no_crash(&result);
    assert_frame_contains(&result, "(inspection failed -- see diagnostics.log)");
    assert_diagnostics_log_contains(&fixture, "inspect_panic tool=test-tool");
    assert_process_alive(&result);
}

/// INT-INFO-8 / AC-22-7: plugin panic during inspect_model is caught at the
/// orchestrator boundary. Sibling to the inspect_tool scenario above; both
/// exercise the SAME catch_unwind contract (US-22 reuses the panic-isolation
/// idiom landed for US-21 in step 02-03) but against the `open_model_detail`
/// orchestrator rather than `open_tool_detail`.
///
/// Pipeline:
///
/// 1. The fixture sets up the same tempdir layout as the inspect_tool
///    scenario (TestTool seed model + .modeltap/ diagnostics dir).
/// 2. The launch helper spawns modeltap headless with
///    `MODELTAP_TEST_PLUGINS=test-tool`,
///    `MODELTAP_TEST_TOOL_INSPECT_MODEL_PANIC=1` (distinct from
///    INSPECT_PANIC — this is the model-side seam), and
///    `MODELTAP_HEADLESS_DETAIL_REGS=...` (a JSON payload that synthesizes
///    a DetailScreenState whose first registration points at the TestTool's
///    seed model). A scripted `<enter><esc>q` opens the detail screen,
///    closes it, then quits.
/// 3. The TestTool's `inspect_model()` panics. The model-detail orchestrator
///    catches via `AssertUnwindSafe(plugin.inspect_model(_)).catch_unwind()`,
///    its `merge()` surfaces `INSPECT_PANIC_SENTINEL` in
///    `metadata_kv["_status"]`, and `write_diagnostics_panic_line` appends
///    `inspect_panic tool=test-tool model=test-model-7b message=<sanitised>`
///    to `<diagnostics_dir>/diagnostics.log`.
/// 4. The process exits 0 (clean shutdown via scripted `q`).
///
/// Note on routing: same Gherkin-Ollama-vs-driver-TestTool decoupling as the
/// inspect_tool scenario. The orchestrator's catch_unwind wrap is
/// plugin-agnostic, so the routing swap does not weaken AC-22-7.
#[test]
fn plugin_panic_during_inspect_model_is_caught_at_the_orchestrator_boundary() {
    let fixture = devon_panic_inspect_fixture();
    let result = launch_modeltap_and_navigate_to_test_tool_model_detail(&fixture);

    assert_no_crash(&result);
    // Same sentinel as the inspect_tool path (intentional: AC-22-7 reuses
    // the AC-21-9 sentinel string — see merge() in open_model_detail.rs).
    assert_frame_contains(&result, "(inspection failed -- see diagnostics.log)");
    // Model-detail orchestrator writes `inspect_panic tool=<id> model=<mid>`
    // — assert on the leading `tool=` field plus the `model=` field so the
    // shape is pinned to the model-detail path (not the tool-detail path).
    assert_diagnostics_log_contains(&fixture, "inspect_panic tool=test-tool model=test-model-7b");
    assert_process_alive(&result);
}
