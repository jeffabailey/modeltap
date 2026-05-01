//! Unit tests for `modeltap_core::logic::dedup::compute_dedup_glyph` (step 01-02).
//!
//! Per `architecture-design.md` §6.2 the glyph derivation table is:
//!
//! ```text
//! DedupGlyph =
//!   Pending           if no hash AND not in_progress         → "?"
//!   Hashing           if in_progress contains this model_id  → "~"
//!   Failed            if hash failed (sentinel in cache)     → "-" + "!"
//!   AlreadyUnified    if ≥2 paths share one inode AND no
//!                     other-tool path holds a separate copy  → "#"
//!   DedupAble         if ≥2 separate inodes have same SHA256 → "="
//!   Unique            otherwise                              → "-"
//! ```
//!
//! Test budget (per `quality-framework`):
//!   distinct behaviors:
//!     B1: target with no hash and not in_progress → Pending
//!     B2: target in `in_progress` set → Hashing (overrides everything else)
//!     B3: target in `failed` set → Failed (overrides hashes, classification)
//!     B4: hash known, no peer match → Unique
//!     B5: hash known, peer in other tool, different inodes → DedupAble
//!     B6: hash known, peer in other tool, same (device, inode) → AlreadyUnified
//!     B7: edge cases (empty inventory, peer in same tool only)
//!   budget = 7 × 2 = 14 tests max. We use 11.
//!
//! Conservative-when-uncertain (BR-3): if hash failure, return `Failed`. The
//! action layer treats `Failed` like `Unique` (won't propose unify) but the
//! renderer surfaces the `!` decorator so the user sees something happened.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use modeltap_core::domain::dedup_glyph::DedupGlyph;
use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::logic::dedup::{compute_dedup_glyph, InodeMap, ModelKey};
use modeltap_core::{ContentHash, DiscoveredModel, DisplayLabel, Format, ModelStatus, ToolId};

// --- Helpers ---------------------------------------------------------------

fn entry(
    tool: &'static str,
    id_in_tool: &str,
    path: &str,
    size: u64,
    hash: Option<[u8; 32]>,
) -> InventoryEntry {
    InventoryEntry {
        tool: ToolId(tool),
        model: DiscoveredModel {
            id_in_tool: id_in_tool.to_string(),
            on_disk_path: PathBuf::from(path),
            size_bytes: size,
            format: Format::Gguf,
            display_label: DisplayLabel::from(id_in_tool),
            status: ModelStatus::Healthy,
        },
        content_hash: hash.map(ContentHash),
    }
}

fn key(tool: &'static str, id_in_tool: &str) -> ModelKey {
    (ToolId(tool), id_in_tool.to_string())
}

fn h(byte: u8) -> [u8; 32] {
    [byte; 32]
}

// --- B1: Pending ----------------------------------------------------------

#[test]
fn pending_when_no_hash_and_not_in_progress() {
    // Target has no SHA256 yet, no worker assigned. Glyph: "?".
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, None);
    let inventory = Inventory {
        entries: vec![target.clone()],
    };
    let inodes: InodeMap = HashMap::new();
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::Pending);
}

// --- B2: Hashing ----------------------------------------------------------

#[test]
fn hashing_when_target_is_in_progress() {
    // Worker is currently hashing this model. Glyph: "~". Even if peers exist
    // and could classify the row, in_progress wins until the hash completes.
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, None);
    let inventory = Inventory {
        entries: vec![target.clone()],
    };
    let inodes: InodeMap = HashMap::new();
    let mut in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    in_progress.insert(key("ollama", "llama3:8b"));
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::Hashing);
}

#[test]
fn hashing_overrides_existing_hash_when_target_is_in_progress() {
    // Edge case: in_progress wins even if a stale hash is present (e.g., a
    // re-hash was queued because the file changed). Renderer should show "~"
    // rather than confidently classifying with possibly-stale data.
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, Some(h(1)));
    let peer = entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1)));
    let inventory = Inventory {
        entries: vec![target.clone(), peer],
    };
    let inodes: InodeMap = HashMap::new();
    let mut in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    in_progress.insert(key("ollama", "llama3:8b"));
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::Hashing);
}

// --- B3: Failed -----------------------------------------------------------

#[test]
fn failed_when_target_is_in_failed_set() {
    // Hashing surfaced an error (read error, IO error). Glyph: "-" with "!"
    // decorator. Conservative-when-uncertain (BR-3): the action layer treats
    // this like Unique (won't unify) but the renderer surfaces the "!".
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, None);
    let inventory = Inventory {
        entries: vec![target.clone()],
    };
    let inodes: InodeMap = HashMap::new();
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let mut failed: BTreeSet<ModelKey> = BTreeSet::new();
    failed.insert(key("ollama", "llama3:8b"));

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::Failed);
}

#[test]
fn failed_overrides_hash_classification_when_target_failed() {
    // BR-3 conservative-when-uncertain: even if a hash is somehow present
    // alongside a `failed` entry (defensive — shouldn't happen but the table
    // is explicit), Failed wins over the would-be-DedupAble classification.
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, Some(h(1)));
    let peer = entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1)));
    let inventory = Inventory {
        entries: vec![target.clone(), peer],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3:8b"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 200));
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let mut failed: BTreeSet<ModelKey> = BTreeSet::new();
    failed.insert(key("ollama", "llama3:8b"));

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::Failed);
}

// --- B4: Unique -----------------------------------------------------------

#[test]
fn unique_when_hash_known_and_no_peer_match() {
    // Hash computed, but no other entry in the inventory has the same hash.
    // Glyph: "-" (no decorator).
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, Some(h(1)));
    let other = entry("hf", "mistral-7b.gguf", "/h/mistral", 2000, Some(h(2)));
    let inventory = Inventory {
        entries: vec![target.clone(), other],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3:8b"), (1, 100));
    inodes.insert(key("hf", "mistral-7b.gguf"), (1, 200));
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::Unique);
}

#[test]
fn unique_when_hash_known_and_only_same_tool_peers_match() {
    // Same-tool duplicates are NOT cross-tool dedup (the user's mental model
    // for "=" is "another tool also has this content"). Two ollama entries
    // with the same hash → from ollama's perspective, target is Unique
    // cross-tool.
    let target = entry("ollama", "llama3:8b", "/o/llama3-a", 1000, Some(h(1)));
    let same_tool_peer = entry("ollama", "llama3:8b-alt", "/o/llama3-b", 1000, Some(h(1)));
    let inventory = Inventory {
        entries: vec![target.clone(), same_tool_peer],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3:8b"), (1, 100));
    inodes.insert(key("ollama", "llama3:8b-alt"), (1, 200));
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::Unique);
}

// --- B5: DedupAble --------------------------------------------------------

#[test]
fn dedup_able_when_other_tool_has_same_hash_but_different_inode() {
    // ollama's llama3 and hf's llama-3.gguf have identical content (same
    // SHA256) but live on disk as separate inodes. The unify action would
    // hardlink one into the other. Glyph: "=".
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, Some(h(1)));
    let peer = entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1)));
    let inventory = Inventory {
        entries: vec![target.clone(), peer],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3:8b"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 200)); // different inode
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::DedupAble);
}

// --- B6: AlreadyUnified ---------------------------------------------------

#[test]
fn already_unified_when_other_tool_shares_same_device_and_inode() {
    // ollama's llama3 and hf's llama-3.gguf are already hardlinked: same
    // SHA256, same (device, inode). Glyph: "#" (already unified).
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, Some(h(1)));
    let peer = entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1)));
    let inventory = Inventory {
        entries: vec![target.clone(), peer],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3:8b"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 100)); // SAME device + inode
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::AlreadyUnified);
}

#[test]
fn dedup_able_when_third_tool_holds_separate_copy_even_if_two_already_unified() {
    // Three tools: ollama+hf already share an inode (`#` candidate), but a
    // third tool (lm-studio) has the SAME content on a DIFFERENT inode.
    // §6.2 row #4: AlreadyUnified requires that NO other-tool path holds a
    // separate copy. With lm-studio holding a separate copy, the row's true
    // state is DedupAble — the user can still unify lm-studio in.
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, Some(h(1)));
    let peer_unified = entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1)));
    let peer_separate = entry("lm-studio", "llama-3", "/m/llama3", 1000, Some(h(1)));
    let inventory = Inventory {
        entries: vec![target.clone(), peer_unified, peer_separate],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3:8b"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 100)); // shared with ollama
    inodes.insert(key("lm-studio", "llama-3"), (1, 200)); // separate
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(
        glyph,
        DedupGlyph::DedupAble,
        "third-tool separate copy outranks the partial unification"
    );
}

// --- B7: Edge cases -------------------------------------------------------

#[test]
fn unique_when_hash_known_and_inventory_contains_only_target() {
    // Single-entry inventory: target's hash is known but there's literally
    // no peer to match against. Falls through to Unique.
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, Some(h(1)));
    let inventory = Inventory {
        entries: vec![target.clone()],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3:8b"), (1, 100));
    let in_progress: BTreeSet<ModelKey> = BTreeSet::new();
    let failed: BTreeSet<ModelKey> = BTreeSet::new();

    let glyph = compute_dedup_glyph(&target, &inventory, &inodes, &in_progress, &failed);
    assert_eq!(glyph, DedupGlyph::Unique);
}
