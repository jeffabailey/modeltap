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
use modeltap_tui::dialogs::cross_fs_choice::CrossFsChoice;

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
    /// US-19: count of cross-fs targets that were skipped (user pressed `s`).
    /// These are NOT failures — they are an explicit user choice to leave the
    /// target alone. Recorded separately in the JSONL event and the banner.
    pub cross_fs_targets_skipped: u64,
    /// US-19: count of cross-fs targets that were duplicated by byte-copy
    /// (user pressed `c`). Counted as success for `tools_unified` but the
    /// per-target reclaim is zero (the bytes were duplicated, not reclaimed).
    pub cross_fs_targets_copied: u64,
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
///
/// US-19 cross-fs handling (`cross_fs_choice`):
/// - `None` — production path with no cross-fs targets. Cross-fs links (if
///   any) flow into the underlying plugin's `link()`, which returns
///   `LinkError::CrossFilesystem`; recorded as a failure as in step 03-02.
/// - `Some(Skip)` — cross-fs targets are LEFT UNTOUCHED. Same-fs targets are
///   linked normally. The orchestrator records `cross_fs_targets_skipped`.
/// - `Some(Copy)` — cross-fs targets are duplicated byte-for-byte (atomic
///   write+rename) by the orchestrator. Same-fs targets are linked normally.
///   The orchestrator records `cross_fs_targets_copied`.
pub async fn run(
    plan: UnifyPlan,
    plugins: &[Box<dyn Tool>],
    logger: &mut LaunchLogger,
    cross_fs_choice: Option<CrossFsChoice>,
) -> UnifyOutcome {
    let canonical_src = plan.canonical.path.clone();
    let canonical_size = plan.canonical.size_bytes;

    let mut succeeded: Vec<ToolId> = Vec::new();
    let mut failures: Vec<UnifyFailure> = Vec::new();
    let mut already_linked_count: usize = 0;
    let mut newly_linked_count: usize = 0;
    let mut cross_fs_targets_skipped: u64 = 0;
    let mut cross_fs_targets_copied: u64 = 0;

    for link in &plan.links {
        // US-19 — when the user chose Skip and this is a cross-fs target,
        // we never call `Tool::link`. The target stays at its original path.
        if link.cross_filesystem
            && !link.already_linked
            && matches!(cross_fs_choice, Some(CrossFsChoice::Skip))
        {
            cross_fs_targets_skipped += 1;
            continue;
        }
        // US-19 — when the user chose Copy and this is a cross-fs target,
        // duplicate the canonical's bytes to the target via atomic write+rename.
        // We do NOT go through the plugin's `link()` because that would fail
        // with `EXDEV`; the orchestrator owns the cross-fs copy semantics.
        if link.cross_filesystem
            && !link.already_linked
            && matches!(cross_fs_choice, Some(CrossFsChoice::Copy))
        {
            match copy_cross_fs(&canonical_src, &link.target) {
                Ok(()) => {
                    succeeded.push(link.tool);
                    cross_fs_targets_copied += 1;
                }
                Err(reason) => {
                    failures.push(UnifyFailure {
                        tool: link.tool,
                        target: link.target.clone(),
                        reason,
                    });
                }
            }
            continue;
        }

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

    // US-19: when targets were skipped, they don't count as failures, but they
    // also reduce the "effective" total for the all-cross-fs-skipped case
    // (which classifies as Failed because nothing was done).
    let effective_total = plan
        .links
        .len()
        .saturating_sub(cross_fs_targets_skipped as usize);
    let copied_count = cross_fs_targets_copied as usize;
    let outcome = classify(
        effective_total,
        already_linked_count,
        newly_linked_count + copied_count,
        failures.len(),
    );

    let bytes_reclaimed = if matches!(outcome, UnifyResult::AlreadyUnified) {
        0
    } else {
        // The planner already deduped per inode; we conservatively credit
        // the reclaim only for newly-linked targets. If every plan target
        // succeeded, this matches `plan.bytes_reclaimed_estimate`.
        // Cross-fs Copy does NOT reclaim — duplicating bytes wastes disk;
        // the count is in `cross_fs_targets_copied` separately.
        if failures.is_empty() {
            // Every newly-linked target reclaims; copied targets do not.
            let same_fs_inodes = newly_linked_count as u64;
            let total_new = (newly_linked_count + copied_count) as u64;
            if total_new == 0 {
                0
            } else {
                plan.bytes_reclaimed_estimate
                    .saturating_mul(same_fs_inodes)
                    .checked_div(total_new)
                    .unwrap_or(0)
            }
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
        cross_fs_targets_skipped,
        cross_fs_targets_copied,
    };

    emit(logger, &unify_outcome);
    unify_outcome
}

/// US-19 byte-for-byte cross-fs duplication. Atomic write+rename: write to a
/// temp file in the target's parent directory, fsync, then rename into place.
/// Returns a short user-visible reason on failure.
fn copy_cross_fs(canonical_src: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    use std::io::Write;
    let parent = target
        .parent()
        .ok_or_else(|| "cross-fs-copy: target has no parent dir".to_string())?;
    if let Err(e) = std::fs::create_dir_all(parent) {
        return Err(format!("cross-fs-copy: mkdir failed: {e}"));
    }
    let tmp = parent.join(format!(".modeltap-cross-fs-tmp.{}", std::process::id()));
    let bytes = std::fs::read(canonical_src)
        .map_err(|e| format!("cross-fs-copy: read canonical failed: {e}"))?;
    {
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| format!("cross-fs-copy: create tmp failed: {e}"))?;
        f.write_all(&bytes)
            .map_err(|e| format!("cross-fs-copy: write tmp failed: {e}"))?;
        f.sync_all()
            .map_err(|e| format!("cross-fs-copy: fsync tmp failed: {e}"))?;
    }
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cross-fs-copy: rename failed: {e}"));
    }
    Ok(())
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
        cross_fs_targets_skipped: outcome.cross_fs_targets_skipped,
        cross_fs_targets_copied: outcome.cross_fs_targets_copied,
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
