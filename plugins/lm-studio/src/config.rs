//! LM Studio plugin configuration.
//!
//! Resolution model — env and TOML are ADDITIVE; defaults are a fallback
//! (mirrors `plugins/llama-cli/src/config.rs`).
//!
//! 1. `MODELTAP_LMSTUDIO_DIRS` env var (colon-separated absolute paths).
//!    Test seam — acceptance tests use this to point the plugin at fixture
//!    trees without writing a config TOML.
//! 2. `~/.modeltap/config.toml` `[plugins.lm-studio] search_paths` array.
//!    For tests, `MODELTAP_CONFIG_PATH` overrides the location.
//! 3. Defaults: `$HOME/.cache/lm-studio/models` and `$HOME/.lmstudio/models`
//!    (per `paths::default_paths_from_home`). Same on macOS + Linux per US-20.
//!
//! Cross-platform: macOS and Linux use the same defaults (per US-20).
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::paths::default_paths_from_home;

#[derive(Debug, Clone, Default)]
pub struct LmStudioConfig {
    pub search_paths: Vec<PathBuf>,
}

impl LmStudioConfig {
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }
}

/// The environment surface the loader reads. Decoupled from `std::env` so
/// unit tests can pass synthetic env+filesystem context without globally
/// mutating the process env.
pub struct Environment<'a> {
    /// Value of `MODELTAP_LMSTUDIO_DIRS`, if set.
    pub lmstudio_dirs: Option<&'a str>,
    /// Value of `MODELTAP_CONFIG_PATH`, if set.
    pub config_path: Option<&'a Path>,
    /// Value of `$HOME`, if set.
    pub home: Option<&'a Path>,
}

/// Resolve the LM Studio plugin's configuration from `env`. NEVER panics —
/// missing env vars / unreadable config files / malformed TOML all
/// degrade gracefully to the next resolution layer.
///
/// Env paths and TOML paths are UNIONED. Defaults (`$HOME/.cache/lm-studio/models`,
/// `$HOME/.lmstudio/models`) apply only when BOTH env and TOML are silent.
pub fn load_config(env: &Environment<'_>) -> LmStudioConfig {
    let mut paths: Vec<PathBuf> = Vec::new();

    if let Some(raw) = env.lmstudio_dirs {
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
        // Production default config location: `~/.modeltap/config.toml` —
        // only consulted when MODELTAP_CONFIG_PATH was not set.
        let prod_cfg = home.join(".modeltap").join("config.toml");
        if let Some(toml_paths) = read_config_paths(&prod_cfg) {
            paths.extend(toml_paths);
        }
    }

    if !paths.is_empty() {
        return LmStudioConfig {
            search_paths: paths,
        };
    }

    // Defaults — applied only when env + TOML were both silent.
    // Same on macOS and Linux per US-20 contract.
    LmStudioConfig {
        search_paths: default_paths_from_home(env.home),
    }
}

/// Read the production-ish `Environment` from the actual process. Used by
/// the plugin's production constructor.
pub fn from_process_env() -> Environment<'static> {
    // We need 'static lifetimes for the borrowed strings; std::env::var
    // returns owned `String`, so we leak — once, at startup — to get
    // 'static slices. This is fine: the env values are read exactly once
    // per process and the leaked strings live for the program's lifetime.
    let lmstudio_dirs = std::env::var("MODELTAP_LMSTUDIO_DIRS")
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
        lmstudio_dirs,
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
    #[serde(default, rename = "lm-studio")]
    lm_studio: Option<LmStudioSection>,
}

#[derive(Debug, Deserialize)]
struct LmStudioSection {
    #[serde(default)]
    search_paths: Vec<PathBuf>,
}

fn read_config_paths(path: &Path) -> Option<Vec<PathBuf>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let doc: ConfigDoc = match toml::from_str(&raw) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.lm_studio.config",
                "ignoring malformed config at {}: {}",
                path.display(),
                e
            );
            return None;
        }
    };
    Some(
        doc.plugins
            .lm_studio
            .map(|s| s.search_paths)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_overrides_everything() {
        // Behavior 2: env var precedence — explicit colon-separated paths win.
        let env = Environment {
            lmstudio_dirs: Some("/a:/b:/c"),
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

    #[test]
    fn empty_env_var_falls_through_to_defaults() {
        // Behavior 2 (negative variant): empty MODELTAP_LMSTUDIO_DIRS is
        // treated as silent — defaults from $HOME apply.
        let env = Environment {
            lmstudio_dirs: Some(""),
            config_path: None,
            home: Some(Path::new("/home/devon")),
        };
        let cfg = load_config(&env);
        assert_eq!(
            cfg.search_paths,
            vec![
                PathBuf::from("/home/devon/.cache/lm-studio/models"),
                PathBuf::from("/home/devon/.lmstudio/models"),
            ]
        );
    }

    #[test]
    fn config_toml_paths_used_when_env_absent() {
        // Behavior 2 (TOML branch): `[plugins.lm-studio] search_paths` array
        // is honored when env is silent.
        let temp = tempfile::tempdir().unwrap();
        let cfg = temp.path().join("config.toml");
        std::fs::write(
            &cfg,
            r#"[plugins.lm-studio]
search_paths = ["/data/models", "/srv/models"]
"#,
        )
        .unwrap();
        let env = Environment {
            lmstudio_dirs: None,
            config_path: Some(&cfg),
            home: None,
        };
        let res = load_config(&env);
        assert_eq!(
            res.search_paths,
            vec![PathBuf::from("/data/models"), PathBuf::from("/srv/models")]
        );
    }

    #[test]
    fn missing_config_toml_falls_back_to_defaults() {
        // Behavior 2 (resilience): a non-existent config path does not panic;
        // defaults from $HOME apply.
        let env = Environment {
            lmstudio_dirs: None,
            config_path: Some(Path::new("/nonexistent/no-such-config.toml")),
            home: Some(Path::new("/home/devon")),
        };
        let cfg = load_config(&env);
        assert_eq!(
            cfg.search_paths,
            vec![
                PathBuf::from("/home/devon/.cache/lm-studio/models"),
                PathBuf::from("/home/devon/.lmstudio/models"),
            ]
        );
    }
}
