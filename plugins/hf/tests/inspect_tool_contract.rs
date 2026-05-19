//! Plugin-contract tests for `HfPlugin::inspect_tool` (US-21 step 02-02).
//!
//! Per ADR-016 + `component-boundaries.md` §"HF plugin coexistence note".
//!
//! Behaviors covered (3 distinct = budget 6; this file uses 3):
//!
//! - HF cache dir detection: HF_HOME / ~/.cache/huggingface populates
//!   `install_path` and `search_paths[Default]`.
//! - User-config search paths from `~/.modeltap/config.toml [plugins.hf]
//!   search_paths` are appended after defaults with `SearchPathSource::UserConfig`.
//! - `detected_version` is `None` by design — HF cache has no version concept.

use std::path::PathBuf;

use modeltap_core::domain::inspect::SearchPathSource;
use modeltap_core::Tool;
use modeltap_plugin_hf::HfPlugin;

#[tokio::test]
async fn inspect_tool_search_paths_includes_default_hub_root() {
    let _g_cfg = ScopedEnv::set("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");

    let hub = PathBuf::from("/tmp/some-hf-hub-root");
    let plugin = HfPlugin::new_with_hub_root(hub.clone());
    let detail = plugin.inspect_tool().await.expect("inspect_tool ok");

    let defaults: Vec<_> = detail
        .search_paths
        .iter()
        .filter(|e| e.source == SearchPathSource::Default)
        .collect();
    assert!(
        defaults.iter().any(|e| e.path == hub),
        "default search paths must include the hub root {:?}; got {:?}",
        hub,
        detail.search_paths
    );
}

#[tokio::test]
async fn inspect_tool_detected_version_is_none_for_hf() {
    let _g_cfg = ScopedEnv::set("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    let plugin = HfPlugin::new_with_hub_root(PathBuf::from("/tmp/some-hf-hub-root"));
    let detail = plugin.inspect_tool().await.expect("inspect_tool ok");
    assert_eq!(
        detail.detected_version, None,
        "HF cache has no version concept; detected_version must be None"
    );
}

#[tokio::test]
async fn inspect_tool_search_paths_includes_user_config_entries() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cfg = temp.path().join("config.toml");
    std::fs::write(
        &cfg,
        r#"[plugins.hf]
search_paths = ["/srv/hf-extra"]
"#,
    )
    .expect("write config.toml");
    let _g_cfg = ScopedEnv::set("MODELTAP_CONFIG_PATH", cfg.to_string_lossy().as_ref());

    let plugin = HfPlugin::new_with_hub_root(PathBuf::from("/tmp/some-hf-hub-root"));
    let detail = plugin.inspect_tool().await.expect("inspect_tool ok");

    let user_entries: Vec<_> = detail
        .search_paths
        .iter()
        .filter(|e| e.source == SearchPathSource::UserConfig)
        .collect();
    assert_eq!(
        user_entries.len(),
        1,
        "expected one user-config entry; got {:?}",
        detail.search_paths
    );
    assert_eq!(
        user_entries[0].path,
        PathBuf::from("/srv/hf-extra"),
        "user-config path must round-trip from TOML"
    );

    let default_idx = detail
        .search_paths
        .iter()
        .position(|e| e.source == SearchPathSource::Default)
        .expect("default entry exists");
    let user_idx = detail
        .search_paths
        .iter()
        .position(|e| e.source == SearchPathSource::UserConfig)
        .expect("user-config entry exists");
    assert!(
        default_idx < user_idx,
        "user-config entries must appear AFTER defaults; got {:?}",
        detail.search_paths
    );
}

// ---------------------------------------------------------------------------
// Test helpers — process-env scoping (see plugins/ollama/tests/inspect_tool_contract.rs
// for rationale).
// ---------------------------------------------------------------------------

use std::sync::Mutex;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

struct ScopedEnv {
    key: String,
    prior: Option<String>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl ScopedEnv {
    fn set(key: &str, value: &str) -> Self {
        let guard = ENV_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
        let prior = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key: key.to_string(),
            prior,
            _guard: guard,
        }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        match &self.prior {
            Some(v) => std::env::set_var(&self.key, v),
            None => std::env::remove_var(&self.key),
        }
    }
}
