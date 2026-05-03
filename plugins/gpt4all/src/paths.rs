//! Default path conventions for the GPT4All plugin.
//!
//! GPT4All has TWO official storage roots per platform — the Python SDK and
//! the desktop chat app each ship their own default location, and many users
//! have models in both. This plugin therefore returns BOTH paths in priority
//! order so models stored under either convention surface in `discover()`.
//!
//! Storage paths (verified 2026-05-02):
//!
//!   - Python SDK (cross-platform):
//!     `~/.cache/gpt4all/`
//!
//!   - Desktop app — macOS:
//!     `~/Library/Application Support/nomic-ai/gpt4all-chat/`
//!
//!   - Desktop app — Linux/WSL:
//!     `~/.local/share/nomic-ai/gpt4all-chat/`
//!
//! Note: the Python SDK uses `~/.cache/gpt4all` on ALL platforms — including
//! macOS — per the GPT4All Python documentation. We therefore intentionally
//! compose this from `home.join(".cache")` rather than `dirs::cache_dir()`,
//! because on macOS `dirs::cache_dir()` returns `~/Library/Caches`, which is
//! NOT where GPT4All's SDK stores models.
//!
//! Cross-platform: WSL is treated as Linux (`cfg!(target_os = "linux")` is
//! true under WSL). Native Windows is non-goal for v1 (US-20).
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

/// Resolve the GPT4All default search paths for the given OS and `$HOME`.
///
/// Returns a two-element list `[<python-sdk>, <desktop-app>]` in priority
/// order. Both paths are returned regardless of whether either exists on
/// disk — the discovery walk handles missing directories gracefully.
///
/// Returns an empty `Vec` when `home` is `None` so the plugin degrades to
/// `DiscoverError::NotInstalled` cleanly.
pub fn default_paths_from_home(os: HostOs, home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return Vec::new() };

    // Python SDK location is identical on macOS and Linux per GPT4All docs.
    // Compose from `~/.cache` directly (NOT dirs::cache_dir, which would
    // return `~/Library/Caches` on macOS — wrong for GPT4All).
    let python_sdk = home.join(".cache").join("gpt4all");

    let desktop = match os {
        HostOs::MacOs => home
            .join("Library")
            .join("Application Support")
            .join("nomic-ai")
            .join("gpt4all-chat"),
        HostOs::Linux => home
            .join(".local")
            .join("share")
            .join("nomic-ai")
            .join("gpt4all-chat"),
    };

    vec![python_sdk, desktop]
}

/// Production helper — reads `$HOME` once and resolves for the host OS.
pub fn resolve_default_paths() -> Vec<PathBuf> {
    let home = dirs::home_dir();
    default_paths_from_home(host_os(), home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Behavior 1 — macOS resolution returns BOTH the Python SDK path
    /// (`~/.cache/gpt4all`) AND the desktop app path
    /// (`~/Library/Application Support/nomic-ai/gpt4all-chat`) in that order.
    /// Note: macOS uses `~/.cache/gpt4all` (NOT `~/Library/Caches/gpt4all`)
    /// because that is what the GPT4All Python SDK actually uses.
    #[test]
    fn macos_default_paths_include_python_sdk_and_desktop() {
        let home = Path::new("/Users/devon");
        let paths = default_paths_from_home(HostOs::MacOs, Some(home));
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/Users/devon/.cache/gpt4all"),
                PathBuf::from("/Users/devon/Library/Application Support/nomic-ai/gpt4all-chat"),
            ],
            "macOS defaults must list Python SDK FIRST and desktop app SECOND"
        );
    }

    /// Behavior 2 — Linux/WSL resolution returns BOTH the Python SDK path
    /// (`~/.cache/gpt4all`) AND the XDG desktop app path
    /// (`~/.local/share/nomic-ai/gpt4all-chat`) in that order.
    #[test]
    fn linux_default_paths_include_python_sdk_and_xdg_desktop() {
        let home = Path::new("/home/devon");
        let paths = default_paths_from_home(HostOs::Linux, Some(home));
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/home/devon/.cache/gpt4all"),
                PathBuf::from("/home/devon/.local/share/nomic-ai/gpt4all-chat"),
            ],
            "Linux/WSL defaults must list Python SDK FIRST and XDG desktop app SECOND"
        );
    }

    /// Behavior 3 (negative variant) — no `$HOME` → no defaults.
    /// Both OS branches must degrade to an empty Vec so the plugin can
    /// surface `DiscoverError::NotInstalled` cleanly.
    #[test]
    fn returns_empty_when_home_is_missing() {
        assert!(default_paths_from_home(HostOs::MacOs, None).is_empty());
        assert!(default_paths_from_home(HostOs::Linux, None).is_empty());
    }
}
