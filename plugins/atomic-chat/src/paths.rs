//! Default path conventions for the Atomic Chat plugin.
//!
//! Atomic Chat (a Jan-derived inference app) stores its model tree at:
//!
//!   - macOS:     `~/Library/Application Support/Atomic Chat/data/llamacpp/models/`
//!   - Linux/WSL: `~/.config/Atomic Chat/data/llamacpp/models/`
//!
//! These paths follow the same per-OS convention Jan uses (Application
//! Support on macOS, XDG `~/.config` on Linux). MLX models live at
//! `<data>/mlx/models/...` but are OUT OF SCOPE for v1 per intake C3 / ADR-004
//! OQ-3, so the plugin only walks `llamacpp/models/`.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

/// The OS this build was compiled for. Returned by `host_os()` so unit tests
/// can drive both branches deterministically without `cfg!` macros leaking
/// into pure helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    MacOs,
    Linux,
}

/// Compile-time host OS. Native Windows is not a supported target for v1
/// (per US-20); WSL is treated as Linux because `cfg!(target_os = "linux")`
/// is true under WSL.
pub fn host_os() -> HostOs {
    if cfg!(target_os = "macos") {
        HostOs::MacOs
    } else {
        // Linux + WSL fall through here. Native Windows is rejected at the
        // composition root before any plugin runs.
        HostOs::Linux
    }
}

/// Resolve the Atomic Chat default search path for the given OS and `$HOME`.
/// Returns an empty `Vec` when `home` is `None` so the plugin degrades to
/// `DiscoverError::NotInstalled` cleanly.
///
/// The returned vector contains exactly ONE path — Atomic Chat has only
/// one canonical data location per OS (unlike LM Studio's old/new pair).
pub fn default_paths_from_home(os: HostOs, home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return Vec::new() };
    let p = match os {
        HostOs::MacOs => home
            .join("Library")
            .join("Application Support")
            .join("Atomic Chat")
            .join("data")
            .join("llamacpp")
            .join("models"),
        HostOs::Linux => home
            .join(".config")
            .join("Atomic Chat")
            .join("data")
            .join("llamacpp")
            .join("models"),
    };
    vec![p]
}

/// Production helper — reads `$HOME` once and resolves for the host OS.
pub fn resolve_default_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    default_paths_from_home(host_os(), home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Behavior — macOS resolution is `Application Support`-rooted.
    #[test]
    fn macos_default_path_is_under_application_support() {
        let home = Path::new("/Users/devon");
        let paths = default_paths_from_home(HostOs::MacOs, Some(home));
        assert_eq!(
            paths,
            vec![PathBuf::from(
                "/Users/devon/Library/Application Support/Atomic Chat/data/llamacpp/models"
            )],
            "macOS default must point at ~/Library/Application Support/Atomic Chat/data/llamacpp/models"
        );
    }

    /// Behavior — Linux/WSL resolution is `~/.config`-rooted (XDG-style).
    #[test]
    fn linux_default_path_is_under_dot_config() {
        let home = Path::new("/home/devon");
        let paths = default_paths_from_home(HostOs::Linux, Some(home));
        assert_eq!(
            paths,
            vec![PathBuf::from(
                "/home/devon/.config/Atomic Chat/data/llamacpp/models"
            )],
            "Linux/WSL default must point at ~/.config/Atomic Chat/data/llamacpp/models"
        );
    }

    /// Behavior (negative variant) — no `$HOME` → no defaults.
    #[test]
    fn returns_empty_when_home_is_missing() {
        assert!(default_paths_from_home(HostOs::MacOs, None).is_empty());
        assert!(default_paths_from_home(HostOs::Linux, None).is_empty());
    }
}
