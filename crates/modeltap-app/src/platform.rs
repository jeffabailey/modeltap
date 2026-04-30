//! Platform abstraction for cross-platform CI testing (US-20).
//!
//! `current_platform()` resolves the running host's platform identifier into
//! a `Platform` enum so the composition root (`main()`) can:
//!
//!   1. Refuse to run on native Windows with the documented WSL guidance.
//!   2. Let CI exercise per-OS code paths in a single job by setting
//!      `MODELTAP_FORCE_PLATFORM=<variant>` — the override takes precedence
//!      over the host's actual `cfg!()`-derived platform so a macOS or Linux
//!      runner can simulate every supported target.
//!
//! Per Phase 04 acceptance criteria (US-20): supported targets are
//! macOS x86_64/aarch64, Linux x86_64/aarch64, and Windows-native (refused).
//! WSL is intentionally identical to native Linux — there is no WSL variant.
//!
//! The env var contract is: any unrecognized value falls through to the
//! host-derived platform (`cfg!()` resolution). This is the safer default —
//! a typo in CI must not crash production.

use std::str::FromStr;

/// Supported platform variants. The granularity matches "what CI exercises"
/// rather than every theoretically possible Rust target triple — only the
/// variants the v1 release supports appear here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacosX86_64,
    MacosAarch64,
    LinuxX86_64,
    LinuxAarch64,
    /// Native Windows. Refused at startup per US-20 AC-3.
    Windows,
}

impl FromStr for Platform {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "macos-x86_64" => Ok(Platform::MacosX86_64),
            "macos-aarch64" => Ok(Platform::MacosAarch64),
            "linux-x86_64" => Ok(Platform::LinuxX86_64),
            "linux-aarch64" => Ok(Platform::LinuxAarch64),
            "windows-x86_64" | "windows" => Ok(Platform::Windows),
            _ => Err(()),
        }
    }
}

/// Resolve the current platform.
///
/// Priority:
///   1. `MODELTAP_FORCE_PLATFORM` env var, if set AND parseable.
///   2. `cfg!()` resolution of the host triple.
///
/// An unrecognized env value is IGNORED (logged-warning territory; we treat
/// it as "fall back to host" so a typo cannot brick the binary in prod).
pub fn current_platform() -> Platform {
    if let Some(forced) = forced_platform_from_env() {
        return forced;
    }
    host_platform()
}

fn forced_platform_from_env() -> Option<Platform> {
    let raw = std::env::var("MODELTAP_FORCE_PLATFORM").ok()?;
    Platform::from_str(&raw).ok()
}

fn host_platform() -> Platform {
    // The cfg!() resolution is exhaustive over the target triples v1
    // supports. Anything else falls through to LinuxX86_64 — a conservative
    // "treat as Linux" default which is what WSL also resolves to.
    if cfg!(target_os = "windows") {
        return Platform::Windows;
    }
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            return Platform::MacosAarch64;
        }
        return Platform::MacosX86_64;
    }
    // target_os = "linux" (or anything Unix-flavored that isn't macOS).
    if cfg!(target_arch = "aarch64") {
        return Platform::LinuxAarch64;
    }
    Platform::LinuxX86_64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Behavior 1: every supported MODELTAP_FORCE_PLATFORM value parses to
    /// the matching `Platform` variant. Parametrized over all 5 variants
    /// because the parsing logic is one cohesive behavior — different
    /// inputs of the same operation per Mandate 5.
    #[test]
    fn force_platform_env_parses_supported_values() {
        let cases: &[(&str, Platform)] = &[
            ("macos-x86_64", Platform::MacosX86_64),
            ("macos-aarch64", Platform::MacosAarch64),
            ("linux-x86_64", Platform::LinuxX86_64),
            ("linux-aarch64", Platform::LinuxAarch64),
            ("windows-x86_64", Platform::Windows),
            ("windows", Platform::Windows),
        ];
        for (input, expected) in cases {
            assert_eq!(
                Platform::from_str(input).expect("parse"),
                *expected,
                "MODELTAP_FORCE_PLATFORM={input:?} must parse to {expected:?}"
            );
        }
    }

    /// Behavior 2: unrecognized override value falls back to host platform.
    /// We assert `from_str` returns `Err` for garbage input — `current_platform`
    /// then falls through to `host_platform()`. We can't directly assert
    /// host_platform() equals a specific value (it varies per CI runner), but
    /// we CAN assert that an invalid env value is rejected at the parse step.
    #[test]
    fn unrecognized_force_platform_value_is_rejected_by_parser() {
        assert!(Platform::from_str("not-a-real-platform").is_err());
        assert!(Platform::from_str("").is_err());
        assert!(Platform::from_str("MACOS-X86_64").is_err()); // case-sensitive
    }

    /// Behavior 3: host_platform() returns SOME variant on every supported
    /// build target — the function is total over the cfg!() space. We test
    /// this by simply calling it; any panic or non-termination would fail.
    /// The exact variant depends on the test runner's host, so we assert
    /// only that the result is one of the recognized variants (a smoke
    /// check on totality).
    #[test]
    fn host_platform_returns_a_recognized_variant() {
        let p = host_platform();
        // If we reach this line, host_platform() did not panic.
        // The match is exhaustive — adding a new variant would force a
        // compile error here, which is the point.
        match p {
            Platform::MacosX86_64
            | Platform::MacosAarch64
            | Platform::LinuxX86_64
            | Platform::LinuxAarch64
            | Platform::Windows => {}
        }
    }
}
