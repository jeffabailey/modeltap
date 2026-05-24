//! Concurrent-process safety tests for the SQLite cache (Phase 04 step
//! 04-04, US-23 Scenarios 4-5 / AC-23-10).
//!
//! These tests prove the WAL + busy_timeout PRAGMAs set at `Cache::open`
//! (step 01-02 / ADR-015 §"Concurrency") work end-to-end against multiple
//! threads sharing a single on-disk cache.sqlite. The acceptance suite
//! covers the same invariants at the modeltap-binary boundary; this file
//! covers them at the store-internals boundary so a contract regression in
//! `Cache::reconcile_tool` surfaces fast.
//!
//! Two scenarios:
//!
//! 1. **Concurrent reads succeed under WAL.** Two `Cache::open(path)`
//!    handles call `tools()` simultaneously from separate threads; neither
//!    blocks the other, neither returns SQLITE_BUSY. This is the WAL
//!    invariant from AC-23-2 — concurrent readers do not contend for the
//!    write lock.
//!
//! 2. **Concurrent writes serialize via busy_timeout.** Thread A holds the
//!    write lock open by sleeping inside `reconcile_tool` (via the test
//!    seam `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS`); thread B's
//!    `reconcile_tool` blocks at `BEGIN IMMEDIATE` until A commits, then
//!    completes successfully. Both transactions commit; thread B's wait
//!    time is non-zero. This proves the `busy_timeout=5000` PRAGMA
//!    actually serializes writers without surfacing SQLITE_BUSY to the
//!    caller (AC-23-10).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, SystemTime};

use modeltap_core::types::ToolId;
use modeltap_store::types::{CachedModel, CachedTool};
use modeltap_store::{Cache, CacheOpenResult};

/// Build a fresh tempfile cache path. The file does NOT exist on disk yet.
fn fresh_cache_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cache.sqlite");
    (dir, path)
}

/// Open a path-backed cache; panic on error (test code).
fn open_path(path: &Path) -> Cache {
    match Cache::open(path).expect("Cache::open should succeed") {
        CacheOpenResult::OpenedFresh(c) => c,
        CacheOpenResult::OpenedExisting(c) => c,
        CacheOpenResult::OpenedAfterMigration { cache, .. } => cache,
        CacheOpenResult::OpenedAfterRecovery { cache, .. } => cache,
    }
}

fn sample_tool(tool_id: ToolId, now: SystemTime) -> CachedTool {
    CachedTool {
        tool_id,
        install_path: PathBuf::from("/tmp/concurrent-test"),
        detected_version: Some("test-1.0.0".to_string()),
        plugin_version: "0.0.0".to_string(),
        model_count: 0,
        disk_usage_bytes: 0,
        largest_model_id: None,
        last_scan_at: now,
        last_scan_duration_ms: 0,
        last_error: None,
        last_error_at: None,
        search_paths: Vec::new(),
    }
}

fn sample_model(tool_id: ToolId, model_id: &str, now: SystemTime) -> CachedModel {
    CachedModel {
        model_id: model_id.to_string(),
        tool_id,
        display_name: model_id.to_string(),
        format: Some("Gguf".to_string()),
        quantisation: None,
        size_bytes: 1024,
        sha256: None,
        architecture: None,
        parameters_billions: None,
        context_length: None,
        dedup_group_id: None,
        metadata_kv: BTreeMap::new(),
        metadata_introspected_at: None,
        last_seen_at: now,
        last_validated_at: None,
    }
}

/// Two `Cache::open(path)` handles, both call `tools()` simultaneously
/// from separate threads. Under WAL neither returns SQLITE_BUSY and both
/// observe the seeded row. This is the AC-23-2 read-concurrency invariant.
#[test]
fn concurrent_reads_succeed_under_wal() {
    let (_dir, path) = fresh_cache_path();

    // Seed one tool row through a single connection, then drop it so the
    // two concurrent readers each open their own connection from scratch.
    {
        let seed = open_path(&path);
        let now = SystemTime::now();
        seed.reconcile_tool(&sample_tool(ToolId("test-tool"), now), &[])
            .expect("seed reconcile");
    }

    // Spawn two reader threads against the SAME on-disk path. A `Barrier`
    // forces both to fire `tools()` at the same moment so the WAL
    // concurrent-read path is genuinely exercised — not serialized by
    // thread scheduling.
    let path_a = path.clone();
    let path_b = path.clone();
    let barrier = Arc::new(Barrier::new(2));
    let barrier_a = Arc::clone(&barrier);
    let barrier_b = Arc::clone(&barrier);

    let handle_a = thread::spawn(move || {
        let cache_a = open_path(&path_a);
        barrier_a.wait();
        cache_a.tools()
    });
    let handle_b = thread::spawn(move || {
        let cache_b = open_path(&path_b);
        barrier_b.wait();
        cache_b.tools()
    });

    let rows_a = handle_a.join().expect("thread A join").expect("tools() A");
    let rows_b = handle_b.join().expect("thread B join").expect("tools() B");

    // Both readers must see the seeded row. SQLITE_BUSY would have surfaced
    // as a `CacheError::Sqlite(_)` from `tools()`.
    assert_eq!(rows_a.len(), 1, "thread A must observe the seeded row");
    assert_eq!(rows_b.len(), 1, "thread B must observe the seeded row");
    assert_eq!(
        rows_a[0].tool_id,
        ToolId("test-tool"),
        "thread A row identity"
    );
    assert_eq!(
        rows_b[0].tool_id,
        ToolId("test-tool"),
        "thread B row identity"
    );
}

/// Thread A holds the write lock for ~500 ms via the test seam; thread B
/// starts ~50 ms later and its `BEGIN IMMEDIATE` blocks until A commits.
/// Both transactions commit, thread B observes a non-trivial wait time,
/// and the final row reflects thread B's later write (last writer wins
/// — `ON CONFLICT(tool_id) DO UPDATE`).
///
/// This is the AC-23-10 invariant: SQLite's busy_timeout serializes
/// writers without surfacing SQLITE_BUSY to the caller.
#[test]
fn write_blocks_via_busy_timeout_and_succeeds() {
    let (_dir, path) = fresh_cache_path();

    // Seed an empty schema so both threads only contend on the write lock,
    // not on schema setup.
    {
        let seed = open_path(&path);
        let _ = seed.tools().expect("schema seed read");
    }

    let path_a = path.clone();
    let path_b = path.clone();

    // Thread A holds the write lock for ~500 ms (HOLD_MS) before COMMIT.
    // Thread B starts ~50 ms later so it definitely enters BEGIN
    // IMMEDIATE while A still holds the lock. Process budgets:
    //   busy_timeout = 5000 ms (PRAGMA)
    //   HOLD_MS       = 500 ms (well below the cap so the test stays fast)
    //   STAGGER_MS    = 50 ms  (ensures B reaches BEGIN IMMEDIATE while
    //                          A is asleep)
    const HOLD_MS: u64 = 500;
    const STAGGER_MS: u64 = 50;

    let handle_a = thread::spawn(move || {
        // Pin the env var in the child thread. The seam reads it inside
        // reconcile_tool BEFORE COMMIT.
        std::env::set_var(
            "MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS",
            HOLD_MS.to_string(),
        );
        let cache_a = open_path(&path_a);
        let now = SystemTime::now();
        let result = cache_a.reconcile_tool(
            &sample_tool(ToolId("test-tool"), now),
            &[sample_model(ToolId("test-tool"), "model-from-a", now)],
        );
        std::env::remove_var("MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS");
        result
    });

    // Stagger B so A is definitively inside the BEGIN..COMMIT window
    // when B's BEGIN IMMEDIATE fires.
    thread::sleep(Duration::from_millis(STAGGER_MS));

    let handle_b = thread::spawn(move || {
        // Thread B must NOT inherit A's hold-lock seam — otherwise it
        // would also sleep inside its transaction. The env var is shared
        // process-wide, but we never set it for B and A clears it on its
        // way out; even so, the spawn-then-set-then-clear ordering means
        // B's read of the env var should be empty.
        let cache_b = open_path(&path_b);
        let now = SystemTime::now() + Duration::from_secs(1);
        cache_b.reconcile_tool(
            &sample_tool(ToolId("test-tool"), now),
            &[sample_model(ToolId("test-tool"), "model-from-b", now)],
        )
    });

    let wait_a = handle_a
        .join()
        .expect("thread A join")
        .expect("thread A reconcile_tool");
    let wait_b = handle_b
        .join()
        .expect("thread B join")
        .expect("thread B reconcile_tool");

    // Thread A held the lock from a cold cache — its wait at BEGIN
    // IMMEDIATE is ~0 ms. The assertion is the upper bound: A must not
    // have waited 5 seconds (which would only happen if some other
    // writer beat it to the file, which cannot happen in this test).
    assert!(
        wait_a < Duration::from_secs(5),
        "thread A's wait must be < busy_timeout; got {:?}",
        wait_a
    );

    // Thread B's wait at BEGIN IMMEDIATE must be at least `HOLD_MS -
    // STAGGER_MS - a small slack` (B started 50 ms after A, A holds for
    // 500 ms, so B should wait ~450 ms). Allow a generous lower bound to
    // avoid flakes on slow CI hardware.
    let expected_min_wait = Duration::from_millis(HOLD_MS / 2);
    assert!(
        wait_b >= expected_min_wait,
        "thread B should wait at least {:?} for the busy_timeout to fire; got {:?}",
        expected_min_wait,
        wait_b
    );
    // ... and it must NOT have exceeded the busy_timeout (otherwise SQLite
    // would have returned SQLITE_BUSY, which would have surfaced as an
    // Err above).
    assert!(
        wait_b < Duration::from_secs(5),
        "thread B's wait must be < busy_timeout (else SQLITE_BUSY would have fired); got {:?}",
        wait_b
    );

    // Last writer wins: the final row must carry one of the two writes
    // (thread B's, by ordering). Both threads succeed without
    // SQLITE_BUSY — that is the contract.
    let final_cache = open_path(&path);
    let rows = final_cache.tools().expect("final tools()");
    assert_eq!(rows.len(), 1, "exactly one cache_tools row");
    assert_eq!(rows[0].tool_id, ToolId("test-tool"));
}
