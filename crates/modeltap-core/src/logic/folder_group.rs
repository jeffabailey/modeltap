//! Folder-group pure logic (Step 01-02).
//!
//! Three pure functions over algebraic types — no I/O, no async, no tokio:
//!
//! - [`group_by_hf_repo`] — partitions an HF `ToolInventory.models` slice by
//!   the `<author>/<repo>` prefix of `id_in_tool` and pairs each group with
//!   sidecars supplied by the caller (the HF plugin owns sidecar enumeration
//!   per AC-14 / B-FGD-2).
//! - [`classify_unique_vs_shared`] — projects every child model through the
//!   parent's US-09 compatibility engine ([`compatibility::compute_indicator`])
//!   into the [`FolderClassification`] sum type per the data-models §3 mapping.
//! - [`build_folder_delete_plan`] — assembles the immutable
//!   [`FolderDeletePlan`] that the orchestrator hands to the plugin.
//!
//! Per `docs/feature/folder-group-bulk-delete/design/component-boundaries.md`
//! § "New module: modeltap-core::logic::folder_group" and
//! `architecture-design.md` §4.3.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::domain::RowIndicator;
use crate::logic::compatibility::{
    compute_indicator, is_dedup_key_match, Inventory, InventoryEntry, PluginCapabilityMap,
};
use crate::types::{
    DedupKey, DiscoveredModel, FolderClassification, FolderDeletePlan, FolderGroup, ModelMeta,
    SharedModel, Sidecar, ToolId,
};

/// Partition an HF `ToolInventory.models` slice by the `<author>/<repo>` prefix
/// of each model's `id_in_tool`. Returns one [`FolderGroup`] per repo, each
/// paired with whichever sidecars the caller supplied for that repo key (the
/// HF plugin enumerates sidecars on the filesystem; this layer is pure).
///
/// Order is deterministic and alphabetic by `<author>/<repo>` (BTreeMap-keyed
/// internal grouping). Empty input yields an empty `Vec`.
///
/// # Invariants
///
/// - Every produced `FolderGroup` has `tool == ToolId("hf")` (per B-FGD-1).
/// - `FolderGroup::path` matches the canonical `^[^/]+/[^/]+$` regex by
///   construction (split-by-first-slash extracts exactly author + repo).
/// - `absolute_path` is synthesized from the repo via the HF cache layout
///   convention `models--<author>--<repo>`. If the caller has a more precise
///   on-disk path, the orchestrator may overwrite this; it is correct
///   *relative to the cache root* but the cache root itself is plugin-owned.
/// - Models whose `id_in_tool` does not contain a `/` (malformed) are
///   silently skipped — they cannot be folder-grouped, and dropping them
///   here is preferable to panicking inside a pure function.
pub fn group_by_hf_repo(
    hf_models: &[ModelMeta],
    sidecars_by_repo: &BTreeMap<String, Vec<Sidecar>>,
) -> Vec<FolderGroup> {
    let mut grouped: BTreeMap<String, Vec<ModelMeta>> = BTreeMap::new();
    for model in hf_models {
        if let Some(repo) = repo_prefix(&model.id_in_tool) {
            grouped.entry(repo).or_default().push(model.clone());
        }
    }

    grouped
        .into_iter()
        .filter_map(|(repo, models)| {
            let absolute_path = synthesize_absolute_path(&repo);
            let sidecars = sidecars_by_repo.get(&repo).cloned().unwrap_or_default();
            // `FolderGroup::new` enforces the smart-constructor invariants;
            // by construction here the path is canonical and the tool is hf,
            // so the only way `new` returns Err is if the caller put a
            // malformed `repo` key in `sidecars_by_repo` matching a model id —
            // skip those defensively.
            FolderGroup::new(repo, absolute_path, ToolId("hf"), models, sidecars).ok()
        })
        .collect()
}

/// Extract the `<author>/<repo>` prefix from a model's `id_in_tool` of the form
/// `<author>/<repo>/<file...>`. Returns `None` if the id has fewer than two
/// path segments.
fn repo_prefix(id_in_tool: &str) -> Option<String> {
    let mut parts = id_in_tool.splitn(3, '/');
    let author = parts.next()?;
    let repo = parts.next()?;
    if author.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{author}/{repo}"))
}

/// Synthesize the HF cache directory name `models--<author>--<repo>`. The
/// orchestrator supplies the cache root; we return a relative-shaped path
/// the orchestrator can rebase. Callers that already know the absolute root
/// may overwrite this field on the returned [`FolderGroup`].
fn synthesize_absolute_path(repo: &str) -> PathBuf {
    // `repo` is canonical `<author>/<repo>` per `repo_prefix`.
    let (author, name) = repo
        .split_once('/')
        .expect("repo is canonical by construction");
    PathBuf::from(format!("models--{author}--{name}"))
}

/// Project each child model in `folder` through [`compute_indicator`] and
/// route it into the [`FolderClassification`] sum type per the
/// `data-models.md` §3 mapping:
///
/// | `RowIndicator` | Bucket |
/// |---|---|
/// | `Shared` | `shared` — with `other_tools` from the dedup-group |
/// | `Compatible` | `unique` (single-tool, format accepted elsewhere) |
/// | `FormatLocked` | `unique` (single-tool, format locked to HF) |
/// | `Unknown` | `unique` (conservative-when-uncertain — ADR-002) |
///
/// # Single-engine invariant (AC-13 / D-FGD-4)
///
/// This function is THE SINGLE SOURCE OF TRUTH for shared-vs-unique decisions
/// on folder children. The body calls
/// [`compute_indicator`] for every model — there is no parallel
/// implementation, no by-hand SHA256 comparison, no shadow path. Any future
/// change to the dedup/compatibility rules MUST flow through that one engine.
///
/// The peer reviewer and the unit tests in
/// `crates/modeltap-core/tests/folder_group_logic.rs` enforce this by
/// inspection plus a proptest that pins ADR-002's conservative-when-uncertain
/// rule (Tentative dedup keys never yield Shared).
///
/// Pure function; no I/O.
pub fn classify_unique_vs_shared(
    folder: &FolderGroup,
    inventory: &Inventory,
    capabilities: &PluginCapabilityMap,
) -> FolderClassification {
    let mut unique: Vec<ModelMeta> = Vec::new();
    let mut shared: Vec<SharedModel> = Vec::new();

    for model in &folder.models {
        let target_entry = inventory_entry_for(model);
        // SINGLE-ENGINE SEAM (D-FGD-4 / AC-13): every classification flows
        // through compute_indicator. Do not reimplement the rules here.
        let indicator = compute_indicator(&target_entry, inventory, capabilities);
        match indicator {
            RowIndicator::Shared => {
                let other_tools = peer_tools_sharing(&target_entry, inventory);
                shared.push(SharedModel {
                    model: model.clone(),
                    other_tools,
                });
            }
            // Conservative-when-uncertain (data-models §3): Compatible /
            // FormatLocked / Unknown all land in unique. The cross-tool
            // hardlink-survival guarantee (INT-FGD-4) holds via the HF
            // plugin's ref-counting in `delete_one_at`, not via this
            // classification.
            RowIndicator::Compatible | RowIndicator::FormatLocked | RowIndicator::Unknown => {
                unique.push(model.clone());
            }
        }
    }

    FolderClassification { unique, shared }
}

/// Project a [`ModelMeta`] back into an [`InventoryEntry`] so the
/// compatibility engine can consume it. `content_hash` is `Some(_)` iff the
/// dedup-key has been upgraded to `Content` — `Tentative` keys preserve the
/// engine's conservative-when-uncertain behavior.
fn inventory_entry_for(model: &ModelMeta) -> InventoryEntry {
    let content_hash = match &model.dedup_key {
        DedupKey::Content(hash) => Some(*hash),
        DedupKey::Tentative(_) => None,
    };
    InventoryEntry {
        tool: model.tool,
        model: DiscoveredModel {
            id_in_tool: model.id_in_tool.clone(),
            on_disk_path: model.on_disk_path.clone(),
            size_bytes: model.size_bytes,
            format: model.format,
            display_label: model.display_label.clone(),
            status: model.status.clone(),
        },
        content_hash,
    }
}

/// Find the distinct list of other-tool `ToolId`s whose inventory entry has a
/// dedup-key matching `target`. Delegates to
/// [`compatibility::is_dedup_key_match`] so the single-engine invariant is
/// preserved — the rule that decides Shared (in `compute_indicator`) and the
/// rule that lists the peers (here) share one definition.
///
/// Deterministic order: first-seen order in the inventory (stable per
/// `Vec::iter`), with duplicates collapsed via a linear scan (a `BTreeSet`
/// would re-sort and lose the inventory's natural order which downstream
/// snapshot tests rely on).
fn peer_tools_sharing(target: &InventoryEntry, inventory: &Inventory) -> Vec<ToolId> {
    let mut out: Vec<ToolId> = Vec::new();
    for peer in &inventory.entries {
        if peer.tool == target.tool {
            continue;
        }
        if !is_dedup_key_match(target, peer) {
            continue;
        }
        if !out.contains(&peer.tool) {
            out.push(peer.tool);
        }
    }
    out
}

/// Build the immutable [`FolderDeletePlan`] from a [`FolderGroup`] and its
/// [`FolderClassification`].
///
/// Computes:
/// - `paths_to_unlink_fully` — unique-model paths + all sidecar paths.
/// - `paths_to_unlink_hf_only` — shared-model HF-side paths (the other tool's
///   hardlink keeps the inode alive).
/// - `bytes_to_reclaim` — unique-model bytes + sidecar bytes.
/// - `bytes_to_retain` — shared-model bytes.
///
/// # Invariants (enforced)
///
/// - `paths_to_unlink_fully.len() + paths_to_unlink_hf_only.len() ==
///   folder.file_count()` (INT-FGD-2).
/// - `bytes_to_reclaim + bytes_to_retain == folder.total_bytes()` within
///   1-byte rounding tolerance (INT-FGD-3 / AC-7) — checked by
///   [`FolderDeletePlan::new`]'s smart-constructor. Construction is
///   infallible here by arithmetic identity (sums of disjoint partitions of
///   the same `u64` slice), so we `expect` rather than propagate.
///
/// Pure function; no I/O.
pub fn build_folder_delete_plan(
    folder: &FolderGroup,
    classification: &FolderClassification,
) -> FolderDeletePlan {
    let paths_to_unlink_fully: Vec<PathBuf> = classification
        .unique
        .iter()
        .map(|m| m.on_disk_path.clone())
        .chain(folder.sidecars.iter().map(|s| s.path.clone()))
        .collect();
    let paths_to_unlink_hf_only: Vec<PathBuf> = classification
        .shared
        .iter()
        .map(|s| s.model.on_disk_path.clone())
        .collect();

    let bytes_to_reclaim: u64 = classification
        .unique
        .iter()
        .map(|m| m.size_bytes)
        .sum::<u64>()
        + folder.sidecars.iter().map(|s| s.size_bytes).sum::<u64>();
    let bytes_to_retain: u64 = classification
        .shared
        .iter()
        .map(|s| s.model.size_bytes)
        .sum();

    FolderDeletePlan::new(
        folder.clone(),
        classification.clone(),
        paths_to_unlink_fully,
        paths_to_unlink_hf_only,
        bytes_to_reclaim,
        bytes_to_retain,
    )
    .expect(
        "build_folder_delete_plan: reclaim+retain == total holds by construction \
         (disjoint partition of folder bytes)",
    )
}
