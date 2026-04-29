//! Unit tests for modeltap-tui — port-to-port at the pure-function scope.
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors in US-01 acceptance criteria:
//!     B1: terminal-size guard (refuse < 80, allow >= 80; AC-4)
//!     B2: Quit message → state.should_quit=true, exit 0, emit launch.ended
//!     B3: Ctrl+C message → state.should_quit=true, exit 130, NO launch.ended
//!   budget = 3 × 2 = 6 tests max. We use 5 (terminal-size guard parametrized).
//!
//! Each test enters through a pure-function driving port:
//!   - `check_terminal_width(actual)` — driving port for the size guard
//!   - `update(state, msg)` — driving port for the Elm-style state machine
//!
//! No mocks; all real values; outcomes asserted on returned data.

use modeltap_tui::{check_terminal_width, update, AppState, Msg, TerminalSizeError, UpdateEffect};

// ---------------------------------------------------------------------------
// B1 — terminal-size guard (US-01 AC-4)
// ---------------------------------------------------------------------------

/// Parametrized over the boundary values of the 80-column threshold.
/// One test, four input variations (Mandate 5: parametrize input variations).
#[test]
fn terminal_size_guard_observes_80_column_threshold() {
    // (actual_cols, expected_outcome_description)
    let too_narrow_cases = [0u16, 1, 60, 79];
    for actual in too_narrow_cases {
        let err = check_terminal_width(actual)
            .expect_err(&format!("expected refusal at {} columns", actual));
        assert_eq!(
            err,
            TerminalSizeError {
                required: 80,
                actual,
            },
            "wrong error payload at {} columns",
            actual
        );
        // Display string is the same one shown to the user on stderr.
        let rendered = err.to_string();
        let expected = format!(
            "Terminal too narrow: need at least 80 columns, found {}",
            actual
        );
        assert_eq!(
            rendered, expected,
            "user-facing message wrong at {}",
            actual
        );
    }

    let wide_enough_cases = [80u16, 81, 100, 200, 999];
    for actual in wide_enough_cases {
        let result = check_terminal_width(actual);
        assert!(
            result.is_ok(),
            "expected acceptance at {} columns, got {:?}",
            actual,
            result
        );
    }
}

// ---------------------------------------------------------------------------
// B2 — Quit message produces clean-exit state + launch.ended effect
// ---------------------------------------------------------------------------

#[test]
fn quit_message_marks_clean_exit_and_emits_launch_ended() {
    let initial = AppState::default();

    let (next, effect) = update(initial.clone(), Msg::Quit);

    assert!(next.should_quit, "Quit must set should_quit");
    assert_eq!(next.exit_code, 0, "Quit must produce exit code 0");
    assert_eq!(
        effect,
        UpdateEffect {
            emit_launch_ended: true,
            ..UpdateEffect::default()
        },
        "Quit must emit launch.ended event"
    );
    // Initial state must not have been mutated through interior mutability —
    // `update` is required to be pure. (Cheap check via Clone equality.)
    assert!(
        !initial.should_quit,
        "initial AppState must remain pristine"
    );
}

// ---------------------------------------------------------------------------
// B3 — Ctrl+C message produces SIGINT-exit state and DOES NOT emit launch.ended
// ---------------------------------------------------------------------------

#[test]
fn ctrl_c_message_marks_signal_exit_and_does_not_emit_launch_ended() {
    let initial = AppState::default();

    let (next, effect) = update(initial, Msg::CtrlC);

    assert!(next.should_quit, "CtrlC must set should_quit");
    assert_eq!(
        next.exit_code, 130,
        "CtrlC must produce POSIX 128+SIGINT exit code 130"
    );
    assert_eq!(
        effect,
        UpdateEffect {
            emit_launch_ended: false,
            ..UpdateEffect::default()
        },
        "CtrlC must NOT emit launch.ended (per master-acceptance KPI invariant)"
    );
}

// ---------------------------------------------------------------------------
// B2/B3 negative — unbound key is a no-op (master-acceptance "Unbound key
// silently ignored"; included here to pin the third Msg variant's behavior).
// ---------------------------------------------------------------------------

#[test]
fn unbound_key_is_a_pure_noop() {
    let initial = AppState::default();
    let (next, effect) = update(initial.clone(), Msg::UnboundKey);
    assert_eq!(next, initial, "unbound key must not mutate state");
    assert_eq!(
        effect,
        UpdateEffect::default(),
        "unbound key must produce no effect"
    );
}

// ---------------------------------------------------------------------------
// Panic hook — install must be idempotent (US-01 AC-5 precondition).
// ---------------------------------------------------------------------------

#[test]
fn install_panic_hook_is_idempotent() {
    // Calling twice in the same process must not panic and must not double-
    // chain the default hook (single-Once installation).
    modeltap_tui::install_panic_hook();
    modeltap_tui::install_panic_hook();
    assert!(
        modeltap_tui::panic_hook::is_installed_for_tests(),
        "panic hook installation must be observable via the test probe"
    );
}
