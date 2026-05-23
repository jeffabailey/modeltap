//! modeltap-tui — Elm-style TUI for modeltap (per ADR-006).
//!
//! Step 01-03 surface (replacing the 01-01 scaffold):
//! - `app_state::AppState` — full view-model with selection, scroll, focus.
//! - `msg::Msg` — driving-port message type (keypress + future async events).
//! - `update::update` — pure Elm-style transition.
//! - `keymap::SHORTCUT_TABLE` + `keymap::dispatch` — single source of truth
//!   for both bottom-bar shortcut display and key dispatch.
//! - `render::{left_pane, right_pane, bottom_bar}` — pure render functions.
//! - `panic_hook` — terminal-restoring panic hook (US-01 AC-5).
//! - `check_terminal_width` — refuses to start when terminal is too narrow
//!   (US-01 AC-4).

#![forbid(unsafe_code)]

pub mod app_state;
pub mod dialogs;
pub mod effects;
pub mod keymap;
pub mod layout;
pub mod msg;
pub mod panic_hook;
pub mod render;
pub mod screens;
pub mod update;

pub use app_state::{AppState, FocusPane, RecoveryReason, Screen, ToolView};
pub use layout::{
    check_terminal_width, left_pane_body_rows, right_pane_body_rows, view, TerminalSizeError,
};
pub use msg::Msg;
pub use panic_hook::install_panic_hook;
pub use update::{update, UpdateEffect};
