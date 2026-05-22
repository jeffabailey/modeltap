//! Step-definition helpers for the INT-INFO-* cross-cutting integration
//! scenarios.
//!
//! Driven by `tests/acceptance/integration_checkpoints.rs`. Each helper is
//! named after the Gherkin phrase it implements so the driver reads
//! scenario-order with no glue. Mirrors `tool_detail_steps.rs` shape —
//! plain `#[test]` driving via `assert_cmd::Command::cargo_bin("modeltap")`
//! against a fixture-populated tempdir; no cucumber-rs runtime.
//!
//! Current scope: INT-INFO-8 (plugin panic during inspect_tool / inspect_model
//! caught at the orchestrator boundary). Future INT-INFO-* scenarios extend
//! this module rather than spawning sibling files so the cross-cutting
//! invariants stay co-located.
//!
//! Plugin route: the harness drives the panic through the in-process TestTool
//! (`MODELTAP_TEST_PLUGINS=test-tool` + `MODELTAP_TEST_TOOL_INSPECT_PANIC=1`).
//! The orchestrator's `AssertUnwindSafe(plugin.inspect_tool()).catch_unwind()`
//! wrap in `crates/modeltap-app/src/orchestration/open_tool_detail.rs` is
//! plugin-agnostic, so this routing decision exercises the SAME boundary the
//! production Ollama/HF/lm-studio plugins would hit.

#![allow(dead_code)] // Helpers may be referenced by future INT-INFO-* scenarios.

use std::time::Duration;

use assert_cmd::Command;
use std::process::Output;

pub use modeltap_acceptance::fixtures::inspect_fixtures::{
    devon_panic_inspect_fixture, InspectFixture,
};

/// Captured outcome of one `modeltap` headless launch. Aggregates the raw
/// `std::process::Output` (so post-hoc inspection of stdout / stderr / exit
/// status survives the launch helper returning) into a single struct the
/// Then-step helpers thread through.
///
/// The struct is intentionally small — just the bytes we need to assert on.
/// We do NOT carry the fixture inside the result so the fixture's TempDir
/// stays owned by the test function and outlives every assertion.
pub struct LaunchResult {
    pub output: Output,
    pub stdout: String,
    pub stderr: String,
}

impl LaunchResult {
    fn from_output(output: Output) -> Self {
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Self { output, stdout, stderr }
    }
}

/// `When Devon opens the test-tool detail screen`
///
/// Spawns one modeltap process headless and scripts an `<enter><esc>q`
/// keystroke sequence so the orchestrator's `Msg::OpenToolDetail(test-tool)`
/// path runs end to end. The fixture's diagnostics directory is wired into
/// `MODELTAP_DIAGNOSTICS_DIR` so the panic-handling code path writes
/// `diagnostics.log` under the tempdir rather than `~/.modeltap`.
///
/// Mirrors `tool_detail_steps::build_command` shape: every real plugin is
/// pinned at a non-existent path so the left pane contains exactly one row
/// (the TestTool's), which means `<enter>` on the first/only row
/// unambiguously opens the test-tool detail screen — no `<up>`/`<down>`
/// navigation is needed.
///
/// Returns a `LaunchResult` carrying the captured stdout (the painted
/// frames), stderr, and `ExitStatus`. The Then-step helpers below substring-
/// match the captured stdout and inspect the on-disk diagnostics.log.
pub fn launch_modeltap_and_navigate_to_test_tool_detail(fixture: &InspectFixture) -> LaunchResult {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "120")
        // Registry seam — register the in-process TestTool plugin from step
        // 01-03. This is the only plugin in the left pane because every real
        // plugin is pinned at a nonexistent path below.
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", fixture.test_tool_root())
        // Panic seam landed in step 02-03 part 1 (commit 12f9559) on the
        // TestTool's `inspect_tool` body.
        .env("MODELTAP_TEST_TOOL_INSPECT_PANIC", "1")
        // Lift the production Enter handler from `Msg::ToggleFolderExpansion`
        // (Main view) to `Msg::OpenToolDetail(selected_tool_id)` so the
        // headless scripted `<enter>` reaches the orchestrator.
        .env("MODELTAP_HEADLESS_TOOL_DETAIL", "1")
        // Diagnostics dir override — landed in step 02-03 part 2/3 (commit
        // bd2a975). Resolved in `crates/modeltap-app/src/main.rs` and
        // threaded through to `OpenToolDetailConfig::diagnostics_dir`. With
        // this override, the orchestrator writes `<temp>/.modeltap/diagnostics.log`
        // instead of `~/.modeltap/diagnostics.log` — the test owns the path.
        .env("MODELTAP_DIAGNOSTICS_DIR", fixture.diagnostics_dir())
        .env("MODELTAP_CACHE_PATH", fixture.cache_path())
        .env("MODELTAP_LOG_DIR", fixture.log_dir())
        // Per-plugin isolation — keep every real plugin at a nonexistent
        // path so they discover-NotInstalled and the TestTool is the only
        // left-pane row.
        .env(
            "MODELTAP_OLLAMA_DIR",
            fixture.ollama_dir.to_string_lossy().into_owned(),
        )
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env(
            "MODELTAP_CONFIG_PATH",
            fixture.config_path.to_string_lossy().into_owned(),
        )
        .env("HF_HOME", "/nonexistent/no-such-hf-cache")
        // Short-circuit Ollama's `inspect_tool` HTTP probe to keep the
        // inspect path deterministic (ADR-016 §D12 / R5). Irrelevant to
        // this scenario's assertions but matches the rest of the suite.
        .env("MODELTAP_OLLAMA_VERSION", "0.6.4")
        .env("MODELTAP_HEADLESS_INPUT", "<enter><esc>q")
        .timeout(Duration::from_secs(30));

    let output = cmd.output().expect("spawn modeltap process");
    LaunchResult::from_output(output)
}

/// `Then the TUI does not crash`
///
/// AC-21-9 / AC-22-7 invariant: the orchestrator's `catch_unwind` wrap
/// converts the plugin panic into a structured `InspectError::PluginPanic`,
/// which the merge layer renders as a sentinel `last_error`. The process
/// must therefore exit with status 0 (the headless harness drives the
/// scripted `q` quit naturally) — a non-zero exit would indicate the panic
/// escaped the orchestrator boundary and unwound the modeltap process
/// itself.
///
/// We assert success here rather than later because a non-zero exit
/// invalidates every other assertion: if the process aborted mid-render the
/// captured stdout frame may be truncated and the diagnostics.log write may
/// not have happened.
pub fn assert_no_crash(result: &LaunchResult) {
    assert!(
        result.output.status.success(),
        "modeltap process must exit 0 (panic caught at orchestrator boundary, \
         clean shutdown); got status={:?}, stderr={}",
        result.output.status,
        result.stderr,
    );
}

/// `Then the detail screen shows "<sentinel>"`
///
/// Substring-greps the captured stdout (the headless harness prints every
/// painted frame). After the `<enter>` the detail screen renders; its
/// `Last error:` field contains `INSPECT_PANIC_SENTINEL`. The frame painted
/// before `<esc>` is the one we assert on, but ANY frame containing the
/// sentinel satisfies the assertion (a stdout substring match is sufficient
/// — the sentinel is unique and only appears when the orchestrator's
/// `merge()` path took the panic-recovery branch).
pub fn assert_frame_contains(result: &LaunchResult, needle: &str) {
    assert!(
        result.stdout.contains(needle),
        "captured frame must contain '{needle}'; got stdout:\n{}\nstderr:\n{}",
        result.stdout,
        result.stderr,
    );
}

/// `Then "<diagnostics_dir>/diagnostics.log" gains a line tagged "<line>"`
///
/// Reads `<fixture.diagnostics_dir()>/diagnostics.log` and asserts the
/// substring `line` appears. The orchestrator's
/// `write_diagnostics_panic_line` helper (in
/// `crates/modeltap-app/src/orchestration/open_tool_detail.rs`) appends a
/// single line of the form
/// `inspect_panic tool=<tool_id> message=<sanitised-payload>` per panic; the
/// caller passes only the leading `inspect_panic tool=test-tool` substring
/// so the assertion is robust against changes to the panic payload format
/// while still pinning the structured tag prefix.
///
/// Missing-file is a hard failure (not best-effort) — the orchestrator's
/// write IS best-effort against I/O errors, but the fixture pre-created the
/// directory, so a missing file means the panic-catch path was not exercised.
pub fn assert_diagnostics_log_contains(fixture: &InspectFixture, line: &str) {
    let log_path = fixture.diagnostics_dir().join("diagnostics.log");
    let raw = std::fs::read_to_string(&log_path).unwrap_or_else(|e| {
        panic!(
            "diagnostics.log must exist at {} after panic-catch path runs; \
             read failed: {e}",
            log_path.display()
        )
    });
    assert!(
        raw.contains(line),
        "diagnostics.log at {} must contain substring '{line}'; got:\n{raw}",
        log_path.display(),
    );
}

/// `Then the process is still alive after the panic`
///
/// At the integration-test boundary "still alive" reduces to "exited
/// cleanly under its own keystroke control" — the scripted `q` token quits
/// the event loop naturally, so `output.status.success()` confirms the
/// process reached the quit handler rather than being torn down by an
/// unwinding panic. (A panic that escaped the orchestrator and unwound the
/// main thread would either abort the process with a non-zero status or
/// hang past the 30-second `assert_cmd` timeout — both surface as
/// `output.status.success() == false` here.)
///
/// This is intentionally a stronger statement than `assert_no_crash`:
/// `assert_no_crash` says "the process did not crash mid-render"; this
/// helper says "the process ran past the panic, painted the post-panic
/// frame, accepted the `<esc>` to dismiss the detail screen, and accepted
/// the `q` to quit". The two share an underlying check (exit status) but
/// the Gherkin distinguishes the two assertions, so the step helper keeps
/// them named distinctly for traceability.
pub fn assert_process_alive(result: &LaunchResult) {
    assert!(
        result.output.status.success(),
        "process must have completed its scripted input run; non-zero exit \
         indicates the panic unwound past the orchestrator boundary. \
         status={:?}, stderr={}",
        result.output.status,
        result.stderr,
    );
}
