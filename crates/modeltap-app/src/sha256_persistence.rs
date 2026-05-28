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

use std::time::UNIX_EPOCH;

use modeltap_core::ContentHash;
use modeltap_store::types::CachedSha256;
use modeltap_store::{stat_file_quad, Cache};

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
