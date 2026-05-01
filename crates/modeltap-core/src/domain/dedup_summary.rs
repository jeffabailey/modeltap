//! `DedupSummary` and `UnifiedRow` — aggregates carried on `AppState` for
//! the summary bar and the `[All Unified]` right-pane view.
//!
//! Pure data. Computation lives in `logic::dedup` (step 01-02 / 01-04).
//! Carrying the result on state explicitly keeps render functions pure.
//!
//! `None` semantics: a `None` value means "not yet known — display
//! `computing...`". A real number replaces it once hashing has produced any
//! classification. See `data-models.md` §dedup_summary.
//!
//! Source of truth: `docs/feature/cross-tool-model-unify/design/data-models.md`.

use serde::Serialize;

use crate::types::{DisplayLabel, ToolId};

/// Top-level dedup aggregates carried on `AppState`. Recomputed once per
/// `AppState` transition by `logic::dedup::dedup_summary` and cached here so
/// render functions can stay pure.
#[derive(Debug, Clone, Eq, PartialEq, Default, Serialize)]
pub struct DedupSummary {
    /// `Some(bytes)` once hashing has produced any classification. `None`
    /// while `computing...` should be displayed.
    pub dedup_able_bytes: Option<u64>,
    /// Same nullability semantics. The `Unified: N models` count.
    pub unified_count: Option<u64>,
    /// For the `[All Unified]` footer: sum of `(N-1) * size` over `#` rows.
    pub total_saved_by_unification: Option<u64>,
}

/// One row in the `[All Unified]` right-pane view. Carries enough data to
/// render name + size + tool count + saves without further lookups.
///
/// The `model_id_in_tool` field mirrors the `ToolView::model_ids` pattern
/// already used elsewhere in the codebase (a `String` keyed within the
/// owning tool); a dedicated `ModelId` newtype may emerge in a later step
/// when cross-tool identity becomes load-bearing.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct UnifiedRow {
    /// Stable id within the owning tool (e.g. `mistral:7b-instruct-q4_K_M`).
    pub model_id_in_tool: String,
    /// Display label shown to the user.
    pub display_label: DisplayLabel,
    /// Apparent size of the model in bytes.
    pub size_bytes: u64,
    /// Tools sharing the same inode for this model. Always ≥ 2 for a
    /// genuinely-unified row (a single tool listing means it's not
    /// cross-tool unified).
    pub tools_sharing: Vec<ToolId>,
    /// Bytes saved by unification: `(tools_sharing.len() - 1) * size_bytes`.
    /// Carried explicitly rather than computed on demand so the value is
    /// visible to serializers (`#[derive(Serialize)]`) and stable across
    /// snapshots.
    pub saves_bytes: u64,
}
