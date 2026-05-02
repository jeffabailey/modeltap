//! Unit tests for `modeltap_core::logic::dedup::collect_unified_rows` (step 04-01).
//!
//! Per `data-models.md` §UnifiedRow and architecture-design.md the function
//! assembles the right-pane `[All Unified]` view: one row per cross-tool
//! group whose entries share one `(device, inode)` AND content hash. The row
//! carries the data needed to render name + size + tool count + saves.
//!
//! `saves_bytes = (tools_sharing.len() - 1) * size_bytes` per ADR-002.
//!
//! Test budget (per `quality-framework`):
//!   distinct behaviors:
//!     B1: empty inventory → empty result
//!     B2: no shared-inode groups (unique / dedup-able) → empty
//!     B3: shared-inode group across N tools → 1 row with correct saves_bytes
//!         (enumerated for N=2, 3, 4 to lock the formula)
//!     B4: mixed inventory → only unified groups appear; deterministic order
//!   budget = 4 × 2 = 8 tests max. We use 6.

use std::collections::HashMap;
use std::path::PathBuf;

use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::logic::dedup::{collect_unified_rows, InodeMap, ModelKey};
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

// --- B1: empty inventory --------------------------------------------------

#[test]
fn empty_inventory_yields_empty_rows() {
    let inventory = Inventory { entries: vec![] };
    let inodes: InodeMap = HashMap::new();

    let rows = collect_unified_rows(&inventory, &inodes);
    assert!(
        rows.is_empty(),
        "no entries → no unified rows; got {rows:?}"
    );
}

// --- B2: no shared-inode groups ------------------------------------------

#[test]
fn single_tool_yields_no_rows() {
    // One tool registers one model. No cross-tool sharing possible → empty.
    let inventory = Inventory {
        entries: vec![entry("ollama", "llama3", "/o/llama3", 1000, Some(h(1)))],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3"), (1, 100));

    let rows = collect_unified_rows(&inventory, &inodes);
    assert!(
        rows.is_empty(),
        "single-tool inventory cannot be cross-tool unified; got {rows:?}"
    );
}

#[test]
fn dedup_able_but_separate_inodes_yields_no_rows() {
    // Two tools, same content hash, DIFFERENT inodes — this is dedup-able
    // (could be unified) but NOT yet unified. The All-Unified view shows
    // only already-unified groups, so the result must be empty.
    let inventory = Inventory {
        entries: vec![
            entry("ollama", "llama3", "/o/llama3", 1000, Some(h(1))),
            entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 101)); // different inode

    let rows = collect_unified_rows(&inventory, &inodes);
    assert!(
        rows.is_empty(),
        "dedup-able-but-not-unified must NOT appear in All-Unified; got {rows:?}"
    );
}

// --- B3: N-tool unified group → saves_bytes = (N-1) * size ---------------

#[test]
fn two_tools_sharing_inode_yields_one_row_with_saves_equal_size() {
    // N=2: saves_bytes = (2 - 1) * size = size.
    let inventory = Inventory {
        entries: vec![
            entry("ollama", "llama3", "/o/llama3", 1000, Some(h(1))),
            entry("hf", "llama-3.gguf", "/h/llama3", 1000, Some(h(1))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "llama3"), (1, 100));
    inodes.insert(key("hf", "llama-3.gguf"), (1, 100)); // shared

    let rows = collect_unified_rows(&inventory, &inodes);
    assert_eq!(rows.len(), 1, "exactly one unified row expected");
    let row = &rows[0];
    assert_eq!(row.tools_sharing.len(), 2);
    assert_eq!(row.size_bytes, 1000);
    assert_eq!(
        row.saves_bytes, 1000,
        "(2 - 1) * 1000 = 1000; got {}",
        row.saves_bytes
    );
    // tools_sharing must be sorted deterministically.
    let mut sorted = row.tools_sharing.clone();
    sorted.sort();
    assert_eq!(row.tools_sharing, sorted, "tools_sharing must be sorted");
}

#[test]
fn three_tools_sharing_inode_yields_saves_equal_two_times_size() {
    // N=3: saves_bytes = (3 - 1) * size = 2 * size.
    let inventory = Inventory {
        entries: vec![
            entry("ollama", "mistral", "/o/mistral", 2000, Some(h(2))),
            entry("hf", "mistral.gguf", "/h/mistral", 2000, Some(h(2))),
            entry("lm-studio", "mistral-7b", "/m/mistral", 2000, Some(h(2))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "mistral"), (1, 200));
    inodes.insert(key("hf", "mistral.gguf"), (1, 200));
    inodes.insert(key("lm-studio", "mistral-7b"), (1, 200));

    let rows = collect_unified_rows(&inventory, &inodes);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.tools_sharing.len(), 3);
    assert_eq!(row.size_bytes, 2000);
    assert_eq!(
        row.saves_bytes, 4000,
        "(3 - 1) * 2000 = 4000; got {}",
        row.saves_bytes
    );
}

#[test]
fn four_tools_sharing_inode_yields_saves_equal_three_times_size() {
    // N=4: saves_bytes = (4 - 1) * size = 3 * size.
    let inventory = Inventory {
        entries: vec![
            entry("ollama", "wizard", "/o/wizard", 3000, Some(h(3))),
            entry("hf", "wizard.gguf", "/h/wizard", 3000, Some(h(3))),
            entry("lm-studio", "wizard-7b", "/m/wizard", 3000, Some(h(3))),
            entry("atomic-chat", "wizard-v1", "/a/wizard", 3000, Some(h(3))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "wizard"), (1, 300));
    inodes.insert(key("hf", "wizard.gguf"), (1, 300));
    inodes.insert(key("lm-studio", "wizard-7b"), (1, 300));
    inodes.insert(key("atomic-chat", "wizard-v1"), (1, 300));

    let rows = collect_unified_rows(&inventory, &inodes);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.tools_sharing.len(), 4);
    assert_eq!(row.size_bytes, 3000);
    assert_eq!(
        row.saves_bytes, 9000,
        "(4 - 1) * 3000 = 9000; got {}",
        row.saves_bytes
    );
}

// --- B4: mixed inventory: only unified groups appear, deterministic order

#[test]
fn mixed_inventory_yields_only_unified_groups_in_deterministic_order() {
    // Inventory:
    //   - GroupA "alpha-llm" (size 1000): ollama + hf SHARED inode → unified row
    //   - GroupB "zeta-net"  (size 4000): ollama + hf + lm-studio SHARED inode → unified row
    //   - GroupC "mid-model" (size 2000): ollama + hf SEPARATE inodes (dedup-able) → no row
    //   - Unique "codellama:13b": single tool only → no row
    //
    // Expected: 2 rows, deterministically ordered. By display_label asc:
    //   "alpha-llm" first, then "zeta-net".
    let inventory = Inventory {
        entries: vec![
            // GroupA: unified
            entry("ollama", "alpha-llm", "/o/alpha", 1000, Some(h(1))),
            entry("hf", "alpha-llm.gguf", "/h/alpha", 1000, Some(h(1))),
            // GroupB: unified, three tools
            entry("ollama", "zeta-net", "/o/zeta", 4000, Some(h(2))),
            entry("hf", "zeta-net.gguf", "/h/zeta", 4000, Some(h(2))),
            entry("lm-studio", "zeta-7b", "/m/zeta", 4000, Some(h(2))),
            // GroupC: dedup-able only (separate inodes) — must NOT appear
            entry("ollama", "mid-model", "/o/mid", 2000, Some(h(3))),
            entry("hf", "mid-model.gguf", "/h/mid", 2000, Some(h(3))),
            // Unique: single tool — must NOT appear
            entry("ollama", "codellama:13b", "/o/cl", 9999, Some(h(4))),
        ],
    };
    let mut inodes: InodeMap = HashMap::new();
    // GroupA: shared inode
    inodes.insert(key("ollama", "alpha-llm"), (1, 100));
    inodes.insert(key("hf", "alpha-llm.gguf"), (1, 100));
    // GroupB: shared inode
    inodes.insert(key("ollama", "zeta-net"), (1, 200));
    inodes.insert(key("hf", "zeta-net.gguf"), (1, 200));
    inodes.insert(key("lm-studio", "zeta-7b"), (1, 200));
    // GroupC: separate inodes
    inodes.insert(key("ollama", "mid-model"), (1, 300));
    inodes.insert(key("hf", "mid-model.gguf"), (1, 301));
    // Unique
    inodes.insert(key("ollama", "codellama:13b"), (1, 400));

    let rows = collect_unified_rows(&inventory, &inodes);
    assert_eq!(
        rows.len(),
        2,
        "exactly the 2 unified groups, no dedup-able / unique; got {rows:?}"
    );

    // Deterministic order: by display_label ascending. The representative is
    // chosen deterministically from the cross-tool group (sorted by ToolId then
    // id_in_tool); for these groups `hf` sorts before `ollama` so the labels
    // come from the hf-side entries.
    let labels: Vec<&str> = rows.iter().map(|r| r.display_label.0.as_str()).collect();
    assert_eq!(
        labels,
        vec!["alpha-llm.gguf", "zeta-net.gguf"],
        "rows sorted by display_label asc; representatives chosen deterministically"
    );

    // Spot-check the saves_bytes formula on each unified row.
    let alpha = &rows[0];
    assert_eq!(alpha.tools_sharing.len(), 2);
    assert_eq!(alpha.saves_bytes, 1000); // (2-1) * 1000

    let zeta = &rows[1];
    assert_eq!(zeta.tools_sharing.len(), 3);
    assert_eq!(zeta.saves_bytes, 8000); // (3-1) * 4000

    // Both rows must include the dedup-able GroupC's "mid-model" exclusion
    // and the unique "codellama" exclusion — verify no row matches them.
    assert!(rows.iter().all(|r| r.display_label.0 != "mid-model"
        && !r.display_label.0.starts_with("mid-model")
        && !r.display_label.0.starts_with("codellama")));
}
