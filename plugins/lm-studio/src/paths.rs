//! Default path conventions for the LM Studio plugin.
//!
//! LM Studio has used two on-disk path conventions over its history:
//!
//!   `~/.cache/lm-studio/models/`  — newer (LM Studio 0.3.x+, XDG-compliant).
//!   `~/.lmstudio/models/`         — older (LM Studio 0.2.x and earlier).
//!
//! Some installs migrated; some didn't. The plugin checks BOTH in priority
//! order (new first, old second) so models stored under either convention
//! are surfaced.
//!
//! Cross-platform: macOS and Linux use the same conventions per US-20. WSL
//! is Linux-equivalent. Native Windows is non-goal for v1.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

/// Resolve the LM Studio default search paths from the given `$HOME`.
///
/// Returns the two-element list `[<home>/.cache/lm-studio/models,
/// <home>/.lmstudio/models]` in that priority order.
///
/// Returns an empty `Vec` if `home` is `None` — the plugin will then degrade
/// to `DiscoverError::NotInstalled` cleanly.
pub fn default_paths_from_home(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return Vec::new() };
    vec![
        home.join(".cache").join("lm-studio").join("models"),
        home.join(".lmstudio").join("models"),
    ]
}

/// Resolve the LM Studio default search paths from the process environment.
/// Reads `$HOME` once. Returns an empty `Vec` if `$HOME` is unset.
pub fn resolve_default_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    default_paths_from_home(home.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_paths_returns_new_and_old_in_priority_order() {
        // Behavior 1 — pure: from a synthetic $HOME, the function returns the
        // two LM Studio conventions in (new, old) order.
        let home = Path::new("/Users/devon");
        let paths = default_paths_from_home(Some(home));
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/Users/devon/.cache/lm-studio/models"),
                PathBuf::from("/Users/devon/.lmstudio/models"),
            ],
            "default paths must list new convention (.cache/lm-studio/models) \
             FIRST and older convention (.lmstudio/models) SECOND"
        );
    }

    #[test]
    fn default_paths_returns_empty_when_home_missing() {
        // Behavior 1 (negative variant): no $HOME → no defaults.
        assert!(default_paths_from_home(None).is_empty());
    }

    #[test]
    fn default_paths_match_on_linux_layout() {
        // US-20: macOS + Linux MUST use the same default paths. Linux-style
        // home (`/home/devon`) yields the SAME relative subpath structure as
        // the macOS-style `/Users/devon` test above.
        let home = Path::new("/home/devon");
        let paths = default_paths_from_home(Some(home));
        assert_eq!(paths.len(), 2);
        assert!(paths[0].ends_with(".cache/lm-studio/models"));
        assert!(paths[1].ends_with(".lmstudio/models"));
    }
}
