//! llama-cli plugin configuration.
//!
//! Resolution model — env and TOML are ADDITIVE; defaults are a fallback.
//!
//! 1. `MODELTAP_LLAMACLI_DIRS` env var (colon-separated absolute paths).
//!    Test seam — acceptance tests use this to point the plugin at fixture
//!    trees without writing a config TOML.
//! 2. `~/.modeltap/config.toml` `[plugins.llama-cli] search_paths` array.
//!    For tests, `MODELTAP_CONFIG_PATH` overrides the location.
//! 3. Defaults: `$HOME/llms` and `$HOME/models` — applied ONLY when both
//!    env and TOML are silent.
//!
//! Cross-platform: macOS and Linux use the same defaults (per US-20).
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Default)]
pub struct LlamaCliConfig {
    pub search_paths: Vec<PathBuf>,
}

impl LlamaCliConfig {
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }
}

/// The environment surface the loader reads. Decoupled from `std::env` so
/// unit tests can pass synthetic env+filesystem context without globally
/// mutating the process env.
pub struct Environment<'a> {
    /// Value of `MODELTAP_LLAMACLI_DIRS`, if set.
    pub llamacli_dirs: Option<&'a str>,
    /// Value of `MODELTAP_CONFIG_PATH`, if set.
    pub config_path: Option<&'a Path>,
    /// Value of `$HOME`, if set.
    pub home: Option<&'a Path>,
}

/// Resolve the llama-cli plugin's configuration from `env`. NEVER panics —
/// missing env vars / unreadable config files / malformed TOML all
/// degrade gracefully to the next resolution layer.
///
/// Env paths and TOML paths are UNIONED. Defaults (`$HOME/llms`,
/// `$HOME/models`) apply only when BOTH env and TOML are silent.
pub fn load_config(env: &Environment<'_>) -> LlamaCliConfig {
    let mut paths: Vec<PathBuf> = Vec::new();

    if let Some(raw) = env.llamacli_dirs {
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
        return LlamaCliConfig {
            search_paths: paths,
        };
    }

    // Defaults — applied only when env + TOML were both silent.
    // Same on macOS and Linux per US-20 contract.
    if let Some(home) = env.home {
        return LlamaCliConfig {
            search_paths: vec![home.join("llms"), home.join("models")],
        };
    }

    // No HOME, no config, no env — return empty so discover() can report
    // NotInstalled cleanly.
    LlamaCliConfig::default()
}

/// Read the production-ish `Environment` from the actual process. Used by
/// the plugin's production constructor.
pub fn from_process_env() -> Environment<'static> {
    // We need 'static lifetimes for the borrowed strings; std::env::var
    // returns owned `String`, so we leak — once, at startup — to get
    // 'static slices. This is fine: the env values are read exactly once
    // per process and the leaked strings live for the program's lifetime.
    let llamacli_dirs = std::env::var("MODELTAP_LLAMACLI_DIRS")
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
        llamacli_dirs,
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
    #[serde(default, rename = "llama-cli")]
    llama_cli: Option<LlamaCliSection>,
}

#[derive(Debug, Deserialize)]
struct LlamaCliSection {
    #[serde(default)]
    search_paths: Vec<PathBuf>,
}

fn read_config_paths(path: &Path) -> Option<Vec<PathBuf>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let doc: ConfigDoc = match toml::from_str(&raw) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.llama_cli.config",
                "ignoring malformed config at {}: {}",
                path.display(),
                e
            );
            return None;
        }
    };
    Some(
        doc.plugins
            .llama_cli
            .map(|lc| lc.search_paths)
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_overrides_everything() {
        let env = Environment {
            llamacli_dirs: Some("/a:/b:/c"),
            config_path: None,
            home: Some(Path::new("/home/devon")),
        };
        let cfg = load_config(&env);
        assert_eq!(
            cfg.search_paths,
            vec![
                PathBuf::from("/a"),
                PathBuf::from("/b"),
                PathBuf::from("/c")
            ]
        );
    }

    #[test]
    fn empty_env_var_falls_through_to_defaults() {
        let env = Environment {
            llamacli_dirs: Some(""),
            config_path: None,
            home: Some(Path::new("/home/devon")),
        };
        let cfg = load_config(&env);
        assert_eq!(
            cfg.search_paths,
            vec![
                PathBuf::from("/home/devon/llms"),
                PathBuf::from("/home/devon/models"),
            ]
        );
    }

    #[test]
    fn config_toml_paths_used_when_env_absent() {
        let temp = tempfile::tempdir().unwrap();
        let cfg = temp.path().join("config.toml");
        std::fs::write(
            &cfg,
            r#"[plugins.llama-cli]
search_paths = ["/data/models", "/srv/models"]
"#,
        )
        .unwrap();
        let env = Environment {
            llamacli_dirs: None,
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
    fn missing_config_toml_does_not_panic_falls_back_to_defaults() {
        let env = Environment {
            llamacli_dirs: None,
            config_path: Some(Path::new("/nonexistent/no-such-config.toml")),
            home: Some(Path::new("/home/devon")),
        };
        let cfg = load_config(&env);
        assert_eq!(
            cfg.search_paths,
            vec![
                PathBuf::from("/home/devon/llms"),
                PathBuf::from("/home/devon/models"),
            ]
        );
    }

    #[test]
    fn malformed_config_toml_falls_back_to_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let cfg = temp.path().join("config.toml");
        std::fs::write(&cfg, "this is = not [valid toml").unwrap();
        let env = Environment {
            llamacli_dirs: None,
            config_path: Some(&cfg),
            home: Some(Path::new("/home/devon")),
        };
        let cfg_loaded = load_config(&env);
        assert_eq!(
            cfg_loaded.search_paths,
            vec![
                PathBuf::from("/home/devon/llms"),
                PathBuf::from("/home/devon/models"),
            ]
        );
    }

    #[test]
    fn cross_platform_defaults_use_home_llms_and_home_models() {
        // US-20: macOS and Linux MUST use the same default paths.
        let env = Environment {
            llamacli_dirs: None,
            config_path: None,
            home: Some(Path::new("/Users/devon")),
        };
        let cfg = load_config(&env);
        assert_eq!(
            cfg.search_paths,
            vec![
                PathBuf::from("/Users/devon/llms"),
                PathBuf::from("/Users/devon/models"),
            ]
        );
    }
}
