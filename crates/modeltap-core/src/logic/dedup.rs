//! Pure-domain unique-vs-shared classifier for the zap-all dialog.
//!
//! Per ADR-002 conservative-deletion rule: when the dedup-key is uncertain,
//! treat the model as unique (preserves data). For the WS slice we have no
//! SHA256 yet (lazy hashing arrives in 01-05), so the classifier uses the
//! `on_disk_path` as the only authoritative same-content signal:
//!
//!   - Two `ToolModel` entries with the SAME `on_disk_path` from DIFFERENT
//!     tools are SHARED (deleting from one tool does NOT free those bytes
//!     because another tool still references the same file).
//!   - Everything else is UNIQUE to the queried tool.
//!
//! ADR-002 conservative-deletion rule citation: this conservative posture is
//! the safety guarantee — if we are unsure two files are duplicates, we keep
//! both. The classifier never flags as "shared" anything whose dedup-key is
//! uncertain; the only "shared" signal it accepts is byte-identical paths.
//!
//! When SHA256 hashing lands (01-05+), the classifier will additionally treat
//! same-content-different-paths as shared, but that change is purely additive
//! — the safety property holds.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use serde::Serialize;

use crate::domain::dedup_glyph::DedupGlyph;
use crate::domain::dedup_summary::{DedupSummary, UnifiedRow};
use crate::logic::compatibility::{Inventory, InventoryEntry};
use crate::types::{ContentHash, DisplayLabel, Format, ModelStatus, ToolId};

/// Per-tool projection of one discovered model — the cross-plugin view the
/// classifier consumes. Identical fields to `ModelMeta` minus the dedup_key
/// (the classifier IS the dedup-key authority for the WS slice).
#[derive(Debug, Clone, Serialize)]
pub struct ToolModel {
    pub tool: ToolId,
    pub id_in_tool: String,
    pub on_disk_path: PathBuf,
    pub size_bytes: u64,
    pub format: Format,
    pub display_label: DisplayLabel,
    pub status: ModelStatus,
}

/// Output of `classify_unique_vs_shared`: counts and byte totals for the
/// queried tool. `unique_count + shared_count` equals the number of models
/// that tool registers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueVsSharedReport {
    pub unique_count: u64,
    pub shared_count: u64,
    pub unique_bytes: u64,
    pub shared_bytes: u64,
}

/// Classify each model registered with `tool_id` as either unique to that
/// tool (deleting it actually frees its bytes) or shared with another tool
/// (deleting it only removes the registration; bytes remain referenced
/// elsewhere on disk).
///
/// Per ADR-002 conservative-deletion rule, this WS-slice implementation
/// treats two entries as SHARED only when their `on_disk_path` is byte-equal
/// AND they belong to different tools. All other cases (different paths,
/// uncomputed hashes) are treated as UNIQUE — preserving data is the safer
/// default.
pub fn classify_unique_vs_shared(
    inventory: &[ToolModel],
    tool_id: &ToolId,
) -> UniqueVsSharedReport {
    // Index every on_disk_path → set of tools that reference it.
    let mut path_tools: HashMap<&PathBuf, Vec<ToolId>> = HashMap::new();
    for m in inventory {
        path_tools.entry(&m.on_disk_path).or_default().push(m.tool);
    }

    let mut report = UniqueVsSharedReport {
        unique_count: 0,
        shared_count: 0,
        unique_bytes: 0,
        shared_bytes: 0,
    };

    for m in inventory {
        if m.tool != *tool_id {
            continue;
        }
        // Shared iff some OTHER tool also references this exact path.
        let referenced_by_others = path_tools
            .get(&m.on_disk_path)
            .map(|tools| tools.iter().any(|t| t != tool_id))
            .unwrap_or(false);
        if referenced_by_others {
            report.shared_count = report.shared_count.saturating_add(1);
            report.shared_bytes = report.shared_bytes.saturating_add(m.size_bytes);
        } else {
            report.unique_count = report.unique_count.saturating_add(1);
            report.unique_bytes = report.unique_bytes.saturating_add(m.size_bytes);
        }
    }

    report
}

// ---------------------------------------------------------------------------
// Step 01-02: per-row dedup-glyph classifier + dedup summary
// ---------------------------------------------------------------------------
//
// Per `docs/feature/cross-tool-model-unify/design/architecture-design.md` §6.2
// the row glyph is one of `{?, ~, -, =, #}` plus a `!` decorator on hash
// failure. The classifier is a pure function — all its inputs (in_progress,
// failed, inode map) are passed in by the orchestrator. Per the §6.2
// implementation note: do NOT re-stat in the classifier.

/// Identity of a row in the cross-plugin inventory: `(tool, id_in_tool)`.
/// Mirrors the access pattern already used elsewhere (a `String` keyed within
/// the owning tool, namespaced by `ToolId`). A dedicated `ModelId` newtype
/// may emerge later when cross-tool identity becomes load-bearing.
pub type ModelKey = (ToolId, String);

/// `(device, inode)` pair for each inventory entry, supplied by the
/// orchestrator from a prior `stat()` call. Per architecture-design.md §6.2:
/// the classifier MUST NOT re-stat — pass it in.
pub type InodeMap = HashMap<ModelKey, (u64, u64)>;

/// Compute the per-row dedup glyph for `target` given the cross-plugin
/// inventory and the live hash-pool state.
///
/// Per the §6.2 derivation table, evaluation order is:
///
/// 1. `in_progress` contains the target → `Hashing` (overrides everything).
/// 2. `failed` contains the target → `Failed` (BR-3 conservative-when-uncertain
///    sentinel; renderer maps to `-` with `!` decorator).
/// 3. Target's `content_hash` is `None` → `Pending`.
/// 4. Target's hash matches at least one OTHER-tool peer:
///    - all matching peers (incl. target) share the SAME `(device, inode)` AND
///      no separate-inode peer exists → `AlreadyUnified`
///    - at least one matching peer has a DIFFERENT `(device, inode)` → `DedupAble`
/// 5. Otherwise → `Unique`.
///
/// All inputs are by-reference; the function is O(N) over the inventory size.
/// No I/O, no panics.
pub fn compute_dedup_glyph(
    target: &InventoryEntry,
    inventory: &Inventory,
    inodes: &InodeMap,
    in_progress: &BTreeSet<ModelKey>,
    failed: &BTreeSet<ModelKey>,
) -> DedupGlyph {
    let target_key: ModelKey = (target.tool, target.model.id_in_tool.clone());

    // 1. Hashing wins over any classification — even a stale hash. Renderer
    //    surfaces `~` until the (re-)hash completes.
    if in_progress.contains(&target_key) {
        return DedupGlyph::Hashing;
    }

    // 2. Hash-failure sentinel (BR-3): conservative, never overstate sharing.
    if failed.contains(&target_key) {
        return DedupGlyph::Failed;
    }

    // 3. No hash yet (and no worker assigned): Pending.
    let Some(target_hash) = target.content_hash else {
        return DedupGlyph::Pending;
    };

    // 4. Classify against OTHER-tool peers with matching SHA256.
    let target_inode = inodes.get(&target_key).copied();
    let mut has_separate_inode_peer = false;
    let mut has_shared_inode_peer = false;

    for peer in &inventory.entries {
        if peer.tool == target.tool && peer.model.id_in_tool == target.model.id_in_tool {
            continue; // skip self
        }
        if peer.tool == target.tool {
            continue; // same-tool peers don't drive cross-tool dedup classification
        }
        if !content_hash_matches(target_hash, peer.content_hash) {
            continue;
        }
        let peer_key: ModelKey = (peer.tool, peer.model.id_in_tool.clone());
        let peer_inode = inodes.get(&peer_key).copied();
        match (target_inode, peer_inode) {
            (Some(t), Some(p)) if t == p => has_shared_inode_peer = true,
            _ => has_separate_inode_peer = true,
        }
    }

    if has_separate_inode_peer {
        // §6.2 row #3: ≥2 separate inodes have same SHA256 → DedupAble.
        // This outranks AlreadyUnified per row #4: AlreadyUnified requires
        // that NO other-tool path holds a separate copy. If even one separate
        // copy exists, the user can still unify it in.
        DedupGlyph::DedupAble
    } else if has_shared_inode_peer {
        // §6.2 row #4: ≥2 paths share one inode AND no separate copy.
        DedupGlyph::AlreadyUnified
    } else {
        // §6.2 row #5: otherwise → Unique.
        DedupGlyph::Unique
    }
}

/// Equal-when-both-known content hash comparison. Mirrors the conservative
/// rule in `compatibility::is_dedup_key_match`: `None` on either side means
/// "we are not sure" → not a match. Used by `compute_dedup_glyph` to decide
/// whether a peer participates in the classification.
fn content_hash_matches(target: ContentHash, peer: Option<ContentHash>) -> bool {
    matches!(peer, Some(h) if h == target)
}

/// Compute the top-level dedup aggregates carried on `AppState` for the
/// summary bar and the `[All Unified]` slot badge.
///
/// Per `data-models.md` §dedup_summary the three Option<u64> fields use the
/// convention:
///   - `None` → `computing...` should be displayed
///   - `Some(n)` → real value, render the number
///
/// While `hashing_done` is `false` we honestly cannot report — a not-yet-hashed
/// file might still turn out to be dedup-able. Once `hashing_done` is `true`
/// the function aggregates over the inventory:
///
/// - `dedup_able_bytes` = sum of `size_bytes` over distinct hashes that have
///   at least two cross-tool entries living on DIFFERENT inodes. Counted
///   ONCE per hash-group (the canonical's size; reclaimable bytes ≈ that).
/// - `unified_count` = number of distinct hash-groups whose cross-tool
///   entries already share a single `(device, inode)` AND have no
///   separate-inode peer.
/// - `total_saved_by_unification` = sum over those unified groups of
///   `(N - 1) * size_bytes` where N is the number of paths sharing the inode.
///
/// Pure function, no I/O.
pub fn dedup_summary(inventory: &Inventory, inodes: &InodeMap, hashing_done: bool) -> DedupSummary {
    if !hashing_done {
        return DedupSummary::default();
    }

    // Group entries by SHA256. Entries without a hash do not contribute (they
    // also indicate hashing isn't truly done; but we trust the caller's
    // boolean and conservatively skip them).
    let mut groups: HashMap<ContentHash, Vec<&InventoryEntry>> = HashMap::new();
    for entry in &inventory.entries {
        if let Some(hash) = entry.content_hash {
            groups.entry(hash).or_default().push(entry);
        }
    }

    let mut dedup_able_bytes: u64 = 0;
    let mut unified_count: u64 = 0;
    let mut total_saved_by_unification: u64 = 0;

    for members in groups.values() {
        // Need at least two members to dedup or unify.
        if members.len() < 2 {
            continue;
        }
        // Deduplicate by tool: a single tool listing twice doesn't count as
        // cross-tool sharing for the purposes of the summary bar.
        let cross_tool: Vec<&&InventoryEntry> = {
            let mut tools_seen: BTreeSet<ToolId> = BTreeSet::new();
            members
                .iter()
                .filter(|e| tools_seen.insert(e.tool))
                .collect()
        };
        if cross_tool.len() < 2 {
            continue;
        }

        // Compute distinct (device, inode) pairs across the group.
        let mut inodes_seen: BTreeSet<(u64, u64)> = BTreeSet::new();
        let mut entries_with_inode: u64 = 0;
        for m in &cross_tool {
            let mkey: ModelKey = (m.tool, m.model.id_in_tool.clone());
            if let Some(devino) = inodes.get(&mkey) {
                inodes_seen.insert(*devino);
                entries_with_inode = entries_with_inode.saturating_add(1);
            }
        }

        // Use the size of the first member as the representative size.
        // (All members of a hash-group should have the same size_bytes; we
        // pick one rather than averaging.)
        let size_bytes = cross_tool[0].model.size_bytes;

        if inodes_seen.len() >= 2 {
            // Separate inodes exist → reclaimable. Count this hash-group's
            // size once toward the dedup-able total. (One inode worth of
            // bytes can be reclaimed by hardlinking the others into it.)
            dedup_able_bytes = dedup_able_bytes.saturating_add(size_bytes);
        } else if inodes_seen.len() == 1 && entries_with_inode >= 2 {
            // All cross-tool entries share a single inode AND we have inode
            // data for at least two of them → already unified.
            unified_count = unified_count.saturating_add(1);
            // Saves = (N - 1) * size, where N is the number of cross-tool
            // entries sharing the inode.
            let saves = entries_with_inode
                .saturating_sub(1)
                .saturating_mul(size_bytes);
            total_saved_by_unification = total_saved_by_unification.saturating_add(saves);
        }
        // If `inodes_seen.is_empty()`, we have no inode data — the group is
        // not classifiable. With `hashing_done == true` this is unusual but
        // we conservatively skip rather than misreport.
    }

    DedupSummary {
        dedup_able_bytes: Some(dedup_able_bytes),
        unified_count: Some(unified_count),
        total_saved_by_unification: Some(total_saved_by_unification),
    }
}

// ---------------------------------------------------------------------------
// Step 04-01: collect_unified_rows — render data for the All-Unified view
// ---------------------------------------------------------------------------

/// Assemble the right-pane `[All Unified]` rows: one row per cross-tool
/// group whose entries share one `(device, inode)` AND content hash.
///
/// This is the single source of truth for both the All-Unified right-pane
/// view and the footer aggregates derived from it. NOT an action — purely
/// render-data assembly. Pure function: no I/O, no state.
///
/// Algorithm (mirrors `dedup_summary` group-by-hash logic):
///
/// 1. Group inventory entries by SHA256 content hash. Entries without a
///    hash are skipped (we cannot prove cross-tool sharing without it).
/// 2. Within each hash-group, deduplicate by tool — a single tool listing
///    the same hash twice is not cross-tool sharing.
/// 3. A group is "unified" when ≥ 2 cross-tool entries share ONE
///    `(device, inode)` and no separate-inode peer exists for that hash.
/// 4. For each unified group emit a `UnifiedRow` with:
///    - `model_id_in_tool` + `display_label` + `size_bytes` from a
///      representative member (chosen deterministically by sorted ToolId
///      then id_in_tool).
///    - `tools_sharing` = sorted Vec<ToolId> of members.
///    - `saves_bytes = (tools_sharing.len() - 1) * size_bytes` per ADR-002.
/// 5. Return rows sorted by `display_label` ascending (with `model_id_in_tool`
///    as a tiebreaker) for deterministic render order.
pub fn collect_unified_rows(inventory: &Inventory, inodes: &InodeMap) -> Vec<UnifiedRow> {
    // Group entries by SHA256.
    let mut groups: HashMap<ContentHash, Vec<&InventoryEntry>> = HashMap::new();
    for entry in &inventory.entries {
        if let Some(hash) = entry.content_hash {
            groups.entry(hash).or_default().push(entry);
        }
    }

    let mut rows: Vec<UnifiedRow> = Vec::new();

    for members in groups.values() {
        if members.len() < 2 {
            continue;
        }
        // Deduplicate by tool — a single tool listing the same hash twice
        // doesn't count as cross-tool sharing for the All-Unified view.
        let cross_tool: Vec<&&InventoryEntry> = {
            let mut tools_seen: BTreeSet<ToolId> = BTreeSet::new();
            members
                .iter()
                .filter(|e| tools_seen.insert(e.tool))
                .collect()
        };
        if cross_tool.len() < 2 {
            continue;
        }

        // Compute distinct (device, inode) pairs across the cross-tool group.
        let mut inodes_seen: BTreeSet<(u64, u64)> = BTreeSet::new();
        let mut entries_with_inode: u64 = 0;
        for m in &cross_tool {
            let mkey: ModelKey = (m.tool, m.model.id_in_tool.clone());
            if let Some(devino) = inodes.get(&mkey) {
                inodes_seen.insert(*devino);
                entries_with_inode = entries_with_inode.saturating_add(1);
            }
        }

        // A unified group: one shared inode, no separate-inode peer, and
        // ≥ 2 cross-tool entries actually share that inode.
        if inodes_seen.len() != 1 || entries_with_inode < 2 {
            continue;
        }

        // Pick a deterministic representative: sort cross-tool members by
        // (ToolId, id_in_tool) and take the first. Same-tool size, label, and
        // id are the row's display source.
        let mut sorted_members: Vec<&&InventoryEntry> = cross_tool.clone();
        sorted_members.sort_by(|a, b| {
            a.tool
                .0
                .cmp(b.tool.0)
                .then_with(|| a.model.id_in_tool.cmp(&b.model.id_in_tool))
        });
        let representative = sorted_members[0];

        // tools_sharing: sorted Vec<ToolId>.
        let mut tools_sharing: Vec<ToolId> = cross_tool.iter().map(|e| e.tool).collect();
        tools_sharing.sort();

        let size_bytes = representative.model.size_bytes;
        let saves_bytes = (tools_sharing.len() as u64)
            .saturating_sub(1)
            .saturating_mul(size_bytes);

        rows.push(UnifiedRow {
            model_id_in_tool: representative.model.id_in_tool.clone(),
            display_label: representative.model.display_label.clone(),
            size_bytes,
            tools_sharing,
            saves_bytes,
        });
    }

    // Deterministic render order: by display_label asc, then by model_id_in_tool.
    rows.sort_by(|a, b| {
        a.display_label
            .0
            .cmp(&b.display_label.0)
            .then_with(|| a.model_id_in_tool.cmp(&b.model_id_in_tool))
    });

    rows
}
