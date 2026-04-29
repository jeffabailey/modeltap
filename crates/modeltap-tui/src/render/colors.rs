//! Color-policy shim.
//!
//! Centralizes `NO_COLOR` detection so future render code does not sprinkle
//! `std::env::var_os("NO_COLOR")` reads across modules. Per the WCAG
//! color-independence contract (US-04.AC-3) the entire render layer must
//! ask this shim — never the env directly — whether to emit color.
//!
//! Semantics: the [NO_COLOR](https://no-color.org/) standard says the
//! variable is "active" whenever it is present in the environment, regardless
//! of value (including empty string). We implement that exactly.

/// Returns true when `NO_COLOR` is present in the environment.
///
/// This is a thin wrapper over `std::env::var_os` so production callers and
/// unit tests share the same definition. Tests that want to exercise the
/// "color allowed" path simply pass `false` directly to the render fn rather
/// than mutating process-global env state.
pub fn no_color_active() -> bool {
    std::env::var_os("NO_COLOR").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: process-global env mutation makes it unsafe to assert specific
    // states of NO_COLOR within a parallel test runner. This test only
    // verifies the function is callable and returns a bool — the behavioral
    // assertion lives in the acceptance test
    // `no_color_env_var_preserves_indicator_symbols` which sets NO_COLOR=1
    // on the child binary's environment.
    #[test]
    fn no_color_active_returns_a_bool_without_panicking() {
        let _ = no_color_active();
    }
}
