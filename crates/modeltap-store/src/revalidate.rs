//! Pre-mutate revalidator — the K5 safety mechanism.
//!
//! Step 05-02 (tool-model-info-sqlite-cache). The cache MUST NEVER enable a
//! stale-data destructive action. `Cache::verify_against_fs` is the single
//! seam every mutation orchestrator (unify / delete_all / delete_one /
//! folder_delete) goes through before invoking a plugin's destructive
//! `Tool::link` / `Tool::delete_one` / `Tool::delete_all` method.
//!
//! Per architecture-design.md §8.2 the load-bearing comparison is the
//! `(mtime_epoch_ns, size_bytes, inode, dev)` quad: every element of every
//! file in the model's `cache_model_files` rows is re-`stat()`ed and
//! compared against the cached value. The outcome is one of:
//!
//! - [`ValidationResult::Match`]    — every file matches the cached quad.
//!   The mutation orchestrator may proceed.
//! - [`ValidationResult::Drift { fresh }`] — at least one file's quad
//!   differs from cache. The orchestrator must dispatch `Tool::inspect_model`
//!   plus `Cache::write_models` to refresh, then flag the dialog for
//!   re-confirmation (AC-26-6).
//! - [`ValidationResult::Gone`]     — at least one file `stat()` returns
//!   `ErrorKind::NotFound`. The orchestrator must abort the action and
//!   dispatch a per-tool refresh (AC-26-7).
//!
//! The revalidator is intentionally synchronous (matches the rest of
//! `modeltap-store`) — the composition root wraps every call in
//! `tokio::task::spawn_blocking` per R8.
//!
//! ## What "absent rows" means
//!
//! A model with zero `cache_model_files` rows returns
//! [`ValidationResult::Match`] — there is no cached state to be stale
//! against, so the revalidator cannot block. The orchestrator decides
//! separately whether such a model is safe to mutate (typically by also
//! consulting the `cache_models` row); this method only enforces the
//! cache-quad invariant. The companion `Gone` variant is reserved for the
//! "row exists but the file no longer does" case which IS a cache-quad
//! violation.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::params;

use modeltap_core::types::ToolId;

use crate::error::CacheError;
use crate::open::Cache;
use crate::repo::tools::parse_iso8601_utc;
use crate::types::{CachedFile, FileStat, ModelId, ValidationResult};

impl Cache {
    /// Re-stat every file in `cache_model_files` for `model_id` and compare
    /// to the cached `(mtime_epoch_ns, size_bytes, inode, dev)` quad. The
    /// scan stops at the FIRST file that disagrees (`Drift`) or is missing
    /// (`Gone`); a clean pass returns `Match`.
    ///
    /// Returns `Match` when there are zero rows — see the module-level docs
    /// for the rationale. Multi-file models (HF snapshot pulls, Ollama
    /// manifest+blob pairs) are checked file-by-file; the FIRST drift wins
    /// so the orchestrator gets actionable per-file information.
    ///
    /// The on-disk path comparison uses the EXACT path string stored in
    /// `cache_model_files.path` — no canonicalisation, no symlink
    /// resolution. This matches the store-side invariant that the path
    /// column is the authoritative reference (downstream writers MUST
    /// canonicalise before writing).
    pub fn verify_against_fs(&self, model_id: &ModelId) -> Result<ValidationResult, CacheError> {
        let rows = self.files_for_model(model_id)?;
        if rows.is_empty() {
            return Ok(ValidationResult::Match);
        }
        for row in &rows {
            match stat_file(&row.path) {
                Ok(fresh) => {
                    let cached: FileStat = FileStat::from(row);
                    if !cached.matches(&fresh) {
                        return Ok(ValidationResult::Drift { fresh });
                    }
                }
                Err(StatError::NotFound) => {
                    return Ok(ValidationResult::Gone);
                }
                Err(StatError::Io(e)) => {
                    return Err(CacheError::Io {
                        path: row.path.clone(),
                        source: e,
                    });
                }
            }
        }
        Ok(ValidationResult::Match)
    }

    /// Read every `cache_model_files` row for `model_id` across all tools.
    ///
    /// Multi-file models can span more than one tool conceptually, but in
    /// practice the (model_id, tool_id) composite key partitions cleanly;
    /// this method does not filter by tool_id because the revalidator's
    /// caller already knows the action's target. An unknown `model_id`
    /// returns an empty vec — see `verify_against_fs` for the
    /// "no rows = Match" rationale.
    pub fn files_for_model(&self, model_id: &ModelId) -> Result<Vec<CachedFile>, CacheError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT model_id, tool_id, path, size_bytes,
                        mtime_epoch_ns, inode, dev, last_stat_at
                 FROM cache_model_files
                 WHERE model_id = ?1",
            )?;
            let rows = stmt
                .query_map(params![model_id], |row| {
                    Ok(RawFileRow {
                        model_id: row.get(0)?,
                        tool_id: row.get(1)?,
                        path: row.get(2)?,
                        size_bytes: row.get(3)?,
                        mtime_epoch_ns: row.get(4)?,
                        inode: row.get(5)?,
                        dev: row.get(6)?,
                        last_stat_at: row.get(7)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows.into_iter().map(hydrate_file).collect()
        })
    }

    /// Seed-write helper for `cache_model_files`. Idempotent at the
    /// (path) level via `ON CONFLICT(path) DO UPDATE`. Inserts the rows
    /// inside ONE transaction so a fixture build is atomic.
    ///
    /// Step 05-02 introduces this as the minimum write surface needed by
    /// the revalidator fixtures (`devon-cache-mtime-drift`,
    /// `devon-cache-file-gone`) and the unit tests. The richer per-tool
    /// upsert + cascading delete surface lands when an in-tree plugin
    /// starts populating `cache_model_files` rows from
    /// `Tool::inspect_model`.
    pub fn write_model_files(&self, files: &[CachedFile]) -> Result<(), CacheError> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO cache_model_files (
                        model_id, tool_id, path, size_bytes,
                        mtime_epoch_ns, inode, dev, last_stat_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                    ON CONFLICT(path) DO UPDATE SET
                        model_id       = excluded.model_id,
                        tool_id        = excluded.tool_id,
                        size_bytes     = excluded.size_bytes,
                        mtime_epoch_ns = excluded.mtime_epoch_ns,
                        inode          = excluded.inode,
                        dev            = excluded.dev,
                        last_stat_at   = excluded.last_stat_at",
                )?;
                for file in files {
                    let path_text = path_to_db_text(&file.path);
                    let mtime_ns = mtime_to_epoch_ns(&file.mtime)?;
                    let last_stat = crate::repo::tools::format_iso8601_utc(&file.last_stat_at)?;
                    stmt.execute(params![
                        file.model_id,
                        file.tool_id.0,
                        path_text,
                        file.size_bytes as i64,
                        mtime_ns,
                        file.inode as i64,
                        file.dev as i64,
                        last_stat,
                    ])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }
}

/// Re-`stat()` `path` and project it into a `FileStat`, or `None` when the
/// file no longer exists. Unix-only (MetadataExt) — the WSL target is
/// architecturally identical to Linux. Public so `modeltap-app`'s Tier-3
/// SHA256 seed (US-27) builds the validity quad through the same code path the
/// revalidator uses, keeping quad construction in one place.
pub fn stat_file_quad(path: &Path) -> std::io::Result<Option<FileStat>> {
    match std::fs::metadata(path) {
        Ok(meta) => Ok(Some(FileStat {
            size_bytes: meta.len(),
            mtime: meta.modified()?,
            inode: meta.ino(),
            dev: meta.dev(),
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Reasons `stat_file` can fail. `NotFound` is special-cased because the
/// revalidator translates it into `ValidationResult::Gone`; every other
/// I/O error propagates as `CacheError::Io`.
enum StatError {
    NotFound,
    Io(std::io::Error),
}

/// Re-`stat()` `path` and project it into a `FileStat`. Unix-only —
/// matches the rest of the crate's MetadataExt usage (the WSL target is
/// architecturally identical to Linux per the project's design rules).
fn stat_file(path: &Path) -> Result<FileStat, StatError> {
    let meta = std::fs::metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            StatError::NotFound
        } else {
            StatError::Io(e)
        }
    })?;
    Ok(FileStat {
        size_bytes: meta.len(),
        mtime: meta.modified().map_err(StatError::Io)?,
        inode: meta.ino(),
        dev: meta.dev(),
    })
}

struct RawFileRow {
    model_id: String,
    tool_id: String,
    path: String,
    size_bytes: i64,
    mtime_epoch_ns: i64,
    inode: i64,
    dev: i64,
    last_stat_at: String,
}

fn hydrate_file(raw: RawFileRow) -> Result<CachedFile, CacheError> {
    let tool_id = ToolId(crate::repo::intern::intern_tool_id(&raw.tool_id));
    let mtime = epoch_ns_to_system_time(raw.mtime_epoch_ns)?;
    let last_stat_at = parse_iso8601_utc(&raw.last_stat_at, "cache_model_files.last_stat_at")?;
    Ok(CachedFile {
        model_id: raw.model_id,
        tool_id,
        path: PathBuf::from(raw.path),
        size_bytes: raw.size_bytes as u64,
        mtime,
        inode: raw.inode as u64,
        dev: raw.dev as u64,
        last_stat_at,
    })
}

/// Convert a `SystemTime` to nanoseconds-since-UNIX-epoch as an i64. The
/// `cache_model_files.mtime_epoch_ns` column is INTEGER (i64 range).
///
/// `pub(crate)` so `repo/sha256.rs` reuses the same mtime encoding for the
/// `cache_sha256.mtime_epoch_ns` column (US-27).
pub(crate) fn mtime_to_epoch_ns(t: &SystemTime) -> Result<i64, CacheError> {
    let duration = t
        .duration_since(UNIX_EPOCH)
        .map_err(|e| CacheError::MalformedRow {
            table: "cache_model_files.mtime_epoch_ns",
            detail: format!("mtime before UNIX_EPOCH: {e}"),
        })?;
    // u128 ns total, cast down. `i64::MAX` ns is ~292 years past epoch —
    // well beyond any realistic filesystem mtime, but we guard anyway.
    let ns: u128 = duration.as_nanos();
    // MUTATION: cargo-mutants flags `> -> ==` / `> -> >=` here as MISSED.
    // The guard fires only on mtimes 292+ years past UNIX_EPOCH; no naturally-
    // occurring filesystem mtime can satisfy it, and the test would need to
    // forge a `SystemTime` with a `Duration` exceeding `i64::MAX` nanoseconds
    // (an unsigned 128-bit value the standard library cannot construct without
    // unsafe arithmetic). Equivalent-mutant in practice — the operator change
    // only matters for an input no real filesystem can produce.
    if ns > i64::MAX as u128 {
        return Err(CacheError::MalformedRow {
            table: "cache_model_files.mtime_epoch_ns",
            detail: format!("mtime ns out of i64 range: {ns}"),
        });
    }
    Ok(ns as i64)
}

pub(crate) fn epoch_ns_to_system_time(ns: i64) -> Result<SystemTime, CacheError> {
    // MUTATION: cargo-mutants flags `< -> ==` / `< -> <=` here as MISSED.
    // The guard fires only when a negative `cache_model_files.mtime_epoch_ns`
    // is read out of SQLite. Per the schema this column is written by
    // `mtime_to_epoch_ns` above (whose own guard keeps the output in the
    // [0, i64::MAX] range); a negative value requires direct SQL tampering
    // — a defense-in-depth check, not a behaviour the production write path
    // can produce. Equivalent-mutant in practice.
    if ns < 0 {
        return Err(CacheError::MalformedRow {
            table: "cache_model_files.mtime_epoch_ns",
            detail: format!("negative mtime_epoch_ns: {ns}"),
        });
    }
    let secs = (ns as u64) / 1_000_000_000;
    let sub_ns = ((ns as u64) % 1_000_000_000) as u32;
    Ok(UNIX_EPOCH + Duration::new(secs, sub_ns))
}

fn path_to_db_text(p: &Path) -> String {
    // Mirror repo/tools.rs::path_to_db_text — UTF-8 lossy stringification
    // so the column never carries non-text bytes. The cache is documented
    // (data-models.md §3) as text-only; non-UTF-8 paths are rare on the
    // supported platforms (macOS NFD, Linux UTF-8, WSL UTF-8).
    p.to_string_lossy().into_owned()
}
