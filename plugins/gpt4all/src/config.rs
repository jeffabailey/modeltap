//! GPT4All plugin configuration.
//!
//! Resolution model — env REPLACES defaults (per intake brief, this differs
//! from the additive lm-studio/atomic-chat model):
//!
//! 1. `MODELTAP_GPT4ALL_DIRS` env var (colon-separated absolute paths). If
//!    set and non-empty, these paths are used VERBATIM — defaults are not
//!    added. This matches the `MODELTAP_LMSTUDIO_DIRS` test seam pattern.
//! 2. Otherwise: per-OS defaults (Python SDK + desktop app) per
//!    `paths::default_paths_from_home`.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::PathBuf;

use crate::paths::{default_paths_from_home, host_os};

/// Resolved configuration for the GPT4All plugin. Currently only carries the
/// search-path list; later steps may add format filters or tuning knobs.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub search_paths: Vec<PathBuf>,
}

impl Config {
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }
}

/// Read `MODELTAP_GPT4ALL_DIRS` from the actual process environment.
/// Returns `None` when the var is unset or empty (so the loader can fall
/// through to defaults). Returns `Some(vec)` when one or more colon-separated
/// non-empty paths are present.
pub fn from_process_env() -> Option<Vec<PathBuf>> {
    let raw = std::env::var("MODELTAP_GPT4ALL_DIRS").ok()?;
    let parsed = parse_colon_paths(&raw);
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

/// Pure resolution: env REPLACES defaults. When `env_dirs` is `Some` (and
/// non-empty), it is used verbatim. Otherwise `default_dirs` is used.
pub fn load_config(env_dirs: Option<Vec<PathBuf>>, default_dirs: Vec<PathBuf>) -> Config {
    let search_paths = env_dirs.filter(|v| !v.is_empty()).unwrap_or(default_dirs);
    Config { search_paths }
}

/// Production constructor — wires `from_process_env()` against the per-OS
/// defaults rooted at `$HOME`. Used by `Gpt4AllPlugin::new()`.
pub fn load_from_process() -> Config {
    let env_dirs = from_process_env();
    let defaults = default_paths_from_home(host_os(), dirs::home_dir().as_deref());
    load_config(env_dirs, defaults)
}

fn parse_colon_paths(raw: &str) -> Vec<PathBuf> {
    raw.split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Behavior 1 — env-only: when `env_dirs` is `Some`, those paths win and
    /// defaults are NOT included. (Per intake: env REPLACES, not unions.)
    #[test]
    fn env_paths_replace_defaults() {
        let env = Some(vec![
            PathBuf::from("/tmp/fixture-a"),
            PathBuf::from("/tmp/fixture-b"),
        ]);
        let defaults = vec![PathBuf::from("/home/devon/.cache/gpt4all")];
        let cfg = load_config(env, defaults);
        assert_eq!(
            cfg.search_paths,
            vec![
                PathBuf::from("/tmp/fixture-a"),
                PathBuf::from("/tmp/fixture-b"),
            ],
            "env paths must REPLACE defaults entirely (no union)"
        );
    }

    /// Behavior 2 — defaults-only: when `env_dirs` is `None`, the per-OS
    /// defaults are used unchanged.
    #[test]
    fn defaults_used_when_env_absent() {
        let defaults = vec![
            PathBuf::from("/home/devon/.cache/gpt4all"),
            PathBuf::from("/home/devon/.local/share/nomic-ai/gpt4all-chat"),
        ];
        let cfg = load_config(None, defaults.clone());
        assert_eq!(
            cfg.search_paths, defaults,
            "defaults must be used verbatim when env is absent"
        );
    }

    /// Behavior 3 — env-overrides-defaults edge case: when `env_dirs` is
    /// `Some(empty)`, treat as "env not set" and fall back to defaults.
    /// (Defensive: parse_colon_paths returns empty for "" or ":::"; the
    /// loader must not produce an empty search-path list when defaults exist.)
    #[test]
    fn empty_env_falls_back_to_defaults() {
        let defaults = vec![PathBuf::from("/home/devon/.cache/gpt4all")];
        let cfg = load_config(Some(Vec::new()), defaults.clone());
        assert_eq!(
            cfg.search_paths, defaults,
            "empty env vec must fall back to defaults, not produce empty config"
        );
    }
}
