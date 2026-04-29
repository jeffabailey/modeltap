//! `Msg` — the Elm-style message type for the TUI (per ADR-006).
//!
//! Every keystroke (and every async-arriving event in later steps) becomes
//! one variant of this enum. The pure `update::update()` function consumes
//! a `Msg` and returns the next `AppState` plus a description of any side
//! effects (`UpdateEffect`).

/// All the messages that can drive `update()`. Step 01-03 covers keyboard
/// navigation; later steps add discovery-progress, action-completion, and
/// tick variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// User pressed `q`. Clean shutdown, exit 0.
    Quit,
    /// User pressed Ctrl+C. Shutdown with POSIX SIGINT exit code (130).
    CtrlC,
    /// Right Arrow — advance to the next tool slot (cycles).
    SelectNextTool,
    /// Left Arrow — regress to the previous tool slot (cycles).
    SelectPrevTool,
    /// Down Arrow — advance to the next row in the current tool.
    SelectNextRow,
    /// Up Arrow — regress to the previous row in the current tool.
    SelectPrevRow,
    /// Tab — toggle focus between left and right panes.
    ToggleFocus,
    /// Any unrecognized key. No-op per US-03 AC-6 (silently ignored).
    UnboundKey,
}
