//! Screen-level state machines for the TUI (US-13+).
//!
//! Each screen owns its pure view-model + render fn. The top-level `view()`
//! in `layout.rs` dispatches on `AppState.current_screen` to the appropriate
//! screen's render fn.

pub mod detail;
pub mod help_overlay;
pub mod tool_detail;

pub use detail::{render_detail, DetailScreenState};
pub use tool_detail::{render as render_tool_detail, ToolDetailScreenState};
