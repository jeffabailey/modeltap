//! Tier-3 SHA256 seed — warm-start read side (US-27 step 01-04).
//!
//! Given persisted `cache_sha256` rows whose (mtime,size,inode,dev) quad still
//! matches the on-disk file, `seed_sha256_cache` pre-populates the in-process
//! `Sha256Cache` so the background hash pool finds a hit and does NOT recompute
//! (the AC-27-1 "no hash.computed on the unchanged file" property). A row whose
//! quad has drifted is NOT seeded (it must be recomputed — AC-27-2/3/4).

use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use modeltap_app::sha256_cache::{Sha256Cache, Sha256CacheKey};
use modeltap_app::sha256_persistence::seed_sha256_cache;
use modeltap_core::ports::{HashProgress, Hasher};
use modeltap_core::ContentHash;
use modeltap_store::types::{CachedSha256, FileStat};
use modeltap_store::{stat_file_quad, Cache};

/// A hasher that panics if called — proves the seeded value short-circuits the
/// pool and no recompute happens.
struct PanicHasher;
impl Hasher for PanicHasher {
    fn sha256_streaming(
        &self,
        _path: &Path,
        _progress: &mut dyn FnMut(HashProgress),
    ) -> std::io::Result<ContentHash> {
        panic!("hasher must NOT be called for a seeded (unchanged) file");
    }
}

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    let mut f = std::fs::File::create(&p).expect("create fixture file");
    f.write_all(bytes).expect("write fixture");
    p
}

fn hex32(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

#[test]
fn seeds_in_process_cache_when_quad_matches_so_pool_does_not_recompute() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = Cache::open_in_memory().expect("open_in_memory");

    let path = write_file(dir.path(), "mistral.gguf", b"hello world");
    let fresh = stat_file_quad(&path)
        .expect("stat ok")
        .expect("file present");

    // Persist a cache_sha256 row whose quad matches the on-disk file.
    let stored_hash = hex32(0xAB);
    cache
        .upsert_sha256(&CachedSha256 {
            path: path.clone(),
            stat: fresh.clone(),
            content_hash: stored_hash.clone(),
            computed_at: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        })
        .expect("upsert");

    let sha_cache = Sha256Cache::new();
    let seeded = seed_sha256_cache(&cache, &sha_cache);
    assert_eq!(seeded, 1, "matching quad must be seeded");

    // The pool would look up by (path, mtime_secs, size). A hit returns the
    // stored hash WITHOUT calling the (panicking) hasher.
    let mtime_secs = fresh
        .mtime
        .duration_since(UNIX_EPOCH)
        .expect("mtime after epoch")
        .as_secs();
    let key = Sha256CacheKey {
        path: path.clone(),
        mtime: mtime_secs,
        size: fresh.size_bytes,
    };
    let got = sha_cache
        .get_or_compute(key, &PanicHasher, &mut |_| {})
        .expect("seeded hit must not error");

    let mut expected = [0u8; 32];
    expected.iter_mut().for_each(|b| *b = 0xAB);
    assert_eq!(got, ContentHash(expected), "seeded hash must round-trip");
}

#[test]
fn does_not_seed_when_quad_drifted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let path = write_file(dir.path(), "model.gguf", b"original");
    let fresh = stat_file_quad(&path).expect("stat").expect("present");

    // Persist a row with a DIFFERENT size (simulated drift).
    let drifted = FileStat {
        size_bytes: fresh.size_bytes + 999,
        ..fresh.clone()
    };
    cache
        .upsert_sha256(&CachedSha256 {
            path: path.clone(),
            stat: drifted,
            content_hash: hex32(0xCD),
            computed_at: SystemTime::now(),
        })
        .expect("upsert");

    let sha_cache = Sha256Cache::new();
    let seeded = seed_sha256_cache(&cache, &sha_cache);
    assert_eq!(seeded, 0, "drifted quad must NOT be seeded");
}
