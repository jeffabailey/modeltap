//! `SyntheticSlot` and `LeftPaneSlot` — non-Tool entries that may appear in
//! the left pane.
//!
//! Per ADR-014, synthetic slots never round-trip through `Box<dyn Tool>`;
//! they are render-only and live in core as pure data. The left pane is a
//! heterogeneous list: real tools (whose view-projection is owned by
//! `modeltap-tui::ToolView`) and synthetic entries.
//!
//! `LeftPaneSlot` is generic over the real-tool view type (`T`) so that
//! `modeltap-core` does not depend on `modeltap-tui`. The TUI instantiates
//! it as `LeftPaneSlot<ToolView>`.
//!
//! Source of truth: `docs/feature/cross-tool-model-unify/design/data-models.md`.

use serde::Serialize;

/// A non-Tool entry that may appear in the left pane. Render-only.
///
/// Today only `AllUnified` exists; the variant shape allows additional
/// synthetic entries in the future without churning every consumer.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum SyntheticSlot {
    /// The `[All Unified]` slot at the bottom of the left pane.
    AllUnified {
        /// Number of `#`-glyph rows. Sourced from the dedup classifier.
        /// `None` while hashing is in progress (renders as `(?)` in the badge).
        count: Option<u64>,
        /// Sum of `(N-1) * size` over all unified models. `None` while hashing.
        total_saved_bytes: Option<u64>,
    },
}

/// One slot in the left pane. Either a real tool view (instantiated by the
/// TUI as `LeftPaneSlot<ToolView>`) or a synthetic render-only entry. Both
/// are navigable via j/k.
///
/// The generic parameter keeps `modeltap-core` free of any TUI dependency:
/// the real-tool view type lives in `modeltap-tui` but the variant shape
/// lives here so update logic and render code share one definition.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LeftPaneSlot<T> {
    /// A real, registered tool. `T` is the view-projection (`ToolView` in TUI).
    Real(T),
    /// A render-only entry such as `[All Unified]`.
    Synthetic(SyntheticSlot),
}
