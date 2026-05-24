//! Application-level configuration loader for `~/.modeltap/config.toml`.
//!
//! tool-model-info-sqlite-cache step 04-02 (US-23 AC-23-8 / AC-23-9, cache
//! opt-out paths). The plugin-side `[plugins.<name>]` sections continue to
//! be parsed inside each plugin crate (see `plugins/lm-studio/src/config.rs`,
//! `plugins/ollama/src/config.rs`). This module is the COMPOSITION-ROOT-only
//! view of the same `config.toml` file: it reads the `[cache]` section so
//! the launch path can short-circuit the warm-start orchestrator when the
//! user has set `cache.enabled = false`.
//!
//! Resolution order (mirrors the plugin pattern):
//!   1. `MODELTAP_CONFIG_PATH` env override (test seam — every acceptance
//!      test in the workspace pins this to either a fixture path or
//!      `/nonexistent/no-such-config.toml`).
//!   2. `$HOME/.modeltap/config.toml` — the documented user path.
//!   3. Defaults — `cache.enabled = true` (cache is opt-out, not opt-in).
//!
//! A missing file is NOT an error: it returns the default `AppConfig`. A
//! malformed file is logged via `tracing::warn!` and also returns defaults
//! — cache failure must never prevent launch (C-INFO-2).
//!
//! The CLI `--no-cache` flag is resolved against this config at the
//! composition root: the flag wins when both are set, so a user with
//! `cache.enabled = true` in their TOML can still bypass for one launch.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Merged application configuration. Today only the `[cache]` section is
/// app-level; plugin sections live inside each plugin crate's config loader
/// and are not represented here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub cache: CacheConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            cache: CacheConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheConfig {
    /// `[cache] enabled = <bool>` in `~/.modeltap/config.toml`. Defaults to
    /// `true` — cache is opt-out. Setting `false` here is equivalent to
    /// passing `--no-cache` on every launch (AC-23-9).
    pub enabled: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Load the user's `~/.modeltap/config.toml` (or `MODELTAP_CONFIG_PATH`
/// override) into an `AppConfig`. Returns `AppConfig::default()` when the
/// file is missing, unreadable, or malformed. Never panics.
pub fn load_from_env() -> AppConfig {
    let path = resolve_config_path();
    match path {
        Some(p) => load_from_path(&p),
        None => AppConfig::default(),
    }
}

/// Resolve the config-file path. `MODELTAP_CONFIG_PATH` wins when set;
/// otherwise `$HOME/.modeltap/config.toml`. Returns `None` only when neither
/// env var is set (caller falls through to defaults).
fn resolve_config_path() -> Option<PathBuf> {
    if let Some(env_path) = std::env::var_os("MODELTAP_CONFIG_PATH") {
        return Some(PathBuf::from(env_path));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".modeltap").join("config.toml"))
}

/// Load configuration from a specific file path. Test entry point.
pub fn load_from_path(path: &Path) -> AppConfig {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return AppConfig::default(),
    };
    parse_str(&raw, path)
}

fn parse_str(raw: &str, path: &Path) -> AppConfig {
    let doc: ConfigDoc = match toml::from_str(raw) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.app.config",
                "ignoring malformed config at {}: {}",
                path.display(),
                e
            );
            return AppConfig::default();
        }
    };
    AppConfig {
        cache: CacheConfig {
            enabled: doc.cache.and_then(|c| c.enabled).unwrap_or(true),
        },
    }
}

/// TOML doc shape. Only the `[cache]` section is consumed here; every other
/// section (e.g. `[plugins.lm-studio]`) is ignored by `#[serde(default)]` on
/// unrecognized fields plus the absence of a `deny_unknown_fields` attribute.
#[derive(Debug, Deserialize, Default)]
struct ConfigDoc {
    #[serde(default)]
    cache: Option<CacheSection>,
}

#[derive(Debug, Deserialize)]
struct CacheSection {
    #[serde(default)]
    enabled: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_config(contents: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("tempfile");
        f.write_all(contents.as_bytes()).expect("write");
        f
    }

    #[test]
    fn default_is_cache_enabled_true() {
        // Cache is opt-OUT: a fresh install with no config file must keep
        // the cache active by default (AC-23-9 implicit precondition).
        assert!(AppConfig::default().cache.enabled);
    }

    #[test]
    fn missing_file_returns_defaults() {
        let cfg = load_from_path(Path::new("/nonexistent/no-such-config.toml"));
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    fn cache_enabled_false_parsed() {
        let f = write_config("[cache]\nenabled = false\n");
        let cfg = load_from_path(f.path());
        assert!(!cfg.cache.enabled, "explicit enabled=false must propagate");
    }

    #[test]
    fn cache_enabled_true_parsed() {
        let f = write_config("[cache]\nenabled = true\n");
        let cfg = load_from_path(f.path());
        assert!(cfg.cache.enabled);
    }

    #[test]
    fn empty_cache_section_keeps_default() {
        // [cache] present but enabled key missing: default to true.
        let f = write_config("[cache]\n");
        let cfg = load_from_path(f.path());
        assert!(cfg.cache.enabled);
    }

    #[test]
    fn ignores_unknown_sections() {
        // The plugin-side sections must not break the app loader. Mirrors
        // a realistic user config that mixes [cache] with [plugins.*].
        let f = write_config(
            "[cache]\nenabled = false\n\n[plugins.lm-studio]\nsearch_paths = []\n",
        );
        let cfg = load_from_path(f.path());
        assert!(!cfg.cache.enabled);
    }

    #[test]
    fn malformed_toml_falls_back_to_defaults() {
        // Cache failure must never prevent launch (C-INFO-2). A garbage
        // TOML file must not panic and must return defaults.
        let f = write_config("this is not = valid = toml [");
        let cfg = load_from_path(f.path());
        assert_eq!(cfg, AppConfig::default());
    }
}
