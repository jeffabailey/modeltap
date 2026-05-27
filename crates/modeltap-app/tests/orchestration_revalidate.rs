//! Unit tests for `orchestration::revalidate::pre_mutate` (Step 05-02 part 2/2).
//!
//! Part 1 (commit b12223a) landed `Cache::verify_against_fs` in modeltap-store
//! and exhaustively covered Match/Drift/Gone via real `std::fs::metadata`
//! against tempdir fixtures. This file covers the ORCHESTRATOR-side wrapper:
//!
//! - Outcome mapping: every `ValidationResult` variant becomes the right
//!   `PreMutateOutcome` variant (Match -> Proceed, Drift -> Drift{fresh, cached},
//!   Gone -> Gone), and `CacheError` becomes `StoreError`.
//! - JSONL emission shape: every call appends exactly one `revalidate.invoked`
//!   line to `<log_dir>/launch.log` with schema `modeltap.launch.v1` and the
//!   four required fields (tool, model, outcome, duration_ms).
//!
//! Wired into the four destructive entry points
//! (actions::{unify,zap,delete_one,folder_delete}::run); end-to-end exercise
//! lands in step 05-04 cucumber. This dispatch verifies the orchestrator
//! contract via direct invocation.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use modeltap_app::orchestration::revalidate::{self, PreMutateOutcome};
use modeltap_core::types::ToolId;
use modeltap_store::types::{CachedFile, CachedModel, CachedTool};
use modeltap_store::{Cache, CacheOpenResult};

const TEST_TOOL_ID: ToolId = ToolId("test-tool");

fn fresh_cache() -> (tempfile::TempDir, Cache) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cache.sqlite");
    let cache = match Cache::open(&path).expect("open") {
        CacheOpenResult::OpenedFresh(c) => c,
        other => panic!("expected OpenedFresh, got {other:?}"),
    };
    (dir, cache)
}

fn stat_quad(path: &Path) -> (u64, SystemTime, u64, u64) {
    let meta = std::fs::metadata(path).expect("stat seed file");
    (
        meta.len(),
        meta.modified().expect("modified"),
        meta.ino(),
        meta.dev(),
    )
}

/// Seed the parent `cache_tools` + `cache_models` rows so a subsequent
/// `write_model_files` call satisfies the composite FK
/// (`cache_model_files.(model_id, tool_id) REFERENCES cache_models`). The
/// store crate sets `PRAGMA foreign_keys = ON` per connection in
/// `Cache::open` (see `crates/modeltap-store/src/open.rs:222`), so without
/// these parent rows SQLite rejects the file insert with extended_code 787
/// — "FOREIGN KEY constraint failed".
fn seed_parent_rows(cache: &Cache, model_id: &str) {
    cache
        .write_tool(&CachedTool {
            tool_id: TEST_TOOL_ID,
            install_path: std::path::PathBuf::from("/test-tool"),
            detected_version: None,
            plugin_version: "0.0.0".to_string(),
            model_count: 1,
            disk_usage_bytes: 0,
            largest_model_id: None,
            last_scan_at: SystemTime::now(),
            last_scan_duration_ms: 0,
            last_error: None,
            last_error_at: None,
            search_paths: Vec::new(),
        })
        .expect("write_tool");
    cache
        .write_models(
            &TEST_TOOL_ID,
            &[CachedModel {
                model_id: model_id.to_string(),
                tool_id: TEST_TOOL_ID,
                display_name: model_id.to_string(),
                format: None,
                quantisation: None,
                size_bytes: 0,
                sha256: None,
                architecture: None,
                parameters_billions: None,
                context_length: None,
                dedup_group_id: None,
                metadata_kv: BTreeMap::new(),
                metadata_introspected_at: None,
                last_seen_at: SystemTime::now(),
                last_validated_at: None,
            }],
        )
        .expect("write_models");
}

fn seed_matching_file(cache: &Cache, model_id: &str, file_path: &Path) {
    seed_parent_rows(cache, model_id);
    let (size, mtime, inode, dev) = stat_quad(file_path);
    cache
        .write_model_files(&[CachedFile {
            model_id: model_id.to_string(),
            tool_id: TEST_TOOL_ID,
            path: file_path.to_path_buf(),
            size_bytes: size,
            mtime,
            inode,
            dev,
            last_stat_at: SystemTime::now(),
        }])
        .expect("write_model_files");
}

/// Read every line of `<log_dir>/launch.log` and return the parsed JSON
/// objects for the `revalidate.invoked` event only.
fn read_revalidate_events(log_dir: &Path) -> Vec<Value> {
    let path = log_dir.join("launch.log");
    if !path.exists() {
        return Vec::new();
    }
    let text = std::fs::read_to_string(&path).expect("read launch.log");
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("parse jsonl"))
        .filter(|v| v.get("event").and_then(|e| e.as_str()) == Some("revalidate.invoked"))
        .collect()
}

// ---------------------------------------------------------------------------
// Outcome mapping — one behavior per ValidationResult variant.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pre_mutate_returns_proceed_when_cache_matches_fs() {
    let (dir, cache) = fresh_cache();
    let file = dir.path().join("model.gguf");
    std::fs::write(&file, b"hello").expect("write seed");
    seed_matching_file(&cache, "m1", &file);

    let log_dir = tempfile::tempdir().expect("log tempdir");
    let outcome = revalidate::pre_mutate(
        &cache,
        &TEST_TOOL_ID,
        &"m1".to_string(),
        Some(log_dir.path()),
    )
    .await;

    assert!(
        matches!(outcome, PreMutateOutcome::Proceed),
        "matching quad must yield Proceed, got {outcome:?}"
    );
}

#[tokio::test]
async fn pre_mutate_returns_drift_when_mtime_diverges() {
    let (dir, cache) = fresh_cache();
    let file = dir.path().join("model.gguf");
    std::fs::write(&file, b"hello").expect("write seed");
    let (size, on_disk_mtime, inode, dev) = stat_quad(&file);
    // Seed with a stale mtime (1h before on-disk).
    let stale = on_disk_mtime - Duration::from_secs(3600);
    seed_parent_rows(&cache, "m1");
    cache
        .write_model_files(&[CachedFile {
            model_id: "m1".to_string(),
            tool_id: TEST_TOOL_ID,
            path: file.clone(),
            size_bytes: size,
            mtime: stale,
            inode,
            dev,
            last_stat_at: SystemTime::now(),
        }])
        .expect("seed stale row");

    let log_dir = tempfile::tempdir().expect("log tempdir");
    let outcome = revalidate::pre_mutate(
        &cache,
        &TEST_TOOL_ID,
        &"m1".to_string(),
        Some(log_dir.path()),
    )
    .await;

    match outcome {
        PreMutateOutcome::Drift { fresh, cached } => {
            // Fresh quad reflects on-disk state — the orchestrator uses this
            // to refresh the cache row before re-prompting.
            assert_eq!(fresh.mtime, on_disk_mtime, "fresh mtime mirrors on-disk");
            assert_eq!(fresh.size_bytes, size);
            assert_eq!(fresh.inode, inode);
            assert_eq!(fresh.dev, dev);
            // Cached quad reflects the (stale) seeded row — the orchestrator
            // surfaces both so a UX layer can render "was X, now Y".
            assert_eq!(cached.mtime, stale, "cached mtime mirrors seeded row");
            assert_eq!(cached.size_bytes, size);
        }
        other => panic!("expected Drift, got {other:?}"),
    }
}

#[tokio::test]
async fn pre_mutate_returns_gone_when_file_absent() {
    let (dir, cache) = fresh_cache();
    let absent = dir.path().join("never-existed.gguf");
    seed_parent_rows(&cache, "m1");
    cache
        .write_model_files(&[CachedFile {
            model_id: "m1".to_string(),
            tool_id: TEST_TOOL_ID,
            path: absent.clone(),
            size_bytes: 42,
            mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            inode: 999,
            dev: 1,
            last_stat_at: SystemTime::now(),
        }])
        .expect("seed gone row");

    let log_dir = tempfile::tempdir().expect("log tempdir");
    let outcome = revalidate::pre_mutate(
        &cache,
        &TEST_TOOL_ID,
        &"m1".to_string(),
        Some(log_dir.path()),
    )
    .await;

    assert!(
        matches!(outcome, PreMutateOutcome::Gone),
        "missing file must yield Gone, got {outcome:?}"
    );
}

// ---------------------------------------------------------------------------
// Empty-rows guard — no rows means no cached state to be stale against, so
// the orchestrator's K5 gate must NOT block. Mirrors the store-side
// "absent rows = Match" rationale documented in revalidate.rs.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pre_mutate_returns_proceed_when_model_has_no_cached_files() {
    let (_dir, cache) = fresh_cache();
    let log_dir = tempfile::tempdir().expect("log tempdir");
    let outcome = revalidate::pre_mutate(
        &cache,
        &TEST_TOOL_ID,
        &"unknown-model".to_string(),
        Some(log_dir.path()),
    )
    .await;

    assert!(
        matches!(outcome, PreMutateOutcome::Proceed),
        "zero cache_model_files rows must yield Proceed (no cached state \
         to be stale against), got {outcome:?}"
    );
}

// ---------------------------------------------------------------------------
// JSONL emission shape — every call writes one revalidate.invoked line.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pre_mutate_emits_one_revalidate_invoked_line_per_call() {
    let (dir, cache) = fresh_cache();
    let file = dir.path().join("model.gguf");
    std::fs::write(&file, b"hello").expect("write seed");
    seed_matching_file(&cache, "m1", &file);

    let log_dir = tempfile::tempdir().expect("log tempdir");

    // Three sequential calls = three lines.
    for _ in 0..3 {
        let _ = revalidate::pre_mutate(
            &cache,
            &TEST_TOOL_ID,
            &"m1".to_string(),
            Some(log_dir.path()),
        )
        .await;
    }

    let events = read_revalidate_events(log_dir.path());
    assert_eq!(
        events.len(),
        3,
        "three pre_mutate calls must produce three revalidate.invoked lines"
    );
}

#[tokio::test]
async fn pre_mutate_event_carries_schema_tool_model_outcome_and_duration_ms() {
    let (dir, cache) = fresh_cache();
    let file = dir.path().join("model.gguf");
    std::fs::write(&file, b"hello").expect("write seed");
    seed_matching_file(&cache, "m1", &file);

    let log_dir = tempfile::tempdir().expect("log tempdir");
    let _ = revalidate::pre_mutate(
        &cache,
        &TEST_TOOL_ID,
        &"m1".to_string(),
        Some(log_dir.path()),
    )
    .await;

    let events = read_revalidate_events(log_dir.path());
    assert_eq!(events.len(), 1, "exactly one event per call");
    let ev = &events[0];

    assert_eq!(
        ev.get("schema").and_then(|s| s.as_str()),
        Some("modeltap.launch.v1"),
        "schema must be modeltap.launch.v1"
    );
    assert_eq!(
        ev.get("event").and_then(|s| s.as_str()),
        Some("revalidate.invoked")
    );
    assert_eq!(
        ev.get("tool").and_then(|s| s.as_str()),
        Some(TEST_TOOL_ID.0),
        "tool field must echo the tool_id"
    );
    assert_eq!(
        ev.get("model").and_then(|s| s.as_str()),
        Some("m1"),
        "model field must echo the model_id"
    );
    assert_eq!(
        ev.get("outcome").and_then(|s| s.as_str()),
        Some("proceed"),
        "Match -> outcome=proceed"
    );
    assert!(
        ev.get("duration_ms").and_then(|d| d.as_u64()).is_some(),
        "duration_ms must be a u64 (got {:?})",
        ev.get("duration_ms")
    );
}

#[tokio::test]
async fn pre_mutate_outcome_field_distinguishes_proceed_drift_gone() {
    // Three independent caches/fixtures, one per outcome, into the same
    // log dir so we can read back three lines and assert each carries the
    // correct outcome string.
    let log_dir = tempfile::tempdir().expect("log tempdir");

    // Proceed
    {
        let (dir, cache) = fresh_cache();
        let file = dir.path().join("proceed.gguf");
        std::fs::write(&file, b"x").unwrap();
        seed_matching_file(&cache, "mp", &file);
        let _ = revalidate::pre_mutate(
            &cache,
            &TEST_TOOL_ID,
            &"mp".to_string(),
            Some(log_dir.path()),
        )
        .await;
    }

    // Drift
    {
        let (dir, cache) = fresh_cache();
        let file = dir.path().join("drift.gguf");
        std::fs::write(&file, b"x").unwrap();
        let (size, mtime, inode, dev) = stat_quad(&file);
        seed_parent_rows(&cache, "md");
        cache
            .write_model_files(&[CachedFile {
                model_id: "md".to_string(),
                tool_id: TEST_TOOL_ID,
                path: file.clone(),
                size_bytes: size,
                mtime: mtime - Duration::from_secs(3600),
                inode,
                dev,
                last_stat_at: SystemTime::now(),
            }])
            .expect("seed drift");
        let _ = revalidate::pre_mutate(
            &cache,
            &TEST_TOOL_ID,
            &"md".to_string(),
            Some(log_dir.path()),
        )
        .await;
    }

    // Gone
    {
        let (dir, cache) = fresh_cache();
        let absent = dir.path().join("absent.gguf");
        seed_parent_rows(&cache, "mg");
        cache
            .write_model_files(&[CachedFile {
                model_id: "mg".to_string(),
                tool_id: TEST_TOOL_ID,
                path: absent,
                size_bytes: 1,
                mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
                inode: 1,
                dev: 1,
                last_stat_at: SystemTime::now(),
            }])
            .expect("seed gone");
        let _ = revalidate::pre_mutate(
            &cache,
            &TEST_TOOL_ID,
            &"mg".to_string(),
            Some(log_dir.path()),
        )
        .await;
    }

    let events = read_revalidate_events(log_dir.path());
    assert_eq!(events.len(), 3, "three calls, three events");

    let outcomes: Vec<&str> = events
        .iter()
        .map(|ev| ev.get("outcome").and_then(|o| o.as_str()).unwrap_or(""))
        .collect();
    assert!(
        outcomes.contains(&"proceed"),
        "must carry outcome=proceed for Match"
    );
    assert!(
        outcomes.contains(&"drift"),
        "must carry outcome=drift for Drift"
    );
    assert!(
        outcomes.contains(&"gone"),
        "must carry outcome=gone for Gone"
    );
}

#[tokio::test]
async fn pre_mutate_log_dir_none_is_silent_noop_no_panic() {
    // Per kpi-instrumentation §"Privacy" + AC-7: an absent / unwritable log
    // dir must NEVER block the destructive flow. pre_mutate with log_dir=None
    // must execute the revalidation and return the outcome without emitting
    // anything anywhere.
    let (dir, cache) = fresh_cache();
    let file = dir.path().join("model.gguf");
    std::fs::write(&file, b"x").unwrap();
    seed_matching_file(&cache, "m1", &file);

    let outcome = revalidate::pre_mutate(&cache, &TEST_TOOL_ID, &"m1".to_string(), None).await;
    assert!(
        matches!(outcome, PreMutateOutcome::Proceed),
        "log_dir=None must still revalidate; got {outcome:?}"
    );
}
