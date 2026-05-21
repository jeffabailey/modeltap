//! Plugin-contract tests for `OllamaPlugin::inspect_tool` (US-21 step 02-03).
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.12 — Ollama overrides `inspect_tool` (HTTP probe + env-var seam) and
//! must satisfy the `InspectCapability::Supported` 3-test suite (happy path,
//! determinism, panic isolation).
//!
//! ## Step-02-02 vs step-02-03 separation
//!
//! Step 02-02 shipped the Ollama-specific inspect tests (env-var short-circuit,
//! HTTP unreachable, default search paths, user-config search paths). Those
//! remain — see the four `inspect_tool_*` tests in the rest of this file —
//! because they exercise Ollama-specific BEHAVIORS that the cross-plugin
//! `Supported` contract does not cover.
//!
//! Step 02-03 ADDS the cross-plugin §3.12.S.* contract via
//! `modeltap_core::tests::inspect::run_inspect_tool_contract` (the `Supported`
//! 3-test suite). This file therefore has two layers:
//!
//! - `ollama_satisfies_inspect_tool_contract` — invokes the cross-plugin
//!   §3.12.S.* harness against the Ollama plugin.
//! - The four step-02-02 tests below — Ollama-specific behaviours.
//!
//! ## Determinism + env scoping
//!
//! Every test acquires the global `ENV_MUTEX` (defined at the bottom of this
//! file) before mutating process env vars, so the harness's two consecutive
//! `inspect_tool()` calls inside `test_inspect_tool_deterministic` cannot
//! race with the step-02-02 tests that toggle `MODELTAP_OLLAMA_VERSION`.

use std::path::PathBuf;

use modeltap_core::domain::inspect::{InspectError, SearchPathSource, ToolDetail};
use modeltap_core::tests::inspect::{run_inspect_tool_contract, InspectCapability};
use modeltap_core::{Tool, ToolId};
use modeltap_plugin_ollama::OllamaPlugin;

/// §3.12 Supported contract: invoke the cross-plugin harness against the
/// Ollama plugin. The harness exercises §3.12.S.1 (happy path), §3.12.S.2
/// (determinism across two consecutive calls), §3.12.S.3 (panic isolation
/// surfaces as `Err(PluginPanic)`).
///
/// `MODELTAP_OLLAMA_VERSION=0.6.4` is set so the HTTP probe is short-
/// circuited (D12 / R5) — without it the probe would hit localhost:11434
/// which CI cannot guarantee.
#[tokio::test]
async fn ollama_satisfies_inspect_tool_contract() {
    let _env = ScopedEnv::with(&[
        ("MODELTAP_OLLAMA_VERSION", EnvOp::Set("0.6.4")),
        ("MODELTAP_OLLAMA_API_URL", EnvOp::Set("http://127.0.0.1:1")),
        (
            "MODELTAP_CONFIG_PATH",
            EnvOp::Set("/nonexistent/no-such-config.toml"),
        ),
    ]);

    let fixture = tempfile::tempdir().expect("tempdir for ollama inspect contract fixture");
    let plugin = OllamaPlugin::new_with_root(fixture.path().to_path_buf());
    run_inspect_tool_contract(
        &plugin,
        ToolId("ollama"),
        fixture.path(),
        InspectCapability::Supported,
    )
    .await;
}

// ---------------------------------------------------------------------------
// Step-02-02 Ollama-specific behaviors. Preserved verbatim from the file's
// 02-02 commit; documented in `acceptance-test-plan.md` §R5 + `wave-decisions.md` §D12.
// ---------------------------------------------------------------------------

/// `MODELTAP_OLLAMA_VERSION=<v>` short-circuits the HTTP probe: the returned
/// `ToolDetail.detected_version` is exactly `Some(v)`. Per D12 / R5: CI has
/// no Ollama daemon; the env-var is the test seam every Ollama scenario
/// uses so no flakes hit the real localhost socket.
#[tokio::test]
async fn inspect_tool_honors_env_var_short_circuit() {
    let _env = ScopedEnv::with(&[
        ("MODELTAP_OLLAMA_API_URL", EnvOp::Set("http://127.0.0.1:1")),
        ("MODELTAP_OLLAMA_VERSION", EnvOp::Set("0.6.4")),
        (
            "MODELTAP_CONFIG_PATH",
            EnvOp::Set("/nonexistent/no-such-config.toml"),
        ),
    ]);

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
    let _env = ScopedEnv::with(&[
        ("MODELTAP_OLLAMA_VERSION", EnvOp::Unset),
        (
            "MODELTAP_OLLAMA_API_URL",
            EnvOp::Set("http://127.0.0.1:1/api/version"),
        ),
        (
            "MODELTAP_CONFIG_PATH",
            EnvOp::Set("/nonexistent/no-such-config.toml"),
        ),
    ]);

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
    let _env = ScopedEnv::with(&[
        ("MODELTAP_OLLAMA_VERSION", EnvOp::Set("0.6.4")),
        (
            "MODELTAP_CONFIG_PATH",
            EnvOp::Set("/nonexistent/no-such-config.toml"),
        ),
    ]);

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
// parallel tests; tests below run serial because each constructs a single
// `ScopedEnv` which acquires the global `ENV_MUTEX` exactly once for the
// test's full duration. The Drop impl restores the prior values so a failing
// test does not leak state into sibling tests.
//
// IMPORTANT: `std::sync::Mutex` is NOT reentrant. A test must NOT construct
// two `ScopedEnv` instances on the same thread — the second `lock()` call
// would deadlock. Use `ScopedEnv::with(&[...])` to batch all env-var
// modifications a single test needs into ONE guard.
// ---------------------------------------------------------------------------

use std::sync::Mutex;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

/// A single scoped env-var modification: either set to a value, or unset.
enum EnvOp<'a> {
    Set(&'a str),
    Unset,
}

/// RAII guard that holds the global `ENV_MUTEX` and applies a batch of
/// env-var modifications, restoring all prior values (or removing them if
/// they were not set before) when dropped.
struct ScopedEnv {
    restores: Vec<(String, Option<String>)>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl ScopedEnv {
    /// Construct a guard that applies all `(key, op)` modifications under
    /// ONE acquisition of `ENV_MUTEX`. Use this any time a test needs to
    /// touch more than one env var — it is the only safe pattern.
    fn with(ops: &[(&str, EnvOp<'_>)]) -> Self {
        let guard = ENV_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
        let mut restores = Vec::with_capacity(ops.len());
        for (key, op) in ops {
            let prior = std::env::var(key).ok();
            match op {
                EnvOp::Set(value) => std::env::set_var(key, value),
                EnvOp::Unset => std::env::remove_var(key),
            }
            restores.push(((*key).to_string(), prior));
        }
        Self {
            restores,
            _guard: guard,
        }
    }

    /// Convenience for the (common) single-key set case.
    fn set(key: &str, value: &str) -> Self {
        Self::with(&[(key, EnvOp::Set(value))])
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        // Restore in reverse application order so symmetric pairs unwind
        // correctly even though the keys here are distinct.
        for (key, prior) in self.restores.iter().rev() {
            match prior {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    }
}
