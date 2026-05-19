//! HF `inspect_tool` override (US-21 step 02-02).
//!
//! Sibling module to `folder_delete.rs` (per component-boundaries.md §"HF
//! plugin coexistence note") — the two coexist without modifying each
//! other's surface.
//!
//! ## Detection strategy
//!
//! HF cache has no notion of a "tool version" — the `huggingface_hub` library
//! itself isn't installed in modeltap's process, and the on-disk cache is
//! a passive blob/snapshot tree. So `detected_version` is `None` by design.
//! The TUI renders this as `"(not detectable)"` per AC-21-3.
//!
//! ## Search paths
//!
//! Default entry: the hub root resolved at construction time
//! (`<HF_HOME>/hub/` or `$HOME/.cache/huggingface/hub/` per
//! `discover::resolve_hub_root`).
//!
//! User-config entries from `~/.modeltap/config.toml [plugins.hf] search_paths
//! = [...]` are appended after defaults with `SearchPathSource::UserConfig`.
//!
//! ## Object-Calisthenics scope
//!
//! Adapter side of the hexagon — strict OC rules are relaxed.

use std::path::PathBuf;

use modeltap_core::domain::inspect::{InspectError, SearchPathEntry, SearchPathSource, ToolDetail};
use modeltap_core::ToolId;

use crate::TOOL_NAME;

/// Env-var: location of `~/.modeltap/config.toml` (test seam — mirrors the
/// pattern in `plugins/lm-studio/src/config.rs`).
const ENV_CONFIG_PATH_OVERRIDE: &str = "MODELTAP_CONFIG_PATH";

/// Build the `ToolDetail` for the HF plugin. Pure orchestration; never
/// panics, never returns `Err`. Cache-sourced fields are zero / `None`;
/// the orchestrator overrides them from the cache row.
pub fn build_tool_detail(hub_root: PathBuf) -> ToolDetail {
    ToolDetail {
        tool_id: TOOL_NAME,
        install_path: hub_root.clone(),
        detected_version: None,
        plugin_version: plugin_version_string(),
        search_paths: build_search_paths(&hub_root),
        model_count: 0,
        disk_usage_bytes: 0,
        largest_model: None,
        last_scan_at: None,
        last_scan_duration_ms: None,
        last_error: None,
        last_error_at: None,
    }
}

/// Async entry point used by `HfPlugin::inspect_tool`. Wraps the sync
/// builder in `spawn_blocking` for symmetry with the Ollama plugin and
/// so the eventual filesystem-probe extensions (e.g. reading
/// `version.txt`) won't park the runtime thread.
pub async fn inspect_tool_impl(hub_root: PathBuf) -> Result<ToolDetail, InspectError> {
    let join = tokio::task::spawn_blocking(move || build_tool_detail(hub_root)).await;
    match join {
        Ok(detail) => Ok(detail),
        Err(join_err) => Err(InspectError::PluginPanic {
            tool: TOOL_NAME,
            message: format!("hf inspect_tool task panicked: {join_err}"),
        }),
    }
}

/// Plugin-version string per ADR-016: `"modeltap-plugin-hf <semver>"`.
fn plugin_version_string() -> String {
    format!("modeltap-plugin-hf {}", env!("CARGO_PKG_VERSION"))
}

/// Build the `search_paths` vector: default entry for the hub root,
/// followed by any user-config entries.
fn build_search_paths(hub_root: &std::path::Path) -> Vec<SearchPathEntry> {
    let mut out = Vec::new();
    out.push(SearchPathEntry {
        path: hub_root.to_path_buf(),
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
                target: "modeltap.hf.config",
                "ignoring malformed config at {}: {e}",
                config_path.display(),
            );
            return Vec::new();
        }
    };
    doc.plugins
        .and_then(|p| p.hf)
        .map(|h| h.search_paths)
        .unwrap_or_default()
}

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
    hf: Option<HfSection>,
}

#[derive(Debug, serde::Deserialize)]
struct HfSection {
    #[serde(default)]
    search_paths: Vec<PathBuf>,
}

const _: ToolId = TOOL_NAME;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_search_paths_default_entry_carries_hub_root() {
        let hub = PathBuf::from("/tmp/hf-hub");
        let entries = build_search_paths(&hub);
        assert!(entries
            .iter()
            .any(|e| e.path == hub && e.source == SearchPathSource::Default));
    }

    #[test]
    fn plugin_version_string_carries_crate_version() {
        let v = plugin_version_string();
        assert!(
            v.starts_with("modeltap-plugin-hf "),
            "plugin_version must start with crate name; got {v}"
        );
    }
}
