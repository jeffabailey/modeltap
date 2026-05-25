//! Revalidator unit tests (step 05-02 — K5 pre-mutate safety mechanism).
//!
//! `Cache::verify_against_fs(model_id)` is THE single seam every mutation
//! orchestrator goes through; these tests cover the three outcomes that
//! drive the orchestrator's branch logic:
//!
//! 1. **Match**   — every file in `cache_model_files` re-`stat()`s to the
//!    cached `(mtime_epoch_ns, size_bytes, inode, dev)` quad. Proceed.
//! 2. **Drift**   — at least one file's quad disagrees. Refresh + re-confirm.
//! 3. **Gone**    — at least one file is missing. Abort + refresh inventory.
//!
//! Plus boundary cases:
//!
//! 4. Empty rows  — model with no `cache_model_files` rows returns Match
//!    (revalidator has no cache state to compare against; the orchestrator
//!    consults `cache_models` separately).
//! 5. Multi-file Match — every file in a 2-file model matches.
//! 6. Multi-file Drift — second file drifts while first matches; first
//!    drift wins (Match returned only when ALL files pass).
//! 7. `FileStat::matches` — pure comparison: identical quads match, any
//!    single field difference fails.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use modeltap_core::types::ToolId;
use modeltap_store::types::{CachedFile, CachedModel, CachedTool, FileStat, ValidationResult};
use modeltap_store::{Cache, CacheOpenResult};

const TEST_TOOL_ID: ToolId = ToolId("test-tool");

/// Build a fresh tempfile-backed cache; returns the dir handle (kept alive
/// for the test) and the open `Cache`.
///
/// **Step 06-02 fix** — also seeds a parent row in `cache_tools` so the FK
/// constraint on `cache_model_files.tool_id → cache_tools.tool_id` is
/// satisfied. The original fixture wrote `cache_model_files` rows without
/// seeding the tool / model parents, which started failing the moment the
/// FK PRAGMA went green (mutation testing revealed the silent green-bar
/// dependency).
fn fresh_cache() -> (tempfile::TempDir, Cache) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cache.sqlite");
    let cache = match Cache::open(&path).expect("open") {
        CacheOpenResult::OpenedFresh(c) => c,
        other => panic!("expected OpenedFresh, got {other:?}"),
    };
    // Seed parent cache_tools row so FK references on cache_model_files
    // succeed.
    cache
        .write_tool(&CachedTool {
            tool_id: TEST_TOOL_ID,
            install_path: PathBuf::from("/test-install"),
            detected_version: None,
            plugin_version: "test-plugin 0.0.0".to_string(),
            model_count: 0,
            disk_usage_bytes: 0,
            largest_model_id: None,
            last_scan_at: SystemTime::now(),
            last_scan_duration_ms: 0,
            last_error: None,
            last_error_at: None,
            search_paths: vec![],
        })
        .expect("seed parent cache_tools row");
    (dir, cache)
}

/// Seed a parent `cache_models` row for `model_id` so subsequent
/// `cache_model_files` writes satisfy the FK constraint. Idempotent via the
/// `ON CONFLICT(model_id, tool_id) DO UPDATE` semantics on `write_models`.
fn seed_parent_model(cache: &Cache, model_id: &str) {
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
        .expect("seed parent cache_models row");
}

/// Stat helper — returns (size, mtime, inode, dev) so the seed values
/// exactly match what `verify_against_fs` will re-read.
fn stat_quad(path: &Path) -> (u64, SystemTime, u64, u64) {
    let meta = std::fs::metadata(path).expect("stat seed file");
    (
        meta.len(),
        meta.modified().expect("modified"),
        meta.ino(),
        meta.dev(),
    )
}

/// Seed one `cache_model_files` row whose quad matches the on-disk file.
/// Idempotently seeds the parent `cache_models` row first so the FK
/// constraint is satisfied.
fn seed_matching_file(cache: &Cache, model_id: &str, file_path: &Path) {
    seed_parent_model(cache, model_id);
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

/// Behavior 1 — Match. A seeded file whose quad agrees with the on-disk
/// metadata returns `ValidationResult::Match`.
#[test]
fn verify_against_fs_returns_match_when_quad_agrees() {
    let (dir, cache) = fresh_cache();
    let file = dir.path().join("model.gguf");
    std::fs::write(&file, b"hello world").expect("write seed");
    seed_matching_file(&cache, "m1", &file);

    let result = cache.verify_against_fs(&"m1".to_string()).expect("verify");
    assert_eq!(
        result,
        ValidationResult::Match,
        "fresh quad must match cached quad"
    );
}

/// Behavior 2 — Drift. Mutating the file's mtime AFTER seeding makes the
/// quad disagree. The fresh `FileStat` is surfaced for the orchestrator's
/// downstream refresh.
#[test]
fn verify_against_fs_returns_drift_when_mtime_changes() {
    let (dir, cache) = fresh_cache();
    let file = dir.path().join("drifting.gguf");
    std::fs::write(&file, b"initial").expect("write seed");

    // Seed the cache row with an mtime SHIFTED into the past so the live
    // file is unambiguously newer. Using the on-disk mtime minus 1h
    // avoids relying on filetime mutation (which would add a dep).
    seed_parent_model(&cache, "m1");
    let (size, on_disk_mtime, inode, dev) = stat_quad(&file);
    let stale_mtime = on_disk_mtime - Duration::from_secs(3600);
    cache
        .write_model_files(&[CachedFile {
            model_id: "m1".to_string(),
            tool_id: TEST_TOOL_ID,
            path: file.clone(),
            size_bytes: size,
            mtime: stale_mtime,
            inode,
            dev,
            last_stat_at: SystemTime::now(),
        }])
        .expect("write_model_files");

    let result = cache.verify_against_fs(&"m1".to_string()).expect("verify");
    match result {
        ValidationResult::Drift { fresh } => {
            assert_eq!(fresh.size_bytes, size);
            assert_eq!(fresh.inode, inode);
            assert_eq!(fresh.dev, dev);
            assert_eq!(
                fresh.mtime, on_disk_mtime,
                "Drift must carry the fresh on-disk mtime so the orchestrator can refresh"
            );
        }
        other => panic!("expected Drift, got {other:?}"),
    }
}

/// Behavior 2b — Drift on size change. The quad has four elements; size
/// flipping alone must trigger Drift (proves the comparison covers all
/// four, not just mtime).
#[test]
fn verify_against_fs_returns_drift_when_size_changes() {
    let (dir, cache) = fresh_cache();
    let file = dir.path().join("size-drift.gguf");
    std::fs::write(&file, b"initial").expect("write seed");
    let (true_size, mtime, inode, dev) = stat_quad(&file);

    // Seed with WRONG size — everything else matches.
    seed_parent_model(&cache, "m1");
    cache
        .write_model_files(&[CachedFile {
            model_id: "m1".to_string(),
            tool_id: TEST_TOOL_ID,
            path: file.clone(),
            size_bytes: true_size + 999,
            mtime,
            inode,
            dev,
            last_stat_at: SystemTime::now(),
        }])
        .expect("write_model_files");

    let result = cache.verify_against_fs(&"m1".to_string()).expect("verify");
    assert!(
        matches!(result, ValidationResult::Drift { .. }),
        "size mismatch alone must trigger Drift; got {result:?}"
    );
}

/// Behavior 3 — Gone. A seeded row pointing at a path that does NOT exist
/// returns `ValidationResult::Gone`.
#[test]
fn verify_against_fs_returns_gone_when_file_missing() {
    let (dir, cache) = fresh_cache();
    let absent = dir.path().join("never-existed.gguf");
    // Synthesize plausible quad values — these never get read because the
    // stat fails first.
    seed_parent_model(&cache, "m1");
    cache
        .write_model_files(&[CachedFile {
            model_id: "m1".to_string(),
            tool_id: TEST_TOOL_ID,
            path: absent,
            size_bytes: 1024,
            mtime: SystemTime::now(),
            inode: 42,
            dev: 7,
            last_stat_at: SystemTime::now(),
        }])
        .expect("write_model_files");

    let result = cache.verify_against_fs(&"m1".to_string()).expect("verify");
    assert_eq!(
        result,
        ValidationResult::Gone,
        "missing file must surface as Gone, not Drift"
    );
}

/// Behavior 4 — Empty rows. A model_id with zero `cache_model_files` rows
/// returns Match (no cache state = nothing to be stale against). See the
/// module-level doc comment on `verify_against_fs` for the rationale.
#[test]
fn verify_against_fs_returns_match_when_no_rows_exist() {
    let (_dir, cache) = fresh_cache();
    let result = cache
        .verify_against_fs(&"never-seeded".to_string())
        .expect("verify");
    assert_eq!(result, ValidationResult::Match);
}

/// Behavior 5 — Multi-file Match. Two files for one model both match → Match.
#[test]
fn verify_against_fs_returns_match_for_multi_file_model_with_all_clean() {
    let (dir, cache) = fresh_cache();
    let file_a = dir.path().join("a.gguf");
    let file_b = dir.path().join("b.gguf");
    std::fs::write(&file_a, b"alpha").expect("write a");
    std::fs::write(&file_b, b"beta").expect("write b");
    seed_matching_file(&cache, "multi", &file_a);
    seed_matching_file(&cache, "multi", &file_b);

    let result = cache
        .verify_against_fs(&"multi".to_string())
        .expect("verify");
    assert_eq!(result, ValidationResult::Match);
}

/// Behavior 6 — Multi-file Drift wins. If the second file drifts while
/// the first matches, the loop must continue past the first and return
/// Drift on the second. Critical for the K5 invariant: a clean-but-stale
/// pair must never look like Match.
#[test]
fn verify_against_fs_returns_drift_when_any_file_drifts() {
    let (dir, cache) = fresh_cache();
    let file_a = dir.path().join("clean.gguf");
    let file_b = dir.path().join("dirty.gguf");
    std::fs::write(&file_a, b"alpha").expect("write a");
    std::fs::write(&file_b, b"beta").expect("write b");
    seed_matching_file(&cache, "mix", &file_a);

    // File B: seed with a stale mtime (mtime - 1h) so the second iteration
    // of the loop catches Drift.
    let (size_b, mtime_b, inode_b, dev_b) = stat_quad(&file_b);
    cache
        .write_model_files(&[CachedFile {
            model_id: "mix".to_string(),
            tool_id: TEST_TOOL_ID,
            path: file_b,
            size_bytes: size_b,
            mtime: mtime_b - Duration::from_secs(3600),
            inode: inode_b,
            dev: dev_b,
            last_stat_at: SystemTime::now(),
        }])
        .expect("write_model_files");

    let result = cache.verify_against_fs(&"mix".to_string()).expect("verify");
    assert!(
        matches!(result, ValidationResult::Drift { .. }),
        "second-file drift must propagate; got {result:?}"
    );
}

/// Behavior 7 — `FileStat::matches` is a pure comparator. Identical quads
/// match; any single field difference flips to false. Covers all four
/// fields parametrically (one assertion per field — five rows).
#[test]
fn file_stat_matches_is_pure_and_covers_all_four_fields() {
    let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let base = FileStat {
        size_bytes: 1024,
        mtime: now,
        inode: 42,
        dev: 7,
    };
    let same = base.clone();
    assert!(base.matches(&same), "identical quads must match");

    // Differ by size.
    let mut variant = base.clone();
    variant.size_bytes = 2048;
    assert!(!base.matches(&variant), "size delta breaks match");

    // Differ by mtime.
    let mut variant = base.clone();
    variant.mtime = now + Duration::from_secs(1);
    assert!(!base.matches(&variant), "mtime delta breaks match");

    // Differ by inode.
    let mut variant = base.clone();
    variant.inode = 99;
    assert!(!base.matches(&variant), "inode delta breaks match");

    // Differ by dev.
    let mut variant = base.clone();
    variant.dev = 8;
    assert!(!base.matches(&variant), "dev delta breaks match");
}

/// `FileStat::from(&CachedFile)` projects the four load-bearing fields,
/// dropping the row-only fields (`model_id`, `tool_id`, `path`,
/// `last_stat_at`). Proves the round-trip the revalidator uses.
#[test]
fn file_stat_from_cached_file_projects_the_quad() {
    let mtime = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let row = CachedFile {
        model_id: "m1".to_string(),
        tool_id: TEST_TOOL_ID,
        path: PathBuf::from("/tmp/x"),
        size_bytes: 4096,
        mtime,
        inode: 17,
        dev: 3,
        last_stat_at: SystemTime::now(),
    };
    let stat: FileStat = FileStat::from(&row);
    assert_eq!(stat.size_bytes, 4096);
    assert_eq!(stat.mtime, mtime);
    assert_eq!(stat.inode, 17);
    assert_eq!(stat.dev, 3);
}
