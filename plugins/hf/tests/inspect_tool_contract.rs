//! Plugin-contract tests for `HfPlugin::inspect_tool` (US-21 step 02-03).
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.12 — HF overrides `inspect_tool` and must satisfy the
//! `InspectCapability::Supported` 3-test suite (happy path, determinism,
//! panic isolation).
//!
//! Step 02-02 shipped the HF-specific inspect tests (default hub-root search
//! path, `detected_version: None`, user-config search paths). Those remain
//! — they exercise HF-specific behaviours the cross-plugin `Supported`
//! contract does not assert. Step 02-03 ADDS the cross-plugin §3.12.S.*
//! contract via `modeltap_core::tests::inspect::run_inspect_tool_contract`.

use std::path::PathBuf;

use modeltap_core::domain::inspect::SearchPathSource;
use modeltap_core::tests::inspect::{run_inspect_tool_contract, InspectCapability};
use modeltap_core::{Tool, ToolId};
use modeltap_plugin_hf::HfPlugin;

/// §3.12 Supported contract: invoke the cross-plugin harness against the
/// HF plugin. The harness exercises §3.12.S.1 (happy path), §3.12.S.2
/// (determinism), §3.12.S.3 (panic isolation).
#[tokio::test]
async fn hf_satisfies_inspect_tool_contract() {
    let _g_cfg = ScopedEnv::set("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");

    let fixture = tempfile::tempdir().expect("tempdir for hf inspect contract fixture");
    let plugin = HfPlugin::new_with_hub_root(fixture.path().to_path_buf());
    run_inspect_tool_contract(
        &plugin,
        ToolId("hf"),
        fixture.path(),
        InspectCapability::Supported,
    )
    .await;
}

// ---------------------------------------------------------------------------
// Step-02-02 HF-specific behaviors. Preserved verbatim from the file's 02-02
// commit; documented in `component-boundaries.md` §"HF plugin coexistence note".
// ---------------------------------------------------------------------------

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
