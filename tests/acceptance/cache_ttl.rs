//! Per-tool TTL eligibility acceptance scenarios (US-25 AC-25-2 / AC-25-4,
//! AC-23-1, AC-25-7).
//!
//! tool-model-info-sqlite-cache step 04-03. Three scenarios that exercise the
//! warm-start orchestrator's per-tool partition logic, production cache-path
//! resolution, and the transient I/O fallback.
//!
//! The driver invokes `warm_start::run` directly against a fixture-seeded
//! `cache.sqlite` (Strategy A — in-process, no subprocess). The step-phrase
//! implementations live in `steps/cache_ttl.rs`; this driver wires them in
//! scenario order.

#[path = "steps/cache_ttl.rs"]
mod cache_ttl;

use cache_ttl::*;

// ---------------------------------------------------------------------------
// Scenario 1: "Per-tool TTL forces cold paint for stale tool entries while
// other tools paint from cache"
// ---------------------------------------------------------------------------
//
// AC-25-2 + AC-25-4: with the `devon-cache-stale-tool` fixture (one tool
// 25h old, two tools 2h/1h old) and a 24h tool_ttl_seconds, warm-start MUST
// (a) paint the fresh tools' models into the returned inventory, and
// (b) return the stale tool's id in `stale_tool_ids` so the downstream
// cold-scan dispatcher can refresh it.
#[test]
fn per_tool_ttl_forces_cold_paint_for_stale_tool_entries() {
    let mut world = CacheTtlWorld::new();

    given_the_stale_tool_fixture_is_seeded(&world);
    given_tool_ttl_is_24_hours(&mut world);

    when_warm_start_runs(&mut world);

    then_warm_start_returns_existing_source(&world);
    then_inventory_contains_fresh_tools_models(&world);
    then_stale_tool_appears_in_stale_tool_ids(&world);
    then_fresh_tools_absent_from_stale_tool_ids(&world);
}

// ---------------------------------------------------------------------------
// Scenario 2: "Production cache path resolves via XDG_DATA_HOME on Linux or
// Library/Application Support on macOS"
// ---------------------------------------------------------------------------
//
// AC-23-1 / OQ-1: when `MODELTAP_CACHE_PATH` is unset, cache_path::resolve
// MUST land on the platform's documented data-dir suffix. The test pins
// `HOME` (and on Linux `XDG_DATA_HOME`) inside a tempdir so the resolver's
// `dirs::data_dir()` call returns a predictable value.
#[test]
fn production_cache_path_resolves_via_xdg_or_application_support() {
    given_no_cache_overrides_are_set();
    let resolved = when_cache_path_resolve_runs_with_pinned_home();
    then_resolved_path_matches_platform_default(&resolved);
}

// ---------------------------------------------------------------------------
// Scenario 3: "Transient I/O fallback treats every tool as stale without
// crashing when models_for_tool consistently fails"
// ---------------------------------------------------------------------------
//
// AC-25-7 (transient cache I/O failure during warm-start MUST not crash):
// when the per-tool reads ALL fail (e.g., the cache_models table is gone
// mid-launch — an in-process analogue of a disk-level transient I/O
// failure), warm-start returns an empty inventory and adds every fresh
// tool's id to `stale_tool_ids` so cold-start owns the entire launch. The
// process does NOT crash; the outer call site at composition root continues
// to cold-start (C-INFO-2).
#[test]
fn transient_io_fallback_treats_every_tool_as_stale_without_crashing() {
    let mut world = CacheTtlWorld::new();

    given_the_stale_tool_fixture_is_seeded(&world);
    given_tool_ttl_is_24_hours(&mut world);
    // Drop the `cache_models` table after seeding. The next read will fail
    // with `CacheError::Sqlite(no such table: cache_models)` — the
    // closest-faithful in-process surrogate for a disk-level transient
    // I/O failure available to the test process.
    given_cache_models_table_is_dropped(&mut world);

    when_warm_start_runs(&mut world);

    then_warm_start_returns_existing_source(&world);
    // Empty inventory: every per-tool models_for_tool() returned a Sqlite
    // error, which the fallback gate routes through to `stale_tool_ids`.
    then_inventory_is_empty(&world);
    // The two FRESH tools (TTL-eligible but unreadable) joined the stale
    // list — the stale-by-TTL tool was already in the list.
    then_all_three_tools_appear_in_stale_tool_ids(&world);
    // And, critically: the run returned Ok — no panic, no abort.
    then_warm_start_did_not_error(&world);
}
