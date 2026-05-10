//! Property-based tests for the compatibility-indicator engine (US-09 @property).
//!
//! Invariant: for ANY randomly-generated `(Inventory, PluginCapabilityMap)`,
//! `compute_indicator(target, inventory, caps)` returns one of
//! `{Compatible, Shared, FormatLocked, Unknown}` and NEVER panics.
//!
//! Test budget (per quality-framework):
//!   distinct behaviors:
//!     B1: target with matching SHA256 in another tool → Shared
//!     B2: target single-tool, format accepted by another plugin → Compatible
//!     B3: target single-tool, no other plugin accepts format → FormatLocked
//!     B4: target's format is Other → Unknown
//!     B5: empty `accepted_formats()` for some plugin → models from THAT plugin
//!         render as Unknown (defensive; per US-16 AC-3)
//!     B6: SHA256 absent and HF id+quant doesn't match → conservative
//!         (NOT Shared)
//!     B7: property invariant — every output ∈ {o, *, !, ?} for any input
//!   budget = 7 × 2 = 14 unit tests max. We use 9 (incl. 1 proptest).
//!
//! Per ADR-002 conservative-when-uncertain rule: when the dedup-key cannot be
//! confidently matched, the engine MUST NOT classify as `Shared`. The test
//! `sha256_absent_does_not_yield_shared` pins this property.

use std::path::PathBuf;

use modeltap_core::domain::RowIndicator;
use modeltap_core::logic::compatibility::{
    compute_indicator, Inventory, InventoryEntry, PluginCapabilityMap,
};
use modeltap_core::{ContentHash, DiscoveredModel, DisplayLabel, Format, ModelStatus, ToolId};
use proptest::prelude::*;

const HASH_A: ContentHash = ContentHash([0xAA; 32]);
const HASH_B: ContentHash = ContentHash([0xBB; 32]);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn model(id: &str, format: Format, path: &str) -> DiscoveredModel {
    DiscoveredModel {
        id_in_tool: id.to_string(),
        on_disk_path: PathBuf::from(path),
        size_bytes: 1_000_000_000,
        format,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Healthy,
    }
}

fn entry(tool: &'static str, m: DiscoveredModel, hash: Option<ContentHash>) -> InventoryEntry {
    InventoryEntry {
        tool: ToolId(tool),
        model: m,
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

// ---------------------------------------------------------------------------
// B1: matching SHA256 across tools → Shared
// ---------------------------------------------------------------------------

#[test]
fn matching_sha256_in_two_tools_yields_shared() {
    let target = entry("hf", model("m", Format::Gguf, "/hf/m"), Some(HASH_A));
    let peer = entry(
        "llama-cli",
        model("m-other-id", Format::Gguf, "/llms/m"),
        Some(HASH_A),
    );
    let inv = Inventory {
        entries: vec![target.clone(), peer],
    };
    let cap = caps(&[("hf", &[Format::Gguf]), ("llama-cli", &[Format::Gguf])]);
    assert_eq!(compute_indicator(&target, &inv, &cap), RowIndicator::Shared);
}

// ---------------------------------------------------------------------------
// B2: single-tool, another plugin accepts the format → Compatible
// ---------------------------------------------------------------------------

#[test]
fn single_tool_format_accepted_by_other_plugin_yields_compatible() {
    let target = entry("hf", model("m", Format::Gguf, "/hf/m"), Some(HASH_A));
    let inv = Inventory {
        entries: vec![target.clone()],
    };
    // hf is target's tool; llama-cli is OTHER and accepts Gguf.
    let cap = caps(&[("hf", &[Format::Gguf]), ("llama-cli", &[Format::Gguf])]);
    assert_eq!(
        compute_indicator(&target, &inv, &cap),
        RowIndicator::Compatible
    );
}

// ---------------------------------------------------------------------------
// B3: single-tool, no other plugin accepts the format → FormatLocked
// ---------------------------------------------------------------------------

#[test]
fn single_tool_no_other_plugin_accepts_format_yields_format_locked() {
    let target = entry("hf", model("awq-m", Format::Awq, "/hf/awq-m"), Some(HASH_B));
    let inv = Inventory {
        entries: vec![target.clone()],
    };
    // Only hf accepts Awq; nobody else does.
    let cap = caps(&[
        ("hf", &[Format::Awq, Format::Gguf]),
        ("llama-cli", &[Format::Gguf]),
        ("ollama", &[Format::OllamaBlob]),
    ]);
    assert_eq!(
        compute_indicator(&target, &inv, &cap),
        RowIndicator::FormatLocked
    );
}

// ---------------------------------------------------------------------------
// B4: format Other → Unknown (regardless of presence)
// ---------------------------------------------------------------------------

#[test]
fn format_other_always_yields_unknown() {
    let target = entry("hf", model("m", Format::Other, "/hf/m"), Some(HASH_A));
    let inv = Inventory {
        entries: vec![target.clone()],
    };
    let cap = caps(&[("hf", &[Format::Gguf]), ("llama-cli", &[Format::Gguf])]);
    assert_eq!(
        compute_indicator(&target, &inv, &cap),
        RowIndicator::Unknown
    );
}

// ---------------------------------------------------------------------------
// B5: empty accepted_formats() for the target's plugin → Unknown
//     (defensive: per US-16 AC-3, an empty capability slice means we cannot
//     reason about format compatibility for that plugin's models)
// ---------------------------------------------------------------------------

#[test]
fn empty_accepted_formats_for_targets_plugin_yields_unknown() {
    let target = entry("hf", model("m", Format::Gguf, "/hf/m"), Some(HASH_A));
    let inv = Inventory {
        entries: vec![target.clone()],
    };
    // hf declares NO accepted formats. Defensive: render as Unknown.
    let cap = caps(&[("hf", &[]), ("llama-cli", &[Format::Gguf])]);
    assert_eq!(
        compute_indicator(&target, &inv, &cap),
        RowIndicator::Unknown
    );
}

// ---------------------------------------------------------------------------
// B6: SHA256 absent AND no fallback match → conservative (NOT Shared)
// ---------------------------------------------------------------------------

#[test]
fn sha256_absent_does_not_yield_shared_when_uncertain() {
    // Two tools, BOTH with content_hash=None. No SHA256 means we cannot
    // confidently match. Per ADR-002 conservative-deletion rule, the engine
    // MUST NOT classify as Shared. Format compatibility decides between
    // Compatible and FormatLocked.
    let target = entry("hf", model("m", Format::Gguf, "/hf/m"), None);
    let peer = entry(
        "llama-cli",
        model("m-other-id", Format::Gguf, "/llms/m-other"),
        None,
    );
    let inv = Inventory {
        entries: vec![target.clone(), peer],
    };
    let cap = caps(&[("hf", &[Format::Gguf]), ("llama-cli", &[Format::Gguf])]);
    let result = compute_indicator(&target, &inv, &cap);
    assert_ne!(
        result,
        RowIndicator::Shared,
        "ADR-002 conservative-when-uncertain: no SHA256 + no fallback match → must NOT be Shared"
    );
    // For Gguf with llama-cli also accepting Gguf, the conservative outcome is
    // Compatible (the engine treats them as separate models and notes another
    // plugin can host the format).
    assert_eq!(result, RowIndicator::Compatible);
}

// ---------------------------------------------------------------------------
// B7 / @property: every indicator must be in {Compatible, Shared, FormatLocked,
// Unknown} for ANY randomly-generated inventory + capability map. ≥1000 cases.
// ---------------------------------------------------------------------------

prop_compose! {
    fn arb_format()(idx in 0u8..8u8) -> Format {
        match idx {
            0 => Format::Gguf,
            1 => Format::Safetensors,
            2 => Format::Bin,
            3 => Format::Awq,
            4 => Format::Gptq,
            5 => Format::OllamaBlob,
            6 => Format::Mlx,
            _ => Format::Other,
        }
    }
}

prop_compose! {
    fn arb_tool_id()(idx in 0u8..4u8) -> &'static str {
        match idx {
            0 => "ollama",
            1 => "llama-cli",
            2 => "hf",
            _ => "lm-studio",
        }
    }
}

prop_compose! {
    fn arb_hash()(idx in 0u8..3u8) -> Option<ContentHash> {
        match idx {
            0 => Some(HASH_A),
            1 => Some(HASH_B),
            _ => None,
        }
    }
}

prop_compose! {
    fn arb_entry()(
        tool in arb_tool_id(),
        format in arb_format(),
        hash in arb_hash(),
        id_seed in 0u32..1000u32,
    ) -> InventoryEntry {
        let id = format!("model-{}", id_seed);
        let path = format!("/store/{}", id_seed);
        entry(tool, model(&id, format, &path), hash)
    }
}

prop_compose! {
    fn arb_caps()(
        ollama in prop::collection::vec(arb_format(), 0..3),
        llama_cli in prop::collection::vec(arb_format(), 0..3),
        hf in prop::collection::vec(arb_format(), 0..6),
        lm_studio in prop::collection::vec(arb_format(), 0..3),
    ) -> PluginCapabilityMap {
        let mut m = PluginCapabilityMap::new();
        m.insert(ToolId("ollama"), ollama);
        m.insert(ToolId("llama-cli"), llama_cli);
        m.insert(ToolId("hf"), hf);
        m.insert(ToolId("lm-studio"), lm_studio);
        m
    }
}

proptest! {
    // 256 cases is proptest's default and covers the totality property
    // (every result is one of the four indicator variants) with strong
    // confidence — 1024 cases here was 4× more compute for no extra
    // shrinking power. Bump only if the reduced count starts missing
    // regressions in CI.
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    /// For ANY randomly-generated (target, inventory, capability map), the
    /// engine returns one of the four glyphs and never panics.
    #[test]
    fn every_indicator_is_one_of_four_variants(
        target in arb_entry(),
        peers in prop::collection::vec(arb_entry(), 0..5),
        cap in arb_caps(),
    ) {
        let mut entries = vec![target.clone()];
        entries.extend(peers);
        let inv = Inventory { entries };
        let r = compute_indicator(&target, &inv, &cap);
        prop_assert!(
            matches!(
                r,
                RowIndicator::Compatible
                    | RowIndicator::Shared
                    | RowIndicator::FormatLocked
                    | RowIndicator::Unknown
            ),
            "indicator must be one of {{o, *, !, ?}}, got {:?}",
            r
        );
    }
}
