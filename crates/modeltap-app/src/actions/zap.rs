//! `actions::zap::run` — orchestrates a confirmed zap-all action.
//!
//! Called by the headless / production event loop when the dialog returns
//! `UpdateEffect::trigger_zap = Some(tool_id)`. Per ADR-009, this calls
//! `Tool::delete_all` ONCE — NOT a loop of `delete_one`. The plugin's
//! `delete_all` performs its own transactional manifest+blob cleanup.
//!
//! Per `kpi-instrumentation.md` §"action.zap_all", on completion we emit
//! exactly one JSONL event with the cross-tool aggregate: tool name,
//! models removed, bytes reclaimed, outcome string. No model names, no
//! paths, no usernames — the schema is privacy-preserving by design.

use std::path::Path;

use modeltap_core::{DeleteOutcome, Tool, ToolId};
use modeltap_store::Cache;

use crate::observability::{LaunchLogger, RecordKind};
use modeltap_app::orchestration::revalidate::{self, PreMutateOutcome};

/// Result of a confirmed zap-all action. Surfaced to the right pane as the
/// "Last action" footer and used by acceptance tests to assert success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZapOutcome {
    pub tool: ToolId,
    pub models_removed: u64,
    pub bytes_reclaimed: u64,
    pub outcome: ZapResult,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ZapResult {
    /// Every manifest was successfully removed. `models_removed > 0`.
    Success,
    /// Some manifests removed, some failed. `models_removed > 0` but less
    /// than the attempted set.
    Partial,
    /// Nothing to remove (empty tool). `models_removed == 0`.
    Empty,
    /// `delete_all` returned an error before any work. `models_removed == 0`,
    /// `bytes_reclaimed == 0`. The error is logged to the diagnostics log.
    Failed,
    /// Step 05-02 part 2/2 — K5 gate fired: at least one of the tool's
    /// cached models failed pre-mutate revalidation (Drift / Gone / store
    /// error). No `delete_all` call, no filesystem mutation. The caller
    /// MUST dispatch a per-tool refresh + re-prompt per AC-26-6 / AC-26-7.
    /// JSONL `outcome` field carries `"cache_stale"`.
    CacheStale,
}

impl ZapResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            ZapResult::Success => "success",
            ZapResult::Partial => "partial",
            ZapResult::Empty => "empty",
            ZapResult::Failed => "failed",
            ZapResult::CacheStale => "cache_stale",
        }
    }
}

/// Run a confirmed zap-all action. Calls the plugin's `delete_all`,
/// classifies the outcome, emits one `action.zap_all` JSONL event, and
/// returns a `ZapOutcome` for the UI footer.
///
/// Step 05-02 part 2/2 — when `cache` is `Some(c)`, every model the
/// plugin's `discover()` enumerates is revalidated via
/// `revalidate::pre_mutate` BEFORE the `delete_all` dispatch. The first
/// non-Proceed short-circuits with `ZapResult::CacheStale` — zero
/// `delete_all` invocations, zero filesystem mutation. `None` preserves
/// the v0 behaviour (no gate) for `--no-cache` launches and pre-step-05-04
/// call sites.
pub async fn run(
    plugin: &dyn Tool,
    logger: &mut LaunchLogger,
    cache: Option<&Cache>,
    cache_log_dir: Option<&Path>,
) -> ZapOutcome {
    let tool_id = plugin.name();

    // K5 pre-mutate gate. Enumerate the tool's current model set via the
    // same `discover()` the destructive path will operate on; revalidate
    // each against the cache. Empty inventory = nothing to gate (the
    // `Empty` outcome is produced inside the existing classify path
    // anyway). If `discover()` fails we fail-closed via CacheStale so the
    // caller surfaces "could not verify cache before zap" rather than
    // silently bypassing the gate.
    if let Some(c) = cache {
        match plugin.discover().await {
            Ok(models) => {
                for m in &models {
                    match revalidate::pre_mutate(c, &tool_id, &m.id_in_tool, cache_log_dir).await {
                        PreMutateOutcome::Proceed => continue,
                        PreMutateOutcome::Drift { .. }
                        | PreMutateOutcome::Gone
                        | PreMutateOutcome::StoreError(_) => {
                            let outcome = ZapOutcome {
                                tool: tool_id,
                                models_removed: 0,
                                bytes_reclaimed: 0,
                                outcome: ZapResult::CacheStale,
                            };
                            emit(logger, &outcome);
                            return outcome;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    target: "modeltap.action.zap",
                    "discover failed during K5 pre-mutate gate: {e}"
                );
                let outcome = ZapOutcome {
                    tool: tool_id,
                    models_removed: 0,
                    bytes_reclaimed: 0,
                    outcome: ZapResult::CacheStale,
                };
                emit(logger, &outcome);
                return outcome;
            }
        }
    }

    let result = plugin.delete_all().await;
    let outcome = match result {
        Ok(outcomes) => classify(&outcomes, tool_id),
        Err(e) => {
            tracing::warn!(target: "modeltap.action.zap", "delete_all failed: {e}");
            // For a hard failure, emit a `failed` event so observability
            // sees the attempt without revealing path/model details.
            let outcome = ZapOutcome {
                tool: tool_id,
                models_removed: 0,
                bytes_reclaimed: 0,
                outcome: ZapResult::Failed,
            };
            emit(logger, &outcome);
            return outcome;
        }
    };
    emit(logger, &outcome);
    outcome
}

/// Map a `Vec<DeleteOutcome>` to a single aggregate `ZapOutcome` for the
/// tool-level event. Bytes reclaimed sums across outcomes (the plugin
/// already deduplicated shared blobs).
fn classify(outcomes: &[DeleteOutcome], tool_id: ToolId) -> ZapOutcome {
    let attempted = outcomes.len() as u64;
    let removed: u64 = outcomes.iter().filter(|o| o.registration_removed).count() as u64;
    let bytes: u64 = outcomes.iter().map(|o| o.bytes_freed).sum();
    let result = if attempted == 0 {
        ZapResult::Empty
    } else if removed == attempted {
        ZapResult::Success
    } else if removed > 0 {
        ZapResult::Partial
    } else {
        // All attempts failed — surface as Failed.
        ZapResult::Failed
    };
    ZapOutcome {
        tool: tool_id,
        models_removed: removed,
        bytes_reclaimed: bytes,
        outcome: result,
    }
}

fn emit(logger: &mut LaunchLogger, outcome: &ZapOutcome) {
    logger.record(RecordKind::ActionZapAll {
        tool: outcome.tool.to_string(),
        models_removed: outcome.models_removed,
        bytes_reclaimed: outcome.bytes_reclaimed,
        outcome: outcome.outcome.as_str(),
    });
}
