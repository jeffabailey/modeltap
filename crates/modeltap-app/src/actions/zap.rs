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

use modeltap_core::{DeleteOutcome, Tool, ToolId};

use crate::observability::{LaunchLogger, RecordKind};

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
}

impl ZapResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            ZapResult::Success => "success",
            ZapResult::Partial => "partial",
            ZapResult::Empty => "empty",
            ZapResult::Failed => "failed",
        }
    }
}

/// Run a confirmed zap-all action. Calls the plugin's `delete_all`,
/// classifies the outcome, emits one `action.zap_all` JSONL event, and
/// returns a `ZapOutcome` for the UI footer.
pub async fn run(plugin: &dyn Tool, logger: &mut LaunchLogger) -> ZapOutcome {
    let tool_id = plugin.name();
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
