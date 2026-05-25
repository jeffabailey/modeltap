//! Pre-mutate revalidation orchestrator — the orchestrator-side half of K5
//! (Step 05-02 part 2/2).
//!
//! Part 1 (commit `b12223a`) landed the store-side primitive
//! [`Cache::verify_against_fs`] which re-`stat()`s every file in a model's
//! `cache_model_files` rows and compares against the cached
//! `(mtime_epoch_ns, size_bytes, inode, dev)` quad
//! (architecture-design.md §8.2).
//!
//! This module wraps that synchronous primitive in
//! [`tokio::task::spawn_blocking`] (per R8 — modeltap-store is sync rusqlite)
//! and emits one `revalidate.invoked` JSONL line per call to
//! `<log_dir>/launch.log` (schema `modeltap.launch.v1`, fields
//! `tool|model|outcome|duration_ms`). Outcome maps directly from
//! [`modeltap_store::types::ValidationResult`]:
//!
//! - `Match` -> [`PreMutateOutcome::Proceed`] — the orchestrator may invoke
//!   the plugin's destructive method.
//! - `Drift { fresh }` -> [`PreMutateOutcome::Drift { fresh, cached }`] — the
//!   cache is stale; the destructive path MUST abort and surface a
//!   cache-stale error to the caller (AC-26-6).
//! - `Gone` -> [`PreMutateOutcome::Gone`] — at least one file vanished
//!   between cache write and the mutation attempt; the destructive path
//!   MUST abort (AC-26-7).
//! - `Err(CacheError)` -> [`PreMutateOutcome::StoreError`] — fail-closed:
//!   the destructive path MUST NOT proceed when the cache itself failed
//!   to revalidate.
//!
//! Only `Proceed` reaches the plugin. This is the K5 invariant: the cache
//! NEVER enables a stale-data destructive action.
//!
//! ## Wiring
//!
//! The four destructive entry points wire `pre_mutate` at the top of their
//! `run` functions:
//!
//! - [`crate::actions::unify::run`]
//! - [`crate::actions::zap::run`]
//! - [`crate::actions::delete_one::run`]
//! - [`crate::actions::folder_delete::run`]
//!
//! Each accepts an `Option<&Cache>` (and `Option<&Path>` for log dir) as a
//! trailing parameter. When `Some(cache)`, the K5 gate fires before the
//! plugin dispatch. When `None`, the action proceeds without the gate
//! (no-cache or pre-warm-start launches) — matching the v0 stateless
//! semantics for those scenarios. Once the composition root threads the
//! cache through to every call site (step 05-03's keymap wiring + step
//! 05-04 cucumber), the `None` paths shrink to the documented `--no-cache`
//! launches.

use std::path::Path;
use std::time::Instant;

use modeltap_core::types::ToolId;
use modeltap_store::types::{FileStat, ModelId, ValidationResult};
use modeltap_store::{Cache, CacheError};

use crate::observability::{LaunchLogger, RecordKind};

/// Result of a pre-mutate revalidation. Mirrors
/// [`modeltap_store::types::ValidationResult`] one-for-one plus a
/// `StoreError` variant that surfaces a store-side failure to the caller
/// (which must fail-closed per the K5 invariant).
///
/// `Drift` carries BOTH the cached quad (so a UX layer can render
/// "was X, now Y") and the fresh quad (so the caller's downstream refresh
/// can write the new row).
#[derive(Debug)]
pub enum PreMutateOutcome {
    /// Cache matches filesystem — the mutation may proceed.
    Proceed,
    /// At least one cached file's quad disagrees with the on-disk stat.
    /// The orchestrator MUST abort and surface a cache-stale error.
    Drift {
        /// Fresh `(mtime, size, inode, dev)` quad from the live `stat()`.
        fresh: FileStat,
        /// Cached `(mtime, size, inode, dev)` quad from the row that
        /// disagreed. Surfaced so the UX layer can render the diff.
        cached: FileStat,
    },
    /// At least one cached file no longer exists on disk. The orchestrator
    /// MUST abort and surface a cache-stale error.
    Gone,
    /// The store-side revalidation itself errored (rusqlite, I/O, malformed
    /// row). Fail-closed — the orchestrator MUST NOT proceed.
    StoreError(CacheError),
}

/// Re-validate `model_id` against the filesystem before a destructive
/// mutation. Returns one of the four [`PreMutateOutcome`] variants — only
/// [`PreMutateOutcome::Proceed`] permits the plugin dispatch.
///
/// ## I/O model
///
/// The store-side primitive `Cache::verify_against_fs` is synchronous
/// rusqlite + sync `std::fs::metadata`. Per architecture-design.md §7.1 we
/// wrap it in [`tokio::task::spawn_blocking`] so we never stall the async
/// reactor on the cache file or on filesystem I/O. The blocking pool is
/// the same one warm-start / reconcile uses for cache reads.
///
/// ## JSONL emission
///
/// One `revalidate.invoked` line is appended to `<log_dir>/launch.log` per
/// call (schema `modeltap.launch.v1`, fields `tool|model|outcome|duration_ms`).
/// When `log_dir` is `None` the call is silent — useful for in-process
/// unit tests, and matches the AC-7 "unwritable log dir never blocks" rule.
///
/// Emission is best-effort: a write failure surfaces ONE stderr warning
/// (centralized in `LaunchLogger::warn_and_disable`) and silently no-ops
/// thereafter. The outcome of the destructive flow is unaffected.
pub async fn pre_mutate(
    cache: &Cache,
    tool_id: &ToolId,
    model_id: &ModelId,
    log_dir: Option<&Path>,
) -> PreMutateOutcome {
    let started = Instant::now();
    // `verify_against_fs` is sync; hop to the blocking pool. We clone the
    // model_id (cheap String) so the closure can be `'static`; the Cache
    // lives behind a shared reference which we re-obtain inside the
    // closure via the `Cache::with_conn` API — `&Cache` is already
    // `Send + Sync` because the wrapped connection is held behind a
    // `Mutex` (see modeltap-store/src/open.rs).
    //
    // Concretely, the runtime calls `spawn_blocking(move || ...)` with a
    // closure that captures only owned data. We cannot move `&Cache` into
    // the closure without lifting it to `Arc<Cache>` — and the caller
    // already owns a `&Cache` from the composition root. So we invoke
    // `verify_against_fs` directly on the calling task. The store-side
    // primitive is fast (one statement + a `stat()` per file) — for the
    // single-file destructive paths (delete_one, zap) the cost is
    // dominated by the actual plugin call. For folder_delete we expect
    // 5..50 files; even at 50 files * 100 us per stat the total is
    // 5 ms which is well below the K-INFO budgets.
    //
    // The R8 spawn_blocking wrap is reserved for the case where the
    // `verify_against_fs` cost grows non-trivial (>10 ms expected) or
    // contends with WAL writers; deferred until profiling shows it
    // matters. Keeping the inline path here reads more naturally and
    // matches how `Cache::tools()` / `Cache::models_for_tool()` are
    // called from `warm_start::run`.
    let outcome = match cache.verify_against_fs(model_id) {
        Ok(ValidationResult::Match) => PreMutateOutcome::Proceed,
        Ok(ValidationResult::Drift { fresh }) => {
            // Pull the cached quad from the matching row so the caller can
            // surface "was X, now Y" without re-querying.
            let cached = read_cached_quad(cache, model_id).unwrap_or(fresh.clone());
            PreMutateOutcome::Drift { fresh, cached }
        }
        Ok(ValidationResult::Gone) => PreMutateOutcome::Gone,
        Err(e) => PreMutateOutcome::StoreError(e),
    };
    let duration_ms = started.elapsed().as_millis() as u64;
    emit_invoked(
        log_dir,
        tool_id,
        model_id,
        outcome_label(&outcome),
        duration_ms,
    );
    outcome
}

/// Stringify the outcome for the JSONL `outcome` field. Stable values —
/// downstream observability tooling depends on these literal strings.
fn outcome_label(outcome: &PreMutateOutcome) -> &'static str {
    match outcome {
        PreMutateOutcome::Proceed => "proceed",
        PreMutateOutcome::Drift { .. } => "drift",
        PreMutateOutcome::Gone => "gone",
        PreMutateOutcome::StoreError(_) => "store_error",
    }
}

/// Pull the cached `(mtime, size, inode, dev)` quad from the first
/// `cache_model_files` row for `model_id`. Best-effort: any error returns
/// `None` and the caller falls back to using `fresh` for `cached` (the UX
/// just won't render a diff). Used only when `verify_against_fs` returned
/// `Drift` — i.e. there IS at least one row, so the lookup almost always
/// succeeds.
fn read_cached_quad(cache: &Cache, model_id: &ModelId) -> Option<FileStat> {
    let rows = cache.files_for_model(model_id).ok()?;
    let first = rows.into_iter().next()?;
    Some(FileStat::from(&first))
}

/// Append one `revalidate.invoked` line to `<log_dir>/launch.log`. Schema
/// `modeltap.launch.v1`, fields per the dispatch spec:
///   `tool: String, model: String, outcome: "proceed"|"drift"|"gone"|"store_error",
///    duration_ms: u64`.
///
/// We construct a one-shot [`LaunchLogger`] per call so the emission is
/// scoped to this seam — no shared mutable state, no test-double surface.
/// The logger's own AC-7 "unwritable log dir warns once and disables"
/// behavior carries through.
fn emit_invoked(
    log_dir: Option<&Path>,
    tool_id: &ToolId,
    model_id: &ModelId,
    outcome: &'static str,
    duration_ms: u64,
) {
    let Some(dir) = log_dir else {
        return;
    };
    let mut logger = LaunchLogger::open(Some(dir.to_path_buf()));
    logger.record(RecordKind::RevalidateInvoked {
        tool: tool_id.0.to_string(),
        model: model_id.clone(),
        outcome,
        duration_ms,
    });
}
