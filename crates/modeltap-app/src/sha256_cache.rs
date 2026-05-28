//! In-process SHA-256 cache + `Hasher` adapter (US-13, ADR-002, ADR-003).
//!
//! ## Design
//!
//! - Cache key: `(path, mtime, size)`. Different mtime or size → cache miss
//!   and recompute. Different path → distinct entry.
//! - Storage: `Arc<Mutex<HashMap<Sha256CacheKey, ContentHash>>>`. Cloning a
//!   `Sha256Cache` shares the underlying map — the orchestrator constructs
//!   one cache and clones handles into the detail screen, the unify worker,
//!   etc.
//! - Lifetime: in-process only. **NO disk persistence** per ADR-003. The
//!   cache is dropped on app exit; subsequent launches recompute.
//!
//! ## ADR citations
//!
//! - ADR-002 §"Lazy. Process-local cache." — SHA-256 is lazy (computed only
//!   when the user opens a detail screen) and cached in process memory.
//! - ADR-003 §"Stateless rediscovery, no persistent index." — there is no
//!   `~/.modeltap/sha256-cache.toml`. The cache is volatile by design.
//!
//! ## Real adapter (`Sha2Hasher`)
//!
//! Backed by the RustCrypto `sha2` crate. Reads the file in 64 KiB chunks,
//! emits a `HashProgress` after every chunk, and returns the final digest.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use modeltap_core::ports::{HashProgress, Hasher};
use modeltap_core::ContentHash;
use sha2::{Digest, Sha256};

/// Key for the SHA-256 cache. Two files at the same path with the same mtime
/// and size are assumed byte-identical — the standard mtime/size invalidation
/// policy. ADR-003 §"Persistent SHA256 cache only" alternative-rejected note
/// acknowledges this is not adversary-proof; it is sufficient for the in-
/// process session-scoped cache where the user has just observed the file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sha256CacheKey {
    pub path: PathBuf,
    pub mtime: u64,
    pub size: u64,
}

/// In-process SHA-256 cache. Cloning shares the underlying map.
///
/// Per ADR-003: NO disk persistence. The map lives only in process memory
/// and is dropped on exit. If a future commit adds `std::fs::write` to a
/// cache path here, the `no_state_files` integration test (ADR-003 §
/// "Enforcement") will fail.
#[derive(Debug, Clone, Default)]
pub struct Sha256Cache {
    inner: Arc<Mutex<HashMap<Sha256CacheKey, ContentHash>>>,
}

impl Sha256Cache {
    /// Construct an empty cache. The orchestrator typically calls this once
    /// at startup and clones handles into screens / workers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up a cached hash; compute via `hasher` on miss.
    ///
    /// `progress` is forwarded to the hasher on miss (so the detail screen
    /// can render "computing dedup key... N%"). On a hit, `progress` is NOT
    /// invoked — the hash is already known.
    pub fn get_or_compute<H: Hasher + ?Sized>(
        &self,
        key: Sha256CacheKey,
        hasher: &H,
        progress: &mut dyn FnMut(HashProgress),
    ) -> std::io::Result<ContentHash> {
        // Fast path: read lock + clone the value. Drop the lock before the
        // (potentially-multi-second) hash so other clones can read other
        // entries during the computation.
        if let Some(hit) = self.inner.lock().unwrap().get(&key).copied() {
            return Ok(hit);
        }
        let computed = hasher.sha256_streaming(&key.path, progress)?;
        self.inner.lock().unwrap().insert(key, computed);
        Ok(computed)
    }

    /// Non-mutating lookup: return the cached hash for `key` if present, else
    /// `None`. The hash pool worker peeks BEFORE computing so it can report
    /// whether a job was a cache HIT (seeded from Tier-3 or a prior in-session
    /// hash) or an actual COMPUTE — the `hash.computed` observability event
    /// fires only on a compute (US-27 AC-27-1), and the Tier-3 writeback only
    /// persists freshly computed hashes.
    pub fn peek(&self, key: &Sha256CacheKey) -> Option<ContentHash> {
        self.inner.lock().unwrap().get(key).copied()
    }

    /// Pre-populate one entry without computing. Used by the US-27 Tier-3 seed
    /// at warm-start: a persisted `cache_sha256` row whose `(mtime,size,inode,
    /// dev)` quad still matches the on-disk file is lifted into this in-process
    /// cache so the background hash pool's `get_or_compute` hits and never
    /// recomputes the unchanged file (AC-27-1). This is the ONLY way a hash
    /// enters the cache without a `Hasher` call; the source of truth for the
    /// persisted value is the SQLite `cache_sha256` table (ADR-018 Tier 3).
    pub fn seed(&self, key: Sha256CacheKey, hash: ContentHash) {
        self.inner.lock().unwrap().insert(key, hash);
    }
}

// ---------------------------------------------------------------------------
// Real Hasher adapter — SHA-256 over a file in 64 KiB chunks with progress.
// ---------------------------------------------------------------------------

/// Production `Hasher` implementation backed by the `sha2` crate. Reads the
/// file in 64 KiB chunks; after each chunk emits a `HashProgress` with the
/// current percent-complete.
pub struct Sha2Hasher;

impl Sha2Hasher {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Sha2Hasher {
    fn default() -> Self {
        Self::new()
    }
}

const CHUNK_SIZE: usize = 64 * 1024;

impl Hasher for Sha2Hasher {
    fn sha256_streaming(
        &self,
        path: &Path,
        progress: &mut dyn FnMut(HashProgress),
    ) -> std::io::Result<ContentHash> {
        let metadata = std::fs::metadata(path)?;
        let total = metadata.len();
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut bytes_hashed: u64 = 0;

        progress(HashProgress {
            percent_complete: 0,
            bytes_hashed: 0,
        });

        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            bytes_hashed += n as u64;
            let pct = bytes_hashed
                .saturating_mul(100)
                .checked_div(total)
                .map(|p| p.min(100) as u8)
                .unwrap_or(100u8);
            progress(HashProgress {
                percent_complete: pct,
                bytes_hashed,
            });
        }

        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest[..]);
        progress(HashProgress {
            percent_complete: 100,
            bytes_hashed,
        });
        Ok(ContentHash(out))
    }
}
