//! `modeltap cache verify` — full re-hash drift detector (US-27 AC-27-5).
//!
//! Unlike the lazy `(mtime,size,inode,dev)` quad-check used at warm-start, this
//! reads each persisted file's FULL content and recomputes its SHA256, so it
//! catches a same-mtime/same-size content swap the quad cannot. Drifted entries
//! are reported and their `cache_sha256` rows are corrected in place; a
//! `cache_verify drift_count=N` line is appended to diagnostics.log.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use modeltap_core::ports::Hasher;
use modeltap_store::stat_file_quad;
use modeltap_store::types::CachedSha256;
use modeltap_store::Cache;

use crate::sha256_cache::Sha2Hasher;
use crate::sha256_persistence::{hash_to_hex, open_store_cache};

/// Outcome of a `cache verify` pass over the persistent SHA256 store.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VerifyReport {
    /// Rows whose file was readable and re-hashed.
    pub checked: u64,
    /// Rows whose file could not be read (gone/permission) — skipped, not drift.
    pub skipped: u64,
    /// Paths whose recomputed hash differed from the stored hash. The cache
    /// rows for these were corrected in place to the recomputed value.
    pub drifted: Vec<PathBuf>,
}

/// Recompute every `cache_sha256` row's content hash, correct drifted rows in
/// place, and return the report. Pure of stdout/diagnostics side effects so it
/// is directly unit-testable; `run_cache_verify` wraps it with the CLI surface.
pub fn verify_cache(cache: &Cache, hasher: &dyn Hasher) -> VerifyReport {
    let mut report = VerifyReport::default();
    let rows = match cache.all_sha256() {
        Ok(rows) => rows,
        Err(_) => return report,
    };
    let mut sink = |_| {};
    for row in rows {
        let recomputed = match hasher.sha256_streaming(&row.path, &mut sink) {
            Ok(h) => h,
            Err(_) => {
                report.skipped += 1;
                continue;
            }
        };
        report.checked += 1;
        let recomputed_hex = hash_to_hex(&recomputed);
        if recomputed_hex != row.content_hash {
            report.drifted.push(row.path.clone());
            // Correct the row in place with the recomputed hash + fresh quad.
            if let Ok(Some(stat)) = stat_file_quad(&row.path) {
                let _ = cache.upsert_sha256(&CachedSha256 {
                    path: row.path.clone(),
                    stat,
                    content_hash: recomputed_hex,
                    computed_at: SystemTime::now(),
                });
            }
        }
    }
    report
}

/// CLI entry point for `modeltap cache verify`. Opens the cache at `cache_path`,
/// runs [`verify_cache`], prints drift to stdout, and appends
/// `cache_verify drift_count=N` to diagnostics.log. Returns the process exit
/// code (always 0 — verify is a report, not a gate).
pub fn run_cache_verify(cache_path: &Path) -> i32 {
    let Some(cache) = open_store_cache(Some(cache_path)) else {
        println!("cache verify: no cache at {}", cache_path.display());
        append_diagnostics("cache_verify drift_count=0");
        return 0;
    };
    let hasher = Sha2Hasher::new();
    let report = verify_cache(&cache, &hasher);
    for path in &report.drifted {
        println!("drift: {}", path.display());
    }
    append_diagnostics(&format!("cache_verify drift_count={}", report.drifted.len()));
    println!(
        "cache verify complete: {} checked, {} drifted, {} skipped",
        report.checked,
        report.drifted.len(),
        report.skipped
    );
    0
}

fn append_diagnostics(line: &str) {
    let Some(dir) = resolve_diagnostics_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("diagnostics.log");
    let mut s = line.to_string();
    s.push('\n');
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(s.as_bytes()));
}

/// Diagnostics dir: `MODELTAP_DIAGNOSTICS_DIR` override > `~/.modeltap` > None.
/// Mirrors the resolver in `modeltap-store::recovery`.
fn resolve_diagnostics_dir() -> Option<PathBuf> {
    if let Some(env_dir) = std::env::var_os("MODELTAP_DIAGNOSTICS_DIR") {
        return Some(PathBuf::from(env_dir));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".modeltap"))
}
