//! Migration 0002 — adds the file-level `cache_sha256` table (US-27, Release 3).
//!
//! Per ADR-018 §"Release 3 (US-27, deferred — opt-in)": a new `cache_sha256`
//! table keyed at the file level (path-as-PK) storing
//! `(path, mtime_epoch_ns, size_bytes, inode, dev, content_hash, computed_at)`.
//! The migration is PURELY ADDITIVE — it must not touch `cache_models`,
//! `cache_tools`, or `cache_model_files` rows.
//!
//! Two scenarios:
//!   1. Fresh DB lands at user_version=2 with the cache_sha256 table present.
//!   2. A pre-existing v1 DB with populated cache_models migrates forward to
//!      v2 with ZERO data loss (OpenedAfterMigration { from: 1, to: 2 }).

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use modeltap_store::{Cache, CacheOpenResult, EXPECTED_SCHEMA_VERSION};

const MIGRATION_0001: &str = include_str!("../migrations/0001_initial.sql");

fn fresh_cache_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("cache.sqlite");
    (dir, path)
}

/// True iff a table named `table_name` exists in the SQLite file at `path`.
/// Opens its own read-only connection so it does not contend with `Cache`.
fn table_exists(path: &Path, table_name: &str) -> bool {
    let conn = Connection::open(path).expect("open for table check");
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table_name],
            |row| row.get(0),
        )
        .expect("query sqlite_master");
    count == 1
}

/// Seed a raw v1 database at `path`: apply 0001, set user_version=1, insert one
/// tool + one model. Returns the inserted model_id for the post-migration
/// data-preservation assertion. Uses rusqlite directly so the seed is pinned at
/// v1 regardless of the binary's EXPECTED_SCHEMA_VERSION.
fn seed_v1_db(path: &Path) -> String {
    let conn = Connection::open(path).expect("open raw v1");
    conn.execute_batch(MIGRATION_0001).expect("apply 0001");
    conn.pragma_update(None, "user_version", 1_i64)
        .expect("set user_version=1");

    conn.execute(
        "INSERT INTO cache_tools (
            tool_id, install_path, plugin_version, model_count,
            disk_usage_bytes, last_scan_at
         ) VALUES ('ollama', '/home/devon/.ollama', '0.2.6', 1, 4368438912, '2026-05-20T00:00:00.000Z')",
        [],
    )
    .expect("insert tool");

    let model_id = "mistral:7b-instruct-q4_K_M";
    conn.execute(
        "INSERT INTO cache_models (
            model_id, tool_id, display_name, size_bytes, sha256, last_seen_at
         ) VALUES (?1, 'ollama', 'Mistral 7B', 4368438912,
                   'e8a35b5e2f4f4e7a1c8f6b9d3c1a5e7f9a2c4e6b8d0f1a3c5e7b9d1f3a5c7e9b',
                   '2026-05-20T00:00:00.000Z')",
        [model_id],
    )
    .expect("insert model");

    model_id.to_string()
}

#[test]
fn expected_schema_version_is_two_after_0002() {
    assert_eq!(
        EXPECTED_SCHEMA_VERSION, 2,
        "migration 0002 must bump EXPECTED_SCHEMA_VERSION to 2"
    );
}

#[test]
fn fresh_db_lands_at_v2_with_cache_sha256_table() {
    let (_dir, path) = fresh_cache_path();
    let result = Cache::open(&path).expect("open fresh");
    let cache = match result {
        CacheOpenResult::OpenedFresh(c) => c,
        other => panic!("expected OpenedFresh on a non-existent path, got {other:?}"),
    };

    assert_eq!(
        cache.user_version().expect("user_version"),
        2,
        "fresh DB must land at user_version=2 after 0002"
    );
    assert!(
        table_exists(&path, "cache_sha256"),
        "0002 must create the cache_sha256 table"
    );
}

#[test]
fn v1_db_migrates_forward_to_v2_without_data_loss() {
    let (_dir, path) = fresh_cache_path();
    let model_id = seed_v1_db(&path);

    let result = Cache::open(&path).expect("open existing v1");
    let cache = match result {
        CacheOpenResult::OpenedAfterMigration { from, to, cache } => {
            assert_eq!(from, 1, "seeded DB starts at v1");
            assert_eq!(to, 2, "migrator rolls forward to v2");
            cache
        }
        other => panic!("expected OpenedAfterMigration {{ from: 1, to: 2 }}, got {other:?}"),
    };

    // The cache_sha256 table now exists...
    assert!(
        table_exists(&path, "cache_sha256"),
        "forward migration must add cache_sha256"
    );

    // ...and the pre-existing cache_models row survived the migration intact.
    let models = cache
        .models_for_tool(&modeltap_core::types::ToolId("ollama"))
        .expect("models_for_tool");
    assert_eq!(models.len(), 1, "the seeded model row must survive 0002");
    assert_eq!(models[0].model_id, model_id);
    assert_eq!(
        models[0].sha256.as_deref(),
        Some("e8a35b5e2f4f4e7a1c8f6b9d3c1a5e7f9a2c4e6b8d0f1a3c5e7b9d1f3a5c7e9b"),
        "the model-level sha256 (Tier 2) must be preserved across the migration"
    );
}
