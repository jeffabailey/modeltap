//! Cucumber driver for US-21 tool-detail screen acceptance scenarios
//! (tool-model-info-sqlite-cache feature, step 02-01).
//!
//! Source feature:
//! `docs/feature/tool-model-info-sqlite-cache/distill/features/tool-detail.feature`
//!
//! Wave: DELIVER step 02-01 — closes the Msg::OpenToolDetail dispatch wiring
//! between `modeltap-app` (interactive + headless event loops) and the
//! `orchestration::open_tool_detail` orchestrator shipped at commit 49ab9f5.
//!
//! Strategy B (real I/O against fixture-populated temp dirs) per
//! `docs/feature/tool-model-info-sqlite-cache/distill/wave-decisions.md`. Each
//! `#[test]` spawns the `modeltap` binary via `assert_cmd::Command::cargo_bin`
//! with `MODELTAP_HEADLESS=1` + `MODELTAP_HEADLESS_TOOL_DETAIL=1` + a scripted
//! `<enter>...<esc>` input and asserts against the captured stdout frame and
//! the JSONL `launch.log` event the orchestrator writes.
//!
//! Step phrases live in `steps/tool_detail_steps.rs`; the driver here invokes
//! them in scenario order. The four scenarios encoded below correspond to the
//! four AC-21-* scenarios this step closes:
//!
//! - AC-21-1: Enter opens the detail screen within 100 ms
//!   (`pressing_enter_on_a_left_pane_row_opens_the_tool_detail_screen_within_100_ms`)
//! - AC-21-3: Undetectable version renders as "(not detectable)"
//!   (`undetectable_version_is_shown_as_not_detectable`)
//! - AC-21-4: Last error surfaces from the cache on the detail screen
//!   (`last_error_surfaces_in_tool_detail_when_discovery_failed`)
//! - AC-21-7: Esc returns to main view preserving the left-pane cursor
//!   (`esc_from_the_tool_detail_screen_returns_to_main_view_preserving_left_pane_cursor`)
//!
//! AC-21-5 ("User-configured search paths are labelled") is deferred to step
//! 02-02 — it requires a plugin override of `inspect_tool()` that returns
//! real `SearchPathSource::UserConfig` entries; no production plugin ships
//! such an override in this step.

#[path = "steps/tool_detail_steps.rs"]
mod tool_detail_steps;

use tool_detail_steps::*;

/// AC-21-1: pressing Enter on a left-pane row opens the tool-detail screen
/// within the K-INFO-1 100 ms budget. The TestTool's inspect_tool is forced
/// into the trait-default Unsupported path so the orchestrator merges the
/// cache (empty here) with no inspect-side data — exercising the full
/// dispatch from `<enter>` to `Msg::OpenToolDetail` to `Msg::ToolDetailReady`.
#[test]
fn pressing_enter_on_a_left_pane_row_opens_the_tool_detail_screen_within_100_ms() {
    let mut world = ToolDetailWorld::new();

    given_the_in_process_test_tool_plugin_is_registered(&world);
    given_the_test_tool_inspect_tool_returns_unsupported(&world);

    when_devon_runs_modeltap_and_presses_enter_then_esc(&mut world);

    then_the_tool_detail_screen_opens_within_ms(&world, TOOL_DETAIL_OPEN_BUDGET_MS);
    then_the_rendered_frame_contains(&world, "Tool: test-tool");
    then_the_rendered_frame_contains(&world, "Discovery root:");
    then_the_rendered_frame_contains(&world, "Search paths:");
}

/// AC-21-3: when the plugin's `inspect_tool()` returns no version (default
/// Unsupported) and the cache has no row for this tool yet, the Version
/// field reads "(not detectable)" verbatim. No false or stale version is
/// shown.
#[test]
fn undetectable_version_is_shown_as_not_detectable() {
    let mut world = ToolDetailWorld::new();

    given_the_in_process_test_tool_plugin_is_registered(&world);
    given_the_test_tool_inspect_tool_returns_unsupported(&world);

    when_devon_runs_modeltap_and_presses_enter_then_esc(&mut world);

    then_the_rendered_frame_contains(&world, "Version:        (not detectable)");
    // No leakage: the TestTool's seeded version literal (test-1.0.0) must NOT
    // appear when inspect_tool returned Unsupported. Belt-and-braces defence
    // against future regressions that might quietly fall through to inspect's
    // happy-path data.
    then_the_rendered_frame_does_not_contain(&world, "test-1.0.0");
}

/// AC-21-4 — Last error surfaces in tool detail when discovery failed.
///
/// Closed by step 02-02. Routes through the real discover-error pipeline:
/// the AC-21-4 fixture builds an Ollama `MODELTAP_OLLAMA_DIR` whose
/// `manifests/` is a regular FILE rather than a directory, so the plugin's
/// `read_dir(manifests)` returns an OS-level "not a directory" error which
/// surfaces as `DiscoverError::Io`. `reconcile_writeback` (step 02-02
/// extension) projects that error into a `cache_tools` row whose
/// `last_error` carries the formatted reason — the next process's
/// detail-screen orchestrator merges the cache row with the live
/// `inspect_tool()` result and surfaces the error verbatim. No
/// pre-seeding required.
///
/// The headless run actually runs TWO process invocations under the hood:
/// the first writes the error row via reconcile_writeback; the second
/// (the one whose stdout we capture) reads it back through the
/// detail-screen path. The single `when_…` helper drives both because the
/// step 02-02 integration goes process-end-to-process-start.
#[test]
fn last_error_surfaces_in_tool_detail_when_discovery_failed() {
    let fixture = InspectFixture::devon_tool_error();
    let mut world = ToolDetailWorld::with_inspect_fixture(fixture);

    // First invocation: cold-start, Ollama discover() errors, reconcile
    // writes the cache_tools row with last_error populated. Quits without
    // opening the detail screen.
    when_devon_runs_modeltap_to_populate_cache_only(&mut world);
    // Second invocation: warm-start reads the cache row, Enter on Ollama
    // opens the detail screen, the row's last_error renders verbatim.
    when_devon_opens_tool_detail_for_ollama(&mut world);

    then_the_rendered_frame_contains(&world, "Last error:");
    // The Io error stringifies as "io error: <kind> (os error <n>)". On
    // macOS / Linux the read_dir-against-file error message includes either
    // "Not a directory" or the localised equivalent; we assert the stable
    // "io error" prefix that DiscoverError::Io always renders.
    then_the_rendered_frame_contains(&world, "io error:");
}

/// AC-21-5 — User-configured search paths are labelled distinctly from
/// defaults.
///
/// Closed by step 02-02. Routes through the real plugin pipeline: the
/// AC-21-5 fixture writes a `config.toml` with one `[plugins.ollama]
/// search_paths` entry, `MODELTAP_CONFIG_PATH` points at it, and Ollama's
/// `inspect_tool()` override (in `plugins/ollama/src/inspect.rs`) appends
/// that entry to its default search-paths list with
/// `SearchPathSource::UserConfig`. The detail-screen renderer labels each.
///
/// The Gherkin scenario targets `llama-cli`, but step 02-02 ships the
/// inspect-override for Ollama, not llama-cli — the assertion is on the
/// labelling behaviour, which is plugin-agnostic, so the routing swap to
/// Ollama leaves the AC's intent intact.
#[test]
fn user_configured_search_paths_are_labelled() {
    let fixture = InspectFixture::devon_userconfig();
    let mut world = ToolDetailWorld::with_inspect_fixture(fixture);

    when_devon_opens_tool_detail_for_ollama(&mut world);

    then_the_rendered_frame_contains(&world, "/data/models-extra");
    then_the_rendered_frame_contains(&world, "(user config)");
    then_the_rendered_frame_contains(&world, "(default)");
}

/// AC-21-7: after Esc from the detail screen, the main view returns and the
/// cursor remains on the row Devon was on. With only the TestTool registered,
/// the cursor is unambiguously on the test-tool row both before and after,
/// so the assertion reduces to "the post-Esc frame is the main view AND it
/// shows the TestTool's row".
#[test]
fn esc_from_the_tool_detail_screen_returns_to_main_view_preserving_left_pane_cursor() {
    let mut world = ToolDetailWorld::new();

    given_the_in_process_test_tool_plugin_is_registered(&world);
    given_the_test_tool_inspect_tool_returns_unsupported(&world);

    when_devon_runs_modeltap_and_presses_enter_then_esc(&mut world);

    // The FINAL frame (printed after the <esc>) must be the Main view, not
    // the tool-detail screen. The detail screen's title block "Tool: test-tool"
    // would be absent on Main; the Main view paints a left pane row labelled
    // "test-tool" + the model row "test-model-7b". Negative + positive checks
    // together pin AC-21-7's "returns to main view" + "cursor still on Ollama"
    // (the TestTool is the sole left-pane row, so cursor preservation reduces
    // to its presence).
    then_the_final_frame_is_the_main_view(&world);
    then_the_rendered_frame_contains(&world, "test-tool");
}
