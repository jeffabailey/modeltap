//! Screen-level state machines for the TUI (US-13+).
//!
//! Each screen owns its pure view-model + render fn. The top-level `view()`
//! in `layout.rs` dispatches on `AppState.current_screen` to the appropriate
//! screen's render fn.

pub mod detail;

pub use detail::{render_detail, DetailScreenState};
