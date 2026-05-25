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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use modeltap_core::domain::inspect::{
    InspectError, ModelDetail, ModelId, SearchPathEntry, SearchPathSource, ToolDetail,
};
use modeltap_core::ToolId;

use crate::TOOL_NAME;

/// Maximum number of KV entries the Ollama plugin emits into `metadata_kv` per
/// AC-22-6 ("≤10 keys per plugin, tool-relevant subset, not entire manifest").
/// The current selection emits at most 4 keys (architecture, parameter_size,
/// template-excerpt, system-excerpt); the bound is enforced at the construction
/// boundary so future field additions stay within budget without a separate
/// audit. Excerpt cap mirrors the AC-22-6 "≈200 chars" guidance.
const METADATA_MAX_KEYS: usize = 10;
const TEMPLATE_EXCERPT_CHARS: usize = 200;

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

// ---------------------------------------------------------------------------
// inspect_model: read <models_root>/manifests/<.../id> manifest JSON and
// extract a small tool-relevant KV subset for the model-detail screen.
// Step 03-02 part 1/N (US-22 AC-22-3..AC-22-6).
// ---------------------------------------------------------------------------

/// Async entry point used by `OllamaPlugin::inspect_model`. Mirrors the
/// `inspect_tool_impl` shape (sync core in `spawn_blocking`) so the JSON
/// read + parse never parks the runtime thread.
///
/// Contract per ADR-016 + US-22 acceptance criteria:
/// - `Ok(ModelDetail)` with `format = Some("Ollama manifest v2")` and
///   `metadata_kv` populated with the tool-relevant subset
///   (`config.architecture`, `parameters`, `template`, `system`) when the
///   manifest is readable and parseable.
/// - `Err(InspectError::FileReadable { path, source })` when the manifest
///   file is missing or otherwise unreadable.
/// - `Err(InspectError::FormatUnreadable { path, detail })` when the file
///   is readable but its JSON is malformed.
/// - `metadata_kv` MUST be ≤ 10 keys per AC-22-6 (enforced at construction).
/// - Never panics (AC-22-7 trait invariant; verified by the step 02-03
///   plugin-contract harness).
pub async fn inspect_model_impl(
    models_root: PathBuf,
    model_id: ModelId,
) -> Result<ModelDetail, InspectError> {
    let join =
        tokio::task::spawn_blocking(move || build_model_detail(&models_root, &model_id)).await;
    match join {
        Ok(res) => res,
        Err(join_err) => Err(InspectError::PluginPanic {
            tool: TOOL_NAME,
            message: format!("ollama inspect_model task panicked: {join_err}"),
        }),
    }
}

/// Synchronous core: locate the manifest file for `model_id`, read + parse
/// it, and project the KV subset. Pure orchestration over the locator +
/// reader + projector helpers; the helpers carry the actual error semantics.
fn build_model_detail(models_root: &Path, model_id: &ModelId) -> Result<ModelDetail, InspectError> {
    let manifests_dir = models_root.join("manifests");
    let manifest_path = locate_manifest_for_id(&manifests_dir, model_id)?;
    let raw =
        std::fs::read_to_string(&manifest_path).map_err(|source| InspectError::FileReadable {
            path: manifest_path.clone(),
            source,
        })?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| InspectError::FormatUnreadable {
            path: manifest_path.clone(),
            detail: format!("manifest JSON parse failed: {e}"),
        })?;

    let (metadata_kv, architecture, parameters_billions) = project_manifest_metadata(&parsed);

    Ok(ModelDetail {
        model_id: model_id.clone(),
        format: Some("Ollama manifest v2".to_string()),
        quantisation: parsed
            .pointer("/config/quantization_level")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        architecture,
        parameters: parameters_billions,
        context_length: parsed
            .pointer("/config/context_length")
            .and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok()),
        metadata_kv,
        introspected_at: Some(std::time::SystemTime::now()),
    })
}

/// Walk `<manifests_dir>` looking for the manifest file whose path translates
/// (via the same `<repo>:<tag>` projection used by `discovery::manifest_id`)
/// into the requested `model_id`. Returns `Err(InspectError::FileReadable)`
/// when no matching manifest exists OR the directory itself is unreadable.
///
/// The locator deliberately re-implements the projection (rather than calling
/// into `discovery.rs`'s private helper) so it can short-circuit on the first
/// match instead of building the full inventory.
fn locate_manifest_for_id(
    manifests_dir: &Path,
    model_id: &ModelId,
) -> Result<PathBuf, InspectError> {
    if !manifests_dir.exists() {
        return Err(InspectError::FileReadable {
            path: manifests_dir.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("ollama manifests dir missing: {}", manifests_dir.display()),
            ),
        });
    }
    for entry in walkdir::WalkDir::new(manifests_dir).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(candidate) = manifest_id_for_path(entry.path(), manifests_dir) {
            if candidate == model_id.as_str() {
                return Ok(entry.path().to_path_buf());
            }
        }
    }
    Err(InspectError::FileReadable {
        path: manifests_dir.join(model_id.as_str()),
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "no Ollama manifest matches model_id {} under {}",
                model_id,
                manifests_dir.display()
            ),
        ),
    })
}

/// Translate a manifest file path (under `<manifests_dir>/<registry>/<repo>/<tag>`)
/// into the canonical `<repo>:<tag>` identifier. Mirrors the projection used by
/// `discovery::manifest_id` — the registry segment is discarded and a literal
/// `library` repo segment is also dropped so ids read as `llama3:8b...` rather
/// than `registry.ollama.ai/library/llama3:8b...`.
fn manifest_id_for_path(manifest: &Path, manifests_root: &Path) -> Option<String> {
    let rel = manifest.strip_prefix(manifests_root).ok()?;
    let segs: Vec<String> = rel
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
    if segs.len() < 3 {
        return None;
    }
    let tag = segs.last().cloned()?;
    let repo_segs = &segs[1..segs.len() - 1];
    if repo_segs.is_empty() {
        return None;
    }
    let filtered: Vec<&str> = repo_segs
        .iter()
        .filter(|s| s.as_str() != "library")
        .map(|s| s.as_str())
        .collect();
    let repo_joined = if filtered.is_empty() {
        repo_segs
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        filtered.join("/")
    };
    Some(format!("{repo_joined}:{tag}"))
}

/// Project the parsed manifest JSON into the small tool-relevant KV subset
/// per AC-22-5 + AC-22-6. The selected keys are the same fields the AC-22-3
/// scenario asserts substring-matches against:
/// - `config.architecture` (e.g., "llama")
/// - `parameters` (the parameter-size string, e.g., "7B")
/// - `template` (excerpt — truncated to ~200 chars so the detail screen does
///   not wrap a 4 KB Jinja template)
/// - `system` (excerpt — same truncation rule as template)
///
/// Also returns the architecture / parameters-billions value-objects so the
/// top-level `ModelDetail` fields render without re-parsing the manifest.
fn project_manifest_metadata(
    parsed: &serde_json::Value,
) -> (BTreeMap<String, String>, Option<String>, Option<f64>) {
    let mut kv: BTreeMap<String, String> = BTreeMap::new();

    let architecture = parsed
        .pointer("/config/architecture")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(arch) = &architecture {
        kv.insert("config.architecture".to_string(), arch.clone());
    }

    let param_size_str = parsed
        .pointer("/config/parameter_size")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(p) = &param_size_str {
        kv.insert("parameters".to_string(), p.clone());
    }
    let parameters_billions = param_size_str.as_deref().and_then(parse_parameter_size);

    if let Some(template) = parsed.get("template").and_then(|v| v.as_str()) {
        kv.insert(
            "template".to_string(),
            excerpt(template, TEMPLATE_EXCERPT_CHARS),
        );
    }
    if let Some(system) = parsed.get("system").and_then(|v| v.as_str()) {
        kv.insert(
            "system".to_string(),
            excerpt(system, TEMPLATE_EXCERPT_CHARS),
        );
    }

    // AC-22-6 budget enforcement (defensive): never emit more than the cap.
    // The current selection emits at most 4 keys, so this is a no-op today;
    // it guards against future field additions that would silently overshoot.
    while kv.len() > METADATA_MAX_KEYS {
        if let Some((k, _)) = kv.iter().next_back().map(|(k, v)| (k.clone(), v.clone())) {
            kv.remove(&k);
        }
    }

    (kv, architecture, parameters_billions)
}

/// Truncate a multi-line string to `max_chars` Unicode characters, appending
/// an ellipsis when truncation occurs. The detail-screen renderer prints the
/// value on a single line, so we also collapse interior newlines to spaces
/// so the excerpt stays compact.
fn excerpt(raw: &str, max_chars: usize) -> String {
    let collapsed: String = raw
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    if collapsed.chars().count() <= max_chars {
        return collapsed;
    }
    let prefix: String = collapsed.chars().take(max_chars).collect();
    format!("{prefix}…")
}

/// Best-effort parse of Ollama's `parameter_size` string (e.g., `"7B"`,
/// `"13B"`, `"70B"`, `"7.24B"`) into a `f64` count-of-billions. Returns
/// `None` on any parse failure — the cache field stays `None` and the
/// renderer falls back to `(not detectable)` for that slot. Suffix handling:
/// `B` / `b` => parameters as-billions; `M` / `m` => parameters/1000.
fn parse_parameter_size(raw: &str) -> Option<f64> {
    let trimmed = raw.trim();
    let (num_str, scale): (&str, f64) = if let Some(stripped) = trimmed.strip_suffix(['B', 'b']) {
        (stripped, 1.0)
    } else if let Some(stripped) = trimmed.strip_suffix(['M', 'm']) {
        (stripped, 0.001)
    } else {
        (trimmed, 1.0)
    };
    num_str.trim().parse::<f64>().ok().map(|n| n * scale)
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

    // -----------------------------------------------------------------------
    // inspect_model — projection / locator / size-parser unit coverage
    // (step 03-02 part 1/N).
    // -----------------------------------------------------------------------

    const SAMPLE_MANIFEST: &str = r#"{
  "schemaVersion": 2,
  "config": {
    "architecture": "llama",
    "parameter_size": "7B",
    "quantization_level": "Q4_K_M"
  },
  "template": "{{ .System }}\nUser: {{ .Prompt }}\n",
  "system": "You are a helpful assistant."
}"#;

    #[test]
    fn project_manifest_metadata_emits_expected_keys_within_budget() {
        let parsed: serde_json::Value = serde_json::from_str(SAMPLE_MANIFEST).unwrap();
        let (kv, arch, params) = project_manifest_metadata(&parsed);
        assert_eq!(arch.as_deref(), Some("llama"));
        assert_eq!(params, Some(7.0));
        assert!(
            kv.len() <= METADATA_MAX_KEYS,
            "metadata_kv must be ≤ {METADATA_MAX_KEYS} keys per AC-22-6; got {}",
            kv.len()
        );
        assert_eq!(
            kv.get("config.architecture").map(|s| s.as_str()),
            Some("llama")
        );
        assert_eq!(kv.get("parameters").map(|s| s.as_str()), Some("7B"));
        // template excerpt should appear (newlines collapsed to spaces).
        let tmpl = kv.get("template").expect("template key");
        assert!(
            tmpl.contains("{{ .System }}"),
            "template excerpt must contain the literal opening tag; got {tmpl}"
        );
        assert!(
            !tmpl.contains('\n'),
            "template excerpt must collapse newlines to spaces; got {tmpl}"
        );
        // system excerpt should appear.
        let sys = kv.get("system").expect("system key");
        assert!(sys.contains("helpful"));
    }

    #[test]
    fn project_manifest_metadata_handles_missing_fields_gracefully() {
        let parsed: serde_json::Value = serde_json::from_str(r#"{"schemaVersion":2}"#).unwrap();
        let (kv, arch, params) = project_manifest_metadata(&parsed);
        assert!(kv.is_empty(), "absent fields => empty kv");
        assert_eq!(arch, None);
        assert_eq!(params, None);
    }

    #[test]
    fn parse_parameter_size_handles_billions_suffix() {
        assert_eq!(parse_parameter_size("7B"), Some(7.0));
        assert_eq!(parse_parameter_size("13b"), Some(13.0));
        assert_eq!(parse_parameter_size("7.24B"), Some(7.24));
    }

    #[test]
    fn parse_parameter_size_handles_millions_suffix() {
        let v = parse_parameter_size("125M").unwrap();
        assert!((v - 0.125).abs() < 1e-9);
    }

    #[test]
    fn parse_parameter_size_returns_none_on_garbage() {
        assert_eq!(parse_parameter_size("unknown"), None);
        assert_eq!(parse_parameter_size(""), None);
    }

    #[test]
    fn excerpt_truncates_long_template_and_collapses_newlines() {
        let long = "a\nb\nc".repeat(200);
        let e = excerpt(&long, 50);
        assert!(
            e.chars().count() <= 51,
            "excerpt must respect cap +1 for ellipsis"
        );
        assert!(!e.contains('\n'), "newlines must be collapsed");
        assert!(e.ends_with('…'), "long input must end with ellipsis");
    }

    #[test]
    fn excerpt_passes_short_strings_through_unchanged_modulo_newlines() {
        assert_eq!(excerpt("hello", 50), "hello");
        assert_eq!(excerpt("a\nb", 50), "a b");
    }

    #[test]
    fn manifest_id_for_path_matches_discovery_projection() {
        let manifests_root = Path::new("/x/.ollama/models/manifests");
        let manifest = Path::new(
            "/x/.ollama/models/manifests/registry.ollama.ai/library/llama3/8b-instruct-q4_K_M",
        );
        let id = manifest_id_for_path(manifest, manifests_root).expect("id");
        assert_eq!(id, "llama3:8b-instruct-q4_K_M");
    }

    #[test]
    fn build_model_detail_reads_manifest_and_returns_format_label() {
        let temp = tempfile::tempdir().unwrap();
        let manifests = temp
            .path()
            .join("manifests")
            .join("registry.ollama.ai")
            .join("library")
            .join("llama3");
        std::fs::create_dir_all(&manifests).unwrap();
        std::fs::write(manifests.join("8b-instruct-q4_K_M"), SAMPLE_MANIFEST).unwrap();
        let id = ModelId::from("llama3:8b-instruct-q4_K_M");
        let detail = build_model_detail(temp.path(), &id).expect("build_model_detail succeeds");
        assert_eq!(detail.format.as_deref(), Some("Ollama manifest v2"));
        assert_eq!(detail.architecture.as_deref(), Some("llama"));
        assert_eq!(detail.quantisation.as_deref(), Some("Q4_K_M"));
        assert!(
            detail.metadata_kv.contains_key("config.architecture"),
            "metadata_kv must include config.architecture"
        );
    }

    #[test]
    fn build_model_detail_returns_file_readable_when_id_not_found() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("manifests")).unwrap();
        let id = ModelId::from("does-not-exist:tag");
        let err = build_model_detail(temp.path(), &id).expect_err("must error");
        assert!(matches!(err, InspectError::FileReadable { .. }));
    }

    #[test]
    fn build_model_detail_returns_format_unreadable_on_malformed_json() {
        let temp = tempfile::tempdir().unwrap();
        let manifests = temp
            .path()
            .join("manifests")
            .join("registry.ollama.ai")
            .join("library")
            .join("broken");
        std::fs::create_dir_all(&manifests).unwrap();
        std::fs::write(manifests.join("tag"), "{ not json").unwrap();
        let id = ModelId::from("broken:tag");
        let err = build_model_detail(temp.path(), &id).expect_err("must error");
        assert!(matches!(err, InspectError::FormatUnreadable { .. }));
    }
}
