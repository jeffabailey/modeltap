//! Fixture builders and seam helpers for the `tool-model-info-sqlite-cache`
//! acceptance suite.
//!
//! Per acceptance-test-plan.md §1 CM-A and §3 (fixture strategy), this module
//! owns two cooperating pieces:
//!
//! 1. **`devon_cache_empty` fixture** — a per-scenario tempdir tree with the
//!    same shape as the parent's `devon-multi-tool` fixture (synthetic tool
//!    directories) plus a `xdg-data/modeltap/` directory the scenario points
//!    `MODELTAP_CACHE_PATH` into. NO `cache.sqlite` exists at construction
//!    time — the walking-skeleton's first process writes it. The fixture also
//!    pre-creates the TestTool's model directory and the synthetic
//!    `test-model-7b.gguf` file the TestTool's `discover()` returns.
//!
//! 2. **`CacheVerifier` CACHE seam helper** — an `@cache-introspection`-tagged
//!    test utility that opens the cache file via a READ-ONLY
//!    `rusqlite::Connection` so step-definition assertions can verify
//!    `PRAGMA user_version` and row counts without contending with the real
//!    `modeltap` process for the write lock. Per acceptance-test-plan.md §1
//!    CM-A: this is a test-utility, not a production import, and is gated
//!    to `[dev-dependencies]`.
//!
//! The CACHE seam is the only place the acceptance crate reads the SQLite
//! file directly; all other assertions go through the modeltap binary's
//! observable surface (stdout frame capture, JSONL log events).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

use crate::test_tool::{TEST_MODEL_FILENAME, TEST_TOOL_NAME};

/// Filesystem layout owned by one M1 walking-skeleton scenario. The TempDir
/// is retained so the directory survives until the test asserts; dropping
/// the fixture recursively removes the tree.
///
/// Layout (all paths relative to `temp.path()`):
///
/// ```text
/// <temp>/
///   xdg-data/
///     modeltap/                     <-- MODELTAP_CACHE_PATH parent dir
///   test-tool/
///     models/
///       test-model-7b.gguf          <-- TestTool::discover() returns this
///   logs/                           <-- MODELTAP_LOG_DIR
///   modeltap-home/                  <-- ~/.modeltap (diagnostics.log etc)
/// ```
///
/// Process A writes `<temp>/xdg-data/modeltap/cache.sqlite` via the warm-start
/// reconcile path; process B reads it via the same env var.
pub struct DevonCacheEmptyFixture {
    pub temp: TempDir,
}

impl DevonCacheEmptyFixture {
    /// Build a fresh fixture tree. Pre-writes the TestTool's synthetic model
    /// file so the first `modeltap` process's discovery has a non-zero
    /// `size_bytes` to record in the cache row.
    pub fn build() -> Self {
        let temp = TempDir::new().expect("create devon-cache-empty tempdir");
        // xdg-data/modeltap/ — the cache directory; cache.sqlite is absent.
        let xdg_modeltap = temp.path().join("xdg-data").join("modeltap");
        std::fs::create_dir_all(&xdg_modeltap).expect("create xdg-data/modeltap");
        // test-tool/models/<file>.gguf — the file TestTool::discover reports.
        let model_dir = temp.path().join("test-tool").join("models");
        std::fs::create_dir_all(&model_dir).expect("create test-tool/models");
        let model_path = model_dir.join(TEST_MODEL_FILENAME);
        std::fs::write(&model_path, b"synthetic-walking-skeleton-gguf-bytes")
            .expect("seed synthetic gguf");
        // logs/ — MODELTAP_LOG_DIR receives launch.log.
        let log_dir = temp.path().join("logs");
        std::fs::create_dir_all(&log_dir).expect("create logs/");
        // modeltap-home/ — diagnostics.log (not yet exercised in WS, but
        // matches acceptance-test-plan.md §3 fixture skeleton).
        let modeltap_home = temp.path().join("modeltap-home");
        std::fs::create_dir_all(&modeltap_home).expect("create modeltap-home/");
        Self { temp }
    }

    /// Absolute path to `<temp>/xdg-data/modeltap/cache.sqlite` — the value
    /// the scenario sets `MODELTAP_CACHE_PATH` to.
    pub fn cache_path(&self) -> PathBuf {
        self.temp
            .path()
            .join("xdg-data")
            .join("modeltap")
            .join("cache.sqlite")
    }

    /// Absolute path to `<temp>/test-tool` — the root the TestTool's
    /// `discover()` walks. The scenario sets `MODELTAP_TEST_TOOL_ROOT` to
    /// `<test_tool_root>/models` so the TestTool finds `test-model-7b.gguf`.
    pub fn test_tool_root(&self) -> PathBuf {
        self.temp.path().join("test-tool").join("models")
    }

    /// Absolute path to `<temp>/logs` — the value the scenario sets
    /// `MODELTAP_LOG_DIR` to. Process A and process B share this directory so
    /// the JSONL `launch.warm_paint_ms` event from process B is visible to
    /// the CACHE seam's caller.
    pub fn log_dir(&self) -> PathBuf {
        self.temp.path().join("logs")
    }
}

// ---------------------------------------------------------------------------
// CACHE seam helper — acceptance-test-plan.md §1 CM-A.
// ---------------------------------------------------------------------------

/// Read-only view over the cache file. Opens via
/// `rusqlite::Connection::open_with_flags(_, SQLITE_OPEN_READ_ONLY)` so the
/// helper never contends with the `modeltap` process for the write lock.
///
/// Only the surface the walking-skeleton scenario needs is exposed:
///
/// - `pragma_user_version()` — proves the v0→v1 migrator landed (AC-23-3).
/// - `count_rows(table, where_clause)` — proves the warm-start reconcile
///   wrote the TestTool's row (AC for cache_models / cache_tools).
///
/// Step-definitions-skeleton.md §A maps these to the
/// `@cache-introspection`-tagged step assertions.
pub struct CacheVerifier {
    conn: Connection,
}

impl CacheVerifier {
    /// Open the cache file at `path` read-only. Returns `Err` if the file is
    /// absent — callers use `path.exists()` to disambiguate "process A never
    /// wrote" from "open failed".
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        Ok(Self { conn })
    }

    /// Return `PRAGMA user_version` as a `u32`. The walking-skeleton scenario
    /// asserts this equals 1 (proves the embedded migration in
    /// `crates/modeltap-store/migrations/0001_initial.sql` ran).
    pub fn pragma_user_version(&self) -> rusqlite::Result<u32> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
    }

    /// Count rows in `table`. If `where_clause` is `Some(s)`, the helper
    /// appends `WHERE {s}` to the query. The clause's parameters are inlined
    /// by the caller (the walking-skeleton scenarios use a small fixed set of
    /// literal values — `tool_id = 'test-tool'` — so this is safe; broader
    /// queries lands in phase 04 with proper parameter binding).
    pub fn count_rows(&self, table: &str, where_clause: Option<&str>) -> rusqlite::Result<i64> {
        let sql = match where_clause {
            Some(clause) => format!("SELECT COUNT(*) FROM {table} WHERE {clause}"),
            None => format!("SELECT COUNT(*) FROM {table}"),
        };
        self.conn.query_row(&sql, [], |row| row.get::<_, i64>(0))
    }

    /// Convenience: read the `model_count` column from the single
    /// `cache_tools` row matching `tool_id`. Returns `Ok(None)` if no row
    /// matches so the caller can distinguish "no row" from "row with zero".
    pub fn model_count_for(&self, tool_id: &str) -> rusqlite::Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT model_count FROM cache_tools WHERE tool_id = ?1")?;
        let mut rows = stmt.query([tool_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }
}

/// Returns the stable `tool_id` string the TestTool registers under. Exposed
/// so step-definition helpers can pass it to `CacheVerifier::count_rows` /
/// `model_count_for` without re-importing `TEST_TOOL_NAME` from the parent
/// module.
pub fn test_tool_id_str() -> &'static str {
    TEST_TOOL_NAME.0
}

// ---------------------------------------------------------------------------
// Unit tests — exercise the CACHE seam against an in-memory SQLite + a real
// tempdir-backed cache so the helper is self-validating before the walking-
// skeleton scenario consumes it.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_store::types::{CachedModel, CachedTool};
    use modeltap_store::{Cache, CacheOpenResult};
    use std::collections::BTreeMap;
    use std::time::SystemTime;

    fn seed_one_row_cache(path: &Path) {
        let cache = match Cache::open(path).expect("seed open") {
            CacheOpenResult::OpenedFresh(c) => c,
            other => panic!("expected OpenedFresh, got {:?}", other),
        };
        let now = SystemTime::now();
        cache
            .write_tool(&CachedTool {
                tool_id: TEST_TOOL_NAME,
                install_path: PathBuf::from("/tmp/test-tool-install"),
                detected_version: Some("test-1.0.0".to_string()),
                plugin_version: "test-fixture 0.0.0".to_string(),
                model_count: 1,
                disk_usage_bytes: 1024,
                largest_model_id: Some("test-model-7b".to_string()),
                last_scan_at: now,
                last_scan_duration_ms: 0,
                last_error: None,
                last_error_at: None,
                search_paths: Vec::new(),
            })
            .expect("write_tool seed");
        cache
            .write_models(
                &TEST_TOOL_NAME,
                &[CachedModel {
                    model_id: "test-model-7b".to_string(),
                    tool_id: TEST_TOOL_NAME,
                    display_name: "Test Model 7B".to_string(),
                    format: Some("gguf".to_string()),
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
                }],
            )
            .expect("write_models seed");
    }

    #[test]
    fn cache_verifier_reads_pragma_user_version_after_seed() {
        let tmp = TempDir::new().expect("tmpdir");
        let path = tmp.path().join("cache.sqlite");
        seed_one_row_cache(&path);

        let verifier = CacheVerifier::open(&path).expect("open verifier");
        assert_eq!(
            verifier.pragma_user_version().expect("pragma"),
            1,
            "v1 migrator must have landed user_version = 1"
        );
    }

    #[test]
    fn cache_verifier_counts_rows_with_and_without_where_clause() {
        let tmp = TempDir::new().expect("tmpdir");
        let path = tmp.path().join("cache.sqlite");
        seed_one_row_cache(&path);

        let verifier = CacheVerifier::open(&path).expect("open verifier");
        assert_eq!(
            verifier.count_rows("cache_tools", None).expect("count"),
            1,
            "one seeded cache_tools row"
        );
        assert_eq!(
            verifier
                .count_rows("cache_models", Some("tool_id = 'test-tool'"))
                .expect("count where"),
            1,
            "one seeded cache_models row for test-tool"
        );
        assert_eq!(
            verifier
                .count_rows("cache_models", Some("tool_id = 'not-a-tool'"))
                .expect("count where empty"),
            0,
            "no rows for an unknown tool_id"
        );
    }

    #[test]
    fn cache_verifier_model_count_for_returns_some_for_seeded_tool() {
        let tmp = TempDir::new().expect("tmpdir");
        let path = tmp.path().join("cache.sqlite");
        seed_one_row_cache(&path);

        let verifier = CacheVerifier::open(&path).expect("open verifier");
        assert_eq!(
            verifier.model_count_for("test-tool").expect("query"),
            Some(1),
            "model_count for test-tool equals 1"
        );
        assert_eq!(
            verifier.model_count_for("missing-tool").expect("query"),
            None,
            "missing tool returns None"
        );
    }

    #[test]
    fn devon_cache_empty_fixture_layout_matches_spec() {
        let fix = DevonCacheEmptyFixture::build();
        // cache.sqlite must NOT exist at construction time.
        assert!(
            !fix.cache_path().exists(),
            "fixture must not pre-create cache.sqlite"
        );
        // Parent of cache_path must exist (xdg-data/modeltap/).
        assert!(
            fix.cache_path().parent().unwrap().exists(),
            "cache_path parent dir must exist"
        );
        // The synthetic model file must exist where TestTool::discover looks.
        let expected_model = fix.test_tool_root().join(TEST_MODEL_FILENAME);
        assert!(
            expected_model.exists(),
            "fixture must seed the TestTool's model file at {}",
            expected_model.display()
        );
        // log_dir exists.
        assert!(fix.log_dir().exists(), "log_dir must exist");
    }
}
