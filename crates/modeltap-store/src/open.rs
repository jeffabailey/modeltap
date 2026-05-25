//! `Cache::open` — the public entry point.
//!
//! Per `architecture-design.md` §5.1 (public API surface) and ADR-015
//! §"Concurrency". Sets `PRAGMA journal_mode = WAL` and
//! `PRAGMA busy_timeout = 5000` on every connection, then routes to the
//! migrator (`migrate.rs`) which forwards `user_version` from 0 to
//! `EXPECTED_SCHEMA_VERSION`.
//!
//! Step 04-01 closes the recovery loop. On open, three recoverable failure
//! modes route to `recovery::recover_and_reopen` which renames the broken
//! file aside, appends a `cache_recovery` line to diagnostics.log, and
//! re-opens a fresh empty cache at the original path:
//!
//! 1. `SQLITE_CORRUPT` / `SQLITE_NOTADB` from any opening rusqlite call.
//! 2. `PRAGMA user_version > EXPECTED_SCHEMA_VERSION` (downgrade).
//! 3. `rusqlite_migration::Migrations::to_latest` returning an error.
//!
//! AC-23-11 invariant: cache failure NEVER prevents inventory view from
//! rendering — the composition root surfaces this as a dismissable banner
//! and the inventory view paints normally below it via the cold-start
//! fallback.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::CacheError;
use crate::migrate::{migrate_to_latest, EXPECTED_SCHEMA_VERSION};
use crate::recovery::{recover_and_reopen, RecoveryReason};

/// The cache handle. Owns the SQLite connection behind a `Mutex` so the
/// public API can present `&self` methods to callers (matching the surface
/// in `architecture-design.md` §5.1). The composition root wraps writes in
/// `spawn_blocking`, so the mutex is contended only across one async task.
pub struct Cache {
    conn: Mutex<Connection>,
}

/// Result of opening a cache. Step 01-02 produces `OpenedFresh` for a
/// non-existent path and `OpenedExisting` when the schema is already at
/// `EXPECTED_SCHEMA_VERSION`. Step 04-01 wires the `OpenedAfterRecovery`
/// branch (SQLITE_CORRUPT / downgrade / migration-failure).
#[derive(Debug)]
pub enum CacheOpenResult {
    /// File existed and schema already matched `EXPECTED_SCHEMA_VERSION`.
    OpenedExisting(Cache),

    /// File existed at a lower schema version; the migrator ran forward.
    OpenedAfterMigration { from: u32, to: u32, cache: Cache },

    /// File did not exist (or was empty); fresh schema applied.
    OpenedFresh(Cache),

    /// File was renamed away (corrupt / downgrade / migration-failure) and a
    /// fresh cache was opened in its place. Carries the `RecoveryReason` so
    /// the composition root can surface the cause in the recovery banner.
    /// `renamed_to` is the absolute path the broken file was renamed to so
    /// support / triage can find it on disk.
    OpenedAfterRecovery {
        reason: RecoveryReason,
        renamed_to: PathBuf,
        cache: Cache,
    },
}

impl Cache {
    /// Open (or create) the SQLite cache at `path`.
    ///
    /// - Creates the parent directory if missing.
    /// - Sets `PRAGMA journal_mode = WAL` and `PRAGMA busy_timeout = 5000`.
    /// - Runs migrations forward to `EXPECTED_SCHEMA_VERSION` if needed.
    /// - On `SQLITE_CORRUPT`, future-version, or migration failure: renames
    ///   the broken file aside, writes a `cache_recovery` diagnostics line,
    ///   and re-opens a fresh empty cache at the original path.
    ///
    /// Returns one of:
    /// - `OpenedFresh` — file did not exist before this call.
    /// - `OpenedExisting` — file existed and is already at the expected schema.
    /// - `OpenedAfterMigration { from, to }` — file existed at a lower
    ///   version and the migrator rolled it forward.
    /// - `OpenedAfterRecovery { reason, renamed_to }` — the file was broken
    ///   and was renamed aside; the returned `cache` is a fresh empty DB.
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

        // First attempt: standard open. If the file is corrupt or not a
        // database we route to recovery; everything else propagates.
        let conn_result = Connection::open(path).map_err(CacheError::Sqlite);
        let mut conn = match conn_result {
            Ok(c) => c,
            Err(err) => {
                if is_corruption_error(&err) {
                    return Self::run_recovery(path, &RecoveryReason::Corrupted);
                }
                return Err(err);
            }
        };

        if let Err(err) = Self::apply_open_pragmas(&conn) {
            if is_corruption_error(&err) {
                drop(conn);
                return Self::run_recovery(path, &RecoveryReason::Corrupted);
            }
            return Err(err);
        }

        // Read the on-disk schema version BEFORE running migrations so we can
        // distinguish (a) downgrade from (b) normal forward-migration.
        let before_version = match read_user_version(&conn) {
            Ok(v) => v,
            Err(err) => {
                if is_corruption_error(&err) {
                    drop(conn);
                    return Self::run_recovery(path, &RecoveryReason::Corrupted);
                }
                return Err(err);
            }
        };

        // Downgrade check: if the on-disk schema is newer than what this
        // binary supports, recover (rename aside + fresh cache). Only applies
        // to pre-existing files — a fresh file has user_version=0 and is
        // never a downgrade case.
        if file_existed_before && before_version > EXPECTED_SCHEMA_VERSION {
            drop(conn);
            return Self::run_recovery(
                path,
                &RecoveryReason::Downgrade {
                    found: before_version,
                    expected: EXPECTED_SCHEMA_VERSION,
                },
            );
        }

        // Run migrations. On failure, rename and recover.
        if let Err(err) = migrate_to_latest(&mut conn) {
            drop(conn);
            return Self::run_recovery(
                path,
                &RecoveryReason::MigrationFailed {
                    from: before_version,
                    to: EXPECTED_SCHEMA_VERSION,
                },
            )
            .map_err(|recovery_err| {
                // If recovery itself failed, surface the recovery error rather
                // than the original migration error: the user's actionable
                // path is to investigate the I/O failure that prevented
                // recovery, not the migration that triggered it.
                let _ = err;
                recovery_err
            });
        }

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

    /// Common tail for the three recovery-triggering paths. Renames the
    /// broken file aside via `recovery::recover_and_reopen`, wraps the fresh
    /// connection in a `Cache`, and returns the `OpenedAfterRecovery` variant.
    fn run_recovery(path: &Path, reason: &RecoveryReason) -> Result<CacheOpenResult, CacheError> {
        let (conn, renamed_to) = recover_and_reopen(path, reason)?;
        Ok(CacheOpenResult::OpenedAfterRecovery {
            reason: reason.clone(),
            renamed_to,
            cache: Cache {
                conn: Mutex::new(conn),
            },
        })
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

/// True iff the underlying SQLite error indicates the file is not a valid
/// database. Two error codes route to recovery:
///
/// - `SQLITE_CORRUPT` (`DatabaseCorrupt`) — valid header, body damaged.
/// - `SQLITE_NOTADB` (`NotADatabase`) — header is not a valid SQLite file.
///
/// Both are handled identically: rename the broken file aside and open a
/// fresh empty cache at the original path.
fn is_corruption_error(err: &CacheError) -> bool {
    let CacheError::Sqlite(sqlite_err) = err else {
        return false;
    };
    let Some(code) = sqlite_err.sqlite_error_code() else {
        return false;
    };
    matches!(
        code,
        rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
    )
}

// Compile-time sanity: keep `EXPECTED_SCHEMA_VERSION` non-zero. Step 01-02
// always migrates to 1; future versions only ever bump.
const _: () = {
    assert!(EXPECTED_SCHEMA_VERSION >= 1);
};
