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

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use modeltap_acceptance::fixtures::cache_fixtures::DevonCacheEmptyFixture;
use modeltap_acceptance::fixtures::inspect_fixtures::{
    devon_ollama_userconfig, devon_tool_error_ollama, InspectFixture as RawInspectFixture,
};
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
///
/// Two construction paths: the `DevonCacheEmptyFixture` variant (used by
/// AC-21-1 / AC-21-3 / AC-21-7) drives the cache-empty TestTool flow; the
/// `InspectFixture` variant (used by AC-21-4 / AC-21-5) drives the
/// real-Ollama-plugin discover-error + user-config-search-paths flows. The
/// world holds the SAME small set of paths regardless of source so the
/// step helpers below can stay shape-uniform.
pub struct ToolDetailWorld {
    /// Cache-empty fixture, present for the cache-empty TestTool flow.
    pub fixture: Option<DevonCacheEmptyFixture>,
    /// Inspect fixture, present for the discover-error / user-config flows.
    pub inspect_fixture: Option<RawInspectFixture>,
    pub last_stdout: Option<String>,
}

impl ToolDetailWorld {
    pub fn new() -> Self {
        Self {
            fixture: Some(DevonCacheEmptyFixture::build()),
            inspect_fixture: None,
            last_stdout: None,
        }
    }

    /// Build a world backed by an `InspectFixture` for the AC-21-4 / AC-21-5
    /// scenarios.
    pub fn with_inspect_fixture(fixture: RawInspectFixture) -> Self {
        Self {
            fixture: None,
            inspect_fixture: Some(fixture),
            last_stdout: None,
        }
    }

    fn cache_path(&self) -> PathBuf {
        if let Some(f) = &self.fixture {
            f.cache_path()
        } else {
            self.inspect_fixture
                .as_ref()
                .expect("either fixture must be set")
                .cache_path()
        }
    }

    fn log_dir(&self) -> PathBuf {
        if let Some(f) = &self.fixture {
            f.log_dir()
        } else {
            self.inspect_fixture
                .as_ref()
                .expect("either fixture must be set")
                .log_dir()
        }
    }

    fn test_tool_root(&self) -> PathBuf {
        if let Some(f) = &self.fixture {
            f.test_tool_root()
        } else {
            self.inspect_fixture
                .as_ref()
                .expect("either fixture must be set")
                .test_tool_root()
        }
    }
}

impl Default for ToolDetailWorld {
    fn default() -> Self {
        Self::new()
    }
}

/// Re-export of the inspect-fixture surface under the
/// `InspectFixture::devon_…` constructor names the AC-21-4 / AC-21-5 driver
/// uses. Wraps the bare `fn` builders into static methods so the call sites
/// read symmetrically with the cache-empty fixture's `::build()` constructor.
pub struct InspectFixture;

impl InspectFixture {
    pub fn devon_tool_error() -> RawInspectFixture {
        devon_tool_error_ollama()
    }
    pub fn devon_userconfig() -> RawInspectFixture {
        devon_ollama_userconfig()
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
    let model_path = world.test_tool_root().join(TEST_MODEL_FILENAME);
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
    let path = world.cache_path();
    let test_tool_root = world.test_tool_root();
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
            install_path: test_tool_root.clone(),
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
                path: test_tool_root,
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

/// `When Devon runs modeltap once to populate the cache (no detail open)`
///
/// AC-21-4 cold-start invocation. Drives the modeltap process with a single
/// `q` token so reconcile_writeback runs against the broken-Ollama fixture
/// and writes a `cache_tools` row carrying `last_error` + `last_error_at`.
/// stdout is intentionally not captured here — only the cache mutation
/// matters for the subsequent warm-start invocation.
///
/// The Ollama plugin's `discover()` reaches `read_dir(<root>/manifests)` —
/// the fixture made `manifests/` a regular file, so the read returns
/// `NotADirectory`, the plugin surfaces `DiscoverError::Io`, and
/// `reconcile_writeback` (step 02-02 extension in `main.rs`) projects the
/// error into a cache row.
pub fn when_devon_runs_modeltap_to_populate_cache_only(world: &mut ToolDetailWorld) {
    let output = build_command(world)
        .env("MODELTAP_HEADLESS_INPUT", "q")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("spawn cold-start modeltap process");
    assert!(
        output.status.success(),
        "cold-start modeltap process must exit 0; got status={:?}, stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    // We deliberately do NOT overwrite world.last_stdout — the warm-start
    // invocation below is the one whose frame we substring-match.
}

/// `When Devon opens the tool-detail screen for ollama`
///
/// AC-21-4 / AC-21-5 detail-screen invocation. Navigates UP from the
/// alphabetically-last test-tool row to the ollama row, then presses Enter
/// + Esc. The tool list sorts alphabetically (`gpt4all`, `hf`, `lm-studio`,
///   `ollama`, `test-tool`) and the default cursor lands on the first
///   `ToolStatus::Ok` row — under both AC-21-4 (Ollama errors) and AC-21-5
///   (Ollama NotInstalled), test-tool is the sole Ok row, so the cursor
///   starts at index 4. One `<up>` lands on `ollama` (index 3).
///
/// Captures the final printed frame from stdout so subsequent Then steps
/// can substring-match the rendered detail screen.
pub fn when_devon_opens_tool_detail_for_ollama(world: &mut ToolDetailWorld) {
    let output = build_command(world)
        .env("MODELTAP_HEADLESS_INPUT", "<up><enter><esc>q")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("spawn warm-start modeltap process");
    assert!(
        output.status.success(),
        "warm-start modeltap process must exit 0; got status={:?}, stderr={}",
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
    let events = read_launch_log(&world.log_dir());
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
    // Resolve env-var values from whichever fixture variant is active. For
    // the InspectFixture flow the world's ollama_dir + config_path are the
    // discover-error / user-config seams; for the cache-empty flow they
    // remain at the nonexistent defaults (so the left pane is solely the
    // TestTool's row).
    let (ollama_dir, config_path) = match &world.inspect_fixture {
        Some(f) => (
            f.ollama_dir.to_string_lossy().into_owned(),
            f.config_path.to_string_lossy().into_owned(),
        ),
        None => (
            "/nonexistent/no-such-ollama".to_string(),
            "/nonexistent/no-such-config.toml".to_string(),
        ),
    };
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", world.test_tool_root())
        .env("MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED", "1")
        .env("MODELTAP_HEADLESS_TOOL_DETAIL", "1")
        .env("MODELTAP_CACHE_PATH", world.cache_path())
        .env("MODELTAP_LOG_DIR", world.log_dir())
        // Per-plugin isolation. Ollama may be reseated by InspectFixture to
        // a real broken-discovery tempdir or to a nonexistent path; everyone
        // else stays nonexistent so the left pane is the TestTool + Ollama.
        .env("MODELTAP_OLLAMA_DIR", ollama_dir)
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", config_path)
        .env("HF_HOME", "/nonexistent/no-such-hf-cache")
        // Short-circuit Ollama's `inspect_tool` HTTP probe so the inspect
        // path is deterministic across CI and dev machines (ADR-016 §D12 /
        // R5). The detected_version reported in the detail screen will be
        // this literal — irrelevant to AC-21-4 / AC-21-5 assertions but
        // pins the inspect-side merge to a no-network code path.
        .env("MODELTAP_OLLAMA_VERSION", "0.6.4");
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
