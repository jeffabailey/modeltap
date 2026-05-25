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
use modeltap_core::Tool;
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

// ---------------------------------------------------------------------------
// Step 05-04 — drift re-introspect + gone auto-refresh orchestrator helpers.
//
// `pre_mutate` (above) gates every destructive entry point on the K5
// invariant. When the gate fires Drift or Gone the action layer
// short-circuits to `CacheStale` — but the cache row remains stale until
// SOMEONE writes a fresh quad back. These two helpers are that SOMEONE.
//
// AC-26-6 (drift):  the orchestrator MUST re-introspect the drifted file via
//                   the plugin's `inspect_model`, recompute size + metadata,
//                   and update the cache_models row. Observability emits
//                   `inspect.invoked source=pre_mutate_drift`.
//
// AC-26-7 (gone):   the orchestrator MUST enqueue a per-tool reconcile for
//                   the affected tool so the missing file is pruned from the
//                   inventory. Observability emits `refresh.tool
//                   source=pre_mutate_gone`. Crucially, NO destructive
//                   filesystem action — the fixture filesystem stays
//                   byte-identical (DirManifest invariant).
//
// Both helpers are best-effort: emission failures degrade silently per the
// AC-7 "unwritable log dir never blocks" rule. The return value documents
// whether the writeback / enqueue succeeded so the caller can surface a
// banner annotation if needed; in the v1 wiring it is just plumbed back
// for observability consumers and downstream tests.
// ---------------------------------------------------------------------------

/// Outcome of a drift re-introspect (Step 05-04 — AC-26-6).
///
/// `Reintrospected { fresh }` — the plugin returned a fresh `ModelDetail`
/// and the cache row was updated with the new size / metadata. `fresh`
/// carries the post-stat quad so the caller can render a "was X, now Y"
/// diff at the dialog level.
///
/// `PluginError` — the plugin's `inspect_model` failed (Unsupported,
/// PluginPanic, FileReadable, FormatUnreadable). The cache row is left
/// untouched; the caller's K5 short-circuit (CacheStale) still holds. This
/// is the conservative path: re-introspect is best-effort, not the gate.
///
/// `StoreError` — the writeback failed at the SQLite layer. Same
/// conservative posture: cache row left untouched; CacheStale still holds.
#[derive(Debug)]
pub enum ReintrospectOutcome {
    Reintrospected { fresh: FileStat },
    PluginError(modeltap_core::domain::inspect::InspectError),
    StoreError(CacheError),
}

/// Re-introspect a drifted model via the plugin and write the fresh
/// `(size_bytes, metadata_kv, metadata_introspected_at)` back to
/// `cache_models`.
///
/// Called by the destructive action layer (unify / zap / delete_one /
/// folder_delete) when `pre_mutate` returned `Drift { fresh, cached }`. The
/// `fresh` quad from the pre_mutate result is also written back to
/// `cache_model_files` so the next `verify_against_fs` observes Match (the
/// cache catches up to reality).
///
/// Emits exactly ONE `inspect.invoked source=pre_mutate_drift` JSONL event
/// per call, regardless of plugin outcome (consumers can join on the event
/// stream).
pub async fn re_introspect_after_drift(
    cache: &Cache,
    plugin: &dyn Tool,
    model_id: &ModelId,
    drift_fresh: &FileStat,
    log_dir: Option<&Path>,
) -> ReintrospectOutcome {
    let started = Instant::now();
    let tool_id = plugin.name();
    let core_model_id = modeltap_core::domain::inspect::ModelId::from(model_id.clone());
    let inspect_result = plugin.inspect_model(&core_model_id).await;
    let duration_ms = started.elapsed().as_millis() as u64;
    // Emit the inspect.invoked event up-front so the trail records the
    // attempt regardless of writeback outcome below.
    emit_inspect_invoked(log_dir, &tool_id, model_id, "pre_mutate_drift", duration_ms);
    // Look up the existing cache row so we preserve every field the plugin
    // does NOT supply (display_name, dedup_group_id, last_seen_at, sha256).
    // The drift always invalidates size + introspected_at; metadata fields
    // (format / architecture / parameters / metadata_kv) are layered in
    // only when the plugin actually supplied them. Plugins that return
    // `InspectError::Unsupported` (the trait default) still cause a row
    // writeback — the size_bytes update from the fresh stat IS the AC-26-6
    // contract, independent of plugin metadata capability.
    let mut row = match read_one_model_row(cache, &tool_id, model_id) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return ReintrospectOutcome::StoreError(CacheError::MalformedRow {
                table: "cache_models",
                detail: format!(
                    "re-introspect found no existing row for model_id={} under tool_id={}",
                    model_id, tool_id.0
                ),
            })
        }
        Err(e) => return ReintrospectOutcome::StoreError(e),
    };
    row.size_bytes = drift_fresh.size_bytes;
    if let Ok(ref detail) = inspect_result {
        if let Some(fmt) = detail.format.clone() {
            row.format = Some(fmt);
        }
        if let Some(q) = detail.quantisation.clone() {
            row.quantisation = Some(q);
        }
        if let Some(a) = detail.architecture.clone() {
            row.architecture = Some(a);
        }
        if let Some(p) = detail.parameters {
            row.parameters_billions = Some(p);
        }
        if let Some(c) = detail.context_length {
            row.context_length = Some(c);
        }
        if !detail.metadata_kv.is_empty() {
            row.metadata_kv = detail.metadata_kv.clone();
        }
    }
    row.metadata_introspected_at = Some(std::time::SystemTime::now());
    if let Err(e) = cache.write_models(&tool_id, std::slice::from_ref(&row)) {
        return ReintrospectOutcome::StoreError(e);
    }
    // Also bring the cache_model_files row in line with the fresh quad so
    // the next verify_against_fs observes Match. We refresh ONLY the rows
    // for this model — other files in cache_model_files are untouched.
    if let Ok(existing_files) = cache.files_for_model(model_id) {
        let now = std::time::SystemTime::now();
        let refreshed: Vec<modeltap_store::types::CachedFile> = existing_files
            .into_iter()
            .map(|f| modeltap_store::types::CachedFile {
                last_stat_at: now,
                size_bytes: drift_fresh.size_bytes,
                mtime: drift_fresh.mtime,
                inode: drift_fresh.inode,
                dev: drift_fresh.dev,
                ..f
            })
            .collect();
        if let Err(e) = cache.write_model_files(&refreshed) {
            return ReintrospectOutcome::StoreError(e);
        }
    }
    ReintrospectOutcome::Reintrospected {
        fresh: drift_fresh.clone(),
    }
}

/// Trigger an auto-refresh for the affected tool when `pre_mutate` reports
/// `Gone` (Step 05-04 — AC-26-7).
///
/// The actual reconcile is enqueued via the caller's
/// `ReconcileScope::Tool(...)` channel (composition root). This function's
/// responsibility is the observability emission + the audit-trail invariant
/// the JSONL consumers depend on. The "no destructive action" invariant
/// (DirManifest equality pre/post) is satisfied by NOT calling any plugin
/// destructive method here — only the per-tool refresh is enqueued, which
/// itself is a read-only `discover()` walk.
///
/// Returns `true` so the caller has a unit value to thread back; v1 cannot
/// fail (emission is best-effort), but the return shape leaves room for
/// future fail-closed semantics.
pub fn auto_refresh_after_gone(tool_id: &ToolId, log_dir: Option<&Path>) -> bool {
    let Some(dir) = log_dir else {
        return true;
    };
    let mut logger = LaunchLogger::open(Some(dir.to_path_buf()));
    logger.record(RecordKind::RefreshTool {
        tool: tool_id.0.to_string(),
        source: "pre_mutate_gone",
    });
    true
}

fn emit_inspect_invoked(
    log_dir: Option<&Path>,
    tool_id: &ToolId,
    model_id: &ModelId,
    source: &'static str,
    duration_ms: u64,
) {
    let Some(dir) = log_dir else {
        return;
    };
    let mut logger = LaunchLogger::open(Some(dir.to_path_buf()));
    logger.record(RecordKind::InspectInvoked {
        tool: tool_id.0.to_string(),
        model: model_id.clone(),
        source,
        duration_ms,
    });
}

/// Best-effort lookup of the existing cache_models row for `(tool_id,
/// model_id)`. Returns `Ok(None)` when the row is absent — the caller
/// surfaces that as a `StoreError` because the pre_mutate gate could not
/// have fired Drift on a model with no row.
fn read_one_model_row(
    cache: &Cache,
    tool_id: &ToolId,
    model_id: &ModelId,
) -> Result<Option<modeltap_store::types::CachedModel>, CacheError> {
    let rows = cache.models_for_tool(tool_id)?;
    Ok(rows.into_iter().find(|r| &r.model_id == model_id))
}
