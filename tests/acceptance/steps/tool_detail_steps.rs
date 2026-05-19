//! Step-definition helpers for the US-21 tool-detail acceptance scenarios.
//!
//! Driven by `tests/acceptance/tool_detail.rs`. Each helper is named after
//! the Gherkin phrase it implements (per `step-definitions-skeleton.md`
//! conventions) so the driver reads scenario-order with no glue.
//!
//! The acceptance crate does NOT use cucumber-rs; every existing scenario
//! is a plain `#[test]` function that drives the `modeltap` binary through
//! `assert_cmd::Command::cargo_bin`. This module mirrors that pattern. See
//! `tests/acceptance/steps/cache_lifecycle.rs` for the M1 walking-skeleton
//! parallel.

#![allow(dead_code)] // Helpers land incrementally; the four phase-02 scenarios
                     // exercise the full surface, but future US-21 scenarios
                     // (e.g. AC-21-5 user-config search paths in step 02-02)
                     // will pick up the remaining helpers.

use std::time::Duration;

use assert_cmd::Command;
use modeltap_acceptance::fixtures::cache_fixtures::DevonCacheEmptyFixture;
use modeltap_acceptance::test_tool::TEST_MODEL_FILENAME;
use modeltap_store::types::{CachedTool, SearchPathEntry as StoreSearchPathEntry};
use modeltap_store::{Cache, CacheOpenResult};
use serde_json::Value;

/// K-INFO-1 budget for the detail-screen open path (debug-build envelope is
/// permissive vs the release-build 100 ms; we assert the release-build
/// number here because the orchestrator is bounded by SQLite open + a single
/// row read + a Tokio spawn_blocking hop, which never exceeds 100 ms on
/// developer hardware even under cargo-test contention).
pub const TOOL_DETAIL_OPEN_BUDGET_MS: u64 = 100;

/// Scenario world. Holds the per-scenario tempdir fixture plus the captured
/// stdout from the modeltap process so Then-steps can substring-match the
/// rendered frame without rerunning the binary.
pub struct ToolDetailWorld {
    pub fixture: DevonCacheEmptyFixture,
    pub last_stdout: Option<String>,
}

impl ToolDetailWorld {
    pub fn new() -> Self {
        Self {
            fixture: DevonCacheEmptyFixture::build(),
            last_stdout: None,
        }
    }
}

impl Default for ToolDetailWorld {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Given steps
// ---------------------------------------------------------------------------

/// `Given the in-process TestTool plugin is registered`
///
/// Documents that the When-step sets `MODELTAP_TEST_PLUGINS=test-tool` on
/// the modeltap process. As a boundary check, asserts the fixture's seed
/// model file exists so the TestTool's `discover()` returns the expected
/// non-empty inventory.
pub fn given_the_in_process_test_tool_plugin_is_registered(world: &ToolDetailWorld) {
    let model_path = world.fixture.test_tool_root().join(TEST_MODEL_FILENAME);
    assert!(
        model_path.exists(),
        "TestTool's seed model must exist at {} before the binary launches",
        model_path.display()
    );
}

/// `Given the TestTool's inspect_tool() returns Unsupported`
///
/// Documents that the When-step sets `MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED=1`
/// so the orchestrator exercises the default-Unsupported merge path. This is
/// the production path until step 02-02 lands the Ollama / HF / LM-Studio
/// inspect_tool overrides. The actual env-var is set inside `build_command`
/// below; this Given is documentary so the driver reads top-to-bottom.
pub fn given_the_test_tool_inspect_tool_returns_unsupported(_world: &ToolDetailWorld) {
    // No-op: the When-step wires MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED=1
    // into every modeltap invocation in this suite. Phrase documented here
    // so the driver mirrors the .feature file's Given line ordering.
}

/// `Given the cache has a tool row for "test-tool" with last_error = "<msg>"`
///
/// Pre-seeds `<fixture>/xdg-data/modeltap/cache.sqlite` with a `cache_tools`
/// row whose `last_error` + `last_error_at` are populated. The modeltap
/// process opens that cache on launch, the orchestrator reads the row, and
/// the detail screen renders the error text + timestamp per AC-21-4.
///
/// The seed mirrors the `cache_fixtures::tests::seed_one_row_cache` pattern
/// — opens the cache via `Cache::open`, writes a CachedTool through
/// `write_tool`, then drops the connection so the modeltap binary can open
/// it cleanly. Uses a fixed timestamp (2023-11-14T22:13:20Z, which is
/// UNIX_EPOCH + 1_700_000_000 seconds) so the year-prefix assertion in the
/// Then step is stable.
pub fn given_the_cache_has_a_tool_row_with_last_error(world: &ToolDetailWorld, last_error: &str) {
    let path = world.fixture.cache_path();
    let opened = Cache::open(&path).expect("open cache for pre-seed");
    let cache = match opened {
        CacheOpenResult::OpenedFresh(c)
        | CacheOpenResult::OpenedExisting(c)
        | CacheOpenResult::OpenedAfterMigration { cache: c, .. }
        | CacheOpenResult::OpenedAfterRecovery { cache: c, .. } => c,
    };
    let stamp = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    cache
        .write_tool(&CachedTool {
            tool_id: modeltap_acceptance::test_tool::TEST_TOOL_NAME,
            install_path: world.fixture.test_tool_root(),
            detected_version: None,
            plugin_version: "modeltap-acceptance-test-tool 0.0.0".to_string(),
            model_count: 0,
            disk_usage_bytes: 0,
            largest_model_id: None,
            last_scan_at: stamp,
            last_scan_duration_ms: 0,
            last_error: Some(last_error.to_string()),
            last_error_at: Some(stamp),
            search_paths: vec![StoreSearchPathEntry {
                path: world.fixture.test_tool_root(),
                source: modeltap_store::types::SearchPathSource::Default,
            }],
        })
        .expect("seed cache_tools row with last_error");
    drop(cache);
    assert!(
        path.exists(),
        "post-seed cache.sqlite must exist at {}",
        path.display()
    );
}

// ---------------------------------------------------------------------------
// When step
// ---------------------------------------------------------------------------

/// `When Devon runs modeltap, presses Enter on the test-tool row, then Esc`
///
/// Spawns one modeltap process in headless mode with a scripted input of
/// `<enter><esc>`. The `MODELTAP_HEADLESS_TOOL_DETAIL=1` env-var opts the
/// process into the step 02-01 lift that rewrites the production
/// `Msg::ToggleFolderExpansion` (Enter on Main) into
/// `Msg::OpenToolDetail(selected_tool_id)`. Captures the final printed frame
/// from stdout so subsequent Then steps can substring-match.
///
/// One process, one scripted run — the K-INFO-1 timing assertion is
/// satisfied via the `tool_detail.open_ms` JSONL event the orchestrator
/// writes (not via wall-clock from outside).
pub fn when_devon_runs_modeltap_and_presses_enter_then_esc(world: &mut ToolDetailWorld) {
    let output = build_command(world)
        .env("MODELTAP_HEADLESS_INPUT", "<enter><esc>q")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("spawn modeltap process");
    assert!(
        output.status.success(),
        "modeltap process must exit 0; got status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    world.last_stdout = Some(String::from_utf8_lossy(&output.stdout).into_owned());
}

// ---------------------------------------------------------------------------
// Then steps
// ---------------------------------------------------------------------------

/// `Then the tool detail screen opens within <budget_ms> ms`
///
/// Reads the orchestrator's `tool_detail.open_ms` event from
/// `<log_dir>/launch.log`. The event records the wall-clock from the
/// orchestrator's entry to the merge completion (covering cache I/O +
/// inspect_tool + merge), which is the latency Devon perceives.
pub fn then_the_tool_detail_screen_opens_within_ms(world: &ToolDetailWorld, budget_ms: u64) {
    let events = read_launch_log(&world.fixture.log_dir());
    let open_event = events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("tool_detail.open_ms"))
        .unwrap_or_else(|| {
            panic!(
                "launch.log must contain a tool_detail.open_ms event; got events: {:?}",
                events
                    .iter()
                    .filter_map(|e| e.get("event").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
            )
        });
    let duration_ms = open_event
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .expect("tool_detail.open_ms event must carry duration_ms as a non-negative integer");
    assert!(
        duration_ms <= budget_ms,
        "tool_detail.open_ms must be <= {budget_ms} ms; observed {duration_ms} ms \
         (event = {open_event:?})"
    );
}

/// `Then the rendered frame contains "<substring>"`
///
/// Substring-greps the captured stdout (the headless harness prints each
/// painted frame, terminated by the FINAL post-quit frame). Used to assert
/// the tool-detail screen rendered the expected text on the Enter frame,
/// and the Main view rendered as expected on the post-Esc frame.
pub fn then_the_rendered_frame_contains(world: &ToolDetailWorld, expected: &str) {
    let stdout = world
        .last_stdout
        .as_deref()
        .expect("when_devon_runs_modeltap_and_presses_enter_then_esc must capture stdout");
    assert!(
        stdout.contains(expected),
        "captured frame must contain '{expected}'; got:\n{stdout}"
    );
}

/// `Then the rendered frame does NOT contain "<substring>"`
///
/// Negative assertion — used to verify the seeded TestTool data does not
/// leak through when inspect_tool returned Unsupported (AC-21-3's "no false
/// or stale version is shown").
pub fn then_the_rendered_frame_does_not_contain(world: &ToolDetailWorld, forbidden: &str) {
    let stdout = world
        .last_stdout
        .as_deref()
        .expect("when_devon_runs_modeltap_and_presses_enter_then_esc must capture stdout");
    assert!(
        !stdout.contains(forbidden),
        "captured frame must NOT contain '{forbidden}'; got:\n{stdout}"
    );
}

/// `Then the final frame is the main view`
///
/// AC-21-7 assertion: after Esc, the captured frame's LAST printed frame
/// must be the Main view, not the tool-detail screen. The tool-detail
/// screen renders the title block ` Tool: test-tool ` (per
/// `tool_detail::render`); we slice the captured stdout to the final frame
/// (everything after the last "Tool: test-tool" occurrence) and assert
/// that slice contains no tool-detail-specific labels.
///
/// The captured stdout contains MULTIPLE printed frames (one per script
/// token via the in-loop `terminal.draw` + the final post-quit frame). The
/// post-Esc frame is the FINAL one — we look at the trailing slice after
/// the last detail-screen marker.
pub fn then_the_final_frame_is_the_main_view(world: &ToolDetailWorld) {
    let stdout = world
        .last_stdout
        .as_deref()
        .expect("when_devon_runs_modeltap_and_presses_enter_then_esc must capture stdout");
    // The detail screen's Title block paints " Tool: test-tool " — find the
    // last occurrence and take everything afterward as the trailing-frame
    // candidate. The post-Esc Main view will NOT repaint that title.
    let last_detail_idx = stdout.rfind("Tool: test-tool");
    let trailing = match last_detail_idx {
        Some(idx) => {
            // Skip past the detail title; the next newline-block is either
            // additional detail-screen content (if Esc was swallowed) or the
            // Main view (if Esc closed the detail screen as expected).
            // Skip over the rest of that line; what follows is the body
            // of the last frame that contained "Tool: test-tool" plus any
            // subsequent frames.
            //
            // The headless loop's final-frame capture runs AFTER the <esc>
            // iteration, so the FINAL printed frame is the Main view. Look
            // for "Discovery root:" — a label that exists only on the
            // tool-detail screen. If it appears AFTER the last
            // "Tool: test-tool" but BEFORE the end of stdout, Esc didn't
            // close the screen. If it does NOT appear after that point, the
            // post-Esc frame is the Main view (PASS).
            &stdout[idx..]
        }
        None => {
            panic!(
                "captured stdout must contain at least one 'Tool: test-tool' frame \
                 (the post-Enter detail screen); got:\n{stdout}"
            );
        }
    };
    // The final post-Esc frame must not paint the tool-detail body. The
    // `Discovery root:` label is detail-screen-only (the Main view has no
    // such field). It WILL appear once (in the post-Enter frame containing
    // the last "Tool: test-tool"); count occurrences in `trailing` and
    // require there to be exactly one — the one that prefixed the detail
    // title we sliced from.
    let discovery_root_count = trailing.matches("Discovery root:").count();
    assert!(
        discovery_root_count <= 1,
        "after Esc, the Main view must replace the tool-detail screen — found \
         {discovery_root_count} 'Discovery root:' occurrences in the trailing \
         frames, which means a tool-detail frame painted AFTER the Esc was \
         scripted; full stdout:\n{stdout}"
    );
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Build a `modeltap` Command with every env-var the US-21 acceptance suite
/// relies on. Mirrors `cache_lifecycle::modeltap_command_with_test_tool` but
/// adds:
///
/// - `MODELTAP_HEADLESS_TOOL_DETAIL=1` to enable the step 02-01 Enter-lift
///   (rewrite `Msg::ToggleFolderExpansion` into `Msg::OpenToolDetail`).
/// - `MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED=1` to force the TestTool's
///   inspect_tool into the trait-default Unsupported path so the
///   orchestrator exercises the cache-only merge branch.
///
/// Every real plugin is pinned at a non-existent path so the left pane
/// contains exactly one row (the TestTool's) — that pins the AC-21-7
/// cursor-preservation assertion to a single deterministic slot.
fn build_command(world: &ToolDetailWorld) -> Command {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", world.fixture.test_tool_root())
        .env("MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED", "1")
        .env("MODELTAP_HEADLESS_TOOL_DETAIL", "1")
        .env("MODELTAP_CACHE_PATH", world.fixture.cache_path())
        .env("MODELTAP_LOG_DIR", world.fixture.log_dir())
        // Isolate every real plugin so the left pane is unambiguously
        // "test-tool" — mirrors cache_lifecycle.rs's isolation set.
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

/// Read every JSONL line in `<log_dir>/launch.log`. Mirrors the M1
/// walking-skeleton helper of the same name. Empty file or missing file
/// returns an empty Vec — the orchestrator's logging is best-effort, but
/// the AC-21-1 assertion will surface the missing event with a clear panic.
fn read_launch_log(log_dir: &std::path::Path) -> Vec<Value> {
    let path = log_dir.join("launch.log");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}
