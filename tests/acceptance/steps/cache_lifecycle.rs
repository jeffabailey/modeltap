//! Cache lifecycle step-definitions for the `tool-model-info-sqlite-cache`
//! walking-skeleton acceptance scenario.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/step-definitions-skeleton.md`
//! §A + §B, this module provides the Given/When/Then phrase implementations
//! the M1 scenario consumes. The project does NOT use cucumber-rs — every
//! existing acceptance test in the workspace is a plain `#[test]` function
//! that drives the `modeltap` binary through `assert_cmd::Command::cargo_bin`
//! against a `tempfile::TempDir` fixture. This module mirrors that pattern:
//! the step phrases are exposed as ordinary Rust functions named after the
//! Gherkin phrase, the walking-skeleton test driver
//! (`tests/acceptance/cache_walking_skeleton.rs`) calls them in scenario
//! order, and each function asserts the same conditions the Gherkin phrase
//! documents in the .feature file.
//!
//! Future scenarios in `cache-state-model.feature` / `manual-refresh.feature`
//! /etc. (phases 02+) will reuse the helpers exported here; the M1 slice
//! covers only the phrases the walking skeleton itself exercises.

#![allow(dead_code)] // Step phrases land incrementally; many will be unused
                     // until later phases pick them up. The allow keeps the
                     // module compile-warning-free during phase 01.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use assert_cmd::Command;
use modeltap_acceptance::fixtures::cache_fixtures::{
    test_tool_id_str, CacheVerifier, DevonCacheEmptyFixture,
};
use modeltap_acceptance::test_tool::{TEST_METADATA_KIND_VALUE, TEST_MODEL_FILENAME};
use serde_json::Value;
use tempfile::TempDir;

/// The maximum wall-clock the walking skeleton allows for process B's
/// warm-paint event. Per acceptance-test-plan.md §4 step 8 + AC-6 of the
/// step's roadmap entry: ≤ 150 ms (debug-build envelope; release-build
/// budget is 100 ms per K-INFO-1).
pub const WARM_PAINT_BUDGET_MS: u64 = 150;

/// Mutable scenario state carried across step phrases. The walking-skeleton
/// driver constructs one of these at the top of the test and threads it
/// through each step call. Equivalent to the cucumber-rs `World` type, but
/// without the macro machinery — pure structs + functions.
pub struct WalkingSkeletonWorld {
    pub fixture: DevonCacheEmptyFixture,
    /// Captured stdout of the most recent `modeltap` invocation. Process A
    /// writes here, then process B overwrites; the WS driver inspects this
    /// after process B exits to assert "right-pane contains the model name".
    pub last_stdout: Option<String>,
    /// Process B's exit status code so the WS driver can assert success.
    pub process_b_exit_code: Option<i32>,
}

impl WalkingSkeletonWorld {
    pub fn new() -> Self {
        Self {
            fixture: DevonCacheEmptyFixture::build(),
            last_stdout: None,
            process_b_exit_code: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Given steps (§A.Given / §B)
// ---------------------------------------------------------------------------

/// `Given the cache file does not exist`
///
/// Per §A: asserts `!world.cache_path.exists()` and that the parent
/// directory exists (or creates it). Phase-01 fixture already builds the
/// parent dir in `DevonCacheEmptyFixture::build`, so this step is the
/// boundary check.
pub fn given_the_cache_file_does_not_exist(world: &WalkingSkeletonWorld) {
    let path = world.fixture.cache_path();
    assert!(
        !path.exists(),
        "precondition violated: cache.sqlite already exists at {}",
        path.display()
    );
    assert!(
        path.parent().expect("cache_path has a parent").exists(),
        "cache_path's parent directory must exist (xdg-data/modeltap/)"
    );
}

/// `Given the in-process TestTool plugin is registered`
///
/// Per §B: documents that the scenario sets `MODELTAP_TEST_PLUGINS=test-tool`
/// on the modeltap process. The actual env var is set by the When step
/// (`modeltap_command_with_test_tool`) so this Given is a documentation-
/// shaped no-op asserting the fixture's TestTool model file is present (the
/// precondition for `MODELTAP_TEST_PLUGINS=test-tool` to discover anything).
pub fn given_the_in_process_test_tool_plugin_is_registered(world: &WalkingSkeletonWorld) {
    let model_path = world.fixture.test_tool_root().join(TEST_MODEL_FILENAME);
    assert!(
        model_path.exists(),
        "TestTool's seed model must exist at {} before the binary launches",
        model_path.display()
    );
}

/// `Given the TestTool will discover one model "<name>" at "<path>"`
///
/// Per §B: creates a sparse file at `<path>` relative to `world.temp_dir`.
/// The phase-01 fixture pre-writes the file in `build()`, so this step
/// asserts the documented expectations.
pub fn given_the_test_tool_will_discover_one_model_at(
    world: &WalkingSkeletonWorld,
    expected_name: &str,
    expected_path_relative_to_fixture: &Path,
) {
    let abs = world
        .fixture
        .temp
        .path()
        .join(expected_path_relative_to_fixture);
    assert!(
        abs.exists(),
        "TestTool model file must exist at {}",
        abs.display()
    );
    assert_eq!(
        TEST_MODEL_FILENAME, "test-model-7b.gguf",
        "TestTool's filename constant drift would break the WS scenario"
    );
    // The scenario's `<name>` refers to the display label the TUI renders
    // — both the model id and the display label share the "Test-Model-7B-..."
    // shape — but for the file system check we only need the file path.
    assert!(
        expected_name.contains("Test-Model")
            || expected_name.contains("Test Model")
            || expected_name.contains("test-model"),
        "scenario passed an unexpected model name: {expected_name}"
    );
}

// ---------------------------------------------------------------------------
// When steps (§A.When / §B)
// ---------------------------------------------------------------------------

/// Common command builder. Sets every env-var the WS scenario relies on:
///
/// - `MODELTAP_HEADLESS=1` + `--quit-after-paint` so the binary paints one
///   frame and exits (parent contract).
/// - `MODELTAP_TEST_PLUGINS=test-tool` to register the in-process TestTool
///   plugin via the cfg-gated registry seam (step 01-03).
/// - `MODELTAP_TEST_TOOL_ROOT=<fixture>/test-tool/models` so the TestTool
///   reports its synthetic model file.
/// - `MODELTAP_CACHE_PATH=<fixture>/xdg-data/modeltap/cache.sqlite` so the
///   warm-start path (step 01-04) reads/writes the per-scenario cache.
/// - `MODELTAP_LOG_DIR=<fixture>/logs` so process B's
///   `launch.warm_paint_ms` event lands somewhere we can read.
/// - Isolation env vars pinning every other plugin at a non-existent path
///   so the WS scenario is hermetic w.r.t. the developer's real
///   Ollama/HF/LM-Studio installs (matches the parent's
///   `us_02_discover_ollama.rs` pattern).
fn modeltap_command_with_test_tool(world: &WalkingSkeletonWorld) -> Command {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", world.fixture.test_tool_root())
        .env("MODELTAP_CACHE_PATH", world.fixture.cache_path())
        .env("MODELTAP_LOG_DIR", world.fixture.log_dir())
        // Isolate every real plugin at a non-existent path so the inventory
        // contains ONLY the TestTool's row. Mirrors us_02_discover_ollama.rs.
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache");
    cmd
}

/// `When Devon runs "modeltap" in headless mode and quits after first paint`
///
/// Spawns process A. Asserts success exit code. Captures stdout into the
/// world so subsequent Then steps can verify the right-pane contents.
pub fn when_devon_runs_modeltap_and_quits_after_first_paint(world: &mut WalkingSkeletonWorld) {
    let output = modeltap_command_with_test_tool(world)
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("spawn modeltap process A");
    assert!(
        output.status.success(),
        "process A must exit 0; got status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    world.last_stdout = Some(String::from_utf8_lossy(&output.stdout).into_owned());
}

/// `When a second modeltap process launches against the same cache file`
///
/// Truncates `launch.log` first so the `launch.warm_paint_ms` event observed
/// from process B is the FIRST such entry (process A's reconcile-writeback
/// path is a stub — it emits no warm-paint event, but truncating eliminates
/// any future cross-talk risk). Then spawns process B with the same
/// `MODELTAP_CACHE_PATH`; process B's warm-start orchestration (step 01-04)
/// must open the cache, read the row process A wrote, and emit
/// `launch.warm_paint_ms`.
pub fn when_a_second_modeltap_process_launches_against_the_same_cache_file(
    world: &mut WalkingSkeletonWorld,
) {
    // Truncate launch.log so the warm-paint assertion observes ONLY process
    // B's event. Process A's path emits NO warm-paint (OpenedFresh source);
    // the truncate is defensive against future regressions.
    let log_path = world.fixture.log_dir().join("launch.log");
    if log_path.exists() {
        std::fs::write(&log_path, b"").expect("truncate launch.log between processes");
    }

    let output = modeltap_command_with_test_tool(world)
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("spawn modeltap process B");
    assert!(
        output.status.success(),
        "process B must exit 0; got status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    world.process_b_exit_code = Some(output.status.code().unwrap_or(1));
    world.last_stdout = Some(String::from_utf8_lossy(&output.stdout).into_owned());
}

// ---------------------------------------------------------------------------
// Then steps (§A.Then / §B)
// ---------------------------------------------------------------------------

/// `Then the cache file at "<path>" exists with PRAGMA user_version = <n>`
pub fn then_the_cache_file_exists_with_pragma_user_version(
    world: &WalkingSkeletonWorld,
    expected_user_version: u32,
) {
    let path = world.fixture.cache_path();
    assert!(
        path.exists(),
        "cache.sqlite must exist at {} after process A; got missing file",
        path.display()
    );
    let verifier = CacheVerifier::open(&path).expect("open cache for verification");
    let observed = verifier.pragma_user_version().expect("query user_version");
    assert_eq!(
        observed, expected_user_version,
        "PRAGMA user_version must be {expected_user_version} after migration; got {observed}"
    );
}

/// `Then cache_models contains exactly N row(s) for tool_id "<id>"`
pub fn then_cache_models_contains_exactly_rows_for_tool_id(
    world: &WalkingSkeletonWorld,
    expected_rows: i64,
    tool_id: &str,
) {
    let path = world.fixture.cache_path();
    let verifier = CacheVerifier::open(&path).expect("open cache for verification");
    let where_clause = format!("tool_id = '{}'", tool_id.replace('\'', "''"));
    let observed = verifier
        .count_rows("cache_models", Some(&where_clause))
        .expect("count rows in cache_models");
    assert_eq!(
        observed, expected_rows,
        "cache_models must contain exactly {expected_rows} row(s) for tool_id={tool_id}; got {observed}"
    );
}

/// `Then cache_tools contains a row for tool_id "<id>" with model_count = N`
pub fn then_cache_tools_contains_a_row_with_model_count(
    world: &WalkingSkeletonWorld,
    tool_id: &str,
    expected_model_count: i64,
) {
    let path = world.fixture.cache_path();
    let verifier = CacheVerifier::open(&path).expect("open cache for verification");
    let observed = verifier
        .model_count_for(tool_id)
        .expect("query cache_tools for model_count");
    assert_eq!(
        observed,
        Some(expected_model_count),
        "cache_tools must contain a row for tool_id={tool_id} with model_count={expected_model_count}; got {observed:?}"
    );
}

/// `Then the second process's TUI shows "<name>" in the right pane`
///
/// The headless harness prints the rendered frame to stdout (see
/// `crates/modeltap-app/src/headless.rs::print_frame`). The right pane sits
/// at the right half of the 100-column TestBackend; we substring-match the
/// captured stdout to confirm the model name is present in the painted frame.
pub fn then_the_second_processes_tui_shows_in_the_right_pane(
    world: &WalkingSkeletonWorld,
    expected_substring: &str,
) {
    let stdout = world
        .last_stdout
        .as_deref()
        .expect("when_a_second_modeltap_process_launches... must capture stdout");
    assert!(
        stdout.contains(expected_substring),
        "process B's painted frame must contain '{expected_substring}'; got:\n{stdout}"
    );
}

/// `Then the second process's warm-paint time is at most N ms`
///
/// Reads `launch.log` and asserts at least one `launch.warm_paint_ms` event
/// is present with a `duration_ms` field ≤ budget. Returns the observed
/// duration so the WS driver can echo it into the test output / report.
pub fn then_the_second_processes_warm_paint_time_is_at_most(
    world: &WalkingSkeletonWorld,
    budget_ms: u64,
) -> u64 {
    let events = read_launch_log(&world.fixture.log_dir());
    let warm_event = events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("launch.warm_paint_ms"))
        .unwrap_or_else(|| {
            panic!(
                "process B's launch.log must contain a launch.warm_paint_ms event; got events: {:?}",
                events
                    .iter()
                    .filter_map(|e| e.get("event").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
            )
        });
    let duration_ms = warm_event
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .expect("warm_paint event must carry duration_ms as a non-negative integer");
    assert!(
        duration_ms <= budget_ms,
        "warm-paint time must be <= {budget_ms} ms; observed {duration_ms} ms (event = {warm_event:?})"
    );
    duration_ms
}

/// `Then the second process's summary bar shows "as of just now" or "as of <N> seconds ago"`
///
/// This phrase lands in phase 02+ when the summary-bar provenance renderer
/// (step 02-04 / US-24) is wired. The walking-skeleton's exit gate per the
/// step's AC-7 is the right-pane substring; the provenance line is asserted
/// once the formatter exists. For phase 01 this is a structural no-op that
/// documents the phrase the .feature file uses.
pub fn then_the_second_processes_summary_bar_shows_provenance(_world: &WalkingSkeletonWorld) {
    // Documentary — the summary-bar provenance renderer is implemented in
    // phase 02. The walking-skeleton EXIT GATE per the step's AC-7 is
    // satisfied by the right-pane substring; this assertion lands a stricter
    // sub-condition once the renderer exists.
}

// ---------------------------------------------------------------------------
// Helpers (not Gherkin step phrases — internal plumbing)
// ---------------------------------------------------------------------------

/// Read every JSONL line in `<log_dir>/launch.log`. Empty lines are skipped.
/// Returns an empty Vec if the file is absent (process B's launch logging
/// is best-effort per `warm_start::emit_warm_paint_event`).
pub fn read_launch_log(log_dir: &Path) -> Vec<Value> {
    let path = log_dir.join("launch.log");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Convenience aggregator for the WS driver: invokes the cache verifier and
/// returns `(user_version, cache_models_rows_for_test_tool, cache_tools_model_count_for_test_tool)`
/// in one pass so the WS driver can assert all three with one verifier
/// instantiation. Not a Gherkin step — composes the §A primitives.
pub fn cache_inventory_snapshot(world: &WalkingSkeletonWorld) -> (u32, i64, Option<i64>) {
    let path = world.fixture.cache_path();
    let verifier = CacheVerifier::open(&path).expect("open cache for snapshot");
    let user_version = verifier.pragma_user_version().expect("user_version");
    let where_clause = format!("tool_id = '{}'", test_tool_id_str());
    let models_rows = verifier
        .count_rows("cache_models", Some(&where_clause))
        .expect("count cache_models");
    let model_count = verifier
        .model_count_for(test_tool_id_str())
        .expect("model_count");
    (user_version, models_rows, model_count)
}

/// Used by the WS driver to capture an `Output` if the test needs to inspect
/// stderr in the future. Not currently a Gherkin step — exposed so phase
/// 02+ scenarios that need post-exit stderr inspection have a hook.
pub fn capture_modeltap_output(
    world: &WalkingSkeletonWorld,
    extra_args: &[&str],
    timeout: Duration,
) -> Output {
    let mut cmd = modeltap_command_with_test_tool(world);
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.timeout(timeout)
        .output()
        .expect("spawn modeltap with extra args")
}

/// Convenience: returns the metadata kind value the TestTool reports in its
/// `inspect_model` response. Phase 02+ scenarios will assert this via the
/// model-detail screen's Metadata section; the walking-skeleton does not
/// exercise the detail screen, so this is a forward-compatibility hook.
pub fn expected_test_tool_metadata_kind_value() -> &'static str {
    TEST_METADATA_KIND_VALUE
}

/// Tempdir adoption helper for phase 02+ scenarios that need a sibling
/// tempdir alongside `DevonCacheEmptyFixture`. Wraps `TempDir::new` so a
/// future drift in tempdir construction is centralised. Not consumed by the
/// walking-skeleton driver.
pub fn new_sibling_tempdir() -> TempDir {
    TempDir::new().expect("create sibling tempdir")
}

/// Path-coercion helper: turns a relative path-template ("test-tool/models/X")
/// into an absolute PathBuf rooted in the fixture's tempdir. Phase 02+
/// scenarios use this to resolve fixture-relative paths the Gherkin
/// templates supply.
pub fn resolve_fixture_relative(world: &WalkingSkeletonWorld, relative: &str) -> PathBuf {
    world.fixture.temp.path().join(relative)
}
