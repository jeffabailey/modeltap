//! Unit tests for the in-process SHA256 cache (US-13, ADR-002, ADR-003).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: First call computes via Hasher; result returned.
//!     B2: Second call with same (path, mtime, size) returns cached value
//!         without invoking Hasher again.
//!     B3: Cache miss when mtime changes (file modified) → recompute.
//!     B4: Cache miss when size changes → recompute.
//!     B5: Hasher progress callbacks are forwarded to the supplied progress sink
//!         (lazy-hash UX — for the >50GB scenario but exercised on small input).
//!   budget = 5 × 2 = 10 tests max. We use 6.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use modeltap_app::sha256_cache::{Sha256Cache, Sha256CacheKey};
use modeltap_core::ports::{HashProgress, Hasher};
use modeltap_core::ContentHash;

/// Test double for the Hasher port. Records call count and returns canned
/// hashes per call. Optionally yields progress callbacks.
struct FakeHasher {
    call_count: Arc<Mutex<u32>>,
    canned_hash: ContentHash,
    progress_pcts: Vec<u8>,
}

impl FakeHasher {
    fn new(canned_hash: ContentHash) -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
            canned_hash,
            progress_pcts: Vec::new(),
        }
    }

    fn with_progress(mut self, pcts: Vec<u8>) -> Self {
        self.progress_pcts = pcts;
        self
    }

    fn call_count(&self) -> u32 {
        *self.call_count.lock().unwrap()
    }
}

impl Hasher for FakeHasher {
    fn sha256_streaming(
        &self,
        _path: &std::path::Path,
        progress: &mut dyn FnMut(HashProgress),
    ) -> std::io::Result<ContentHash> {
        *self.call_count.lock().unwrap() += 1;
        for pct in &self.progress_pcts {
            progress(HashProgress {
                percent_complete: *pct,
                bytes_hashed: (*pct as u64) * 1_000_000,
            });
        }
        Ok(self.canned_hash)
    }
}

const HASH_A: ContentHash = ContentHash([0xAA; 32]);
const HASH_B: ContentHash = ContentHash([0xBB; 32]);

fn key(path: &str, mtime: u64, size: u64) -> Sha256CacheKey {
    Sha256CacheKey {
        path: PathBuf::from(path),
        mtime,
        size,
    }
}

// ---------------------------------------------------------------------------
// B1 — First call computes via Hasher; result returned.
// ---------------------------------------------------------------------------

#[test]
fn first_call_computes_hash_via_hasher() {
    let hasher = FakeHasher::new(HASH_A);
    let cache = Sha256Cache::new();
    let mut sink = |_: HashProgress| {};

    let result = cache
        .get_or_compute(key("/foo.gguf", 100, 4_400_000_000), &hasher, &mut sink)
        .expect("hash succeeds");

    assert_eq!(result, HASH_A, "first call returns Hasher's output");
    assert_eq!(hasher.call_count(), 1, "Hasher must be invoked once");
}

// ---------------------------------------------------------------------------
// B2 — Second call with same key returns cached value without invoking Hasher.
// ---------------------------------------------------------------------------

#[test]
fn second_call_with_same_key_returns_cached_without_recomputing() {
    let hasher = FakeHasher::new(HASH_A);
    let cache = Sha256Cache::new();
    let mut sink = |_: HashProgress| {};
    let k = key("/foo.gguf", 100, 4_400_000_000);

    let _ = cache.get_or_compute(k.clone(), &hasher, &mut sink).unwrap();
    let _ = cache.get_or_compute(k, &hasher, &mut sink).unwrap();

    assert_eq!(
        hasher.call_count(),
        1,
        "second call MUST hit cache; Hasher must not be invoked again"
    );
}

// ---------------------------------------------------------------------------
// B3 — Cache miss when mtime changes (file modified) → recompute.
// ---------------------------------------------------------------------------

#[test]
fn cache_miss_when_mtime_changes() {
    let hasher = FakeHasher::new(HASH_A);
    let cache = Sha256Cache::new();
    let mut sink = |_: HashProgress| {};

    let _ = cache
        .get_or_compute(key("/foo.gguf", 100, 4_400_000_000), &hasher, &mut sink)
        .unwrap();
    let _ = cache
        .get_or_compute(key("/foo.gguf", 200, 4_400_000_000), &hasher, &mut sink)
        .unwrap();

    assert_eq!(
        hasher.call_count(),
        2,
        "different mtime → cache miss → Hasher invoked twice"
    );
}

// ---------------------------------------------------------------------------
// B4 — Cache miss when size changes → recompute.
// ---------------------------------------------------------------------------

#[test]
fn cache_miss_when_size_changes() {
    let hasher = FakeHasher::new(HASH_B);
    let cache = Sha256Cache::new();
    let mut sink = |_: HashProgress| {};

    let _ = cache
        .get_or_compute(key("/foo.gguf", 100, 4_400_000_000), &hasher, &mut sink)
        .unwrap();
    let _ = cache
        .get_or_compute(key("/foo.gguf", 100, 5_000_000_000), &hasher, &mut sink)
        .unwrap();

    assert_eq!(
        hasher.call_count(),
        2,
        "different size → cache miss → Hasher invoked twice"
    );
}

// ---------------------------------------------------------------------------
// B5 — Hasher progress callbacks are forwarded to the progress sink.
// (Lazy-hash UX exercised on small input — the same code path runs on >50GB.)
// ---------------------------------------------------------------------------

#[test]
fn progress_callbacks_are_forwarded_to_sink() {
    let hasher = FakeHasher::new(HASH_A).with_progress(vec![10, 50, 100]);
    let cache = Sha256Cache::new();

    let observed: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let observed_clone = observed.clone();
    let mut sink = move |p: HashProgress| {
        observed_clone.lock().unwrap().push(p.percent_complete);
    };

    let _ = cache
        .get_or_compute(key("/big.gguf", 100, 50_000_000_000), &hasher, &mut sink)
        .unwrap();

    let pcts = observed.lock().unwrap().clone();
    assert_eq!(
        pcts,
        vec![10, 50, 100],
        "Hasher progress callbacks must reach the supplied sink"
    );
}

// ---------------------------------------------------------------------------
// B2-shared — Cache is sharable across cloned handles (Arc<Mutex<HashMap>>).
// Two handles backed by the same store deduplicate invocations.
// ---------------------------------------------------------------------------

#[test]
fn shared_cache_handles_deduplicate_across_clones() {
    let hasher = FakeHasher::new(HASH_A);
    let cache = Sha256Cache::new();
    let cache2 = cache.clone();
    let mut sink = |_: HashProgress| {};
    let k = key("/foo.gguf", 100, 4_400_000_000);

    let _ = cache.get_or_compute(k.clone(), &hasher, &mut sink).unwrap();
    let _ = cache2.get_or_compute(k, &hasher, &mut sink).unwrap();

    assert_eq!(
        hasher.call_count(),
        1,
        "cache.clone() must share the underlying store"
    );
}
