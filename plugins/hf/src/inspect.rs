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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use modeltap_core::domain::inspect::{
    InspectError, ModelDetail, ModelId, SearchPathEntry, SearchPathSource, ToolDetail,
};
use modeltap_core::ToolId;

use crate::TOOL_NAME;

/// Maximum number of KV entries the HF plugin emits into `metadata_kv` per
/// AC-22-6 ("≤10 keys per plugin, tool-relevant subset, not entire
/// `config.json`"). The current selection emits at most 6 keys (model_type,
/// architectures, hidden_size, num_attention_heads, num_hidden_layers,
/// max_position_embeddings); the bound is enforced at the construction
/// boundary so future field additions stay within budget without a separate
/// audit. Mirrors the Ollama plugin's `METADATA_MAX_KEYS`.
const METADATA_MAX_KEYS: usize = 10;

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

// ---------------------------------------------------------------------------
// inspect_model: read <hub>/models--<org>--<repo>/snapshots/<rev>/config.json
// and project a small tool-relevant KV subset for the model-detail screen.
// Step 03-02 part 2/N (US-22 AC-22-3..AC-22-6, HF half).
// ---------------------------------------------------------------------------

/// Async entry point used by `HfPlugin::inspect_model`. Mirrors the
/// `inspect_tool_impl` shape (sync core in `spawn_blocking`) so the JSON
/// read + parse never parks the runtime thread.
///
/// Contract per ADR-016 + US-22 acceptance criteria:
/// - `Ok(ModelDetail)` with `format = Some("safetensors v2")` (the canonical
///   HF on-disk format label) and `metadata_kv` populated with the
///   tool-relevant subset (`model_type`, `architectures`, `hidden_size`,
///   `num_attention_heads`, `num_hidden_layers`, `max_position_embeddings`)
///   when `config.json` is readable and parseable.
/// - `Err(InspectError::FileReadable { path, source })` when `config.json`
///   is missing or the snapshot directory cannot be located for `model_id`.
/// - `Err(InspectError::FormatUnreadable { path, detail })` when the file
///   is readable but its JSON is malformed.
/// - `metadata_kv` MUST be ≤ 10 keys per AC-22-6 (enforced at construction).
/// - Never panics (AC-22-7 trait invariant; verified by the step 02-03
///   plugin-contract harness).
pub async fn inspect_model_impl(
    hub_root: PathBuf,
    model_id: ModelId,
) -> Result<ModelDetail, InspectError> {
    let join =
        tokio::task::spawn_blocking(move || build_model_detail(&hub_root, &model_id)).await;
    match join {
        Ok(res) => res,
        Err(join_err) => Err(InspectError::PluginPanic {
            tool: TOOL_NAME,
            message: format!("hf inspect_model task panicked: {join_err}"),
        }),
    }
}

/// Synchronous core: locate the `config.json` for `model_id`, read + parse
/// it, project the KV subset, and lift the typed `ModelDetail` fields. Pure
/// orchestration over the locator + reader + projector helpers; the helpers
/// carry the actual error semantics.
fn build_model_detail(
    hub_root: &Path,
    model_id: &ModelId,
) -> Result<ModelDetail, InspectError> {
    let config_path = locate_config_json_for_id(hub_root, model_id)?;
    let raw = std::fs::read_to_string(&config_path).map_err(|source| {
        InspectError::FileReadable {
            path: config_path.clone(),
            source,
        }
    })?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| InspectError::FormatUnreadable {
            path: config_path.clone(),
            detail: format!("hf config.json parse failed: {e}"),
        })?;

    let (metadata_kv, architecture, context_length, parameters_billions) =
        project_config_metadata(&parsed);

    Ok(ModelDetail {
        model_id: model_id.clone(),
        format: Some("safetensors v2".to_string()),
        quantisation: None,
        architecture,
        parameters: parameters_billions,
        context_length,
        metadata_kv,
        introspected_at: Some(std::time::SystemTime::now()),
    })
}

/// Locate `<hub>/models--<org>--<repo>/snapshots/<rev>/config.json` for the
/// requested `model_id`.
///
/// `model_id` follows the HF discovery projection `<org>/<repo>[/<filename>]`
/// (see `discover::build_discovered_model`). The locator strips any trailing
/// `/<filename>` segment (config.json lives once per snapshot, not per
/// artifact), reconstructs the `models--<org>--<repo>` directory name, picks
/// the snapshot revision via `refs/main` if present (else the lexicographically
/// first snapshot subdirectory), and joins `config.json`.
///
/// Returns `Err(InspectError::FileReadable)` when any layer is missing
/// (model dir absent, snapshots dir empty, refs/main + lex fallback both
/// produce no candidate, `config.json` missing). The caller maps the
/// subsequent `std::fs::read_to_string` error onto the same variant when the
/// file is found but unreadable.
fn locate_config_json_for_id(
    hub_root: &Path,
    model_id: &ModelId,
) -> Result<PathBuf, InspectError> {
    let id = model_id.as_str();
    let (org, repo) = parse_org_repo_from_model_id(id).ok_or_else(|| InspectError::FileReadable {
        path: hub_root.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("hf model_id {id} does not have <org>/<repo> shape"),
        ),
    })?;
    let model_dir = hub_root.join(format!("models--{org}--{repo}"));
    if !model_dir.exists() {
        return Err(InspectError::FileReadable {
            path: model_dir.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "hf model dir missing for {id}: {}",
                    model_dir.display()
                ),
            ),
        });
    }
    let snapshot_dir = resolve_snapshot_dir(&model_dir).ok_or_else(|| InspectError::FileReadable {
        path: model_dir.join("snapshots"),
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "hf snapshot dir missing or empty under {}",
                model_dir.display()
            ),
        ),
    })?;
    let config_path = snapshot_dir.join("config.json");
    if !config_path.exists() {
        return Err(InspectError::FileReadable {
            path: config_path.clone(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("hf config.json missing at {}", config_path.display()),
            ),
        });
    }
    Ok(config_path)
}

/// Parse `<org>/<repo>` (the first two path segments) out of an HF
/// `model_id`. Returns `None` if the id does not carry both segments.
///
/// Examples:
/// - `"mistralai/Mistral-7B-v0.1/model.safetensors"` → `Some(("mistralai", "Mistral-7B-v0.1"))`
/// - `"mistralai/Mistral-7B-v0.1"` → `Some(("mistralai", "Mistral-7B-v0.1"))`
/// - `"mistralai"` → `None`
/// - `""` → `None`
fn parse_org_repo_from_model_id(model_id: &str) -> Option<(&str, &str)> {
    let mut parts = model_id.splitn(3, '/');
    let org = parts.next()?;
    let repo = parts.next()?;
    if org.is_empty() || repo.is_empty() {
        return None;
    }
    Some((org, repo))
}

/// Pick a snapshot directory under `<model_dir>/snapshots/`. Priority:
/// 1. `<model_dir>/refs/main` content (HF revision pointer) — if it exists
///    AND names a snapshot subdirectory that exists, use that snapshot.
/// 2. Else: the lexicographically first snapshot subdirectory.
/// 3. Else: `None` (no snapshot present).
fn resolve_snapshot_dir(model_dir: &Path) -> Option<PathBuf> {
    let snapshots = model_dir.join("snapshots");
    if !snapshots.exists() {
        return None;
    }
    if let Some(rev) = read_refs_main(model_dir) {
        let candidate = snapshots.join(&rev);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(&snapshots)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();
    subdirs.into_iter().next()
}

/// Read `<model_dir>/refs/main` and return its trimmed content. The file is
/// a small text file holding the snapshot revision (sha). Returns `None` if
/// the file is absent or unreadable.
fn read_refs_main(model_dir: &Path) -> Option<String> {
    let path = model_dir.join("refs").join("main");
    let raw = std::fs::read_to_string(&path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Project the parsed `config.json` into the small tool-relevant KV subset
/// per AC-22-5 + AC-22-6. The selected keys are the same fields the AC-22-3
/// HF scenario asserts substring-matches against:
/// - `model_type` (e.g., "mistral")
/// - `architectures` (Vec → comma-joined, e.g., "MistralForCausalLM")
/// - `hidden_size` (integer)
/// - `num_attention_heads` (integer)
/// - `num_hidden_layers` (integer)
/// - `max_position_embeddings` (integer, optional)
///
/// Also returns the typed `ModelDetail` fields the top-level struct surfaces:
/// `architecture` (first entry in `architectures`), `context_length`
/// (from `max_position_embeddings` clamped to u32), and `parameters_billions`
/// (best-effort estimate from hidden_size + num_hidden_layers; returns `None`
/// when the inputs are absent — the renderer falls back to `(not detectable)`).
fn project_config_metadata(
    parsed: &serde_json::Value,
) -> (
    BTreeMap<String, String>,
    Option<String>,
    Option<u32>,
    Option<f64>,
) {
    let mut kv: BTreeMap<String, String> = BTreeMap::new();

    if let Some(model_type) = parsed.get("model_type").and_then(|v| v.as_str()) {
        kv.insert("model_type".to_string(), model_type.to_string());
    }

    let architectures_joined = parsed
        .get("architectures")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|s| !s.is_empty());
    if let Some(s) = &architectures_joined {
        kv.insert("architectures".to_string(), s.clone());
    }
    // The typed `architecture` field carries the FIRST entry (most callers
    // treat `architectures` as a singleton in practice).
    let architecture = parsed
        .get("architectures")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find_map(|v| v.as_str().map(|s| s.to_string())));

    let hidden_size = parsed.get("hidden_size").and_then(|v| v.as_u64());
    if let Some(n) = hidden_size {
        kv.insert("hidden_size".to_string(), n.to_string());
    }

    let num_attention_heads = parsed.get("num_attention_heads").and_then(|v| v.as_u64());
    if let Some(n) = num_attention_heads {
        kv.insert("num_attention_heads".to_string(), n.to_string());
    }

    let num_hidden_layers = parsed.get("num_hidden_layers").and_then(|v| v.as_u64());
    if let Some(n) = num_hidden_layers {
        kv.insert("num_hidden_layers".to_string(), n.to_string());
    }

    let max_position_embeddings = parsed
        .get("max_position_embeddings")
        .and_then(|v| v.as_u64());
    if let Some(n) = max_position_embeddings {
        kv.insert("max_position_embeddings".to_string(), n.to_string());
    }
    let context_length = max_position_embeddings.and_then(|n| u32::try_from(n).ok());

    // AC-22-6 budget enforcement (defensive): never emit more than the cap.
    // The current selection emits at most 6 keys, so this is a no-op today;
    // it guards against future field additions that would silently overshoot.
    while kv.len() > METADATA_MAX_KEYS {
        if let Some((k, _)) = kv.iter().next_back().map(|(k, v)| (k.clone(), v.clone())) {
            kv.remove(&k);
        }
    }

    // Best-effort parameters-billions estimate. Transformer parameter count
    // dominated by the attention + MLP blocks scales roughly as
    // `12 * num_hidden_layers * hidden_size^2`. The estimate is intentionally
    // coarse — the renderer reads it as `(not detectable)` when None — and is
    // here so the cache row carries a numeric value alongside the raw KV
    // strings without invoking the full safetensors-shard byte-count walk.
    let parameters_billions = match (hidden_size, num_hidden_layers) {
        (Some(h), Some(l)) if h > 0 && l > 0 => {
            let approx = 12.0 * (l as f64) * (h as f64) * (h as f64);
            Some(approx / 1.0e9)
        }
        _ => None,
    };

    (kv, architecture, context_length, parameters_billions)
}

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

    // -----------------------------------------------------------------------
    // inspect_model — projection / locator unit coverage (step 03-02 part 2/N).
    // -----------------------------------------------------------------------

    const SAMPLE_CONFIG: &str = r#"{
  "model_type": "mistral",
  "architectures": ["MistralForCausalLM"],
  "hidden_size": 4096,
  "num_attention_heads": 32,
  "num_hidden_layers": 32,
  "max_position_embeddings": 32768,
  "vocab_size": 32000
}"#;

    #[test]
    fn project_config_metadata_emits_expected_keys_within_budget() {
        let parsed: serde_json::Value = serde_json::from_str(SAMPLE_CONFIG).unwrap();
        let (kv, arch, ctx, params) = project_config_metadata(&parsed);
        assert_eq!(arch.as_deref(), Some("MistralForCausalLM"));
        assert_eq!(ctx, Some(32768));
        assert!(params.is_some(), "params estimate must be Some when h+l set");
        assert!(
            kv.len() <= METADATA_MAX_KEYS,
            "metadata_kv must be ≤ {METADATA_MAX_KEYS} keys per AC-22-6; got {}",
            kv.len()
        );
        assert_eq!(kv.get("model_type").map(|s| s.as_str()), Some("mistral"));
        assert_eq!(
            kv.get("architectures").map(|s| s.as_str()),
            Some("MistralForCausalLM")
        );
        assert_eq!(kv.get("hidden_size").map(|s| s.as_str()), Some("4096"));
        assert_eq!(
            kv.get("num_attention_heads").map(|s| s.as_str()),
            Some("32")
        );
        assert_eq!(
            kv.get("num_hidden_layers").map(|s| s.as_str()),
            Some("32")
        );
        assert_eq!(
            kv.get("max_position_embeddings").map(|s| s.as_str()),
            Some("32768")
        );
    }

    #[test]
    fn project_config_metadata_handles_missing_fields_gracefully() {
        let parsed: serde_json::Value =
            serde_json::from_str(r#"{"vocab_size":32000}"#).unwrap();
        let (kv, arch, ctx, params) = project_config_metadata(&parsed);
        assert!(kv.is_empty(), "absent fields => empty kv");
        assert_eq!(arch, None);
        assert_eq!(ctx, None);
        assert_eq!(params, None);
    }

    #[test]
    fn project_config_metadata_joins_multiple_architectures_with_comma() {
        let parsed: serde_json::Value = serde_json::from_str(
            r#"{"architectures":["MistralForCausalLM","MistralModel"]}"#,
        )
        .unwrap();
        let (kv, arch, _ctx, _params) = project_config_metadata(&parsed);
        assert_eq!(
            kv.get("architectures").map(|s| s.as_str()),
            Some("MistralForCausalLM,MistralModel")
        );
        // typed `architecture` field carries the first entry only.
        assert_eq!(arch.as_deref(), Some("MistralForCausalLM"));
    }

    #[test]
    fn parse_org_repo_from_model_id_strips_filename_segment() {
        assert_eq!(
            parse_org_repo_from_model_id("mistralai/Mistral-7B/model.safetensors"),
            Some(("mistralai", "Mistral-7B"))
        );
        assert_eq!(
            parse_org_repo_from_model_id("mistralai/Mistral-7B"),
            Some(("mistralai", "Mistral-7B"))
        );
        assert_eq!(parse_org_repo_from_model_id("mistralai"), None);
        assert_eq!(parse_org_repo_from_model_id(""), None);
        assert_eq!(parse_org_repo_from_model_id("/repo"), None);
    }

    #[test]
    fn resolve_snapshot_dir_prefers_refs_main_when_present() {
        let temp = tempfile::tempdir().unwrap();
        let model_dir = temp.path().join("models--org--repo");
        let snap_a = model_dir.join("snapshots/aaa");
        let snap_z = model_dir.join("snapshots/zzz");
        std::fs::create_dir_all(&snap_a).unwrap();
        std::fs::create_dir_all(&snap_z).unwrap();
        let refs_dir = model_dir.join("refs");
        std::fs::create_dir_all(&refs_dir).unwrap();
        std::fs::write(refs_dir.join("main"), "zzz\n").unwrap();
        let picked = resolve_snapshot_dir(&model_dir).expect("snapshot resolved");
        assert!(
            picked.ends_with("zzz"),
            "refs/main must take priority over lex order; got {}",
            picked.display()
        );
    }

    #[test]
    fn resolve_snapshot_dir_falls_back_to_lex_first_when_refs_main_absent() {
        let temp = tempfile::tempdir().unwrap();
        let model_dir = temp.path().join("models--org--repo");
        std::fs::create_dir_all(model_dir.join("snapshots/bbb")).unwrap();
        std::fs::create_dir_all(model_dir.join("snapshots/aaa")).unwrap();
        let picked = resolve_snapshot_dir(&model_dir).expect("snapshot resolved");
        assert!(
            picked.ends_with("aaa"),
            "lex-first fallback must pick 'aaa'; got {}",
            picked.display()
        );
    }

    #[test]
    fn resolve_snapshot_dir_returns_none_when_snapshots_dir_absent() {
        let temp = tempfile::tempdir().unwrap();
        let model_dir = temp.path().join("models--org--repo");
        std::fs::create_dir_all(&model_dir).unwrap();
        assert!(resolve_snapshot_dir(&model_dir).is_none());
    }

    #[test]
    fn build_model_detail_reads_config_json_and_returns_format_label() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path();
        let snapshot = hub
            .join("models--mistralai--Mistral-7B")
            .join("snapshots/abc123");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(snapshot.join("config.json"), SAMPLE_CONFIG).unwrap();
        let refs = hub.join("models--mistralai--Mistral-7B").join("refs");
        std::fs::create_dir_all(&refs).unwrap();
        std::fs::write(refs.join("main"), "abc123").unwrap();

        let id = ModelId::from("mistralai/Mistral-7B/model.safetensors");
        let detail = build_model_detail(hub, &id).expect("build_model_detail succeeds");
        assert_eq!(detail.format.as_deref(), Some("safetensors v2"));
        assert_eq!(detail.architecture.as_deref(), Some("MistralForCausalLM"));
        assert_eq!(detail.context_length, Some(32768));
        assert!(
            detail.metadata_kv.contains_key("model_type"),
            "metadata_kv must include model_type"
        );
        assert!(
            detail.metadata_kv.contains_key("architectures"),
            "metadata_kv must include architectures"
        );
    }

    #[test]
    fn build_model_detail_returns_file_readable_when_model_dir_missing() {
        let temp = tempfile::tempdir().unwrap();
        let id = ModelId::from("nobody/nothing");
        let err = build_model_detail(temp.path(), &id).expect_err("must error");
        assert!(
            matches!(err, InspectError::FileReadable { .. }),
            "missing model dir must map to FileReadable; got {err:?}"
        );
    }

    #[test]
    fn build_model_detail_returns_format_unreadable_on_malformed_json() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = temp
            .path()
            .join("models--org--repo")
            .join("snapshots/rev1");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(snapshot.join("config.json"), "{ not json").unwrap();
        let id = ModelId::from("org/repo/model.safetensors");
        let err = build_model_detail(temp.path(), &id).expect_err("must error");
        assert!(
            matches!(err, InspectError::FormatUnreadable { .. }),
            "malformed json must map to FormatUnreadable; got {err:?}"
        );
    }

    #[test]
    fn build_model_detail_returns_file_readable_when_config_json_missing() {
        let temp = tempfile::tempdir().unwrap();
        let snapshot = temp
            .path()
            .join("models--org--repo")
            .join("snapshots/rev1");
        std::fs::create_dir_all(&snapshot).unwrap();
        // No config.json written.
        let id = ModelId::from("org/repo");
        let err = build_model_detail(temp.path(), &id).expect_err("must error");
        assert!(
            matches!(err, InspectError::FileReadable { .. }),
            "missing config.json must map to FileReadable; got {err:?}"
        );
    }
}
