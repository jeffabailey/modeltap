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

use std::path::{Path, PathBuf};

use modeltap_core::logic::plan::{PlannedLink, UnifyPlan};
use modeltap_core::{LinkError, LinkOutcome, LinkResult, ModelMeta, Tool, ToolId};
use modeltap_store::Cache;
use modeltap_tui::dialogs::cross_fs_choice::CrossFsChoice;
use modeltap_tui::render::bytes::format_bytes;

use crate::observability::{LaunchLogger, RecordKind};
use modeltap_app::orchestration::revalidate::{self, PreMutateOutcome};

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
    /// Step 05-02 part 2/2 — K5 gate fired: pre-mutate revalidation found
    /// the cache disagrees with the filesystem (Drift / Gone) or the
    /// store itself errored. No plugin call, no filesystem mutation. The
    /// caller MUST dispatch a refresh + re-prompt per AC-26-6 / AC-26-7.
    /// JSONL `outcome` field carries `"cache_stale"`.
    CacheStale,
}

impl UnifyResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            UnifyResult::Success => "success",
            UnifyResult::Partial => "partial",
            UnifyResult::AlreadyUnified => "already_unified",
            UnifyResult::Failed => "failed",
            UnifyResult::CacheStale => "cache_stale",
        }
    }
}

/// Result of a US-14 dry-run preview. Carries the formatted preview lines for
/// the dialog (what the user sees) and the aggregate counts the JSONL event
/// already emitted. Returned to the composition root so it can dispatch
/// `Msg::UnifyDryRunCompleted(...)` with the lines, which the dialog reads to
/// transition `UnifyMode::Confirm` -> `UnifyMode::DryRunPreview { lines }`.
///
/// ADR-006 same-value-type principle: dry-run and real-run share the SAME
/// `UnifyPlan` value. The only branching is at the COMMIT step — real-run
/// iterates `plan.targets` and calls `plugin.link()`; dry-run iterates the
/// SAME plan and emits descriptive lines without mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunOutcome {
    /// Formatted "(dry-run) Would..." lines for display in the dialog.
    pub lines: Vec<String>,
    /// Aggregate count of cross-fs targets in the plan (matches the JSONL
    /// `cross_fs_targets` field). Surfaced separately so the dialog can
    /// render WARNING markers without re-walking the plan.
    pub cross_fs_targets: u64,
    /// Bytes the plan WOULD reclaim if executed. Mirrors the planner's
    /// `bytes_reclaimed_estimate`. Surfaced for the dialog header line.
    pub bytes_would_reclaim: u64,
}

/// Run a US-14 dry-run preview of a unify action. Walks the SAME `plan` value
/// the real-run would walk (per ADR-006), formats each target as a "(dry-run)
/// Would ..." line, emits exactly one `action.unify_dry_run` JSONL event, and
/// returns a `DryRunOutcome` for the UI dialog. This function NEVER calls
/// `plugin.link()` and NEVER writes to the filesystem.
///
/// No-mutation invariant: the only side effect is the JSONL append to the
/// launch log. The fixture file tree is left byte-for-byte unchanged. The
/// US-14 acceptance test snapshots `(path, inode, size, mtime)` tuples
/// before/after to enforce this.
pub fn dry_run(plan: &UnifyPlan, logger: &mut LaunchLogger) -> DryRunOutcome {
    let canonical_path = plan.canonical.path.display().to_string();
    let bytes_would_reclaim = plan.bytes_reclaimed_estimate;
    let cross_fs_targets: u64 = plan
        .links
        .iter()
        .filter(|l| l.cross_filesystem && !l.already_linked)
        .count() as u64;

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "(dry-run) Would create canonical at {}",
        canonical_path
    ));
    lines.push("(dry-run) Would create hardlinks at:".to_string());
    for link in &plan.links {
        if link.already_linked {
            lines.push(format!(
                "  - {} (already linked, would skip)",
                link.target.display()
            ));
        } else if link.cross_filesystem {
            lines.push(format!(
                "  - {} -- WARNING: target on different filesystem -- would fall back to copy",
                link.target.display()
            ));
        } else {
            lines.push(format!("  - {}", link.target.display()));
        }
    }
    lines.push(format!(
        "(dry-run) Reclaim: {}",
        format_bytes(bytes_would_reclaim)
    ));
    lines.push(String::new());
    lines.push("[Enter] proceed   [Esc] cancel".to_string());

    // Sort + dedup tool ids for deterministic JSONL output (mirror the
    // real-run's `tools_unified` ordering rules).
    let mut tools_to_unify: Vec<String> = plan
        .links
        .iter()
        .filter(|l| !l.already_linked)
        .map(|l| l.tool.0.to_string())
        .collect();
    tools_to_unify.sort();
    tools_to_unify.dedup();

    logger.record(RecordKind::ActionUnifyDryRun {
        model_dedup_key_kind: "sha256",
        tools_to_unify,
        bytes_would_reclaim,
        cross_fs_targets,
        outcome: "previewed",
    });

    DryRunOutcome {
        lines,
        cross_fs_targets,
        bytes_would_reclaim,
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
    cache: Option<&Cache>,
    cache_log_dir: Option<&Path>,
) -> UnifyOutcome {
    let canonical_src = plan.canonical.path.clone();
    let canonical_size = plan.canonical.size_bytes;

    // Step 05-02 part 2/2 — K5 pre-mutate gate. When a cache is threaded
    // through (post step-05-04 wiring) every model referenced by the plan is
    // revalidated against the filesystem before any plugin call. The first
    // non-Proceed short-circuits the whole action: zero plugin calls, zero
    // filesystem mutation. When `cache` is `None` (no-cache launches, or
    // pre-step-05-04 call sites) the gate is skipped and the call proceeds
    // exactly as before.
    if let Some(c) = cache {
        // Build the unique (tool, model_id) pairs the plan references:
        // canonical + every link target. De-dup so a plan with a 5-link
        // canonical revalidates each model exactly once.
        let canonical_id = synthetic_id_from_path(&canonical_src);
        let mut checked: Vec<(ToolId, String)> = Vec::with_capacity(1 + plan.links.len());
        checked.push((plan.canonical.tool, canonical_id));
        for link in &plan.links {
            let id = synthetic_id_from_path(&link.target);
            if !checked.iter().any(|(t, m)| *t == link.tool && m == &id) {
                checked.push((link.tool, id));
            }
        }
        for (tool, model_id) in &checked {
            match revalidate::pre_mutate(c, tool, model_id, cache_log_dir).await {
                PreMutateOutcome::Proceed => continue,
                PreMutateOutcome::Drift { .. }
                | PreMutateOutcome::Gone
                | PreMutateOutcome::StoreError(_) => {
                    let outcome = UnifyOutcome {
                        tools_unified: Vec::new(),
                        bytes_reclaimed: 0,
                        outcome: UnifyResult::CacheStale,
                        failures: Vec::new(),
                        cross_fs_targets_skipped: 0,
                        cross_fs_targets_copied: 0,
                    };
                    emit(logger, &outcome);
                    return outcome;
                }
            }
        }
    }

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
/// (for the `LinkOutcome` correlation); the rest is filled with conservative
/// defaults via the shared `super::synthetic_model_meta` helper.
fn synthesize_model_meta(link: &PlannedLink, size_bytes: u64) -> ModelMeta {
    let id_in_tool = synthetic_id_from_path(&link.target);
    super::synthetic_model_meta(link.tool, id_in_tool, link.target.clone(), size_bytes)
}

/// Project a `<path>/<file>` into the synthetic `id_in_tool` the plugin
/// receives at `link()`/`delete_one()` time. Shared between the K5 pre-
/// mutate gate (Step 05-02 part 2/2) and the per-link `ModelMeta`
/// synthesis so the cache lookup uses the SAME identifier the plugin
/// sees.
fn synthetic_id_from_path(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
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

    // ---- US-14 dry-run unit tests ----------------------------------------

    use modeltap_core::logic::plan::{PlanCandidate, PlannedLink, UnifyPlan};
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    fn cand(tool: &'static str, path: &str, exists: bool) -> PlanCandidate {
        PlanCandidate {
            tool: ToolId(tool),
            path: PathBuf::from(path),
            exists,
            device: 1,
            inode: 100,
            size_bytes: 4096,
        }
    }

    fn make_plan(links: Vec<PlannedLink>, bytes_reclaimed: u64) -> UnifyPlan {
        UnifyPlan {
            canonical: cand("ollama", "/c/canonical.bin", true),
            links,
            bytes_reclaimed_estimate: bytes_reclaimed,
        }
    }

    fn null_logger() -> LaunchLogger {
        // None log_dir: writes are silently dropped, but RecordKind::record
        // is still exercised.
        LaunchLogger::open(None)
    }

    #[test]
    fn dry_run_returns_lines_labeled_with_dry_run_prefix() {
        let plan = make_plan(
            vec![PlannedLink {
                tool: ToolId("hf"),
                target: PathBuf::from("/h/target.bin"),
                cross_filesystem: false,
                already_linked: false,
            }],
            4096,
        );
        let mut logger = null_logger();
        let outcome = dry_run(&plan, &mut logger);
        let joined = outcome.lines.join("\n");
        assert!(
            joined.contains("(dry-run)"),
            "dry_run output must be labeled '(dry-run)', got: {}",
            joined
        );
        assert!(
            joined.contains("Would create canonical"),
            "dry_run output must include 'Would create canonical' line, got: {}",
            joined
        );
        assert!(
            joined.contains("Would create hardlinks"),
            "dry_run output must include 'Would create hardlinks' line, got: {}",
            joined
        );
        assert!(
            joined.contains("Reclaim:"),
            "dry_run output must include reclaim summary, got: {}",
            joined
        );
    }

    #[test]
    fn dry_run_surfaces_cross_filesystem_warning_per_target() {
        let plan = make_plan(
            vec![
                PlannedLink {
                    tool: ToolId("hf"),
                    target: PathBuf::from("/h/same-fs.bin"),
                    cross_filesystem: false,
                    already_linked: false,
                },
                PlannedLink {
                    tool: ToolId("Loose GGUFs"),
                    target: PathBuf::from("/l/cross-fs.bin"),
                    cross_filesystem: true,
                    already_linked: false,
                },
            ],
            8192,
        );
        let mut logger = null_logger();
        let outcome = dry_run(&plan, &mut logger);
        let joined = outcome.lines.join("\n");
        assert!(
            joined.contains("WARNING")
                && joined.contains("different filesystem")
                && joined.contains("/l/cross-fs.bin"),
            "cross-fs target must produce a per-target WARNING, got: {}",
            joined
        );
        assert_eq!(
            outcome.cross_fs_targets, 1,
            "outcome.cross_fs_targets must count active cross-fs targets"
        );
    }

    #[test]
    fn dry_run_does_not_mutate_filesystem_for_any_target() {
        // No-mutation property: build a plan whose `target` paths point at
        // real files in tempdir, snapshot them, run dry_run, snapshot again,
        // assert byte-for-byte equality of every (path, inode, size) tuple.
        // mtime is excluded here — the unit test for the dialog state seam
        // is enough; the acceptance test enforces (path, inode, size, mtime)
        // end-to-end. (≥256 iterations covered by the property variant
        // below.)
        let temp = tempfile::tempdir().expect("tempdir");
        let canonical = temp.path().join("canonical.bin");
        let target_a = temp.path().join("target-a.bin");
        let target_b = temp.path().join("target-b.bin");
        std::fs::write(&canonical, b"canonical-bytes-aaaa").unwrap();
        std::fs::write(&target_a, b"target-a-bytes-bbbbbb").unwrap();
        std::fs::write(&target_b, b"target-b-bytes-cccccc").unwrap();

        let pre_a = std::fs::read(&target_a).unwrap();
        let pre_b = std::fs::read(&target_b).unwrap();
        let pre_canon = std::fs::read(&canonical).unwrap();

        let plan = UnifyPlan {
            canonical: PlanCandidate {
                tool: ToolId("ollama"),
                path: canonical.clone(),
                exists: true,
                device: 1,
                inode: 100,
                size_bytes: pre_canon.len() as u64,
            },
            links: vec![
                PlannedLink {
                    tool: ToolId("hf"),
                    target: target_a.clone(),
                    cross_filesystem: false,
                    already_linked: false,
                },
                PlannedLink {
                    tool: ToolId("Loose GGUFs"),
                    target: target_b.clone(),
                    cross_filesystem: false,
                    already_linked: false,
                },
            ],
            bytes_reclaimed_estimate: 2 * pre_canon.len() as u64,
        };
        let mut logger = null_logger();
        let _ = dry_run(&plan, &mut logger);

        // Bytes unchanged.
        assert_eq!(pre_a, std::fs::read(&target_a).unwrap());
        assert_eq!(pre_b, std::fs::read(&target_b).unwrap());
        assert_eq!(pre_canon, std::fs::read(&canonical).unwrap());

        // Inodes unchanged (no replacement happened).
        let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();
        assert_ne!(
            ino(&canonical),
            ino(&target_a),
            "target-a inode must remain distinct from canonical (no link created)"
        );
        assert_ne!(
            ino(&canonical),
            ino(&target_b),
            "target-b inode must remain distinct from canonical (no link created)"
        );
    }

    #[test]
    fn dry_run_no_mutation_property_holds_for_256_iterations() {
        // ≥256 generated plan/fixture combinations. Each iteration:
        //   1. Build a tempdir with N (1..=4) target files of varying sizes.
        //   2. Build a UnifyPlan over them.
        //   3. Snapshot (path, inode, size) for every target.
        //   4. Run dry_run.
        //   5. Assert every snapshot tuple is unchanged.
        // The property is over (plan, fixture) shape; the assertion is the
        // ADR-006 same-value-type / no-mutation guarantee.
        for seed in 0..256u64 {
            let temp = tempfile::tempdir().unwrap();
            let canonical_path = temp.path().join(format!("canon-{}.bin", seed));
            // Vary canonical size deterministically so the plan's
            // bytes_reclaimed_estimate is non-trivial.
            let canon_size = 512 + (seed as usize % 2048);
            let canon_bytes: Vec<u8> = (0..canon_size)
                .map(|i| ((i + seed as usize) % 251) as u8)
                .collect();
            std::fs::write(&canonical_path, &canon_bytes).unwrap();

            let n_targets = 1 + (seed % 4) as usize;
            let mut links = Vec::new();
            let mut targets: Vec<PathBuf> = Vec::new();
            for i in 0..n_targets {
                let tp = temp.path().join(format!("tgt-{}-{}.bin", seed, i));
                let tb: Vec<u8> = (0..canon_size).map(|j| ((j + i + 7) % 251) as u8).collect();
                std::fs::write(&tp, &tb).unwrap();
                targets.push(tp.clone());
                links.push(PlannedLink {
                    tool: match i % 3 {
                        0 => ToolId("hf"),
                        1 => ToolId("Loose GGUFs"),
                        _ => ToolId("lm-studio"),
                    },
                    target: tp,
                    cross_filesystem: (seed + i as u64) % 5 == 0,
                    already_linked: false,
                });
            }
            let plan = UnifyPlan {
                canonical: PlanCandidate {
                    tool: ToolId("ollama"),
                    path: canonical_path.clone(),
                    exists: true,
                    device: 1,
                    inode: 100 + seed,
                    size_bytes: canon_size as u64,
                },
                links,
                bytes_reclaimed_estimate: (n_targets as u64) * canon_size as u64,
            };

            // Pre-snapshot.
            let pre: Vec<(u64, u64, Vec<u8>)> = targets
                .iter()
                .map(|p| {
                    let m = std::fs::metadata(p).unwrap();
                    (m.ino(), m.len(), std::fs::read(p).unwrap())
                })
                .collect();
            let pre_canon = (
                std::fs::metadata(&canonical_path).unwrap().ino(),
                std::fs::metadata(&canonical_path).unwrap().len(),
                std::fs::read(&canonical_path).unwrap(),
            );

            let mut logger = null_logger();
            let _ = dry_run(&plan, &mut logger);

            // Post-snapshot must equal pre-snapshot.
            for (i, p) in targets.iter().enumerate() {
                let m = std::fs::metadata(p).unwrap();
                assert_eq!(
                    pre[i].0,
                    m.ino(),
                    "iter {} target {} inode changed",
                    seed,
                    i
                );
                assert_eq!(pre[i].1, m.len(), "iter {} target {} size changed", seed, i);
                assert_eq!(
                    pre[i].2,
                    std::fs::read(p).unwrap(),
                    "iter {} target {} bytes changed",
                    seed,
                    i
                );
            }
            let post_canon = (
                std::fs::metadata(&canonical_path).unwrap().ino(),
                std::fs::metadata(&canonical_path).unwrap().len(),
                std::fs::read(&canonical_path).unwrap(),
            );
            assert_eq!(pre_canon, post_canon, "iter {} canonical changed", seed);
        }
    }

    #[test]
    fn build_plan_is_deterministic_for_256_iterations() {
        // Plan-equality property: build_plan(model, inventory) for the same
        // input ALWAYS returns the same UnifyPlan (deterministic; no global
        // state). This is the ADR-006 same-value-type guarantee — dry-run
        // and real-run must agree on the plan shape, which they trivially
        // do if build_plan is a pure function of its inputs.
        use modeltap_core::logic::plan::build_plan;
        for seed in 0..256u64 {
            let canonical = PlanCandidate {
                tool: ToolId("ollama"),
                path: PathBuf::from(format!("/c/{}.bin", seed)),
                exists: true,
                device: 1 + (seed % 3),
                inode: 100 + seed,
                size_bytes: 4096 + seed,
            };
            let targets = vec![
                PlanCandidate {
                    tool: ToolId("hf"),
                    path: PathBuf::from(format!("/h/{}.bin", seed)),
                    exists: true,
                    device: 1 + (seed % 3),
                    inode: 200 + seed,
                    size_bytes: 4096 + seed,
                },
                PlanCandidate {
                    tool: ToolId("Loose GGUFs"),
                    path: PathBuf::from(format!("/l/{}.bin", seed)),
                    exists: true,
                    device: 2 + (seed % 3),
                    inode: 300 + seed,
                    size_bytes: 4096 + seed,
                },
            ];
            let plan_a = build_plan(&canonical, &targets);
            let plan_b = build_plan(&canonical, &targets);
            assert_eq!(
                plan_a, plan_b,
                "build_plan must be deterministic at seed {}",
                seed
            );
        }
    }
}
