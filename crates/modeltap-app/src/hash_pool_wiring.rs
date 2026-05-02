//! Composition-root wiring helpers for the background hash pool (step 01-08).
//!
//! Lives in `modeltap-app` (library half) so both `headless::run` and
//! `interactive::event_loop` can reuse the same `HashJob`-construction logic
//! without copy-paste. Pure data transform: no I/O beyond the per-file `stat`
//! that captures `(mtime, size)` for the cache key (per ADR-013 §"Cache key:
//! `(path, mtime, size)`"). The actual hashing happens later in the worker
//! pool.
//!
//! ## Why not in `hash_pool/mod.rs`?
//!
//! `hash_pool` owns the worker pool itself (queue, workers, throttle, handle).
//! This module owns the bridge from `Vec<DiscoveredModel>` → `Vec<HashJob>`.
//! Keeping the bridge separate makes the pool reusable by future callers
//! (e.g., a CLI mode that hashes a single path) without dragging the
//! discovery-shaped input.
//!
//! ## Contract
//!
//! `build_hash_jobs(&[(ToolId, &[DiscoveredModel])]) -> Vec<HashJob>` returns
//! one `HashJob` per `(tool, model)` pair. The job's `mtime` and `size` are
//! captured from `std::fs::metadata` AT JOB-CONSTRUCTION TIME (per ADR-013
//! §"so a file changed mid-hash will not poison the next launch's cache").
//! When a path's metadata cannot be read, the job is still emitted with
//! `mtime = 0` and `size` falling back to the discovery-reported `size_bytes`
//! — the worker will surface the read error as `Msg::HashFailed` when it
//! actually opens the file.

use modeltap_core::{DiscoveredModel, ToolId};

use crate::hash_pool::HashJob;

/// Build the queue of `HashJob`s the composition root passes to
/// `hash_pool::spawn`. One job per `(tool, model)` pair; tool order matches
/// the input slice's order; within a tool, model order matches the
/// `discover()` slice's order.
///
/// Per-job `mtime`/`size` are captured here (one `stat` per file) so the
/// `Sha256Cache` key is stable against file mutations between the discovery
/// pass and the actual hash. This is the ONLY filesystem touch this helper
/// performs; the hashing happens later inside the pool's `spawn_blocking`
/// workers.
pub fn build_hash_jobs(per_tool: &[(ToolId, &[DiscoveredModel])]) -> Vec<HashJob> {
    let mut jobs: Vec<HashJob> = Vec::new();
    for (tool, models) in per_tool {
        for model in *models {
            let (mtime, size) = stat_or_fallback(&model.on_disk_path, model.size_bytes);
            jobs.push(HashJob {
                tool: *tool,
                model_id: model.id_in_tool.clone(),
                path: model.on_disk_path.clone(),
                mtime,
                size,
            });
        }
    }
    jobs
}

/// Best-effort `(mtime, size)` lookup. Returns `(0, fallback_size)` on any
/// `metadata` error so the job still flows through the queue — the worker
/// will produce a `Msg::HashFailed::Io(...)` when it tries to open the file.
/// Doing the stat here (instead of inside the worker) lets the cache key be
/// stable against mutations between job-construction and worker execution.
fn stat_or_fallback(path: &std::path::Path, fallback_size: u64) -> (u64, u64) {
    match std::fs::metadata(path) {
        Ok(m) => {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (mtime, m.len())
        }
        Err(_) => (0, fallback_size),
    }
}
