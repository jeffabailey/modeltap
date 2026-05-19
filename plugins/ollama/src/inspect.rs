//! Ollama `inspect_tool` override (US-21 step 02-02).
//!
//! Per ADR-016 §"Implementation Guidance" + acceptance-test-plan.md §R5 +
//! wave-decisions.md §D12.
//!
//! ## Detection strategy
//!
//! 1. `MODELTAP_OLLAMA_VERSION` env var: if set, short-circuit the HTTP probe
//!    and return that string as `detected_version`. This is the D12 / R5
//!    seam — CI scenarios set it so the suite does not depend on a running
//!    Ollama daemon.
//! 2. Otherwise HTTP GET `<MODELTAP_OLLAMA_API_URL or http://localhost:11434/api/version>`
//!    with a 500 ms total timeout. On success, parse `{"version": "<v>"}`
//!    and return `Some(v)`.
//! 3. On any failure (timeout, connection refused, parse error), return
//!    `Ok(detected_version: None)`. NEVER return `Err` — the cache reconcile
//!    must not loop because the user has no Ollama installed.
//!
//! ## Search paths
//!
//! The plugin emits one `Default` entry for the models root resolved at
//! construction time (`~/.ollama/models/`). User-config search paths from
//! `~/.modeltap/config.toml [plugins.ollama] search_paths = [...]` are
//! appended after the defaults with `SearchPathSource::UserConfig` so AC-21-5
//! can distinguish them in the TUI.
//!
//! ## Object-Calisthenics scope
//!
//! Adapter side of the hexagon — strict OC rules are relaxed.

use std::path::PathBuf;
use std::time::Duration;

use modeltap_core::domain::inspect::{InspectError, SearchPathEntry, SearchPathSource, ToolDetail};
use modeltap_core::ToolId;

use crate::TOOL_NAME;

/// Total budget for the HTTP probe — ADR-016 implementation guidance.
/// Includes connect + read; ureq applies the same value to both.
const HTTP_PROBE_TIMEOUT_MS: u64 = 500;

/// Default production endpoint. Overridable via `MODELTAP_OLLAMA_API_URL`.
const DEFAULT_OLLAMA_API_URL: &str = "http://localhost:11434/api/version";

/// Env-var: short-circuit the HTTP probe with a literal version string.
const ENV_VERSION_OVERRIDE: &str = "MODELTAP_OLLAMA_VERSION";

/// Env-var: override the HTTP endpoint (test seam — points at an unreachable
/// or fake server in CI).
const ENV_API_URL_OVERRIDE: &str = "MODELTAP_OLLAMA_API_URL";

/// Env-var: location of `~/.modeltap/config.toml` (test seam — mirrors the
/// pattern in `plugins/lm-studio/src/config.rs`).
const ENV_CONFIG_PATH_OVERRIDE: &str = "MODELTAP_CONFIG_PATH";

/// Build the `ToolDetail` for the Ollama plugin. Pure orchestration over the
/// env + HTTP probe + config-loader subroutines; never panics, never returns
/// `Err`. The inspect_tool fields the orchestrator overrides from cache are
/// left as `None` / `0` here.
///
/// `models_root` is the resolved discovery root from the plugin's constructor
/// (`OllamaPlugin::models_root`).
///
/// When the HTTP probe fails (timeout, connection refused) AND no env-var
/// short-circuit is set, the returned `ToolDetail` carries `last_error: Some(...)`
/// + `last_error_at: Some(SystemTime::now())`. This is the AC-21-4 hook: the
///   reconcile path picks up the populated `last_error` and writes it into the
///   cache row, so the next launch's tool-detail screen renders the error.
pub fn build_tool_detail(models_root: PathBuf) -> ToolDetail {
    let (detected_version, last_error) = detect_version_with_error();
    let last_error_at = last_error.as_ref().map(|_| std::time::SystemTime::now());
    ToolDetail {
        tool_id: TOOL_NAME,
        install_path: models_root.clone(),
        detected_version,
        plugin_version: plugin_version_string(),
        search_paths: build_search_paths(&models_root),
        // Cache-sourced fields. The orchestrator overrides these in the
        // `Ok` merge branch when the cache row carries scan-state.
        model_count: 0,
        disk_usage_bytes: 0,
        largest_model: None,
        last_scan_at: None,
        last_scan_duration_ms: None,
        last_error,
        last_error_at,
    }
}

/// Async entry point used by `OllamaPlugin::inspect_tool`. Wraps the sync
/// builder in `spawn_blocking` so the HTTP probe (sync ureq) does not park
/// the runtime thread. Never returns `Err` from this layer — propagates
/// `Ok(ToolDetail)` with `detected_version: None` on probe failure.
pub async fn inspect_tool_impl(models_root: PathBuf) -> Result<ToolDetail, InspectError> {
    let join = tokio::task::spawn_blocking(move || build_tool_detail(models_root)).await;
    match join {
        Ok(detail) => Ok(detail),
        Err(join_err) => Err(InspectError::PluginPanic {
            tool: TOOL_NAME,
            message: format!("ollama inspect_tool task panicked: {join_err}"),
        }),
    }
}

/// Plugin-version string per ADR-016: `"modeltap-plugin-ollama <semver>"`.
fn plugin_version_string() -> String {
    format!("modeltap-plugin-ollama {}", env!("CARGO_PKG_VERSION"))
}

/// Resolve `detected_version` AND the optional `last_error` message in one
/// pass. Returns:
/// - `(Some(v), None)` when the env-var short-circuit or HTTP probe succeeds.
/// - `(None, Some(msg))` when the HTTP probe fails (the env-var was not set).
///   `msg` is a human-readable reason ("connection refused", "timeout", ...).
/// - `(None, None)` when no detection path was attempted (defensive — current
///   logic always attempts the HTTP path after the env-var miss, so this
///   variant is unreachable in practice).
fn detect_version_with_error() -> (Option<String>, Option<String>) {
    if let Ok(v) = std::env::var(ENV_VERSION_OVERRIDE) {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return (Some(trimmed.to_string()), None);
        }
    }
    let url =
        std::env::var(ENV_API_URL_OVERRIDE).unwrap_or_else(|_| DEFAULT_OLLAMA_API_URL.to_string());
    match http_probe_version(&url) {
        Ok(v) => (Some(v), None),
        Err(reason) => (None, Some(reason)),
    }
}

/// Synchronous HTTP probe — returns `Ok(version)` on success, `Err(reason)`
/// on any failure (timeout, connection refused, parse error, etc.).
fn http_probe_version(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(HTTP_PROBE_TIMEOUT_MS))
        .build();
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("ollama /api/version unreachable at {url}: {e}"))?;
    if resp.status() != 200 {
        return Err(format!(
            "ollama /api/version at {url} returned status {}",
            resp.status()
        ));
    }
    let body = resp
        .into_string()
        .map_err(|e| format!("ollama /api/version body unreadable: {e}"))?;
    parse_version_json(&body)
        .ok_or_else(|| "ollama /api/version response did not contain a `version` field".to_string())
}

/// Parse `{"version": "<v>"}` from the Ollama `/api/version` response body.
/// Returns `None` on any deserialisation failure.
fn parse_version_json(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed.get("version")?.as_str().map(|s| s.to_string())
}

/// Build the `search_paths` vector: default entry for the models root,
/// followed by any user-config entries from `~/.modeltap/config.toml`.
fn build_search_paths(models_root: &std::path::Path) -> Vec<SearchPathEntry> {
    let mut out = Vec::new();
    out.push(SearchPathEntry {
        path: models_root.to_path_buf(),
        source: SearchPathSource::Default,
    });
    for p in load_user_config_search_paths() {
        out.push(SearchPathEntry {
            path: p,
            source: SearchPathSource::UserConfig,
        });
    }
    out
}

/// Read `[plugins.ollama] search_paths` from `~/.modeltap/config.toml`
/// (or `MODELTAP_CONFIG_PATH` override). Returns an empty vec on any
/// error — config is best-effort.
fn load_user_config_search_paths() -> Vec<PathBuf> {
    let config_path = match resolve_config_path() {
        Some(p) => p,
        None => return Vec::new(),
    };
    let raw = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let doc: ConfigDoc = match toml::from_str(&raw) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.ollama.config",
                "ignoring malformed config at {}: {e}",
                config_path.display(),
            );
            return Vec::new();
        }
    };
    doc.plugins
        .and_then(|p| p.ollama)
        .map(|o| o.search_paths)
        .unwrap_or_default()
}

/// Resolve the config path. Priority:
/// 1. `MODELTAP_CONFIG_PATH` env var (test seam).
/// 2. `$HOME/.modeltap/config.toml`.
/// 3. `None`.
fn resolve_config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(ENV_CONFIG_PATH_OVERRIDE) {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".modeltap").join("config.toml"))
}

#[derive(Debug, serde::Deserialize)]
struct ConfigDoc {
    #[serde(default)]
    plugins: Option<PluginsSection>,
}

#[derive(Debug, serde::Deserialize)]
struct PluginsSection {
    #[serde(default)]
    ollama: Option<OllamaSection>,
}

#[derive(Debug, serde::Deserialize)]
struct OllamaSection {
    #[serde(default)]
    search_paths: Vec<PathBuf>,
}

/// Silence the unused-warning for `ToolId` when only the const re-export
/// is used (some downstream compilers warn under specific feature combos).
const _: ToolId = TOOL_NAME;

// ---------------------------------------------------------------------------
// Unit tests — pure functions only. The async + HTTP behaviors are exercised
// from `tests/inspect_tool_contract.rs` so the real socket path is engaged.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_json_extracts_version_field() {
        let body = r#"{"version":"0.6.4"}"#;
        assert_eq!(parse_version_json(body), Some("0.6.4".to_string()));
    }

    #[test]
    fn parse_version_json_returns_none_on_missing_field() {
        let body = r#"{"build":"abc"}"#;
        assert_eq!(parse_version_json(body), None);
    }

    #[test]
    fn parse_version_json_returns_none_on_malformed_json() {
        assert_eq!(parse_version_json("not-json"), None);
    }

    #[test]
    fn build_search_paths_default_entry_carries_models_root() {
        let root = PathBuf::from("/tmp/ollama-models");
        let entries = build_search_paths(&root);
        assert!(entries
            .iter()
            .any(|e| e.path == root && e.source == SearchPathSource::Default));
    }

    #[test]
    fn plugin_version_string_carries_crate_version() {
        let v = plugin_version_string();
        assert!(
            v.starts_with("modeltap-plugin-ollama "),
            "plugin_version must start with crate name; got {v}"
        );
    }
}
