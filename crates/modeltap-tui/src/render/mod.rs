//! Render — pure `view(&AppState)` (per ADR-006).
//!
//! Split into per-pane submodules:
//! - `left_pane` — list of tools + status annotation.
//! - `right_pane` — model rows + scroll position indicator.
//! - `bottom_bar` — shortcut line driven by `keymap::SHORTCUT_TABLE`.

pub mod bottom_bar;
pub mod colors;
pub mod indicator;
pub mod last_action;
pub mod left_pane;
pub mod right_pane;
pub mod row;
pub mod summary_bar;
pub mod zap_dialog;
