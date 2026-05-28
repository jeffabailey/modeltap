//! Tier-3 SHA256 persistence bridge (US-27, ADR-018).
//!
//! The in-process [`Sha256Cache`](crate::sha256_cache::Sha256Cache) (Tier 1)
//! stays RAM-only by contract (ADR-003). This module is the composition-root
//! bridge between that cache and the persistent file-level `cache_sha256`
//! table (Tier 3) owned by `modeltap-store`:
//!
//! - [`seed_sha256_cache`] (read side) lifts persisted hashes whose validity
//!   quad still matches the on-disk file into the in-process cache at
//!   warm-start, so the background hash pool skips recomputation (AC-27-1).
//! - [`writeback_hash`] (write side) re-stats the path and upserts a freshly
//!   computed hash into `cache_sha256` best-effort on `Msg::HashComputed`
//!   (ADR-018 R10), via [`build_writeback_entry`] / [`hash_to_hex`].
//!
//! All persistence here is OPT-IN: callers invoke these only when
//! `AppConfig.cache.persist_sha256` is true.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use modeltap_core::{ContentHash, ToolId};
use modeltap_store::types::CachedSha256;
use modeltap_store::{stat_file_quad, Cache, CacheOpenResult};
use modeltap_tui::msg::Msg;

use crate::hash_pool::HashJob;
use crate::observability::{LaunchLogger, RecordKind};
use crate::sha256_cache::{Sha256Cache, Sha256CacheKey};

/// Read every persisted `cache_sha256` row; for each whose `(mtime,size,inode,
/// dev)` quad still matches a fresh `stat` of the path, lift its hash into the
/// in-process `Sha256Cache` under the same key the hash pool uses
/// (`(path, mtime_secs, size)` — see `hash_pool_wiring::build_hash_jobs`).
/// Returns the number of entries seeded.
///
/// Best-effort: a cache read error or a per-file stat error skips that entry
/// and never aborts the seed (a stale or unreadable row simply gets recomputed
/// by the pool). Rows whose quad has drifted are intentionally NOT seeded so
/// the pool recomputes them (AC-27-2/3/4).
pub fn seed_sha256_cache(cache: &Cache, sha_cache: &Sha256Cache) -> usize {
    let rows = match cache.all_sha256() {
        Ok(rows) => rows,
        Err(_) => return 0,
    };

    let mut seeded = 0usize;
    for row in rows {
        let fresh = match stat_file_quad(&row.path) {
            Ok(Some(fresh)) => fresh,
            // File gone or unreadable — leave it for the pool / verify.
            Ok(None) | Err(_) => continue,
        };
        if !row.stat.matches(&fresh) {
            // Quad drift — must be recomputed, do not seed (AC-27-2/3/4).
            continue;
        }
        let Some(hash) = parse_hex_hash(&row.content_hash) else {
            continue;
        };
        let Some(mtime_secs) = system_time_to_secs(&fresh.mtime) else {
            continue;
        };
        sha_cache.seed(
            Sha256CacheKey {
                path: row.path.clone(),
                mtime: mtime_secs,
                size: fresh.size_bytes,
            },
            hash,
        );
        seeded += 1;
    }
    seeded
}

/// Parse a 64-char lowercase-hex string into a `ContentHash`. Returns `None`
/// for any wrong-length or non-hex input (a corrupt row is skipped, never
/// panics).
pub fn parse_hex_hash(hex: &str) -> Option<ContentHash> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = (hex.as_bytes()[i * 2] as char).to_digit(16)?;
        let lo = (hex.as_bytes()[i * 2 + 1] as char).to_digit(16)?;
        *byte = (hi * 16 + lo) as u8;
    }
    Some(ContentHash(out))
}

/// Render a `ContentHash` as 64-char lowercase hex for the `cache_sha256`
/// row. Inverse of [`parse_hex_hash`].
pub fn hash_to_hex(hash: &ContentHash) -> String {
    let mut s = String::with_capacity(64);
    for b in hash.0.iter() {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn system_time_to_secs(t: &std::time::SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Build a `CachedSha256` row from a freshly computed hash + the file's fresh
/// stat at hash time. The composition root upserts this best-effort on
/// `Msg::HashComputed` when `persist_sha256` is enabled.
pub fn build_writeback_entry(
    path: std::path::PathBuf,
    stat: modeltap_store::types::FileStat,
    hash: &ContentHash,
    computed_at: std::time::SystemTime,
) -> CachedSha256 {
    CachedSha256 {
        path,
        stat,
        content_hash: hash_to_hex(hash),
        computed_at,
    }
}

/// Best-effort writeback of one freshly computed hash to the persistent
/// `cache_sha256` table (ADR-018 R10). Called by the composition root on
/// `Msg::HashComputed` when `persist_sha256` is enabled.
///
/// The validity quad is captured by RE-STATTING the path here (full-precision
/// `SystemTime` mtime + inode + dev + size) rather than reusing the hash job's
/// second-granularity stat — this is what makes the NEXT launch's
/// [`seed_sha256_cache`] quad comparison (`FileStat::matches`, full precision)
/// succeed for an unchanged file. `computed_at` is `now`.
///
/// Returns `true` iff a row was written. ANY failure (file vanished between
/// hashing and writeback, stat error, SQLite error) returns `false` WITHOUT
/// propagating — a writeback failure must never block the user-facing action.
pub fn writeback_hash(cache: &Cache, path: &std::path::Path, hash: &ContentHash) -> bool {
    let stat = match stat_file_quad(path) {
        Ok(Some(stat)) => stat,
        Ok(None) | Err(_) => return false,
    };
    let entry = build_writeback_entry(path.to_path_buf(), stat, hash, std::time::SystemTime::now());
    cache.upsert_sha256(&entry).is_ok()
}

/// Open the Tier-3 store cache for the seed + per-compute writeback. Returns
/// the live `Cache` handle the composition root holds for the session, or
/// `None` when there is no cache path or the open fails — persistence is then
/// silently disabled and launch proceeds normally (best-effort, ADR-018 R10).
pub fn open_store_cache(cache_path: Option<&Path>) -> Option<Cache> {
    let path = cache_path?;
    match Cache::open(path).ok()? {
        CacheOpenResult::OpenedFresh(c) | CacheOpenResult::OpenedExisting(c) => Some(c),
        CacheOpenResult::OpenedAfterMigration { cache, .. }
        | CacheOpenResult::OpenedAfterRecovery { cache, .. } => Some(cache),
    }
}

/// Composition-root SHA256 persistence context held for a launch's lifetime:
/// the live Tier-3 store handle plus the `(tool, model_id) → path` index the
/// per-compute writeback resolves against. `None` when persistence is off.
pub type PersistCtx = (Cache, HashMap<(ToolId, String), PathBuf>);

/// Build the `(tool, model_id) → path` lookup the writeback needs. Captured
/// from the hash jobs BEFORE they are moved into the pool on spawn.
pub fn job_path_index(jobs: &[HashJob]) -> HashMap<(ToolId, String), PathBuf> {
    jobs.iter()
        .map(|j| ((j.tool, j.model_id.clone()), j.path.clone()))
        .collect()
}

/// At a hash-pool drain site, react to a freshly COMPUTED hash (a cache hit
/// has `was_computed == false` and is skipped): emit the `hash.computed`
/// observability event and persist the hash to the Tier-3 store. Called ONLY
/// when persistence is enabled, so the default (non-persist) launch path is
/// byte-identical. Both side effects are best-effort.
pub fn observe_and_persist_hash(
    msg: &Msg,
    logger: &mut LaunchLogger,
    store_cache: &Cache,
    job_paths: &HashMap<(ToolId, String), PathBuf>,
) {
    let Msg::HashComputed {
        tool,
        model_id,
        hash,
        was_computed: true,
        ..
    } = msg
    else {
        return;
    };
    logger.record(RecordKind::HashComputed {
        tool: tool.0.to_string(),
        model: model_id.clone(),
    });
    if let Some(path) = job_paths.get(&(*tool, model_id.clone())) {
        let _ = writeback_hash(store_cache, path, hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_round_trips_with_hash_to_hex() {
        let h = ContentHash([0xAB; 32]);
        let hex = hash_to_hex(&h);
        assert_eq!(hex.len(), 64);
        assert_eq!(parse_hex_hash(&hex), Some(h));
    }

    #[test]
    fn parse_hex_rejects_wrong_length() {
        assert_eq!(parse_hex_hash("abcd"), None);
        assert_eq!(parse_hex_hash(&"a".repeat(63)), None);
        assert_eq!(parse_hex_hash(&"a".repeat(65)), None);
    }

    #[test]
    fn parse_hex_rejects_non_hex() {
        assert_eq!(parse_hex_hash(&"z".repeat(64)), None);
    }
}
