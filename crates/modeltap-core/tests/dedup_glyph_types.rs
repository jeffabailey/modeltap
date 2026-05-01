//! Type-shape tests for the new domain types added in step 01-01.
//!
//! These tests exist purely to lock the *shape* of the types (variants,
//! fields, derives) before any behavioral consumer is wired in. The first
//! behavioral test that exercises them lives in step 01-04 (summary bar
//! wiring); this file fills the type-only gap so step 01-01 has a
//! falsifiable check beyond `cargo build`.
//!
//! Source of truth: `docs/feature/cross-tool-model-unify/design/data-models.md`.

use std::collections::HashSet;

use modeltap_core::domain::dedup_glyph::DedupGlyph;
use modeltap_core::domain::dedup_summary::{DedupSummary, UnifiedRow};
use modeltap_core::domain::synthetic_slot::{LeftPaneSlot, SyntheticSlot};
use modeltap_core::{DisplayLabel, ToolId};

#[test]
fn dedup_glyph_has_six_documented_variants_and_supports_value_equality() {
    // The contract: six variants, each constructible, all distinct under PartialEq,
    // and all hashable so they can be used as map keys / set members.
    let variants = [
        DedupGlyph::Pending,
        DedupGlyph::Hashing,
        DedupGlyph::Unique,
        DedupGlyph::Failed,
        DedupGlyph::DedupAble,
        DedupGlyph::AlreadyUnified,
    ];

    // Hash + Eq: a HashSet of all variants must contain six distinct entries.
    let unique: HashSet<DedupGlyph> = variants.iter().copied().collect();
    assert_eq!(unique.len(), 6, "expected six distinct DedupGlyph variants");

    // Clone + PartialEq round-trip on a representative variant.
    let original = DedupGlyph::Pending;
    let cloned = original;
    assert_eq!(original, cloned);
}

#[test]
fn synthetic_slot_all_unified_constructs_with_optional_counts() {
    // Both fields are Option<u64> so the renderer can show "(?)" while hashing
    // and a real number once classification completes (data-models.md §dedup_glyph).
    let computing = SyntheticSlot::AllUnified {
        count: None,
        total_saved_bytes: None,
    };
    let resolved = SyntheticSlot::AllUnified {
        count: Some(3),
        total_saved_bytes: Some(1024 * 1024 * 1024),
    };

    assert_ne!(computing, resolved);
    assert_eq!(resolved.clone(), resolved);
}

#[test]
fn left_pane_slot_distinguishes_real_and_synthetic() {
    // `LeftPaneSlot` is generic over the real-tool view type so `modeltap-core`
    // does not depend on `modeltap-tui`. Step 01-04+ will instantiate it with
    // `ToolView`; step 01-01 checks the variant shape with a stand-in payload.
    let real: LeftPaneSlot<&'static str> = LeftPaneSlot::Real("ollama");
    let synthetic: LeftPaneSlot<&'static str> =
        LeftPaneSlot::Synthetic(SyntheticSlot::AllUnified {
            count: Some(3),
            total_saved_bytes: Some(0),
        });

    match real {
        LeftPaneSlot::Real(payload) => assert_eq!(payload, "ollama"),
        LeftPaneSlot::Synthetic(_) => panic!("expected Real variant"),
    }
    match synthetic {
        LeftPaneSlot::Real(_) => panic!("expected Synthetic variant"),
        LeftPaneSlot::Synthetic(SyntheticSlot::AllUnified { count, .. }) => {
            assert_eq!(count, Some(3));
        }
    }
}

#[test]
fn dedup_summary_default_is_all_none_signaling_computing_state() {
    // Per data-models.md §dedup_summary: `None` means "computing..."; a value
    // appears once hashing has produced any classification. Default must
    // therefore map to the pre-hash "nothing known" state.
    let summary = DedupSummary::default();

    assert_eq!(summary.dedup_able_bytes, None);
    assert_eq!(summary.unified_count, None);
    assert_eq!(summary.total_saved_by_unification, None);
}

#[test]
fn dedup_summary_carries_the_three_documented_fields() {
    // Construction with all three fields populated proves the field shape
    // matches the data-models.md contract.
    let summary = DedupSummary {
        dedup_able_bytes: Some(2_500_000_000),
        unified_count: Some(7),
        total_saved_by_unification: Some(1_750_000_000),
    };

    assert_eq!(summary.dedup_able_bytes, Some(2_500_000_000));
    assert_eq!(summary.unified_count, Some(7));
    assert_eq!(summary.total_saved_by_unification, Some(1_750_000_000));
    assert_eq!(summary.clone(), summary);
}

#[test]
fn unified_row_carries_label_size_and_tools_sharing() {
    // Per data-models.md §UnifiedRow: model_id (in-tool string, mirrors
    // existing ToolView::model_ids pattern), display_label, size_bytes,
    // tools_sharing, and saves_bytes = (tools_sharing.len() - 1) * size_bytes.
    let row = UnifiedRow {
        model_id_in_tool: "mistral:7b-instruct-q4_K_M".to_string(),
        display_label: DisplayLabel::from("mistral-7b-instruct"),
        size_bytes: 4_000_000_000,
        tools_sharing: vec![ToolId("ollama"), ToolId("llama-cli"), ToolId("hf")],
        saves_bytes: 8_000_000_000, // (3 - 1) * 4 GB
    };

    assert_eq!(row.tools_sharing.len(), 3);
    assert_eq!(
        row.saves_bytes,
        (row.tools_sharing.len() as u64 - 1) * row.size_bytes
    );
    assert_eq!(row.clone(), row);
}

#[test]
fn types_are_serializable_via_serde() {
    // The `serde::Serialize` derive is part of the contract (data-models.md).
    // A round-trip check would require Deserialize; for type-only step we
    // just confirm Serialize is wired by calling `serde_json::to_string`
    // through the dynamic dispatch path.
    fn assert_serialize<T: serde::Serialize>(_: &T) {}

    assert_serialize(&DedupGlyph::Pending);
    assert_serialize(&SyntheticSlot::AllUnified {
        count: Some(1),
        total_saved_bytes: Some(0),
    });
    assert_serialize(&DedupSummary::default());
    assert_serialize(&UnifiedRow {
        model_id_in_tool: "x".to_string(),
        display_label: DisplayLabel::from("x"),
        size_bytes: 0,
        tools_sharing: vec![],
        saves_bytes: 0,
    });
}
