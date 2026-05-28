//! `modeltap cache verify` logic — full re-hash drift detection (US-27 AC-27-5).
//!
//! The CLI surface (`cache_verify::run_cache_verify`) is thin glue over
//! `verify_cache`; this exercises the core: a row whose persisted `content_hash`
//! no longer matches the file's actual content (a same-quad content swap the
//! lazy quad-check cannot catch) is reported as drift AND corrected in place.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use modeltap_app::cache_verify::verify_cache;
use modeltap_app::sha256_cache::Sha2Hasher;
use modeltap_app::sha256_persistence::hash_to_hex;
use modeltap_core::ports::Hasher;
use modeltap_store::types::CachedSha256;
use modeltap_store::{stat_file_quad, Cache};

fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    let mut f = std::fs::File::create(&p).expect("create file");
    f.write_all(bytes).expect("write");
    p
}

fn hash_hex(hasher: &Sha2Hasher, path: &Path) -> String {
    let mut sink = |_| {};
    hash_to_hex(&hasher.sha256_streaming(path, &mut sink).expect("hash"))
}

fn upsert(cache: &Cache, path: &Path, content_hash: String) {
    let stat = stat_file_quad(path).expect("stat").expect("present");
    cache
        .upsert_sha256(&CachedSha256 {
            path: path.to_path_buf(),
            stat,
            content_hash,
            computed_at: SystemTime::now(),
        })
        .expect("upsert");
}

#[test]
fn verify_detects_content_drift_and_corrects_the_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let hasher = Sha2Hasher::new();

    let good = write_file(dir.path(), "good.gguf", b"unchanged content");
    let swapped = write_file(dir.path(), "swapped.gguf", b"NEW content swapped since hashing");

    // good.gguf: persisted hash matches the file.
    upsert(&cache, &good, hash_hex(&hasher, &good));
    // swapped.gguf: STALE hash (the quad still matches the on-disk file, but the
    // content_hash is wrong — exactly the same-mtime/size content swap that only
    // a full re-hash can catch).
    let stale = "0".repeat(64);
    upsert(&cache, &swapped, stale.clone());

    let report = verify_cache(&cache, &hasher);

    assert_eq!(report.checked, 2, "both readable rows are re-hashed");
    assert_eq!(report.skipped, 0);
    assert_eq!(
        report.drifted,
        vec![swapped.clone()],
        "only the content-swapped row drifts"
    );

    // The drifted row was corrected to the true hash; the good row is untouched.
    let corrected = cache
        .get_sha256_by_path(&swapped)
        .expect("get")
        .expect("present");
    assert_ne!(corrected.content_hash, stale, "stale hash was replaced");
    assert_eq!(corrected.content_hash, hash_hex(&hasher, &swapped));
}

#[test]
fn verify_reports_no_drift_when_every_hash_matches() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let hasher = Sha2Hasher::new();

    let f = write_file(dir.path(), "model.gguf", b"stable bytes");
    upsert(&cache, &f, hash_hex(&hasher, &f));

    let report = verify_cache(&cache, &hasher);
    assert_eq!(report.checked, 1);
    assert!(report.drifted.is_empty(), "no drift when the hash matches");
}

#[test]
fn verify_skips_a_row_whose_file_is_gone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let hasher = Sha2Hasher::new();

    let f = write_file(dir.path(), "vanishing.gguf", b"here for now");
    upsert(&cache, &f, hash_hex(&hasher, &f));
    std::fs::remove_file(&f).expect("remove");

    let report = verify_cache(&cache, &hasher);
    assert_eq!(report.checked, 0);
    assert_eq!(report.skipped, 1, "a vanished file is skipped, not drift");
    assert!(report.drifted.is_empty());
}
