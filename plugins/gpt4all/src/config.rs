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

    /// Behavior 4 — `Config::search_paths()` is the read-side projection.
    /// It MUST return the paths that were stored in the struct, in order.
    /// Mutating the getter to `Vec::leak(Vec::new())` or
    /// `Vec::leak(vec![Default::default()])` would lie to every caller.
    #[test]
    fn config_search_paths_getter_returns_stored_paths_verbatim() {
        let stored = vec![
            PathBuf::from("/etc/gpt4all/one"),
            PathBuf::from("/etc/gpt4all/two"),
        ];
        let cfg = Config {
            search_paths: stored.clone(),
        };
        assert_eq!(cfg.search_paths(), stored.as_slice());
        assert_eq!(cfg.search_paths().len(), 2);
        assert_ne!(
            cfg.search_paths()[0],
            PathBuf::new(),
            "getter must not return Default::default() PathBufs"
        );
    }

    /// Behavior 5 — `parse_colon_paths` MUST drop empty segments from
    /// strings like `":a::b:"` so the loader does not produce a `PathBuf::new()`
    /// (which would cause discovery to scan `""`). Also pins that the
    /// non-empty segments survive in order.
    ///
    /// Kills `parse_colon_paths -> vec![]`,
    /// `parse_colon_paths -> vec![Default::default()]`,
    /// and `delete ! in parse_colon_paths` (which would invert the filter
    /// and KEEP only the empty segments).
    #[test]
    fn parse_colon_paths_drops_empty_segments_and_preserves_order() {
        let parsed = parse_colon_paths(":/a::/b/c:");
        assert_eq!(
            parsed,
            vec![PathBuf::from("/a"), PathBuf::from("/b/c")],
            "must drop empty segments and preserve non-empty ones in order"
        );
        // Negative: must NOT contain a default/empty PathBuf.
        for p in &parsed {
            assert!(!p.as_os_str().is_empty(), "no empty segment must survive");
        }
    }

    /// Behavior 6 — `parse_colon_paths` on a string with NO colons returns
    /// the single path unchanged. Pins the happy-single-path branch and
    /// further fortifies against `vec![]` / `vec![Default::default()]`.
    #[test]
    fn parse_colon_paths_single_path_unchanged() {
        let parsed = parse_colon_paths("/single/path/only");
        assert_eq!(parsed, vec![PathBuf::from("/single/path/only")]);
    }

    /// Behavior 7 — `from_process_env`: when `MODELTAP_GPT4ALL_DIRS` is
    /// set to a colon-separated list, returns `Some(vec)` with those
    /// paths in order. Kills:
    /// - `from_process_env -> None`
    /// - `from_process_env -> Some(vec![])`
    /// - `from_process_env -> Some(vec![Default::default()])`
    ///
    /// Uses a `MutexGuard` to serialize env-var access between tests.
    #[test]
    fn from_process_env_returns_parsed_paths_when_var_set() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: this test crate runs single-process and ENV_LOCK
        // serializes any test that touches MODELTAP_GPT4ALL_DIRS.
        let prior = std::env::var("MODELTAP_GPT4ALL_DIRS").ok();
        std::env::set_var("MODELTAP_GPT4ALL_DIRS", "/x/one:/x/two");

        let got = from_process_env();
        // Restore before assertions so a failed assertion doesn't pollute.
        match prior {
            Some(v) => std::env::set_var("MODELTAP_GPT4ALL_DIRS", v),
            None => std::env::remove_var("MODELTAP_GPT4ALL_DIRS"),
        }

        let paths = got.expect("env was set, must be Some");
        assert_eq!(
            paths,
            vec![PathBuf::from("/x/one"), PathBuf::from("/x/two")],
            "must parse colon list verbatim, in order"
        );
        assert_ne!(paths.len(), 0, "Some(vec![]) mutation must die");
        assert_ne!(
            paths[0],
            PathBuf::new(),
            "Some(vec![Default::default()]) mutation must die"
        );
    }

    /// Behavior 8 — `from_process_env` returns `None` when the env var is
    /// unset (so the loader falls back to defaults). Pins the negative
    /// branch.
    #[test]
    fn from_process_env_returns_none_when_var_unset() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prior = std::env::var("MODELTAP_GPT4ALL_DIRS").ok();
        std::env::remove_var("MODELTAP_GPT4ALL_DIRS");

        let got = from_process_env();
        // Restore.
        if let Some(v) = prior {
            std::env::set_var("MODELTAP_GPT4ALL_DIRS", v);
        }

        assert_eq!(got, None, "unset env var must yield None");
    }

    /// Behavior 9 — `load_from_process` returns a `Config` that is NOT the
    /// default (empty) Config when the env var is set. Kills the
    /// `load_from_process -> Default::default()` mutation.
    #[test]
    fn load_from_process_returns_non_default_config_when_env_set() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prior = std::env::var("MODELTAP_GPT4ALL_DIRS").ok();
        std::env::set_var("MODELTAP_GPT4ALL_DIRS", "/cfg/seed");

        let cfg = load_from_process();
        match prior {
            Some(v) => std::env::set_var("MODELTAP_GPT4ALL_DIRS", v),
            None => std::env::remove_var("MODELTAP_GPT4ALL_DIRS"),
        }

        assert_eq!(
            cfg.search_paths,
            vec![PathBuf::from("/cfg/seed")],
            "env var must take effect through the production loader"
        );
        // Default::default() Config has empty search_paths — assert non-empty.
        assert!(
            !cfg.search_paths.is_empty(),
            "load_from_process must NOT degrade to Default::default()"
        );
    }

    use std::sync::Mutex;
    /// Serializes tests that touch `MODELTAP_GPT4ALL_DIRS`. `std::env`
    /// mutations are process-global; without a lock, parallel test runs
    /// could see each other's writes.
    static ENV_LOCK: Mutex<()> = Mutex::new(());
}
