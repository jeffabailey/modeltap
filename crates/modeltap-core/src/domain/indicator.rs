//! Row-indicator classifier for the right-pane model list (US-04.AC-4).
//!
//! Pure domain function: given a `DiscoveredModel` and a presence-view of the
//! current inventory across tools, return one of `{Compatible, Shared,
//! FormatLocked, Unknown}` to drive the `<o|*|!|?>` glyph rendered in the
//! row's first column.
//!
//! ## Step 02-01 scope: presence-counting only
//!
//! In this step the classifier inspects ONLY:
//!   - Whether the model's `Format` is parseable (`Other` → `Unknown`).
//!   - How many tools have this model registered (≥ 2 → `Shared`, else
//!     `Compatible`).
//!
//! 02-05 will replace the `Compatible` branch with a format-aware engine that
//! consults each plugin's `accepted_formats()` to detect format-locked-but-
//! single-tool models (the `FormatLocked`/`!` case). Until then, the
//! `FormatLocked` variant is a pure placeholder reachable only via
//! synthetic test inputs.
//!
//! See `docs/feature/modeltap-tui/design/adr/ADR-002-conservative-dedup.md`
//! for the dedup-key strategy this classifier participates in.

use crate::types::{DiscoveredModel, Format, ToolId};

/// Visual classification for one right-pane row. The glyph is rendered as the
/// first character of the row; the Style (color/modifier) is derived from this
/// enum + the current `NO_COLOR` policy in the TUI render layer.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum RowIndicator {
    /// `o` — single-tool registration AND a parseable, plugin-compatible format.
    /// Default for the WS slice (only Ollama installed → every model is single-
    /// tool registered). Color: neutral.
    Compatible,
    /// `*` — same model registered under 2+ tools (presence-counting in 02-01;
    /// content-hash in 02-05). Triggers the `also in: <other tools>`
    /// annotation. Color: neutral.
    Shared,
    /// `!` — model is format-locked into its current tool (no other plugin can
    /// host this format). Reserved for 02-07 once the format-compatibility
    /// engine exists. Color: red.
    FormatLocked,
    /// `?` — model's format is unparseable (`Format::Other`) so the classifier
    /// cannot determine compatibility. Pairs with `[format: ?]` in the format
    /// field. Color: yellow.
    Unknown,
}

/// Compact presence-view of one tool: which model ids it has registered. The
/// classifier needs only the `id_in_tool` strings to count cross-tool
/// registrations; full `DiscoveredModel`s are not required for this step's
/// presence-counting strategy.
///
/// 02-05 will expand this view to carry `(ToolId, Vec<DedupKey>)` so the
/// classifier can group by content hash rather than id-string match.
#[derive(Debug, Clone)]
pub struct ToolPresence {
    pub tool: ToolId,
    pub model_ids: Vec<String>,
}

/// Classify a single row's indicator based on the discovered model + the
/// presence-view of all tools.
///
/// Step 02-01 rule:
///   - `Format::Other` → `Unknown`
///   - else: count tools whose `model_ids` contain `model.id_in_tool`.
///     `≥ 2` → `Shared`, else `Compatible`.
///   - `FormatLocked` is unreachable from this function in 02-01; it lands in
///     02-07 alongside the format-compatibility engine.
pub fn classify_row(model: &DiscoveredModel, inventory: &[ToolPresence]) -> RowIndicator {
    if matches!(model.format, Format::Other) {
        return RowIndicator::Unknown;
    }
    classify_by_presence(&model.id_in_tool, inventory)
}

/// Presence-only half of the classifier — assumes the format is parseable.
/// Used by the render layer in 02-01 when only id-strings are plumbed through
/// the view-model. 02-05 will make the full `DiscoveredModel` available end-
/// to-end so the format-aware `classify_row` can be called from render code.
pub fn classify_by_presence(id_in_tool: &str, inventory: &[ToolPresence]) -> RowIndicator {
    let count = inventory
        .iter()
        .filter(|t| t.model_ids.iter().any(|id| id == id_in_tool))
        .count();
    if count >= 2 {
        RowIndicator::Shared
    } else {
        RowIndicator::Compatible
    }
}

/// Collect the `ToolId`s of tools (other than the model's home tool) that
/// also have this model registered. Drives the `also in: <list>` annotation
/// for `Shared` rows. Empty when the model is single-tool or the home tool is
/// not represented in the inventory.
pub fn other_tools_for_model(
    model: &DiscoveredModel,
    home: ToolId,
    inventory: &[ToolPresence],
) -> Vec<ToolId> {
    other_tools_by_presence(&model.id_in_tool, home, inventory)
}

/// Presence-only variant — same as `other_tools_for_model` but takes an
/// id-string instead of a full `DiscoveredModel`. Used by the render layer
/// in 02-01.
pub fn other_tools_by_presence(
    id_in_tool: &str,
    home: ToolId,
    inventory: &[ToolPresence],
) -> Vec<ToolId> {
    inventory
        .iter()
        .filter(|t| t.tool != home)
        .filter(|t| t.model_ids.iter().any(|id| id == id_in_tool))
        .map(|t| t.tool)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DisplayLabel, ModelStatus};
    use std::path::PathBuf;

    fn model(id: &str, format: Format) -> DiscoveredModel {
        DiscoveredModel {
            id_in_tool: id.to_string(),
            on_disk_path: PathBuf::from("/tmp/x"),
            size_bytes: 1_000_000_000,
            format,
            display_label: DisplayLabel::from(id),
            status: ModelStatus::Healthy,
        }
    }

    fn presence(tool: &'static str, ids: &[&str]) -> ToolPresence {
        ToolPresence {
            tool: ToolId(tool),
            model_ids: ids.iter().map(|s| s.to_string()).collect(),
        }
    }

    // -----------------------------------------------------------------------
    // classify_row
    // -----------------------------------------------------------------------

    #[test]
    fn classify_row_returns_compatible_when_only_one_tool_has_the_model() {
        let m = model("mistral:7b", Format::Gguf);
        let inv = vec![presence("ollama", &["mistral:7b"])];
        assert_eq!(classify_row(&m, &inv), RowIndicator::Compatible);
    }

    #[test]
    fn classify_row_returns_shared_when_two_tools_have_the_same_model() {
        let m = model("mistral:7b", Format::Gguf);
        let inv = vec![
            presence("ollama", &["mistral:7b"]),
            presence("llama-cli", &["mistral:7b"]),
        ];
        assert_eq!(classify_row(&m, &inv), RowIndicator::Shared);
    }

    #[test]
    fn classify_row_returns_shared_when_three_tools_have_the_same_model() {
        let m = model("mistral:7b", Format::Gguf);
        let inv = vec![
            presence("ollama", &["mistral:7b"]),
            presence("llama-cli", &["mistral:7b"]),
            presence("hf", &["mistral:7b"]),
        ];
        assert_eq!(classify_row(&m, &inv), RowIndicator::Shared);
    }

    #[test]
    fn classify_row_returns_unknown_when_format_is_other() {
        let m = model("mystery", Format::Other);
        // Even when present in multiple tools, unparseable format wins.
        let inv = vec![
            presence("ollama", &["mystery"]),
            presence("llama-cli", &["mystery"]),
        ];
        assert_eq!(classify_row(&m, &inv), RowIndicator::Unknown);
    }

    #[test]
    fn classify_row_returns_compatible_for_empty_inventory() {
        // Pathological: model exists but no tool reports it (shouldn't happen
        // in production but the classifier must not panic). Treats as single-
        // tool (count 0 → < 2 → Compatible).
        let m = model("ghost:7b", Format::Gguf);
        let inv: Vec<ToolPresence> = vec![];
        assert_eq!(classify_row(&m, &inv), RowIndicator::Compatible);
    }

    // -----------------------------------------------------------------------
    // other_tools_for_model
    // -----------------------------------------------------------------------

    #[test]
    fn other_tools_for_model_excludes_home_tool() {
        let m = model("mistral:7b", Format::Gguf);
        let inv = vec![
            presence("ollama", &["mistral:7b"]),
            presence("llama-cli", &["mistral:7b"]),
            presence("hf", &["mistral:7b"]),
        ];
        let others = other_tools_for_model(&m, ToolId("ollama"), &inv);
        assert_eq!(others.len(), 2);
        assert!(others.iter().any(|t| t.0 == "llama-cli"));
        assert!(others.iter().any(|t| t.0 == "hf"));
        assert!(!others.iter().any(|t| t.0 == "ollama"));
    }

    #[test]
    fn other_tools_for_model_returns_empty_for_single_tool_registration() {
        let m = model("mistral:7b", Format::Gguf);
        let inv = vec![presence("ollama", &["mistral:7b"])];
        let others = other_tools_for_model(&m, ToolId("ollama"), &inv);
        assert!(others.is_empty());
    }

    // -----------------------------------------------------------------------
    // Property: classify_row is sound w.r.t. presence-count for parseable
    // formats (model in N tools → Shared iff N≥2).
    // -----------------------------------------------------------------------

    #[test]
    fn property_shared_iff_present_in_two_or_more_tools_for_parseable_format() {
        // Hand-rolled fuzzer over a handful of cardinalities. The invariant is
        // small and the search space is tiny; full proptest is overkill.
        let formats_parseable = [
            Format::Gguf,
            Format::Safetensors,
            Format::Bin,
            Format::Awq,
            Format::Gptq,
            Format::OllamaBlob,
            Format::Mlx,
        ];
        let tool_ids = ["ollama", "llama-cli", "hf", "lm-studio"];
        for fmt in formats_parseable {
            for n in 0..=tool_ids.len() {
                let m = model("mistral:7b", fmt);
                let inv: Vec<ToolPresence> = tool_ids
                    .iter()
                    .take(n)
                    .map(|t| presence(t, &["mistral:7b"]))
                    .collect();
                let got = classify_row(&m, &inv);
                let expected = if n >= 2 {
                    RowIndicator::Shared
                } else {
                    RowIndicator::Compatible
                };
                assert_eq!(
                    got, expected,
                    "format={:?} n={} expected={:?} got={:?}",
                    fmt, n, expected, got
                );
            }
        }
    }

    #[test]
    fn property_format_other_always_yields_unknown_regardless_of_presence() {
        let tool_ids = ["ollama", "llama-cli", "hf", "lm-studio"];
        for n in 0..=tool_ids.len() {
            let m = model("anything", Format::Other);
            let inv: Vec<ToolPresence> = tool_ids
                .iter()
                .take(n)
                .map(|t| presence(t, &["anything"]))
                .collect();
            assert_eq!(classify_row(&m, &inv), RowIndicator::Unknown, "n={}", n);
        }
    }
}
