//! Sha256Repo — `cache_sha256` reads and writes (US-27, Release 3).
//!
//! The Tier-3 file-level content-hash store from ADR-018. Keyed by absolute
//! path; carries the `(mtime_epoch_ns, size_bytes, inode, dev)` validity quad
//! so a stale entry can be detected against a fresh `stat` before it is
//! trusted. `content_hash` is lowercase hex; `computed_at` is ISO-8601 UTC.
//!
//! Minimum surface for the walking skeleton + verify:
//! - `upsert_sha256`     — insert-or-replace one row (last writer wins).
//! - `get_sha256_by_path`— read one row by path (None when absent).
//! - `invalidate_sha256` — delete one row by path (drift invalidation).
//! - `all_sha256`        — read every row (drives `modeltap cache verify`).

use std::path::Path;

use rusqlite::params;

use crate::error::CacheError;
use crate::open::Cache;
use crate::repo::tools::{format_iso8601_utc, parse_iso8601_utc};
use crate::revalidate::{epoch_ns_to_system_time, mtime_to_epoch_ns};
use crate::types::{CachedSha256, FileStat};

impl Cache {
    /// Insert-or-replace one `cache_sha256` row. Upserts on the `path` PK so a
    /// re-hash of the same file overwrites the previous quad + hash.
    pub fn upsert_sha256(&self, entry: &CachedSha256) -> Result<(), CacheError> {
        let path_text = entry.path.to_string_lossy().into_owned();
        let mtime_ns = mtime_to_epoch_ns(&entry.stat.mtime)?;
        let computed_at = format_iso8601_utc(&entry.computed_at)?;

        self.with_conn_mut(|conn| {
            conn.execute(
                "INSERT INTO cache_sha256 (
                    path, mtime_epoch_ns, size_bytes, inode, dev,
                    content_hash, computed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(path) DO UPDATE SET
                    mtime_epoch_ns = excluded.mtime_epoch_ns,
                    size_bytes = excluded.size_bytes,
                    inode = excluded.inode,
                    dev = excluded.dev,
                    content_hash = excluded.content_hash,
                    computed_at = excluded.computed_at",
                params![
                    path_text,
                    mtime_ns,
                    entry.stat.size_bytes as i64,
                    entry.stat.inode as i64,
                    entry.stat.dev as i64,
                    entry.content_hash,
                    computed_at,
                ],
            )?;
            Ok(())
        })
    }

    /// Read one `cache_sha256` row by absolute path. Returns `None` when no row
    /// exists for that path.
    pub fn get_sha256_by_path(&self, path: &Path) -> Result<Option<CachedSha256>, CacheError> {
        let path_text = path.to_string_lossy().into_owned();
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT path, mtime_epoch_ns, size_bytes, inode, dev,
                        content_hash, computed_at
                 FROM cache_sha256
                 WHERE path = ?1",
            )?;
            let mut rows = stmt
                .query_map(params![path_text], raw_row)?
                .collect::<Result<Vec<_>, _>>()?;
            match rows.pop() {
                Some(raw) => Ok(Some(hydrate(raw)?)),
                None => Ok(None),
            }
        })
    }

    /// Delete the `cache_sha256` row for `path`. Idempotent — deleting an
    /// absent path is a no-op (zero rows affected). Used by drift invalidation.
    pub fn invalidate_sha256(&self, path: &Path) -> Result<(), CacheError> {
        let path_text = path.to_string_lossy().into_owned();
        self.with_conn_mut(|conn| {
            conn.execute(
                "DELETE FROM cache_sha256 WHERE path = ?1",
                params![path_text],
            )?;
            Ok(())
        })
    }

    /// Read every `cache_sha256` row. Order is unspecified. Drives
    /// `modeltap cache verify`, which recomputes each hash and reports drift.
    pub fn all_sha256(&self) -> Result<Vec<CachedSha256>, CacheError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT path, mtime_epoch_ns, size_bytes, inode, dev,
                        content_hash, computed_at
                 FROM cache_sha256",
            )?;
            let rows = stmt
                .query_map([], raw_row)?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter().map(hydrate).collect::<Result<Vec<_>, _>>()
        })
    }
}

struct RawSha256Row {
    path: String,
    mtime_epoch_ns: i64,
    size_bytes: i64,
    inode: i64,
    dev: i64,
    content_hash: String,
    computed_at: String,
}

/// Project a SQLite row into the raw column tuple. Returns a `rusqlite::Result`
/// because column reads can fail; hydration (type conversion) happens in
/// `hydrate` so the `query_map` closure stays infallible past column extraction.
fn raw_row(row: &rusqlite::Row<'_>) -> Result<RawSha256Row, rusqlite::Error> {
    Ok(RawSha256Row {
        path: row.get(0)?,
        mtime_epoch_ns: row.get(1)?,
        size_bytes: row.get(2)?,
        inode: row.get(3)?,
        dev: row.get(4)?,
        content_hash: row.get(5)?,
        computed_at: row.get(6)?,
    })
}

fn hydrate(raw: RawSha256Row) -> Result<CachedSha256, CacheError> {
    let mtime = epoch_ns_to_system_time(raw.mtime_epoch_ns)?;
    let computed_at = parse_iso8601_utc(&raw.computed_at, "cache_sha256.computed_at")?;
    Ok(CachedSha256 {
        path: std::path::PathBuf::from(raw.path),
        stat: FileStat {
            size_bytes: raw.size_bytes as u64,
            mtime,
            inode: raw.inode as u64,
            dev: raw.dev as u64,
        },
        content_hash: raw.content_hash,
        computed_at,
    })
}
