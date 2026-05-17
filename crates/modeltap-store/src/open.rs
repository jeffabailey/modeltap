//! `Cache::open` — the public entry point.
//!
//! Per `architecture-design.md` §5.1 (public API surface) and ADR-015
//! §"Concurrency". Sets `PRAGMA journal_mode = WAL` and
//! `PRAGMA busy_timeout = 5000` on every connection, then routes to the
//! migrator (`migrate.rs`) which forwards `user_version` from 0 to
//! `EXPECTED_SCHEMA_VERSION`.
//!
//! Step 01-02 minimum: the four "happy path" `CacheOpenResult` variants are
//! declared (`OpenedFresh`, `OpenedExisting`, `OpenedAfterMigration`,
//! `OpenedAfterRecovery`). Only `OpenedFresh` and `OpenedExisting` are
//! produced today; the migration and recovery branches land in step 01-03
//! when the corruption/downgrade tests join the suite.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::CacheError;
use crate::migrate::{migrate_to_latest, EXPECTED_SCHEMA_VERSION};

/// The cache handle. Owns the SQLite connection behind a `Mutex` so the
/// public API can present `&self` methods to callers (matching the surface
/// in `architecture-design.md` §5.1). The composition root wraps writes in
/// `spawn_blocking`, so the mutex is contended only across one async task.
pub struct Cache {
    conn: Mutex<Connection>,
}

/// Result of opening a cache. Step 01-02 produces `OpenedFresh` for a
/// non-existent path and `OpenedExisting` when the schema is already at
/// `EXPECTED_SCHEMA_VERSION`. The remaining variants are declared for
/// downstream steps so the public enum is stable.
#[derive(Debug)]
pub enum CacheOpenResult {
    /// File existed and schema already matched `EXPECTED_SCHEMA_VERSION`.
    OpenedExisting(Cache),

    /// File existed at a lower schema version; the migrator ran forward.
    /// Produced by step 01-03 onward; included for surface stability.
    OpenedAfterMigration { from: u32, to: u32, cache: Cache },

    /// File did not exist (or was empty); fresh schema applied.
    OpenedFresh(Cache),

    /// File was renamed away (corrupt / downgrade / migration-failure) and a
    /// fresh cache was opened in its place. Produced by step 01-03 onward.
    #[allow(dead_code)]
    OpenedAfterRecovery {
        reason: RecoveryReason,
        renamed_to: PathBuf,
        cache: Cache,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum RecoveryReason {
    Corrupted,
    Downgrade { found: u32, expected: u32 },
    MigrationFailed { from: u32, to: u32 },
}

impl Cache {
    /// Open (or create) the SQLite cache at `path`.
    ///
    /// - Creates the parent directory if missing.
    /// - Sets `PRAGMA journal_mode = WAL` and `PRAGMA busy_timeout = 5000`.
    /// - Runs migrations forward to `EXPECTED_SCHEMA_VERSION` if needed.
    ///
    /// Returns `OpenedFresh` when the file did not exist before this call,
    /// and `OpenedExisting` when it did and is already at the expected
    /// schema version. The `OpenedAfterMigration` and `OpenedAfterRecovery`
    /// variants are produced by future steps when migration-from-older and
    /// recovery paths land.
    pub fn open(path: &Path) -> Result<CacheOpenResult, CacheError> {
        let file_existed_before = path.exists();

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|source| CacheError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
        }

        let mut conn = Connection::open(path).map_err(CacheError::Sqlite)?;
        Self::apply_open_pragmas(&conn)?;

        let before_version = read_user_version(&conn)?;
        migrate_to_latest(&mut conn)?;
        let after_version = read_user_version(&conn)?;

        let cache = Cache {
            conn: Mutex::new(conn),
        };

        let result = if !file_existed_before {
            CacheOpenResult::OpenedFresh(cache)
        } else if before_version == after_version {
            CacheOpenResult::OpenedExisting(cache)
        } else {
            CacheOpenResult::OpenedAfterMigration {
                from: before_version,
                to: after_version,
                cache,
            }
        };
        Ok(result)
    }

    /// Open a fresh in-memory cache (`:memory:`). Intended for unit tests.
    /// Applies the same PRAGMAs and migrations as `Cache::open` for parity
    /// with path-backed caches.
    pub fn open_in_memory() -> Result<Cache, CacheError> {
        let mut conn = Connection::open_in_memory().map_err(CacheError::Sqlite)?;
        Self::apply_open_pragmas(&conn)?;
        migrate_to_latest(&mut conn)?;
        Ok(Cache {
            conn: Mutex::new(conn),
        })
    }

    /// Apply the open-time PRAGMAs required by ADR-015 §"Concurrency".
    fn apply_open_pragmas(conn: &Connection) -> Result<(), CacheError> {
        // WAL journal mode — concurrent reads, serialized writes. SQLite
        // silently downgrades to "memory" for in-memory databases; that is
        // acceptable per their docs.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // 5 second busy timeout (ADR-015 §"Concurrency").
        conn.pragma_update(None, "busy_timeout", 5_000_i64)?;
        // Foreign keys are off by default in SQLite; the FK declarations in
        // 0001_initial.sql require this to be on for cascade-on-delete to
        // actually cascade.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }

    // ----- Test/inspection helpers exposed publicly so integration tests
    // ----- can read SQLite-state without taking a fresh connection (which
    // ----- would have its own per-connection PRAGMA settings).

    /// Current `PRAGMA user_version`. Public so the migration test can
    /// assert the post-open value.
    pub fn user_version(&self) -> Result<u32, CacheError> {
        let conn = self.conn.lock().expect("cache mutex poisoned");
        read_user_version(&conn)
    }

    /// Current `PRAGMA journal_mode` (lowercased TEXT). Public so the
    /// migration test can assert the post-open value.
    pub fn pragma_journal_mode(&self) -> Result<String, CacheError> {
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let value: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        Ok(value)
    }

    /// Current `PRAGMA busy_timeout` in milliseconds. Public so the
    /// migration test can assert the post-open value.
    pub fn pragma_busy_timeout(&self) -> Result<i64, CacheError> {
        let conn = self.conn.lock().expect("cache mutex poisoned");
        let value: i64 = conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
        Ok(value)
    }

    /// Internal access to the connection. Module-private (not `pub`) so
    /// callers must go through repository methods.
    pub(crate) fn with_conn<R>(
        &self,
        f: impl FnOnce(&Connection) -> Result<R, CacheError>,
    ) -> Result<R, CacheError> {
        let conn = self.conn.lock().expect("cache mutex poisoned");
        f(&conn)
    }

    /// Internal mutable access to the connection for transactional writes.
    pub(crate) fn with_conn_mut<R>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<R, CacheError>,
    ) -> Result<R, CacheError> {
        let mut conn = self.conn.lock().expect("cache mutex poisoned");
        f(&mut conn)
    }
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache").finish_non_exhaustive()
    }
}

fn read_user_version(conn: &Connection) -> Result<u32, CacheError> {
    let v: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if v < 0 {
        return Err(CacheError::MalformedRow {
            table: "PRAGMA user_version",
            detail: format!("negative user_version: {v}"),
        });
    }
    Ok(v as u32)
}

// Compile-time sanity: keep `EXPECTED_SCHEMA_VERSION` non-zero. Step 01-02
// always migrates to 1; future versions only ever bump.
const _: () = {
    assert!(EXPECTED_SCHEMA_VERSION >= 1);
};
