//! Plugin-contract tests for `OllamaPlugin::inspect_tool` (US-21 step 02-02).
//!
//! Per ADR-016 + `acceptance-test-plan.md` §R5 + `wave-decisions.md` §D12.
//!
//! Behaviors covered (5 distinct = test budget 10; this file uses 4):
//!
//! - Env-var short-circuit: `MODELTAP_OLLAMA_VERSION=<v>` is honoured and
//!   skips the HTTP probe entirely (D12 / R5 mitigation).
//! - HTTP timeout / connection refused → `Ok(detected_version: None)` — never
//!   panic, never `Err`, so cache reconcile doesn't loop.
//! - `search_paths` defaults: `<models_root>` tagged `Default`.
//! - `search_paths` user-config merge: env-set `MODELTAP_OLLAMA_SEARCH_PATHS`
//!   contributes `UserConfig`-tagged entries APPENDED after defaults.

use std::path::PathBuf;

use modeltap_core::domain::inspect::{InspectError, SearchPathSource, ToolDetail};
use modeltap_core::Tool;
use modeltap_plugin_ollama::OllamaPlugin;

/// `MODELTAP_OLLAMA_VERSION=<v>` short-circuits the HTTP probe: the returned
/// `ToolDetail.detected_version` is exactly `Some(v)`. Per D12 / R5: CI has
/// no Ollama daemon; the env-var is the test seam every Ollama scenario
/// uses so no flakes hit the real localhost socket.
#[tokio::test]
async fn inspect_tool_honors_env_var_short_circuit() {
    // Force the HTTP URL at an unreachable address to PROVE the env-var
    // short-circuits the probe — if the env-var didn't win, the HTTP path
    // would time out and return None.
    let _g_url = ScopedEnv::set("MODELTAP_OLLAMA_API_URL", "http://127.0.0.1:1");
    let _g_ver = ScopedEnv::set("MODELTAP_OLLAMA_VERSION", "0.6.4");
    let _g_cfg = ScopedEnv::set("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");

    let plugin = OllamaPlugin::new_with_root(PathBuf::from("/tmp/no-such-ollama-test-root"));
    let detail: ToolDetail = plugin.inspect_tool().await.expect("inspect_tool ok path");

    assert_eq!(
        detail.detected_version,
        Some("0.6.4".to_string()),
        "MODELTAP_OLLAMA_VERSION must short-circuit the HTTP probe"
    );
}

/// HTTP probe hits an unreachable address (port 1 always refuses on localhost):
/// the plugin must return `Ok` with `detected_version: None`, never `Err`,
/// never panic. The 500 ms timeout in `inspect.rs` keeps the user-visible
/// detail-screen-open path bounded.
#[tokio::test]
async fn inspect_tool_returns_none_on_http_unreachable() {
    let _g_ver = ScopedEnv::unset("MODELTAP_OLLAMA_VERSION");
    let _g_url = ScopedEnv::set("MODELTAP_OLLAMA_API_URL", "http://127.0.0.1:1/api/version");
    let _g_cfg = ScopedEnv::set("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");

    let plugin = OllamaPlugin::new_with_root(PathBuf::from("/tmp/no-such-ollama-test-root"));
    let result = plugin.inspect_tool().await;

    match result {
        Ok(detail) => assert_eq!(
            detail.detected_version, None,
            "unreachable HTTP must degrade to detected_version: None"
        ),
        Err(InspectError::Unsupported { .. }) => {
            panic!("HTTP unreachable must return Ok(detected_version: None), not Unsupported")
        }
        Err(e) => panic!("HTTP unreachable must return Ok, not Err({e:?})"),
    }
}

/// `search_paths` starts with the discovery root tagged `Default`. The plugin
/// emits one entry per default location it knows about; for Ollama that is
/// `<models_root>`.
#[tokio::test]
async fn inspect_tool_search_paths_includes_default_root() {
    let _g_ver = ScopedEnv::set("MODELTAP_OLLAMA_VERSION", "0.6.4");
    let _g_cfg = ScopedEnv::set("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");

    let root = PathBuf::from("/tmp/some-ollama-root");
    let plugin = OllamaPlugin::new_with_root(root.clone());
    let detail = plugin.inspect_tool().await.expect("inspect_tool ok");

    let defaults: Vec<_> = detail
        .search_paths
        .iter()
        .filter(|e| e.source == SearchPathSource::Default)
        .collect();
    assert!(
        defaults.iter().any(|e| e.path == root),
        "default search paths must include the models root {:?}; got {:?}",
        root,
        detail.search_paths
    );
}

/// User-config search paths (from `~/.modeltap/config.toml` `[plugins.ollama]
/// search_paths = [...]`) are appended AFTER the default entries and tagged
/// `UserConfig`. Drives the AC-21-5 acceptance scenario.
#[tokio::test]
async fn inspect_tool_search_paths_includes_user_config_entries() {
    let _g_ver = ScopedEnv::set("MODELTAP_OLLAMA_VERSION", "0.6.4");

    // Write a config TOML with one [plugins.ollama] search_paths entry.
    let temp = tempfile::tempdir().expect("tempdir");
    let cfg = temp.path().join("config.toml");
    std::fs::write(
        &cfg,
        r#"[plugins.ollama]
search_paths = ["/data/ollama-extra"]
"#,
    )
    .expect("write config.toml");
    let _g_cfg = ScopedEnv::set("MODELTAP_CONFIG_PATH", cfg.to_string_lossy().as_ref());

    let plugin = OllamaPlugin::new_with_root(PathBuf::from("/tmp/some-ollama-root"));
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
        PathBuf::from("/data/ollama-extra"),
        "user-config path must round-trip from the TOML"
    );

    // Ordering: defaults first, user-config last.
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
// Test helpers — process-env scoping. Env-var manipulation is racy across
// parallel tests; the contract tests below run serial because each acquires
// the global ENV_MUTEX. The Drop impl restores the prior value so a failing
// test does not leak state into sibling tests.
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

    fn unset(key: &str) -> Self {
        let guard = ENV_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
        let prior = std::env::var(key).ok();
        std::env::remove_var(key);
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
