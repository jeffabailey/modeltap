//! modeltap-core — pure domain types and logic. No I/O.
//!
//! Step 01-01 surface is intentionally minimal: the `Tool` trait, plugin port
//! types, and indicator engine arrive in 01-02 and 01-04 respectively. For now
//! we only expose the bottom-bar shortcut catalog as a `&'static str` so the
//! TUI can render it deterministically without owning the copy itself.

#![forbid(unsafe_code)]

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
