//! modeltap-core — pure domain types and logic. No I/O.
//!
//! Step 01-02 surface (per ADR-001 + ADR-009):
//! - `Tool` trait — frozen 6-method plugin port.
//! - `types` module — `DiscoveredModel`, `ModelMeta`, `Format`, `ToolId`,
//!   `ToolStatus`, `DedupKey`, outcome+error enums.
//! - `MAIN_BOTTOM_BAR` and `MIN_TERMINAL_COLUMNS` from step 01-01.
//!
//! Architecture rule R1 (per `docs/feature/modeltap-tui/devops/ci-pipeline.md`
//! §4): this crate MUST NOT depend on any plugin crate. Plugin authors
//! implement `Tool` from this side of the boundary; the app crate composes
//! plugins via `inventory`.

#![forbid(unsafe_code)]

pub mod domain;
pub mod logic;
pub mod plugin_factory;
pub mod tool;
pub mod types;

pub use plugin_factory::PluginFactory;
pub use tool::Tool;
pub use types::{
    ContentHash, DedupKey, DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel,
    DisplayLabel, Format, LinkError, LinkOutcome, LinkResult, ModelMeta, ModelStatus, ToolId,
    ToolStatus,
};

/// The bottom-bar shortcut line shown on first paint (US-01 AC-6, US-08 AC-1).
///
/// This lives in `modeltap-core` (not `modeltap-tui`) so that future detail
/// screens and help overlays can reference the same canonical strings without
/// pulling in ratatui. The exact text is part of the acceptance contract —
/// changing it requires changing the master-acceptance.feature scenarios too.
pub const MAIN_BOTTOM_BAR: &str =
    "[<-/->] tools  [up/down] models  [u] unify  [z] zap tool  [?] help  [q] quit";

/// Minimum supported terminal width (columns). Smaller terminals are refused
/// at startup with exit code 2 and a usage error on stderr (US-01 AC-4).
pub const MIN_TERMINAL_COLUMNS: u16 = 80;
