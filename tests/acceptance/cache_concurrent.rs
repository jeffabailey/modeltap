//! Concurrent-process cache acceptance scenarios (US-23 Scenarios 4-5,
//! AC-23-10).
//!
//! tool-model-info-sqlite-cache step 04-04 — Phase 04 concurrent-process
//! safety. Two scenarios, each a plain `#[test]` per the project's cucumber-
//! driver convention (no cucumber-rs macro machinery; see also step 04-02
//! `cache_opt_out.rs` and step 04-03 `cache_ttl.rs`). The step-phrase
//! implementations live in `steps/cache_concurrent.rs`; this driver wires
//! them in scenario order.
//!
//! Strategy B (real I/O against fixture-populated temp dirs) per
//! `docs/feature/tool-model-info-sqlite-cache/distill/wave-decisions.md` §D5.
//! Each scenario spawns two real `modeltap` binary instances against a
//! shared `MODELTAP_CACHE_PATH` so the contention path exercises the WAL +
//! busy_timeout PRAGMAs set at `Cache::open`.
//!
//! Per CLAUDE.md §Running Tests Fast on macOS the concurrent scenarios spawn
//! multiple subprocess instances; if CI hits fd-exhaustion these can be
//! moved to a serial test job by tagging them with a `@concurrent` marker.

#[path = "steps/cache_concurrent.rs"]
mod cache_concurrent;

use cache_concurrent::*;

// ---------------------------------------------------------------------------
// Scenario 1: "Two modeltap processes can read the cache concurrently via
// SQLite WAL"
// ---------------------------------------------------------------------------
//
// AC-23-2 / AC-23-10 read-concurrency invariant: with `journal_mode=WAL`
// set at Cache::open (apply_open_pragmas in modeltap-store/src/open.rs),
// two `modeltap` processes can concurrently read the cache without one
// blocking the other. Process A writes one row in a cold-start launch,
// then process B and C launch back-to-back against the same cache file.
// Both exit 0 — neither's stderr/stdout surfaces `SQLITE_BUSY` because
// WAL allows concurrent readers without contention.
#[test]
fn two_modeltap_processes_can_read_the_cache_concurrently_via_sqlite_wal() {
    let mut world = ConcurrentWorld::new();

    // Given the cache file does not exist
    given_the_cache_file_does_not_exist(&world);
    // Given the TestTool will discover one model at the fixture path
    given_the_test_tool_will_discover_one_model(&world);
    // Given process A has written an initial cache (cold-start seed)
    given_process_a_has_written_an_initial_cache(&mut world);

    // When two modeltap processes B and C launch concurrently against the
    // same cache file
    when_two_modeltap_processes_launch_concurrently(&mut world);

    // Then both processes exit 0
    then_both_processes_exit_zero(&world);
    // And neither process surfaces SQLITE_BUSY in its output
    then_neither_process_emits_sqlite_busy(&world);
}

// ---------------------------------------------------------------------------
// Scenario 2: "Concurrent cache writes serialise via busy_timeout"
// ---------------------------------------------------------------------------
//
// AC-23-10 write-serialization invariant: with `busy_timeout=5000` set at
// Cache::open, two concurrent writers serialize via SQLite's built-in
// busy-wait — no `SQLITE_BUSY` is ever surfaced to the caller. Process A
// launches with `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS=2000` so its
// reconcile_tool transaction sleeps 2 s BEFORE COMMIT (test seam,
// cfg-gated; release builds never read the env var per R3 / OQ-3).
// Process B launches ~100 ms after A and contests for the write lock.
// Both processes exit 0, process B's `launch.log` carries a
// `cache.write_wait_ms` event with `wait_ms` in `[0, 5000]`, and
// `cache_tools.last_scan_at` reflects process B's later write.
#[test]
fn concurrent_cache_writes_serialise_via_busy_timeout() {
    let mut world = ConcurrentWorld::new();

    given_the_cache_file_does_not_exist(&world);
    given_the_test_tool_will_discover_one_model(&world);
    // Seed process A's row first so the post-B comparison has a
    // `last_scan_at` baseline to advance past.
    given_process_a_has_written_an_initial_cache(&mut world);

    // When process A launches holding the write lock for 2 seconds and
    // process B launches 100 ms later
    when_process_a_holds_write_lock_then_b_contests(&mut world);

    // Then process A and B both exit 0
    then_process_a_and_b_both_exit_zero(&world);
    // And process B's launch.log contains a cache.write_wait_ms event with
    // 0 <= wait_ms <= 5000
    let wait_ms = then_process_b_emits_cache_write_wait_event(&world);
    eprintln!(
        "cache_concurrent: process B observed cache.write_wait_ms = {wait_ms} ms \
         (busy_timeout cap = {BUSY_TIMEOUT_MS} ms)"
    );
    // And cache_tools.last_scan_at for test-tool reflects process B's later
    // write
    then_last_scan_at_reflects_process_b_write(&world);
}
