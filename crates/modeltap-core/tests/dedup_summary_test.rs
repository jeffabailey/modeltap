//! Unit tests for `modeltap_core::logic::dedup::dedup_summary` (step 01-02).
//!
//! Per `data-models.md` §dedup_summary the function returns a `DedupSummary`
//! whose three Option<u64> fields use the convention:
//!   - `None` → "computing..." should be displayed
//!   - `Some(n)` → real value, render the number
//!
//! Test budget (per `quality-framework`):
//!   distinct behaviors:
//!     B1: hashing not done → all fields are `None` (computing state)
//!     B2: hashing done, no dedup-able peers → zero values
//!     B3: hashing done, dedup-able peers → sums dedup-able bytes correctly
//!     B4: hashing done, already-unified groups → counts groups + saves
//!   budget = 4 × 2 = 8 tests max. We use 5.

use std::collections::HashMap;
use std::path::PathBuf;

use modeltap_core::domain::dedup_summary::DedupSummary;
use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::logic::dedup::{dedup_summary, InodeMap, ModelKey};
use modeltap_core::{ContentHash, DiscoveredModel, DisplayLabel, Format, ModelStatus, ToolId};

// --- Helpers --------------------------------------------------------------

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

// --- B1: computing state --------------------------------------------------

#[test]
fn returns_all_none_while_hashing_in_progress() {
    // Per data-models.md: a `None` value means "not yet known — display
    // `computing...`". Until hashing is done, the summary cannot honestly
    // report dedup-able bytes (a not-yet-hashed file might end up dedup-able).
    let target = entry("ollama", "llama3:8b", "/o/llama3", 1000, Some(h(1)));
    let peer = entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1)));
    let inventory = Inventory {
        entries: vec![target, peer],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3:8b"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 200));

    let summary = dedup_summary(&inventory, &inodes, /* hashing_done */ false);
    assert_eq!(summary, DedupSummary::default());
    assert_eq!(summary.dedup_able_bytes, None);
    assert_eq!(summary.unified_count, None);
    assert_eq!(summary.total_saved_by_unification, None);
}

// --- B2: nothing to dedup -------------------------------------------------

#[test]
fn returns_zeros_when_hashing_done_but_no_dedup_able_peers() {
    // All distinct content, hashing complete → 0 dedup-able bytes, 0 unified
    // groups, 0 saves. Note: the values are `Some(0)`, NOT `None` — the
    // computation has run, the answer is genuinely zero.
    let a = entry("ollama", "llama3", "/o/llama3", 1000, Some(h(1)));
    let b = entry("hf", "mistral", "/h/mistral", 2000, Some(h(2)));
    let inventory = Inventory {
        entries: vec![a, b],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3"), (1, 100));
    inodes.insert(key("hf", "mistral"), (1, 200));

    let summary = dedup_summary(&inventory, &inodes, true);
    assert_eq!(summary.dedup_able_bytes, Some(0));
    assert_eq!(summary.unified_count, Some(0));
    assert_eq!(summary.total_saved_by_unification, Some(0));
}

// --- B3: dedup-able sums --------------------------------------------------

#[test]
fn sums_dedup_able_bytes_across_distinct_groups() {
    // Two separate dedup-able groups:
    //   - llama3 (size 1000): ollama + hf, different inodes → 1000 dedup-able
    //   - mistral (size 2000): hf + lm-studio, different inodes → 2000 dedup-able
    // Plus an unrelated unique model (codellama, size 9999) — must NOT
    // contribute. Total dedup-able bytes = 1000 + 2000 = 3000.
    let inventory = Inventory {
        entries: vec![
            entry("ollama", "llama3", "/o/llama3", 1000, Some(h(1))),
            entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1))),
            entry("hf", "mistral", "/h/mistral", 2000, Some(h(2))),
            entry("lm-studio", "mistral-7b", "/m/mistral", 2000, Some(h(2))),
            entry("ollama", "codellama:13b", "/o/cl", 9999, Some(h(3))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    // llama3 group: distinct inodes
    inodes.insert(key("ollama", "llama3"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 101));
    // mistral group: distinct inodes
    inodes.insert(key("hf", "mistral"), (1, 200));
    inodes.insert(key("lm-studio", "mistral-7b"), (1, 201));
    // codellama: unique
    inodes.insert(key("ollama", "codellama:13b"), (1, 300));

    let summary = dedup_summary(&inventory, &inodes, true);
    assert_eq!(
        summary.dedup_able_bytes,
        Some(3000),
        "1000 (llama3) + 2000 (mistral)"
    );
    assert_eq!(
        summary.unified_count,
        Some(0),
        "no groups already share an inode"
    );
    assert_eq!(summary.total_saved_by_unification, Some(0));
}

// --- B4: already-unified counts + saves -----------------------------------

#[test]
fn counts_already_unified_groups_and_sums_saved_bytes() {
    // Two already-unified groups:
    //   - llama3 (size 1000): ollama + hf share one inode → saves (2-1)*1000 = 1000
    //   - mistral (size 2000): ollama + hf + lm-studio share one inode → saves (3-1)*2000 = 4000
    // Total unified groups = 2, total_saved_by_unification = 1000 + 4000 = 5000.
    // dedup_able_bytes = 0 (no separate-inode peers among matching hashes).
    let inventory = Inventory {
        entries: vec![
            entry("ollama", "llama3", "/o/llama3", 1000, Some(h(1))),
            entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1))),
            entry("ollama", "mistral", "/o/mistral", 2000, Some(h(2))),
            entry("hf", "mistral.gguf", "/h/mistral", 2000, Some(h(2))),
            entry("lm-studio", "mistral-7b", "/m/mistral", 2000, Some(h(2))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    // llama3: ollama+hf share one inode (1, 100)
    inodes.insert(key("ollama", "llama3"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 100));
    // mistral: all three share one inode (1, 200)
    inodes.insert(key("ollama", "mistral"), (1, 200));
    inodes.insert(key("hf", "mistral.gguf"), (1, 200));
    inodes.insert(key("lm-studio", "mistral-7b"), (1, 200));

    let summary = dedup_summary(&inventory, &inodes, true);
    assert_eq!(summary.dedup_able_bytes, Some(0));
    assert_eq!(summary.unified_count, Some(2), "two unified groups");
    assert_eq!(
        summary.total_saved_by_unification,
        Some(5000),
        "(2-1)*1000 + (3-1)*2000 = 1000 + 4000"
    );
}

#[test]
fn dedup_able_and_already_unified_coexist_in_same_inventory() {
    // Mixed inventory:
    //   - GroupA: 2 separate inodes, same hash → DedupAble. Bytes: 1000.
    //   - GroupB: 2 paths share one inode, same hash → AlreadyUnified.
    //     Saves: (2-1)*4000 = 4000.
    //   - One unique model (codellama).
    let inventory = Inventory {
        entries: vec![
            // GroupA: dedup-able
            entry("ollama", "llama3", "/o/llama3", 1000, Some(h(1))),
            entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1))),
            // GroupB: already unified
            entry("ollama", "wizard", "/o/wizard", 4000, Some(h(2))),
            entry("hf", "wizard.gguf", "/h/wizard", 4000, Some(h(2))),
            // unique
            entry("ollama", "codellama:13b", "/o/cl", 9999, Some(h(3))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    // GroupA: separate inodes
    inodes.insert(key("ollama", "llama3"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 101));
    // GroupB: shared inode
    inodes.insert(key("ollama", "wizard"), (1, 200));
    inodes.insert(key("hf", "wizard.gguf"), (1, 200));
    // unique
    inodes.insert(key("ollama", "codellama:13b"), (1, 300));

    let summary = dedup_summary(&inventory, &inodes, true);
    assert_eq!(summary.dedup_able_bytes, Some(1000), "from GroupA only");
    assert_eq!(summary.unified_count, Some(1), "only GroupB is unified");
    assert_eq!(
        summary.total_saved_by_unification,
        Some(4000),
        "(2-1)*4000 from GroupB"
    );
}

// --- Mutation-coverage hardening ------------------------------------------
// These tests pin specific branch behaviors that earlier mutation-testing
// runs flagged as missed. Each test is constructed so the branch's exact
// boolean structure (`&&` vs `||`, `==` vs `!=`) matters: flipping any of
// them would change the assertion outcome.

#[test]
fn unified_count_requires_at_least_two_entries_with_inode_data() {
    // Two cross-tool entries share a content hash (so they're a candidate
    // hash-group), but ONLY ONE has inode metadata recorded. The remaining
    // entry's inode is unknown.
    //
    // The current `&& entries_with_inode >= 2` guard requires inode data
    // from ≥2 entries before claiming a group is "already unified" — without
    // that evidence the classifier is conservatively silent. Mutating the
    // `&&` to `||` would let `inodes_seen.len() == 1` alone qualify the
    // group as unified, which would falsely increment the count below.
    let inventory = Inventory {
        entries: vec![
            entry("ollama", "llama3", "/o/llama3", 1000, Some(h(7))),
            entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(7))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    // Only one of the two entries has inode info recorded.
    inodes.insert(key("ollama", "llama3"), (1, 500));

    let summary = dedup_summary(&inventory, &inodes, true);
    assert_eq!(
        summary.unified_count,
        Some(0),
        "single inode-bearing entry must not register as unified group; \
         `entries_with_inode >= 2` is required"
    );
    assert_eq!(
        summary.total_saved_by_unification,
        Some(0),
        "no saves can be attributed when the inode evidence is incomplete"
    );
}
