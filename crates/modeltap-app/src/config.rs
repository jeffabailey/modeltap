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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppConfig {
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheConfig {
    /// `[cache] enabled = <bool>` in `~/.modeltap/config.toml`. Defaults to
    /// `true` — cache is opt-out. Setting `false` here is equivalent to
    /// passing `--no-cache` on every launch (AC-23-9).
    pub enabled: bool,

    /// `[cache] tool_ttl_seconds = <u64>` in `~/.modeltap/config.toml`.
    /// Defaults to 86400 (24h). Per-tool TTL eligibility window the
    /// warm-start orchestrator uses to decide whether a cached row paints
    /// from cache or falls through to cold-start (US-25 AC-25-2 / AC-25-4,
    /// step 04-03).
    ///
    /// A row whose `last_scan_at >= now - tool_ttl_seconds` is fresh; older
    /// rows are stale and the tool is dispatched to cold-scan. Setting `0`
    /// effectively disables warm-paint (every tool is stale on each launch).
    pub tool_ttl_seconds: u64,

    /// `[cache] persist_sha256 = <bool>` in `~/.modeltap/config.toml`.
    /// Defaults to `false` — SHA256 persistence is OPT-IN (US-27 / ADR-018),
    /// unlike the opt-out `enabled` flag. When `true`, the warm-start path
    /// seeds the in-process Sha256Cache from the persistent `cache_sha256`
    /// table (Tier 3) and the hash pool writes computed hashes back to it.
    pub persist_sha256: bool,
}

/// Documented default TTL window: 24 hours. Exposed as a constant so
/// downstream tests and the warm-start orchestrator can refer to the same
/// value rather than re-typing the literal.
pub const DEFAULT_TOOL_TTL_SECONDS: u64 = 86_400;

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            tool_ttl_seconds: DEFAULT_TOOL_TTL_SECONDS,
            persist_sha256: false,
        }
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
    let cache_section = doc.cache;
    AppConfig {
        cache: CacheConfig {
            enabled: cache_section
                .as_ref()
                .and_then(|c| c.enabled)
                .unwrap_or(true),
            tool_ttl_seconds: cache_section
                .as_ref()
                .and_then(|c| c.tool_ttl_seconds)
                .unwrap_or(DEFAULT_TOOL_TTL_SECONDS),
            persist_sha256: cache_section
                .as_ref()
                .and_then(|c| c.persist_sha256)
                .unwrap_or(false),
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
    /// `[cache] tool_ttl_seconds = <u64>`. Optional; defaults to 86400 (24h)
    /// when absent or malformed. Step 04-03.
    #[serde(default)]
    tool_ttl_seconds: Option<u64>,
    /// `[cache] persist_sha256 = <bool>`. Optional; defaults to false (opt-in)
    /// when absent. US-27 / ADR-018.
    #[serde(default)]
    persist_sha256: Option<bool>,
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
        let f =
            write_config("[cache]\nenabled = false\n\n[plugins.lm-studio]\nsearch_paths = []\n");
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

    #[test]
    fn default_tool_ttl_is_24_hours() {
        // 04-03: the documented default is 24h (86_400s). A fresh install
        // with no config file must inherit that value.
        assert_eq!(
            AppConfig::default().cache.tool_ttl_seconds,
            DEFAULT_TOOL_TTL_SECONDS
        );
        assert_eq!(DEFAULT_TOOL_TTL_SECONDS, 86_400);
    }

    #[test]
    fn cache_tool_ttl_seconds_parsed_from_toml() {
        // Explicit `tool_ttl_seconds = 3600` (1h) propagates through the
        // loader.
        let f = write_config("[cache]\ntool_ttl_seconds = 3600\n");
        let cfg = load_from_path(f.path());
        assert_eq!(cfg.cache.tool_ttl_seconds, 3600);
        // The other field's default is preserved.
        assert!(cfg.cache.enabled, "enabled stays at its default = true");
    }

    #[test]
    fn cache_tool_ttl_seconds_absent_keeps_default() {
        // `[cache]` present but `tool_ttl_seconds` key missing → default 86_400.
        let f = write_config("[cache]\nenabled = true\n");
        let cfg = load_from_path(f.path());
        assert_eq!(cfg.cache.tool_ttl_seconds, DEFAULT_TOOL_TTL_SECONDS);
    }

    #[test]
    fn persist_sha256_defaults_to_false() {
        // US-27 / ADR-018: SHA256 persistence is OPT-IN. A fresh install with
        // no config file must NOT persist SHA256 (default false), unlike the
        // opt-out `enabled` flag.
        assert!(!AppConfig::default().cache.persist_sha256);
    }

    #[test]
    fn persist_sha256_true_parsed() {
        let f = write_config("[cache]\npersist_sha256 = true\n");
        let cfg = load_from_path(f.path());
        assert!(
            cfg.cache.persist_sha256,
            "explicit persist_sha256=true must propagate"
        );
        // Other defaults preserved.
        assert!(cfg.cache.enabled, "enabled stays at its default = true");
    }

    #[test]
    fn persist_sha256_absent_keeps_default_false() {
        // `[cache]` present but `persist_sha256` key missing → default false.
        let f = write_config("[cache]\nenabled = true\n");
        let cfg = load_from_path(f.path());
        assert!(!cfg.cache.persist_sha256);
    }
}
