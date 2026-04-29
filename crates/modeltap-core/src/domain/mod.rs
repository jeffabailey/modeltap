//! Domain types — pure data, no I/O. Per ADR-006 the TUI's `last_action`
//! field is structured (not a pre-formatted string) so the render layer can
//! lay out the header + body lines independently.

pub mod indicator;
pub mod last_action;

pub use indicator::{
    classify_by_presence, classify_row, other_tools_by_presence, other_tools_for_model,
    RowIndicator, ToolPresence,
};
pub use last_action::{ActionStatus, ActionVerb, LastAction, TargetError};
