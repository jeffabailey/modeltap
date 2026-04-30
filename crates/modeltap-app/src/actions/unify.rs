//! `actions::unify::run` — orchestrates a confirmed unify action (US-10).
//!
//! Called by the headless / production event loop when the dialog returns
//! `UpdateEffect::trigger_unify = Some(plan)`. Per ADR-008, this calls each
//! plugin's `Tool::link` per the plan, collects per-target outcomes, and
//! emits exactly one JSONL `action.unify` event with privacy-preserving
//! aggregates (no model names, no paths, no hashes).
//!
//! ## v1 partial-success policy
//!
//! Per the dispatch directive: cross-filesystem and content-mismatch errors
//! are collected as failed targets; the orchestrator does NOT abort the
//! remaining targets. The user-facing [s/c/x] dialog (US-19) lands in
//! step 03-03 — for now we record `outcome = "partial"` when 1+ link
//! succeeded and 1+ failed.
//!
//! On UNEXPECTED IO errors mid-sequence (not cross-fs, not content-mismatch),
//! the orchestrator logs a warning and continues with the next target. A
//! true atomic-or-revert is deferred to the v1.x rollback work; the
//! conservative-when-uncertain rule (ADR-002) means partial success is
//! always reported truthfully — the user is never told "all linked" when
//! some targets failed.
//!
//! ## Privacy (kpi-instrumentation §"action.unify")
//!
//! Emitted event carries: model_dedup_key_kind ("sha256" — the only kind in
//! v1), tools_unified (sorted ToolId strings), bytes_reclaimed (u64),
//! outcome ("success"|"partial"|"already_unified"|"failed"). NO model
//! names, NO paths, NO hash values.

use std::path::PathBuf;

use modeltap_core::logic::plan::{PlannedLink, UnifyPlan};
use modeltap_core::{
    DedupKey, DisplayLabel, Format, LinkError, LinkOutcome, LinkResult, ModelMeta, ModelStatus,
    Tool, ToolId,
};

use crate::observability::{LaunchLogger, RecordKind};

/// Result of a confirmed unify action. Surfaced to the right pane as the
/// "Last action" footer and used by acceptance tests to assert success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifyOutcome {
    /// Tool ids whose targets were successfully linked (or were already
    /// linked). Sorted for deterministic JSONL output.
    pub tools_unified: Vec<ToolId>,
    /// Bytes reclaimed by the action. Equal to `(unique_inodes_replaced) *
    /// canonical.size_bytes` per ADR-002. Mirrors the planner's estimate
    /// when every target succeeded.
    pub bytes_reclaimed: u64,
    pub outcome: UnifyResult,
    /// Per-target failures (empty on full success). Used by the UI footer
    /// to show "the failed target's path and reason" per US-06.
    pub failures: Vec<UnifyFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifyFailure {
    pub tool: ToolId,
    pub target: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum UnifyResult {
    /// Every non-already-linked target was successfully hardlinked.
    Success,
    /// Some targets succeeded, some failed.
    Partial,
    /// Every target was already hardlinked into the canonical inode (no
    /// destructive work was required). `bytes_reclaimed == 0`.
    AlreadyUnified,
    /// Every target failed (or there were no targets to link).
    Failed,
}

impl UnifyResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            UnifyResult::Success => "success",
            UnifyResult::Partial => "partial",
            UnifyResult::AlreadyUnified => "already_unified",
            UnifyResult::Failed => "failed",
        }
    }
}

/// Run a confirmed unify action. For each `PlannedLink` in `plan.links`,
/// look up the matching plugin and call `Tool::link(canonical, model)`.
/// Collect per-target outcomes, emit one `action.unify` JSONL event, and
/// return a `UnifyOutcome` for the UI footer.
pub async fn run(
    plan: UnifyPlan,
    plugins: &[Box<dyn Tool>],
    logger: &mut LaunchLogger,
) -> UnifyOutcome {
    let canonical_src = plan.canonical.path.clone();
    let canonical_size = plan.canonical.size_bytes;

    let mut succeeded: Vec<ToolId> = Vec::new();
    let mut failures: Vec<UnifyFailure> = Vec::new();
    let mut already_linked_count: usize = 0;
    let mut newly_linked_count: usize = 0;

    for link in &plan.links {
        let Some(plugin) = find_plugin(plugins, link.tool) else {
            // Pathological — UI sent a plan referencing a tool the plugin
            // registry doesn't have. Surface as failed target so the user
            // sees something rather than silently dropping work.
            tracing::warn!(
                target: "modeltap.action.unify",
                "no plugin registered for {} — skipping target",
                link.tool.0
            );
            failures.push(UnifyFailure {
                tool: link.tool,
                target: link.target.clone(),
                reason: format!("no plugin registered for tool {}", link.tool.0),
            });
            continue;
        };

        let model = synthesize_model_meta(link, canonical_size);
        match plugin.link(&canonical_src, &model).await {
            Ok(LinkOutcome { result, .. }) => match result {
                LinkResult::HardLinked { .. } => {
                    succeeded.push(link.tool);
                    newly_linked_count += 1;
                }
                LinkResult::AlreadyLinked { .. } => {
                    succeeded.push(link.tool);
                    already_linked_count += 1;
                }
                LinkResult::Copied { .. } => {
                    // v1 plugins return HardLinked or AlreadyLinked; Copied
                    // is reserved for the cross-fs [s/c/x] dialog (03-03).
                    succeeded.push(link.tool);
                    newly_linked_count += 1;
                }
                LinkResult::Skipped { reason } => {
                    failures.push(UnifyFailure {
                        tool: link.tool,
                        target: link.target.clone(),
                        reason: format!("skipped: {reason}"),
                    });
                }
                LinkResult::Failed { error } => {
                    failures.push(UnifyFailure {
                        tool: link.tool,
                        target: link.target.clone(),
                        reason: error,
                    });
                }
            },
            Err(e) => {
                tracing::warn!(
                    target: "modeltap.action.unify",
                    "link failed for {}: {e}",
                    link.tool.0
                );
                failures.push(UnifyFailure {
                    tool: link.tool,
                    target: link.target.clone(),
                    reason: classify_link_error(&e),
                });
            }
        }
    }

    // Sort succeeded tools for deterministic JSONL output.
    let mut tools_unified = succeeded.clone();
    tools_unified.sort_by_key(|t| t.0);
    tools_unified.dedup_by_key(|t| t.0);

    let outcome = classify(
        plan.links.len(),
        already_linked_count,
        newly_linked_count,
        failures.len(),
    );

    let bytes_reclaimed = if matches!(outcome, UnifyResult::AlreadyUnified) {
        0
    } else {
        // The planner already deduped per inode; we conservatively credit
        // the reclaim only for newly-linked targets. If every plan target
        // succeeded, this matches `plan.bytes_reclaimed_estimate`.
        if failures.is_empty() {
            plan.bytes_reclaimed_estimate
        } else {
            // Partial: prorate by share of newly-linked targets vs the
            // total non-already-linked target count.
            let needs_link = plan.links.iter().filter(|l| !l.already_linked).count() as u64;
            if needs_link == 0 {
                0
            } else {
                plan.bytes_reclaimed_estimate
                    .saturating_mul(newly_linked_count as u64)
                    .checked_div(needs_link)
                    .unwrap_or(0)
            }
        }
    };

    let unify_outcome = UnifyOutcome {
        tools_unified,
        bytes_reclaimed,
        outcome,
        failures,
    };

    emit(logger, &unify_outcome);
    unify_outcome
}

/// Classify the unify outcome from per-target counts.
///
/// Rules:
/// - Zero links scheduled → Failed (degenerate; planner shouldn't produce
///   this, but defense in depth).
/// - All links already-linked, none newly linked, no failures →
///   AlreadyUnified.
/// - At least one newly-linked AND no failures → Success.
/// - At least one success AND at least one failure → Partial.
/// - All failed → Failed.
fn classify(
    total: usize,
    already_linked: usize,
    newly_linked: usize,
    failed: usize,
) -> UnifyResult {
    if total == 0 {
        return UnifyResult::Failed;
    }
    if failed == total {
        return UnifyResult::Failed;
    }
    if failed > 0 {
        return UnifyResult::Partial;
    }
    // No failures.
    if newly_linked == 0 && already_linked == total {
        return UnifyResult::AlreadyUnified;
    }
    UnifyResult::Success
}

/// Build a synthetic `ModelMeta` for the plugin's `link()` call. The plugin
/// only needs `on_disk_path` (where the link target goes) and `id_in_tool`
/// (for the `LinkOutcome` correlation); the other fields are filled in
/// with conservative defaults because the planner doesn't know them.
fn synthesize_model_meta(link: &PlannedLink, size_bytes: u64) -> ModelMeta {
    let id_in_tool = link
        .target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    ModelMeta {
        tool: link.tool,
        id_in_tool: id_in_tool.clone(),
        on_disk_path: link.target.clone(),
        size_bytes,
        format: Format::Other,
        display_label: DisplayLabel::from(id_in_tool.clone()),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from(id_in_tool)),
    }
}

/// Translate a `LinkError` to a short user-visible reason string. Drops
/// path-bearing detail per the C5 privacy rule (paths can leak username).
fn classify_link_error(err: &LinkError) -> String {
    match err {
        LinkError::CrossFilesystem { .. } => "cross-filesystem".to_string(),
        LinkError::ContentMismatch { .. } => "content-mismatch".to_string(),
        LinkError::PermissionDenied { .. } => "permission-denied".to_string(),
        LinkError::MalformedMeta { reason } => format!("malformed-meta: {reason}"),
        LinkError::NotYetImplemented(_) => "not-yet-implemented".to_string(),
        LinkError::Io(_) => "io-error".to_string(),
    }
}

fn find_plugin(plugins: &[Box<dyn Tool>], tool_id: ToolId) -> Option<&dyn Tool> {
    plugins
        .iter()
        .find(|p| p.name().0 == tool_id.0)
        .map(|b| b.as_ref())
}

fn emit(logger: &mut LaunchLogger, outcome: &UnifyOutcome) {
    logger.record(RecordKind::ActionUnify {
        // v1 dedup key is always sha256 (per ADR-002 §"primary identity").
        // Once hf-hub-id+quant lands as a secondary kind, the orchestrator
        // will receive the kind discriminator from the plan.
        model_dedup_key_kind: "sha256",
        tools_unified: outcome
            .tools_unified
            .iter()
            .map(|t| t.0.to_string())
            .collect(),
        bytes_reclaimed: outcome.bytes_reclaimed,
        outcome: outcome.outcome.as_str(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_all_failed_is_failed() {
        assert_eq!(classify(3, 0, 0, 3), UnifyResult::Failed);
    }

    #[test]
    fn classify_all_already_linked_is_already_unified() {
        assert_eq!(classify(2, 2, 0, 0), UnifyResult::AlreadyUnified);
    }

    #[test]
    fn classify_all_newly_linked_is_success() {
        assert_eq!(classify(2, 0, 2, 0), UnifyResult::Success);
    }

    #[test]
    fn classify_mixed_already_linked_and_newly_is_success() {
        assert_eq!(classify(3, 1, 2, 0), UnifyResult::Success);
    }

    #[test]
    fn classify_some_succeeded_some_failed_is_partial() {
        assert_eq!(classify(3, 0, 2, 1), UnifyResult::Partial);
    }

    #[test]
    fn classify_zero_targets_is_failed() {
        assert_eq!(classify(0, 0, 0, 0), UnifyResult::Failed);
    }
}
