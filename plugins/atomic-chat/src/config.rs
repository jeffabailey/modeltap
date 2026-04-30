//! Atomic Chat plugin configuration.
//!
//! Resolution model — env and TOML are ADDITIVE; defaults are a fallback
//! (mirrors `plugins/lm-studio/src/config.rs`).
//!
//! 1. `MODELTAP_ATOMIC_CHAT_DIRS` env var (colon-separated absolute paths).
//!    Test seam — acceptance tests use this to point the plugin at fixture
//!    trees without writing a config TOML.
//! 2. `~/.modeltap/config.toml` `[plugins.atomic-chat] search_paths` array.
//!    For tests, `MODELTAP_CONFIG_PATH` overrides the location.
//! 3. Defaults: per-OS Atomic Chat data path (per `paths::default_paths_from_home`).
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::paths::{default_paths_from_home, host_os};

#[derive(Debug, Clone, Default)]
pub struct AtomicChatConfig {
    pub search_paths: Vec<PathBuf>,
}

impl AtomicChatConfig {
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }
}

/// The environment surface the loader reads. Decoupled from `std::env` so
/// unit tests can pass synthetic env+filesystem context without globally
/// mutating the process env.
pub struct Environment<'a> {
    /// Value of `MODELTAP_ATOMIC_CHAT_DIRS`, if set.
    pub atomic_chat_dirs: Option<&'a str>,
    /// Value of `MODELTAP_CONFIG_PATH`, if set.
    pub config_path: Option<&'a Path>,
    /// Value of `$HOME`, if set.
    pub home: Option<&'a Path>,
}

/// Resolve the Atomic Chat plugin's configuration from `env`. NEVER panics —
/// missing env vars / unreadable config files / malformed TOML all degrade
/// gracefully to the next resolution layer.
///
/// Env paths and TOML paths are UNIONED. Defaults apply only when BOTH env
/// and TOML are silent.
pub fn load_config(env: &Environment<'_>) -> AtomicChatConfig {
    let mut paths: Vec<PathBuf> = Vec::new();

    if let Some(raw) = env.atomic_chat_dirs {
        let env_paths: Vec<PathBuf> = raw
            .split(':')
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        paths.extend(env_paths);
    }

    if let Some(cfg_path) = env.config_path {
        if let Some(toml_paths) = read_config_paths(cfg_path) {
            paths.extend(toml_paths);
        }
    } else if let Some(home) = env.home {
        let prod_cfg = home.join(".modeltap").join("config.toml");
        if let Some(toml_paths) = read_config_paths(&prod_cfg) {
            paths.extend(toml_paths);
        }
    }

    if !paths.is_empty() {
        return AtomicChatConfig {
            search_paths: paths,
        };
    }

    AtomicChatConfig {
        search_paths: default_paths_from_home(host_os(), env.home),
    }
}

/// Read the production-ish `Environment` from the actual process. Used by
/// the plugin's production constructor.
pub fn from_process_env() -> Environment<'static> {
    let atomic_chat_dirs = std::env::var("MODELTAP_ATOMIC_CHAT_DIRS")
        .ok()
        .map(leak_str)
        .map(|s| s as &str);
    let config_path = std::env::var_os("MODELTAP_CONFIG_PATH").map(|s| {
        let p: PathBuf = PathBuf::from(s);
        leak_path(p)
    });
    let home = std::env::var_os("HOME").map(|s| {
        let p: PathBuf = PathBuf::from(s);
        leak_path(p)
    });
    Environment {
        atomic_chat_dirs,
        config_path,
        home,
    }
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn leak_path(p: PathBuf) -> &'static Path {
    Box::leak(p.into_boxed_path())
}

#[derive(Debug, Deserialize)]
struct ConfigDoc {
    #[serde(default)]
    plugins: PluginsSection,
}

#[derive(Debug, Deserialize, Default)]
struct PluginsSection {
    #[serde(default, rename = "atomic-chat")]
    atomic_chat: Option<AtomicChatSection>,
}

#[derive(Debug, Deserialize)]
struct AtomicChatSection {
    #[serde(default)]
    search_paths: Vec<PathBuf>,
}

fn read_config_paths(path: &Path) -> Option<Vec<PathBuf>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let doc: ConfigDoc = match toml::from_str(&raw) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.atomic_chat.config",
                "ignoring malformed config at {}: {}",
                path.display(),
                e
            );
            return None;
        }
    };
    Some(
        doc.plugins
            .atomic_chat
            .map(|s| s.search_paths)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Behavior — env-var override: explicit colon-separated paths win.
    #[test]
    fn env_var_overrides_everything() {
        let env = Environment {
            atomic_chat_dirs: Some("/a:/b:/c"),
            config_path: None,
            home: Some(Path::new("/home/devon")),
        };
        let cfg = load_config(&env);
        assert_eq!(
            cfg.search_paths,
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c"),
            ]
        );
    }

    /// Behavior — TOML branch: `[plugins.atomic-chat] search_paths` is
    /// honored when the env var is silent.
    #[test]
    fn config_toml_paths_used_when_env_absent() {
        let temp = tempfile::tempdir().unwrap();
        let cfg = temp.path().join("config.toml");
        std::fs::write(
            &cfg,
            r#"[plugins.atomic-chat]
search_paths = ["/data/atomic", "/srv/atomic"]
"#,
        )
        .unwrap();
        let env = Environment {
            atomic_chat_dirs: None,
            config_path: Some(&cfg),
            home: None,
        };
        let res = load_config(&env);
        assert_eq!(
            res.search_paths,
            vec![PathBuf::from("/data/atomic"), PathBuf::from("/srv/atomic")]
        );
    }

    /// Behavior — defaults apply when both env and TOML are silent.
    /// Note: this test depends on `host_os()` so we just assert the path
    /// contains the home prefix and the canonical "Atomic Chat" segment.
    #[test]
    fn defaults_applied_when_env_and_toml_both_silent() {
        let env = Environment {
            atomic_chat_dirs: None,
            config_path: Some(Path::new("/nonexistent/no-such-config.toml")),
            home: Some(Path::new("/home/devon")),
        };
        let cfg = load_config(&env);
        assert_eq!(cfg.search_paths.len(), 1);
        let p = cfg.search_paths[0].display().to_string();
        assert!(
            p.contains("Atomic Chat") && p.ends_with("llamacpp/models"),
            "default must point at the Atomic Chat data root; got {p}"
        );
    }
}
