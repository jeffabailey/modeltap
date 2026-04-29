//! modeltap-tui — Elm-style TUI for modeltap (per ADR-006).
//!
//! Step 01-01 surface:
//! - `AppState` and `Msg` (the smallest viable view-model + message type for
//!   the walking-skeleton pane scaffold).
//! - `update()` — pure Elm-style transition.
//! - `view()` — pure render against a ratatui `Frame`.
//! - `panic_hook` — terminal-restoring panic hook (US-01 AC-5).
//! - `check_terminal_width` — refuses to start when terminal is too narrow
//!   (US-01 AC-4).

#![forbid(unsafe_code)]

pub mod event_loop;
pub mod layout;
pub mod panic_hook;

pub use event_loop::{update, AppState, Msg, UpdateEffect};
pub use layout::{check_terminal_width, view, TerminalSizeError};
pub use panic_hook::install_panic_hook;
