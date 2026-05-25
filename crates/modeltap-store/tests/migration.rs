//! Migration + open + repository round-trip tests for modeltap-store.
//!
//! Step 01-02 (tool-model-info-sqlite-cache). These tests exercise the crate
//! against the data-models.md schema spec and ADR-015 §"Schema versioning",
//! §"Concurrency" requirements.
//!
//! The five scenarios are named verbatim per the step roadmap:
//!
//! 1. Fresh DB: user_version=0 migration runs and lands user_version=1
//! 2. Already-migrated DB: re-open is a no-op
//! 3. PRAGMA journal_mode=WAL and busy_timeout=5000 set at open
//! 4. ToolsRepo round-trips a CachedTool through write_tool + tools()
//! 5. ModelsRepo round-trips a CachedModel through write_models + models_for_tool

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use modeltap_core::types::ToolId;
use modeltap_store::types::{CachedModel, CachedTool, SearchPathEntry, SearchPathSource};
use modeltap_store::{Cache, CacheOpenResult, EXPECTED_SCHEMA_VERSION};

/// Build a fresh tempfile cache path. The file does NOT exist on disk yet.
fn fresh_cache_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cache.sqlite");
    (dir, path)
}

fn sample_tool() -> CachedTool {
    CachedTool {
        tool_id: ToolId("ollama"),
        install_path: PathBuf::from("/home/devon/.ollama"),
        detected_version: Some("0.1.50".to_string()),
        plugin_version: "0.2.6".to_string(),
        model_count: 3,
        disk_usage_bytes: 15_000_000_000,
        largest_model_id: Some("mistral:7b-q4_K_M".to_string()),
        last_scan_at: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        last_scan_duration_ms: 850,
        last_error: None,
        last_error_at: None,
        search_paths: vec![SearchPathEntry {
            path: PathBuf::from("/home/devon/.ollama/models"),
            source: SearchPathSource::Default,
        }],
    }
}

fn sample_model() -> CachedModel {
    let mut metadata_kv = BTreeMap::new();
    metadata_kv.insert("general.architecture".to_string(), "llama".to_string());
    metadata_kv.insert("general.quantization_version".to_string(), "2".to_string());

    CachedModel {
        model_id: "mistral:7b-instruct-q4_K_M".to_string(),
        tool_id: ToolId("ollama"),
        display_name: "Mistral 7B Instruct Q4_K_M".to_string(),
        format: Some("GGUF v3".to_string()),
        quantisation: Some("Q4_K_M".to_string()),
        size_bytes: 4_368_438_912,
        sha256: Some(
            "e8a35b5e2f4f4e7a1c8f6b9d3c1a5e7f9a2c4e6b8d0f1a3c5e7b9d1f3a5c7e9b".to_string(),
        ),
        architecture: Some("llama".to_string()),
        parameters_billions: Some(7.24),
        context_length: Some(32_768),
        dedup_group_id: None,
        metadata_kv,
        metadata_introspected_at: Some(UNIX_EPOCH + Duration::from_secs(1_700_000_500)),
        last_seen_at: UNIX_EPOCH + Duration::from_secs(1_700_001_000),
        last_validated_at: None,
    }
}

/// Helper: open a path-backed cache and panic on error.
fn open_path(path: &Path) -> CacheOpenResult {
    Cache::open(path).expect("Cache::open should succeed on fresh path")
}

#[test]
fn expected_schema_version_constant_is_one() {
    assert_eq!(EXPECTED_SCHEMA_VERSION, 1);
}

#[test]
fn fresh_db_user_version_0_migration_runs_and_lands_user_version_1() {
    let (_dir, path) = fresh_cache_path();
    assert!(
        !path.exists(),
        "precondition: cache file must not exist yet"
    );

    let result = open_path(&path);
    let cache = match result {
        CacheOpenResult::OpenedFresh(c) => c,
        other => panic!("expected OpenedFresh on a non-existent path, got {other:?}"),
    };

    assert_eq!(
        cache.user_version().expect("user_version query"),
        EXPECTED_SCHEMA_VERSION,
        "PRAGMA user_version must equal 1 after fresh-DB migration"
    );
    assert!(
        path.exists(),
        "Cache::open on a non-existent path must create the file"
    );
}

#[test]
fn already_migrated_db_re_open_is_a_no_op() {
    let (_dir, path) = fresh_cache_path();

    // First open: fresh migration runs.
    {
        let first = open_path(&path);
        assert!(matches!(first, CacheOpenResult::OpenedFresh(_)));
    }

    // Second open on the same file should be idempotent — schema is already at
    // EXPECTED_SCHEMA_VERSION so the migrator is a no-op.
    let second = open_path(&path);
    match second {
        CacheOpenResult::OpenedExisting(cache) => {
            assert_eq!(cache.user_version().expect("user_version"), 1);
        }
        other => panic!("expected OpenedExisting on re-open, got {other:?}"),
    }
}

#[test]
fn pragma_journal_mode_wal_and_busy_timeout_5000_set_at_open() {
    // Use an in-memory cache: open_in_memory must set the same PRAGMAs that
    // open(path) does, otherwise unit tests cannot rely on identical behavior.
    let cache = Cache::open_in_memory().expect("open_in_memory");

    // SQLite reports journal_mode as a TEXT result. For an in-memory database
    // WAL is silently downgraded to "memory" — that's a SQLite implementation
    // detail. The PRAGMA was issued either way.
    //
    // For a path-backed cache the result is "wal".
    let (_dir, path) = fresh_cache_path();
    let result = open_path(&path);
    let real_cache = match result {
        CacheOpenResult::OpenedFresh(c) => c,
        other => panic!("expected OpenedFresh, got {other:?}"),
    };

    let journal_mode = real_cache.pragma_journal_mode().expect("pragma read");
    assert_eq!(
        journal_mode.to_ascii_lowercase(),
        "wal",
        "PRAGMA journal_mode must be 'wal' on a path-backed cache (AC-23-2)"
    );

    let busy_timeout = real_cache.pragma_busy_timeout().expect("pragma read");
    assert_eq!(
        busy_timeout, 5000,
        "PRAGMA busy_timeout must be 5000 ms (AC-23-2)"
    );

    // Sanity: in-memory cache also exposes the busy_timeout setting (the WAL
    // claim is moot for :memory: per SQLite docs).
    let busy_in_mem = cache.pragma_busy_timeout().expect("pragma read");
    assert_eq!(busy_in_mem, 5000);
}

#[test]
fn tools_repo_round_trips_a_cached_tool_through_write_tool_and_tools() {
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let tool = sample_tool();

    cache.write_tool(&tool).expect("write_tool");

    let rows = cache.tools().expect("tools()");
    assert_eq!(
        rows.len(),
        1,
        "exactly one tool row expected after one write"
    );
    assert_eq!(rows[0], tool, "round-trip must be field-identical");
}

#[test]
fn models_repo_round_trips_a_cached_model_through_write_models_and_models_for_tool() {
    let cache = Cache::open_in_memory().expect("open_in_memory");

    // Owning tool must exist first because cache_models has a FK to cache_tools.
    let tool = sample_tool();
    cache.write_tool(&tool).expect("write_tool");

    let model = sample_model();
    cache
        .write_models(&tool.tool_id, std::slice::from_ref(&model))
        .expect("write_models");

    let rows = cache
        .models_for_tool(&tool.tool_id)
        .expect("models_for_tool");
    assert_eq!(
        rows.len(),
        1,
        "exactly one model row expected after one write"
    );
    assert_eq!(rows[0], model, "round-trip must be field-identical");

    // Bonus: a different tool_id returns an empty list (FK isolation sanity).
    let other = ToolId("hf");
    let empty = cache.models_for_tool(&other).expect("models_for_tool");
    assert!(
        empty.is_empty(),
        "models_for_tool on an unknown tool must be empty"
    );

    // Avoid unused variable lint for `SystemTime` import when reading fields.
    let _ = SystemTime::now();
}

/// Step 06-02 mutation-kill — a CachedModel with an EMPTY metadata_kv map
/// must round-trip cleanly. The hydrate path at `models.rs:163` has a guard
/// `Some(s) if !s.is_empty()` which decides whether to invoke
/// `serde_json::from_str` or fall through to `BTreeMap::new()`. The
/// `with true` mutation would force a JSON parse on potentially-empty data
/// (failure on empty string); the `with false` mutation would skip parsing
/// for every Some(_) variant, losing real data — this test pins the
/// empty-map case.
#[test]
fn models_round_trip_preserves_empty_metadata_kv_through_hydrate_guard() {
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let tool = sample_tool();
    cache.write_tool(&tool).expect("write_tool");

    let model_with_empty_metadata = CachedModel {
        metadata_kv: BTreeMap::new(),
        ..sample_model()
    };
    cache
        .write_models(
            &tool.tool_id,
            std::slice::from_ref(&model_with_empty_metadata),
        )
        .expect("write_models");

    let rows = cache
        .models_for_tool(&tool.tool_id)
        .expect("models_for_tool");
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].metadata_kv.is_empty(),
        "empty metadata_kv must round-trip through the hydrate guard cleanly"
    );
}
