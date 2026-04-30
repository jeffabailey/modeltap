//! Acceptance tests for US-09 (Compatibility indicator engine).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-09 @release-1 scenarios. The 4 scenarios are exercised here through
//! the pure-domain driving port `modeltap_core::logic::compatibility::
//! compute_indicator()`. Per nw-tdd-methodology, a pure domain function IS
//! its own driving port — calling it directly IS port-to-port testing.
//!
//! The real TUI binary's render path is exercised in the existing US-04 row
//! metadata acceptance tests; those continue to assert that the production
//! pipeline produces the same indicators end-to-end. This file's job is to
//! pin the engine's *behavior* against the master-acceptance scenarios.
//!
//! Tags: @us-09 @release-1.

use std::path::PathBuf;

use modeltap_core::domain::RowIndicator;
use modeltap_core::logic::compatibility::{
    compute_indicator, Inventory, InventoryEntry, PluginCapabilityMap,
};
use modeltap_core::{ContentHash, DiscoveredModel, DisplayLabel, Format, ModelStatus, ToolId};

const HASH_A: ContentHash = ContentHash([0xAA; 32]);
const HASH_B: ContentHash = ContentHash([0xBB; 32]);

fn model(id: &str, format: Format, path: &str, size: u64) -> DiscoveredModel {
    DiscoveredModel {
        id_in_tool: id.to_string(),
        on_disk_path: PathBuf::from(path),
        size_bytes: size,
        format,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Healthy,
    }
}

fn entry(tool: &'static str, model: DiscoveredModel, hash: Option<ContentHash>) -> InventoryEntry {
    InventoryEntry {
        tool: ToolId(tool),
        model,
        content_hash: hash,
    }
}

fn caps(pairs: &[(&'static str, &[Format])]) -> PluginCapabilityMap {
    let mut m = PluginCapabilityMap::new();
    for (name, fmts) in pairs {
        m.insert(ToolId(name), fmts.to_vec());
    }
    m
}

// -----------------------------------------------------------------------------
// Scenario 1 (US-09): Multi-tool model gets *
// -----------------------------------------------------------------------------

#[test]
fn multi_tool_model_with_matching_sha256_gets_shared_indicator() {
    let target = entry(
        "hf",
        model("llama3:8b", Format::Gguf, "/hf/llama3.gguf", 4_400_000_000),
        Some(HASH_A),
    );
    let other = entry(
        "Loose GGUFs",
        model(
            "llama3-8b.gguf",
            Format::Gguf,
            "/llms/llama3-8b.gguf",
            4_400_000_000,
        ),
        Some(HASH_A), // SAME content hash → matched dedup-key
    );
    let inventory = Inventory {
        entries: vec![target.clone(), other],
    };
    let plugin_caps = caps(&[("hf", &[Format::Gguf]), ("Loose GGUFs", &[Format::Gguf])]);

    let result = compute_indicator(&target, &inventory, &plugin_caps);

    assert_eq!(
        result,
        RowIndicator::Shared,
        "AC: model registered with 2+ tools matched by SHA256 must yield Shared (*)"
    );
}

// -----------------------------------------------------------------------------
// Scenario 2 (US-09): Format-compatible single-tool model gets o
// -----------------------------------------------------------------------------

#[test]
fn single_tool_gguf_with_other_plugin_accepting_gguf_gets_compatible_indicator() {
    let target = entry(
        "hf",
        model("llama3:8b", Format::Gguf, "/hf/llama3.gguf", 4_400_000_000),
        Some(HASH_A),
    );
    let inventory = Inventory {
        entries: vec![target.clone()],
    };
    let plugin_caps = caps(&[
        ("hf", &[Format::Gguf, Format::Safetensors]),
        ("Loose GGUFs", &[Format::Gguf]), // accepts Gguf -> compatible
        ("ollama", &[Format::OllamaBlob]),
        ("lm-studio", &[Format::Gguf]),
    ]);

    let result = compute_indicator(&target, &inventory, &plugin_caps);

    assert_eq!(
        result,
        RowIndicator::Compatible,
        "AC: single-tool GGUF whose format is accepted by another plugin → Compatible (o)"
    );
}

// -----------------------------------------------------------------------------
// Scenario 3 (US-09): Format-locked model gets !
// -----------------------------------------------------------------------------

#[test]
fn single_tool_awq_with_no_other_plugin_accepting_awq_gets_format_locked_indicator() {
    let target = entry(
        "hf",
        model(
            "TheBloke/foo-AWQ",
            Format::Awq,
            "/hf/foo-awq",
            7_000_000_000,
        ),
        Some(HASH_B),
    );
    let inventory = Inventory {
        entries: vec![target.clone()],
    };
    // Only HF accepts AWQ; nobody else does. The model is format-locked.
    let plugin_caps = caps(&[
        ("hf", &[Format::Awq, Format::Gguf, Format::Safetensors]),
        ("Loose GGUFs", &[Format::Gguf]),
        ("ollama", &[Format::OllamaBlob]),
        ("lm-studio", &[Format::Gguf]),
    ]);

    let result = compute_indicator(&target, &inventory, &plugin_caps);

    assert_eq!(
        result,
        RowIndicator::FormatLocked,
        "AC: single-tool AWQ with no other plugin accepting AWQ → FormatLocked (!)"
    );
}

// -----------------------------------------------------------------------------
// Scenario 4 (US-09 @property): every indicator is in {o, *, !, ?}.
//
// The full proptest invariant is in
// `crates/modeltap-core/tests/compatibility_property.rs` (≥1000 iterations).
// This acceptance smoke variant exercises a hand-rolled cross-product so the
// invariant has a deterministic acceptance witness here too.
// -----------------------------------------------------------------------------

#[test]
fn every_computed_indicator_is_one_of_o_star_bang_question() {
    let formats = [
        Format::Gguf,
        Format::Safetensors,
        Format::Bin,
        Format::Awq,
        Format::Gptq,
        Format::OllamaBlob,
        Format::Mlx,
        Format::Other,
    ];
    let plugin_caps = caps(&[
        (
            "hf",
            &[
                Format::Gguf,
                Format::Safetensors,
                Format::Bin,
                Format::Awq,
                Format::Gptq,
            ],
        ),
        ("Loose GGUFs", &[Format::Gguf]),
        ("ollama", &[Format::OllamaBlob]),
        ("lm-studio", &[Format::Gguf]),
    ]);

    for fmt in formats {
        for hash_state in [Some(HASH_A), Some(HASH_B), None] {
            let target = entry(
                "hf",
                model("model-x", fmt, "/hf/model-x", 1_000_000_000),
                hash_state,
            );
            // Three inventory variants: (a) target alone, (b) target + non-matching
            // peer, (c) target + matching-hash peer.
            let inv_alone = Inventory {
                entries: vec![target.clone()],
            };
            let peer_diff = entry(
                "Loose GGUFs",
                model("peer", Format::Gguf, "/llms/peer", 1_000_000_000),
                Some(HASH_B),
            );
            let inv_with_peer_no_match = Inventory {
                entries: vec![target.clone(), peer_diff.clone()],
            };
            let peer_match = entry(
                "Loose GGUFs",
                model(
                    "peer-shared",
                    Format::Gguf,
                    "/llms/peer-shared",
                    1_000_000_000,
                ),
                hash_state,
            );
            let inv_with_peer_match = Inventory {
                entries: vec![target.clone(), peer_match],
            };

            for inv in [&inv_alone, &inv_with_peer_no_match, &inv_with_peer_match] {
                let r = compute_indicator(&target, inv, &plugin_caps);
                assert!(
                    matches!(
                        r,
                        RowIndicator::Compatible
                            | RowIndicator::Shared
                            | RowIndicator::FormatLocked
                            | RowIndicator::Unknown
                    ),
                    "indicator must be one of {{o, *, !, ?}}, got {:?} for fmt={:?} hash={:?}",
                    r,
                    fmt,
                    hash_state.map(|_| "Some")
                );
            }
        }
    }
}
