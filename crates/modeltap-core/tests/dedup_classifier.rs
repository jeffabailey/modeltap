//! Unit tests for `modeltap_core::logic::dedup::classify_unique_vs_shared`.
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: single-tool inventory → all models classified as unique.
//!     B2: cross-tool inventory with same on-disk path → that path classified
//!         as shared; bytes accounted for in shared bucket.
//!     B3: empty inventory → zero counts, zero bytes.
//!   budget = 3 × 2 = 6 tests max. We use 4.
//!
//! Per ADR-002 conservative-deletion rule: when the dedup-key is uncertain
//! (which is ALWAYS true in this WS slice since SHA256 is not yet computed),
//! treat as unique. Two `DiscoveredModel` entries are classified as SHARED
//! only when their `on_disk_path` is byte-identical AND they belong to
//! different tools.

use std::path::PathBuf;

use modeltap_core::logic::dedup::{classify_unique_vs_shared, ToolModel};
use modeltap_core::{DisplayLabel, Format, ModelStatus, ToolId};

fn model(tool: &'static str, id: &str, path: &str, size: u64) -> ToolModel {
    ToolModel {
        tool: ToolId(tool),
        id_in_tool: id.to_string(),
        on_disk_path: PathBuf::from(path),
        size_bytes: size,
        format: Format::OllamaBlob,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Healthy,
    }
}

#[test]
fn single_tool_inventory_classifies_all_models_as_unique() {
    let inventory = vec![
        model("ollama", "llama3:8b", "/blobs/aaa", 1000),
        model("ollama", "mistral:7b", "/blobs/bbb", 2000),
        model("ollama", "codellama:13b", "/blobs/ccc", 4000),
    ];
    let report = classify_unique_vs_shared(&inventory, &ToolId("ollama"));
    assert_eq!(report.unique_count, 3, "all 3 models unique to ollama");
    assert_eq!(report.shared_count, 0, "no shared models");
    assert_eq!(report.unique_bytes, 7000, "sum of unique sizes");
    assert_eq!(report.shared_bytes, 0, "no shared bytes");
}

#[test]
fn cross_tool_same_path_classifies_as_shared() {
    // Two tools (ollama + llama-cli) both pointing at the same on-disk file.
    // From ollama's perspective, that one file is SHARED (also-in-other-tool).
    let inventory = vec![
        model("ollama", "mistral:7b", "/shared/mistral", 2000),
        model("ollama", "llama3:8b", "/blobs/aaa", 1000),
        model("llama-cli", "mistral-7b.gguf", "/shared/mistral", 2000),
    ];
    let report = classify_unique_vs_shared(&inventory, &ToolId("ollama"));
    // From ollama's view: llama3 is unique (only ollama has it), mistral is
    // shared (llama-cli has same path).
    assert_eq!(report.unique_count, 1, "llama3 unique to ollama");
    assert_eq!(
        report.shared_count, 1,
        "mistral shared between ollama+llama-cli"
    );
    assert_eq!(report.unique_bytes, 1000, "only llama3 size in unique");
    assert_eq!(report.shared_bytes, 2000, "mistral size in shared");
}

#[test]
fn empty_inventory_returns_zero_counts() {
    let inventory: Vec<ToolModel> = vec![];
    let report = classify_unique_vs_shared(&inventory, &ToolId("ollama"));
    assert_eq!(report.unique_count, 0);
    assert_eq!(report.shared_count, 0);
    assert_eq!(report.unique_bytes, 0);
    assert_eq!(report.shared_bytes, 0);
}

#[test]
fn tool_not_present_in_inventory_returns_zeros() {
    let inventory = vec![model("ollama", "llama3:8b", "/blobs/aaa", 1000)];
    let report = classify_unique_vs_shared(&inventory, &ToolId("hf"));
    assert_eq!(
        report.unique_count, 0,
        "hf has no models in inventory; nothing to classify"
    );
    assert_eq!(report.shared_count, 0);
}
