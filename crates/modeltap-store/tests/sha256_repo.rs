//! Sha256Repo CRUD — file-level cache_sha256 round-trip (US-27, step 01-02).
//!
//! The repo is the Tier-3 persistence surface from ADR-018: per-file content
//! hash keyed by absolute path, carrying the (mtime,size,inode,dev) validity
//! quad. These tests pin the minimum CRUD: upsert, get-by-path, invalidate,
//! and all (for `cache verify`).

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use modeltap_store::types::{CachedSha256, FileStat};
use modeltap_store::Cache;

fn sample_entry(path: &str, hash: &str) -> CachedSha256 {
    CachedSha256 {
        path: PathBuf::from(path),
        stat: FileStat {
            size_bytes: 4_368_438_912,
            mtime: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            inode: 123_456,
            dev: 66_310,
        },
        content_hash: hash.to_string(),
        computed_at: UNIX_EPOCH + Duration::from_secs(1_700_001_000),
    }
}

#[test]
fn upsert_then_get_by_path_round_trips() {
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let entry = sample_entry(
        "/home/devon/llms/mistral-7b-instruct-q4_K_M.gguf",
        "e8a35b5e2f4f4e7a1c8f6b9d3c1a5e7f9a2c4e6b8d0f1a3c5e7b9d1f3a5c7e9b",
    );

    cache.upsert_sha256(&entry).expect("upsert_sha256");

    let got = cache
        .get_sha256_by_path(&entry.path)
        .expect("get_sha256_by_path")
        .expect("row must be present after upsert");
    assert_eq!(got, entry, "round-trip must be field-identical");
    assert_eq!(
        got.content_hash, entry.content_hash,
        "lowercase hex content_hash must be preserved verbatim"
    );
}

#[test]
fn get_by_path_unknown_returns_none() {
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let missing = cache
        .get_sha256_by_path(Path::new("/nope/not-here.gguf"))
        .expect("get_sha256_by_path");
    assert!(missing.is_none(), "unknown path must return None");
}

#[test]
fn upsert_same_path_updates_last_writer_wins() {
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let path = "/home/devon/llms/model.gguf";
    cache
        .upsert_sha256(&sample_entry(path, "a".repeat(64).as_str()))
        .expect("first upsert");

    let mut second = sample_entry(path, "b".repeat(64).as_str());
    second.stat.size_bytes = 9_000_000;
    cache.upsert_sha256(&second).expect("second upsert");

    let got = cache
        .get_sha256_by_path(Path::new(path))
        .expect("get")
        .expect("present");
    assert_eq!(got.content_hash, "b".repeat(64), "last writer wins");
    assert_eq!(got.stat.size_bytes, 9_000_000, "quad updated too");
}

#[test]
fn invalidate_removes_the_row() {
    let cache = Cache::open_in_memory().expect("open_in_memory");
    let entry = sample_entry("/x/y.gguf", "c".repeat(64).as_str());
    cache.upsert_sha256(&entry).expect("upsert");

    cache.invalidate_sha256(&entry.path).expect("invalidate");

    let got = cache.get_sha256_by_path(&entry.path).expect("get");
    assert!(got.is_none(), "invalidated row must be gone");
}

#[test]
fn all_sha256_returns_every_row() {
    let cache = Cache::open_in_memory().expect("open_in_memory");
    cache
        .upsert_sha256(&sample_entry("/a.gguf", "1".repeat(64).as_str()))
        .expect("upsert a");
    cache
        .upsert_sha256(&sample_entry("/b.gguf", "2".repeat(64).as_str()))
        .expect("upsert b");

    let mut all = cache.all_sha256().expect("all_sha256");
    all.sort_by(|x, y| x.path.cmp(&y.path));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].path, PathBuf::from("/a.gguf"));
    assert_eq!(all[1].path, PathBuf::from("/b.gguf"));
}
