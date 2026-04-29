//! Elm-style update loop (per ADR-006).
//!
//! `update()` is a pure `(State, Msg) -> (State, UpdateEffect)`. The
//! composition root in `modeltap-app` interprets `UpdateEffect` (write JSONL
//! events, exit the process, etc.) — this module performs no I/O.

/// View-model for the walking-skeleton scaffold. Step 01-02 will extend this
/// with the discovered inventory; for now only the quit/exit-code fields are
/// load-bearing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppState {
    /// True once the user has asked to quit. The composition root checks this
    /// after each update; when set, it tears down the terminal and exits with
    /// `exit_code`.
    pub should_quit: bool,
    /// Exit code to return when `should_quit` is set. 0 = clean quit (q),
    /// 130 = SIGINT (Ctrl+C). Mirrors POSIX `128 + SIGINT`.
    pub exit_code: i32,
}

/// Driving-port message type for the TUI. Step 01-02 adds discovery progress
/// messages; for the walking-skeleton scaffold only Quit, CtrlC, and a
/// catch-all unbound-key noop are honored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// User pressed `q`. Clean shutdown, exit 0.
    Quit,
    /// User pressed Ctrl+C. Shutdown with POSIX SIGINT exit code (130).
    CtrlC,
    /// Any other key: silently ignored at this stage (master-acceptance
    /// US-03 "Unbound key is silently ignored" lands fully in step 01-03).
    UnboundKey,
}

/// Side-effects the composition root must perform after this update. The pure
/// update function only describes effects; it does not execute them. This
/// keeps `update()` testable as a pure function (per ADR-006).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateEffect {
    /// When set, the composition root should emit a `launch.ended` JSONL event
    /// before exiting. Per the master-acceptance "launch.ended NOT emitted on
    /// Ctrl+C" KPI invariant, this is true ONLY for `Msg::Quit`.
    pub emit_launch_ended: bool,
}

/// Pure Elm-style transition. No I/O, no time, no mutation outside the
/// returned `(AppState, UpdateEffect)`.
pub fn update(state: AppState, msg: Msg) -> (AppState, UpdateEffect) {
    match msg {
        Msg::Quit => (
            AppState {
                should_quit: true,
                exit_code: 0,
            },
            UpdateEffect {
                emit_launch_ended: true,
            },
        ),
        Msg::CtrlC => (
            AppState {
                should_quit: true,
                exit_code: 130,
            },
            UpdateEffect {
                emit_launch_ended: false,
            },
        ),
        Msg::UnboundKey => (state, UpdateEffect::default()),
    }
}
