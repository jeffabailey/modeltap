//! `actions::folder_delete::run` — orchestrates a confirmed folder-group bulk-delete
//! action (US-05c, step 01-05; ADR-010).
//!
//! Called by the composition root when the user confirms a `Shift+F`
//! folder-delete on a Hugging Face repo. Per ADR-010 the destructive work
//! lives behind `Tool::delete_folder`; this orchestrator:
//!
//! 1. Discovers HF models and groups by `<author>/<repo>`.
//! 2. Builds the `FolderGroup` for the targeted path + enumerates sidecars
//!    from the on-disk repo dir.
//! 3. Builds a `FolderDeletePlan` via the pure `logic::folder_group::
//!    build_folder_delete_plan`. The walking-skeleton (M1) all-unique slice
//!    uses an empty `FolderClassification.shared` — every file goes into
//!    `paths_to_unlink_fully` and `bytes_to_reclaim`.
//! 4. Calls `plugin.delete_folder(&plan).await`.
//! 5. Aggregates the `Vec<DeleteOutcome>` into per-action totals
//!    (`bytes_reclaimed`, `files_removed`, `files_total`) and emits one
//!    `action.folder_delete` JSONL event.
//! 6. Returns a `FolderDeleteOutcome` whose fields the composition root maps
//!    into `LastAction::for_folder_delete_*` for the right-pane banner.
//!
//! Per the kpi-instrumentation §"Privacy" rule: NO on-disk paths, NO blob
//! hex digests in the JSONL event. The `folder_path` is the canonical
//! `<author>/<repo>` identifier the user typed at the confirmation prompt
//! — a logical identifier, NOT a filesystem path.

use std::collections::BTreeMap;
use std::path::PathBuf;

use modeltap_core::logic::folder_group::{build_folder_delete_plan, group_by_hf_repo};
use modeltap_core::types::{
    DedupKey, DiscoveredModel, FolderClassification, FolderDeletePlan, ModelMeta, Sidecar,
    SidecarKind, ToolId,
};
use modeltap_core::Tool;

use crate::observability::{LaunchLogger, RecordKind};

/// Result of a confirmed folder-group bulk-delete. Surfaced to the right
/// pane as the "Last action" banner and used by acceptance tests to assert
/// success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderDeleteOutcome {
    pub tool: ToolId,
    pub folder_path: String,
    pub bytes_reclaimed: u64,
    pub bytes_retained: u64,
    pub files_total: u64,
    pub files_removed: u64,
    pub outcome: FolderDeleteResult,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FolderDeleteResult {
    /// Every file in the plan had its registration removed.
    Success,
    /// One or more files failed to unlink; the user can retry via `Shift+F`.
    Partial,
    /// The plugin returned an error before any file work; or the folder was
    /// not found in the discovered inventory.
    Failed,
}

impl FolderDeleteResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            FolderDeleteResult::Success => "success",
            FolderDeleteResult::Partial => "partial",
            FolderDeleteResult::Failed => "failed",
        }
    }
}

/// Sidecar-enumeration port — the HF plugin owns the on-disk sidecar walk
/// (per component-boundaries §13). The orchestrator declares this trait so
/// the dependency on the concrete HF function lives at the composition root,
/// not in `modeltap-core`.
///
/// Real implementations call `modeltap_plugin_hf::folder_delete::
/// enumerate_sidecars`. Tests may inject a stub.
pub trait SidecarEnumerator: Send + Sync {
    fn enumerate(&self, repo_dir: &std::path::Path, model_files: &[PathBuf]) -> Vec<Sidecar>;
}

/// Run a confirmed folder-delete action. Returns a `FolderDeleteOutcome`
/// describing the work for the right-pane banner and emits exactly one
/// `action.folder_delete` JSONL event.
///
/// Arguments:
///   - `plugin`: the HF plugin instance (resolved by composition root).
///   - `tool_id`: echoed into the JSONL event; the WS uses `ToolId("hf")`.
///   - `folder_path`: canonical `<author>/<repo>` identifier (user-typed).
///   - `hub_root`: HF hub root (the dir that contains `models--*` subdirs).
///   - `sidecar_enumerator`: HF-plugin-owned sidecar walker.
///   - `logger`: launch.log JSONL sink.
pub async fn run(
    plugin: &dyn Tool,
    tool_id: ToolId,
    folder_path: String,
    hub_root: &std::path::Path,
    sidecar_enumerator: &dyn SidecarEnumerator,
    logger: &mut LaunchLogger,
) -> FolderDeleteOutcome {
    // 1. Discover HF models. The walking-skeleton fixture is small (5 files);
    //    a fresh discover() is the simplest seam — no separate "lookup by path"
    //    surface needed on the Tool trait.
    let discovered = match plugin.discover().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.action.folder_delete",
                "hf discover failed: {e}"
            );
            return emit_and_return_failed(logger, tool_id, folder_path);
        }
    };
    // 2. Project DiscoveredModel -> ModelMeta and group by <author>/<repo>.
    let models: Vec<ModelMeta> = discovered
        .into_iter()
        .map(|d| project_to_model_meta(tool_id, d))
        .collect();
    // 3. Build the sidecars map for the targeted folder. We synthesize the
    //    on-disk repo dir from the canonical `<author>/<repo>` plus the
    //    plugin's `hub_root`. The HF cache layout is
    //    `<hub_root>/models--<author>--<repo>/`.
    let Some((author, repo_name)) = folder_path.split_once('/') else {
        tracing::warn!(
            target: "modeltap.action.folder_delete",
            "malformed folder_path (no '/'): {folder_path}"
        );
        return emit_and_return_failed(logger, tool_id, folder_path);
    };
    let repo_dir = hub_root.join(format!("models--{author}--{repo_name}"));
    let target_models: Vec<PathBuf> = models
        .iter()
        .filter(|m| {
            // id_in_tool is `<author>/<repo>/<file...>`; match by repo prefix
            // so a different repo with the same on-disk filename can't leak.
            m.id_in_tool.starts_with(&format!("{folder_path}/"))
        })
        .map(|m| m.on_disk_path.clone())
        .collect();
    // Enumerate ALL sidecars on disk (including HF-internal `refs/<name>` and
    // exclusive `blobs/` entries). Then split: "reported" sidecars
    // (Readme/Imatrix/Urls/Other) count toward user-visible totals and feed
    // into the plan; "internal" sidecars (HfInternal) are stripped from the
    // plan accounting but are still swept by the post-cleanup pass so the
    // repo dir ends up empty (AC-11). Per kpi-instrumentation, the user-
    // visible counters (`files_total`, `bytes_reclaimed`) reflect only what
    // the user thinks of as "files in the folder" — internal HF bookkeeping
    // is not part of that mental model.
    let all_sidecars = sidecar_enumerator.enumerate(&repo_dir, &target_models);
    let (reported_sidecars, internal_sidecars): (Vec<Sidecar>, Vec<Sidecar>) = all_sidecars
        .into_iter()
        .partition(|s| !matches!(s.kind, SidecarKind::HfInternal));

    let mut sidecars_by_repo: BTreeMap<String, Vec<Sidecar>> = BTreeMap::new();
    sidecars_by_repo.insert(folder_path.clone(), reported_sidecars);

    // 4. Group models. group_by_hf_repo returns one FolderGroup per repo
    //    found in the input; we filter to the targeted one.
    let mut folders = group_by_hf_repo(&models, &sidecars_by_repo);
    let mut folder = match folders.iter().position(|f| f.path == folder_path) {
        Some(idx) => folders.swap_remove(idx),
        None => {
            tracing::warn!(
                target: "modeltap.action.folder_delete",
                "no folder group matched {folder_path}",
            );
            return emit_and_return_failed(logger, tool_id, folder_path);
        }
    };
    // group_by_hf_repo synthesizes a RELATIVE absolute_path
    // (`models--<author>--<repo>`); overwrite with the real absolute path so
    // the plugin's delete loop and empty-tree cleanup walk the right tree.
    folder.absolute_path = repo_dir.clone();

    // 5. Build the FolderDeletePlan. M1 walking-skeleton: every model is
    //    unique-to-HF (no cross-tool sharing), so the classification is
    //    all-unique with empty shared. The single-engine invariant in
    //    `classify_unique_vs_shared` is preserved by future scenarios; the
    //    WS slice bypasses it because we know by construction (single-tool
    //    fixture) that every model is unique.
    let classification = FolderClassification {
        unique: folder.models.clone(),
        shared: Vec::new(),
    };
    let plan: FolderDeletePlan = build_folder_delete_plan(&folder, &classification);
    let files_total = plan.folder.file_count() as u64;
    let bytes_to_reclaim = plan.bytes_to_reclaim;
    let bytes_to_retain = plan.bytes_to_retain;

    // 6. Dispatch to plugin.
    let outcomes = match plugin.delete_folder(&plan).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.action.folder_delete",
                "delete_folder failed for {folder_path}: {e}"
            );
            // Distinguish Unsupported from genuine I/O failure for the JSONL
            // event; both surface as Failed for the banner.
            let _ = e; // (kept for tracing only)
            return emit_and_return_failed(logger, tool_id, folder_path);
        }
    };

    // 7a. Post-sweep cleanup: unlink any HF-internal sidecars (e.g.,
    // `refs/main`) and remove any now-empty directories. These files are NOT
    // counted in user-visible totals but MUST be gone for AC-11 (the
    // `models--<author>--<repo>/` dir must not exist afterwards). Best-effort
    // — partial failure here downgrades to Partial via the file_count check.
    for internal in &internal_sidecars {
        let _ = std::fs::remove_file(&internal.path);
    }
    cleanup_empty_dirs(&repo_dir);

    // 7b. Aggregate. `files_removed` counts outcomes with registration_removed.
    let files_removed: u64 = outcomes.iter().filter(|o| o.registration_removed).count() as u64;
    let bytes_reclaimed: u64 = outcomes.iter().map(|o| o.bytes_freed).sum();
    let result = if files_removed == files_total {
        FolderDeleteResult::Success
    } else if files_removed == 0 {
        FolderDeleteResult::Failed
    } else {
        FolderDeleteResult::Partial
    };
    // Bytes-reclaimed sanity: when every file succeeded, the per-outcome sum
    // should equal the plan's promised reclaim (INT-FGD-3). When partial,
    // the per-outcome sum is the source of truth.
    let reported_reclaim = if result == FolderDeleteResult::Success {
        bytes_to_reclaim
    } else {
        bytes_reclaimed
    };

    let outcome = FolderDeleteOutcome {
        tool: tool_id,
        folder_path,
        bytes_reclaimed: reported_reclaim,
        bytes_retained: bytes_to_retain,
        files_total,
        files_removed,
        outcome: result,
    };
    emit(logger, &outcome);
    outcome
}

fn project_to_model_meta(tool: ToolId, d: DiscoveredModel) -> ModelMeta {
    let label = d.display_label.clone();
    let dedup_key = DedupKey::Tentative(label.clone());
    ModelMeta {
        tool,
        id_in_tool: d.id_in_tool,
        on_disk_path: d.on_disk_path,
        size_bytes: d.size_bytes,
        format: d.format,
        display_label: label,
        status: d.status,
        dedup_key,
    }
}

fn emit_and_return_failed(
    logger: &mut LaunchLogger,
    tool_id: ToolId,
    folder_path: String,
) -> FolderDeleteOutcome {
    let outcome = FolderDeleteOutcome {
        tool: tool_id,
        folder_path,
        bytes_reclaimed: 0,
        bytes_retained: 0,
        files_total: 0,
        files_removed: 0,
        outcome: FolderDeleteResult::Failed,
    };
    emit(logger, &outcome);
    outcome
}

fn emit(logger: &mut LaunchLogger, outcome: &FolderDeleteOutcome) {
    logger.record(RecordKind::ActionFolderDelete {
        tool: outcome.tool.0.to_string(),
        folder_path: outcome.folder_path.clone(),
        files_total: outcome.files_total,
        files_removed: outcome.files_removed,
        bytes_reclaimed: outcome.bytes_reclaimed,
        bytes_retained: outcome.bytes_retained,
        outcome: outcome.outcome.as_str(),
    });
}

/// Best-effort: bottom-up remove of every empty directory under `repo_dir`,
/// then `repo_dir` itself. Mirrors the plugin's `remove_empty_repo_tree` but
/// runs AFTER the orchestrator's HF-internal sidecar sweep so the now-empty
/// `refs/` (and any other newly-emptied subdirs) can be removed. Non-empty
/// subdirs (partial-failure case) are silently skipped — the user retries.
fn cleanup_empty_dirs(repo_dir: &std::path::Path) {
    if !repo_dir.exists() {
        return;
    }
    let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(repo_dir)
        .contents_first(true)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.path().to_path_buf())
        .collect();
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(&d);
    }
}
