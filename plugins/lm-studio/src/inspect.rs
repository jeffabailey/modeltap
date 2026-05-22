//! LM Studio `inspect_model` override (US-22 step 03-02 part 3/N).
//!
//! ## Detection strategy
//!
//! LM Studio stores every model as `.gguf` under a path-style layout
//! `<root>/<org>/<repo>/<file>.gguf` (per `plugins/lm-studio/src/discover.rs`).
//! For inspect we open the file at the path derived from the `model_id` (the
//! same `<org>/<repo>/<filename>` projection the discover walker emits) and
//! parse the GGUF v3 header via
//! [`modeltap_core::domain::gguf::parse_header`].
//!
//! If the resolved on-disk path is a directory rather than a `.gguf` file
//! we fall back to reading `model.json` inside that directory — some
//! older LM Studio layouts ship a JSON sidecar alongside the binary.
//!
//! ## KV selection (≤ 10 per AC-22-6)
//!
//! The plugin emits at most 10 KVs into `ModelDetail.metadata_kv`. From the
//! parsed GGUF header it forwards the standard subset the model-detail
//! screen renders:
//!
//! - `general.architecture` (string)
//! - `general.quantization_version` (string)
//! - `<arch>.context_length` (uint32)
//! - `<arch>.embedding_length` (uint32)
//! - `<arch>.block_count` (uint32)
//! - `tokenizer.ggml.model` (string)
//!
//! `<arch>.*` keys are emitted verbatim with whatever architecture the file
//! advertises (e.g. `llama.context_length`, `mistral.context_length`); the
//! current selection emits at most 6 keys, so the cap holds with room.
//!
//! ## Error mapping
//!
//! - File missing / unreadable → `Err(InspectError::FileReadable)`
//! - Header parse fails (bad magic, truncated, unsupported version,
//!   malformed metadata) → `Err(InspectError::FormatUnreadable)`
//! - Sibling JSON fallback path same shape: missing → `FileReadable`,
//!   malformed JSON → `FormatUnreadable`.
//!
//! Never panics — the trait-contract harness in
//! [[crates/modeltap-core/src/tests/inspect.rs]] verifies the
//! panic-isolation invariant for every plugin including this one.
//!
//! ## Object-Calisthenics scope
//!
//! Adapter side of the hexagon (per ADR-001 plugins live outside the core).
//! Strict OC rules are relaxed here — the file bridges real I/O + JSON
//! parsing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use modeltap_core::domain::gguf;
use modeltap_core::domain::inspect::{InspectError, ModelDetail, ModelId};

use crate::TOOL_NAME;

/// Maximum number of KV entries the LM Studio plugin emits into
/// `metadata_kv` per AC-22-6 (≤ 10 keys per plugin, tool-relevant subset, not
/// the entire GGUF header). Mirrors the Ollama and HF plugins'
/// `METADATA_MAX_KEYS`. Current selection emits at most 6.
const METADATA_MAX_KEYS: usize = 10;

/// GGUF header keys the LM Studio plugin lifts into `ModelDetail.metadata_kv`.
/// `general.*` are architecture-independent; the `<arch>.*` keys are emitted
/// verbatim under whatever architecture the file declares (we don't rewrite
/// to a canonical prefix — the detail screen renders the raw KV key).
const FORWARDED_GENERAL_KEYS: &[&str] = &[
    "general.architecture",
    "general.quantization_version",
    "tokenizer.ggml.model",
];

/// Suffixes (joined with the file's `general.architecture`) that we lift
/// into `metadata_kv`. E.g. for `general.architecture = "llama"` we forward
/// `llama.context_length`, `llama.embedding_length`, `llama.block_count`.
const FORWARDED_ARCH_SUFFIXES: &[&str] = &[
    "context_length",
    "embedding_length",
    "block_count",
];

/// Async entry point used by `LmStudioPlugin::inspect_model`. Mirrors the
/// HF / Ollama shape: sync core in `spawn_blocking` so the file read +
/// header parse never parks the runtime thread.
///
/// `model_id` follows the LM Studio discovery projection
/// `<org>/<repo>/<filename>` (see `discover::compute_id`). `search_paths` is
/// the resolved list of LM Studio model roots — the locator walks each in
/// declaration order and takes the first existing match.
///
/// Contract per ADR-016 + US-22 acceptance criteria:
/// - `Ok(ModelDetail)` with `format = Some("GGUF v3")` (or the sibling
///   `"LM Studio model.json"` for the directory-sidecar fallback) and
///   `metadata_kv` populated with the tool-relevant subset.
/// - `Err(InspectError::FileReadable)` when the path is missing or unreadable.
/// - `Err(InspectError::FormatUnreadable)` when the header / JSON parse fails.
/// - Never panics.
pub async fn inspect_model_impl(
    search_paths: Vec<PathBuf>,
    model_id: ModelId,
) -> Result<ModelDetail, InspectError> {
    let join = tokio::task::spawn_blocking(move || {
        build_model_detail(&search_paths, &model_id)
    })
    .await;
    match join {
        Ok(res) => res,
        Err(join_err) => Err(InspectError::PluginPanic {
            tool: TOOL_NAME,
            message: format!("lm-studio inspect_model task panicked: {join_err}"),
        }),
    }
}

/// Synchronous core: locate the on-disk artefact, dispatch on whether it's a
/// `.gguf` file or a directory-sidecar (`model.json`), and project the KV
/// subset into a `ModelDetail`.
fn build_model_detail(
    search_paths: &[PathBuf],
    model_id: &ModelId,
) -> Result<ModelDetail, InspectError> {
    let path = locate_artifact(search_paths, model_id)?;
    if path.is_dir() {
        return build_from_model_json(&path, model_id);
    }
    build_from_gguf(&path, model_id)
}

/// Walk each search path looking for `<root>/<model_id>`. Returns the first
/// existing path (file OR directory). When `model_id` carries no leading
/// path separators, the join is `<root>/<model_id>`.
fn locate_artifact(
    search_paths: &[PathBuf],
    model_id: &ModelId,
) -> Result<PathBuf, InspectError> {
    let id = model_id.as_str();
    for root in search_paths {
        let candidate = root.join(id);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let probed = search_paths
        .iter()
        .map(|p| p.join(id))
        .collect::<Vec<_>>();
    let first = probed.first().cloned().unwrap_or_else(|| PathBuf::from(id));
    Err(InspectError::FileReadable {
        path: first,
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("lm-studio: no search path contains model_id {id}"),
        ),
    })
}

/// Parse `path` as a GGUF v3 file and project the header KVs into a
/// `ModelDetail`. Per the file-format contract:
/// - I/O errors map to `Err(InspectError::FileReadable)`.
/// - Parse errors map to `Err(InspectError::FormatUnreadable)`.
fn build_from_gguf(path: &Path, model_id: &ModelId) -> Result<ModelDetail, InspectError> {
    let header = gguf::parse_header(path).map_err(|e| map_gguf_error(path, e))?;
    let architecture = header
        .metadata_kv
        .get("general.architecture")
        .cloned();
    let quantisation = header
        .metadata_kv
        .get("general.quantization_version")
        .cloned();
    let context_length = header
        .metadata_kv
        .iter()
        .find(|(k, _)| k.ends_with(".context_length"))
        .and_then(|(_, v)| v.parse::<u32>().ok());

    let metadata_kv = project_gguf_kvs(&header.metadata_kv, architecture.as_deref());

    Ok(ModelDetail {
        model_id: model_id.clone(),
        format: Some(header.format),
        quantisation,
        architecture,
        parameters: None,
        context_length,
        metadata_kv,
        introspected_at: Some(std::time::SystemTime::now()),
    })
}

/// Map a `GgufParseError` into the appropriate `InspectError`.
fn map_gguf_error(path: &Path, e: gguf::GgufParseError) -> InspectError {
    match e {
        gguf::GgufParseError::Io(source) => InspectError::FileReadable {
            path: path.to_path_buf(),
            source,
        },
        other => InspectError::FormatUnreadable {
            path: path.to_path_buf(),
            detail: format!("gguf header parse failed: {other}"),
        },
    }
}

/// Lift the GGUF KVs into the small tool-relevant subset per AC-22-6. The
/// selection is:
/// - every key in [`FORWARDED_GENERAL_KEYS`] (architecture-independent)
/// - every `<arch>.<suffix>` for `suffix in FORWARDED_ARCH_SUFFIXES` and
///   the file's `general.architecture` value (when present)
///
/// The cap is enforced defensively after projection so future field
/// additions stay within budget.
fn project_gguf_kvs(
    header: &BTreeMap<String, String>,
    architecture: Option<&str>,
) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for key in FORWARDED_GENERAL_KEYS {
        if let Some(v) = header.get(*key) {
            out.insert((*key).to_string(), v.clone());
        }
    }
    if let Some(arch) = architecture {
        for suffix in FORWARDED_ARCH_SUFFIXES {
            let key = format!("{arch}.{suffix}");
            if let Some(v) = header.get(&key) {
                out.insert(key, v.clone());
            }
        }
    }
    // Defensive cap — pop lex-last keys until we're within budget. Today's
    // selection emits ≤ 6, so this is a no-op; it guards future expansions.
    while out.len() > METADATA_MAX_KEYS {
        if let Some((k, _)) = out.iter().next_back().map(|(k, v)| (k.clone(), v.clone())) {
            out.remove(&k);
        }
    }
    out
}

/// Read `<dir>/model.json` and project a small KV subset. LM Studio's
/// older directory layout ships this sidecar alongside the model binary; the
/// JSON schema is loose (any string-valued top-level fields, no required
/// keys), so we project every string-typed top-level value with a stable
/// key→string map.
///
/// Per the inspect_model contract: missing file → `FileReadable`; malformed
/// JSON → `FormatUnreadable`. Never panics.
fn build_from_model_json(dir: &Path, model_id: &ModelId) -> Result<ModelDetail, InspectError> {
    let path = dir.join("model.json");
    let raw = std::fs::read_to_string(&path).map_err(|source| InspectError::FileReadable {
        path: path.clone(),
        source,
    })?;
    let parsed: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| InspectError::FormatUnreadable {
            path: path.clone(),
            detail: format!("lm-studio model.json parse failed: {e}"),
        })?;

    let (metadata_kv, architecture, quantisation, context_length) =
        project_model_json(&parsed);

    Ok(ModelDetail {
        model_id: model_id.clone(),
        format: Some("LM Studio model.json".to_string()),
        quantisation,
        architecture,
        parameters: None,
        context_length,
        metadata_kv,
        introspected_at: Some(std::time::SystemTime::now()),
    })
}

/// Project a parsed `model.json` into the tool-relevant KV subset. LM Studio's
/// `model.json` shape is loose — we surface known string fields verbatim and
/// look for `arch` / `architecture` (architecture), `quantization` (quant),
/// `context_length` / `n_ctx` (context length, integer).
fn project_model_json(
    parsed: &serde_json::Value,
) -> (
    BTreeMap<String, String>,
    Option<String>,
    Option<String>,
    Option<u32>,
) {
    let mut kv: BTreeMap<String, String> = BTreeMap::new();

    let architecture = parsed
        .get("arch")
        .and_then(|v| v.as_str())
        .or_else(|| parsed.get("architecture").and_then(|v| v.as_str()))
        .map(|s| s.to_string());
    if let Some(arch) = &architecture {
        kv.insert("arch".to_string(), arch.clone());
    }

    let quantisation = parsed
        .get("quantization")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(q) = &quantisation {
        kv.insert("quantization".to_string(), q.clone());
    }

    let context_length = parsed
        .get("context_length")
        .and_then(|v| v.as_u64())
        .or_else(|| parsed.get("n_ctx").and_then(|v| v.as_u64()))
        .and_then(|n| u32::try_from(n).ok());
    if let Some(n) = context_length {
        kv.insert("context_length".to_string(), n.to_string());
    }

    if let Some(name) = parsed.get("name").and_then(|v| v.as_str()) {
        kv.insert("name".to_string(), name.to_string());
    }
    if let Some(version) = parsed.get("version").and_then(|v| v.as_str()) {
        kv.insert("version".to_string(), version.to_string());
    }

    while kv.len() > METADATA_MAX_KEYS {
        if let Some((k, _)) = kv.iter().next_back().map(|(k, v)| (k.clone(), v.clone())) {
            kv.remove(&k);
        }
    }

    (kv, architecture, quantisation, context_length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn header_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        for (k, v) in pairs {
            out.insert((*k).to_string(), (*v).to_string());
        }
        out
    }

    #[test]
    fn project_gguf_kvs_emits_general_keys_when_present() {
        let header = header_map(&[
            ("general.architecture", "llama"),
            ("general.quantization_version", "Q4_K_M"),
            ("tokenizer.ggml.model", "llama"),
            ("llama.context_length", "4096"),
            ("llama.embedding_length", "4096"),
            ("llama.block_count", "32"),
            ("unused.key", "ignored"),
        ]);
        let out = project_gguf_kvs(&header, Some("llama"));
        assert!(
            out.len() <= METADATA_MAX_KEYS,
            "kv count must be ≤ {METADATA_MAX_KEYS}; got {}",
            out.len()
        );
        assert_eq!(
            out.get("general.architecture").map(|s| s.as_str()),
            Some("llama")
        );
        assert_eq!(
            out.get("general.quantization_version").map(|s| s.as_str()),
            Some("Q4_K_M")
        );
        assert_eq!(
            out.get("llama.context_length").map(|s| s.as_str()),
            Some("4096")
        );
        assert!(
            !out.contains_key("unused.key"),
            "unused keys must NOT be projected"
        );
    }

    #[test]
    fn project_gguf_kvs_handles_missing_architecture_gracefully() {
        let header = header_map(&[("general.quantization_version", "Q4_K_M")]);
        let out = project_gguf_kvs(&header, None);
        // Only general.quantization_version forwarded; no arch suffix lookup.
        assert_eq!(out.len(), 1);
        assert!(out.contains_key("general.quantization_version"));
    }

    #[test]
    fn locate_artifact_returns_first_existing_match() {
        let temp = tempfile::tempdir().unwrap();
        let root_a = temp.path().join("a");
        let root_b = temp.path().join("b");
        std::fs::create_dir_all(&root_a).unwrap();
        std::fs::create_dir_all(&root_b).unwrap();
        std::fs::write(root_b.join("model.gguf"), b"GGUF").unwrap();
        let id = ModelId::from("model.gguf");
        let found = locate_artifact(&[root_a.clone(), root_b.clone()], &id).unwrap();
        assert_eq!(found, root_b.join("model.gguf"));
    }

    #[test]
    fn locate_artifact_returns_file_readable_when_absent_from_every_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        let id = ModelId::from("missing.gguf");
        let err = locate_artifact(&[root.clone()], &id).unwrap_err();
        assert!(matches!(err, InspectError::FileReadable { .. }));
    }

    #[test]
    fn build_from_gguf_returns_ok_for_valid_header() {
        // Synthesise a minimal GGUF v3 file under a tempdir.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        let path = root.join("org/repo/model.gguf");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let bytes = build_minimal_gguf_v3();
        std::fs::write(&path, &bytes).unwrap();

        let id = ModelId::from("org/repo/model.gguf");
        let detail =
            build_model_detail(std::slice::from_ref(&root), &id).expect("inspect must succeed");
        assert_eq!(detail.format.as_deref(), Some("GGUF v3"));
        assert_eq!(detail.architecture.as_deref(), Some("llama"));
        assert_eq!(detail.quantisation.as_deref(), Some("Q4_K_M"));
        assert_eq!(detail.context_length, Some(4096));
        assert!(
            detail.metadata_kv.contains_key("general.architecture"),
            "metadata_kv must include general.architecture"
        );
    }

    #[test]
    fn build_from_gguf_maps_bad_magic_to_format_unreadable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        let path = root.join("bad.gguf");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"NOTAGGUFFILEXXXXX").unwrap();
        let id = ModelId::from("bad.gguf");
        let err =
            build_model_detail(std::slice::from_ref(&root), &id).expect_err("must error");
        assert!(
            matches!(err, InspectError::FormatUnreadable { .. }),
            "bad magic must map to FormatUnreadable; got {err:?}"
        );
    }

    #[test]
    fn build_from_model_json_reads_sibling_json_when_artifact_is_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        let dir = root.join("org/repo/model-dir");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("model.json"),
            r#"{
                "arch": "mistral",
                "quantization": "Q5_K_M",
                "context_length": 8192,
                "name": "Mistral-7B"
            }"#,
        )
        .unwrap();
        let id = ModelId::from("org/repo/model-dir");
        let detail =
            build_model_detail(std::slice::from_ref(&root), &id).expect("inspect must succeed");
        assert_eq!(detail.format.as_deref(), Some("LM Studio model.json"));
        assert_eq!(detail.architecture.as_deref(), Some("mistral"));
        assert_eq!(detail.quantisation.as_deref(), Some("Q5_K_M"));
        assert_eq!(detail.context_length, Some(8192));
        assert_eq!(
            detail.metadata_kv.get("name").map(|s| s.as_str()),
            Some("Mistral-7B")
        );
    }

    #[test]
    fn build_from_model_json_maps_malformed_json_to_format_unreadable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        let dir = root.join("model-dir");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("model.json"), "{ not json").unwrap();
        let id = ModelId::from("model-dir");
        let err =
            build_model_detail(std::slice::from_ref(&root), &id).expect_err("must error");
        assert!(
            matches!(err, InspectError::FormatUnreadable { .. }),
            "malformed json must map to FormatUnreadable; got {err:?}"
        );
    }

    /// Build a minimal valid GGUF v3 header with the standard five KVs the
    /// devon_mistral_gguf_fixture seeds. Shared with the fixture under
    /// `tests/src/fixtures/inspect_fixtures.rs::write_gguf_v3_header` (kept
    /// duplicated rather than re-exported because the test-only helper would
    /// otherwise leak into the plugin's public surface).
    fn build_minimal_gguf_v3() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"GGUF");
        out.extend_from_slice(&3u32.to_le_bytes()); // version
        out.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        out.extend_from_slice(&5u64.to_le_bytes()); // kv_count

        // KV[0] general.architecture = "llama"
        write_string_kv(&mut out, "general.architecture", "llama");
        // KV[1] general.quantization_version = "Q4_K_M"
        write_string_kv(&mut out, "general.quantization_version", "Q4_K_M");
        // KV[2] llama.context_length = 4096 (u32)
        write_u32_kv(&mut out, "llama.context_length", 4096);
        // KV[3] llama.embedding_length = 4096 (u32)
        write_u32_kv(&mut out, "llama.embedding_length", 4096);
        // KV[4] tokenizer.ggml.model = "llama"
        write_string_kv(&mut out, "tokenizer.ggml.model", "llama");
        out
    }

    fn write_string_kv(out: &mut Vec<u8>, key: &str, value: &str) {
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(&8u32.to_le_bytes()); // TYPE_STRING
        out.extend_from_slice(&(value.len() as u64).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }

    fn write_u32_kv(out: &mut Vec<u8>, key: &str, value: u32) {
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(&4u32.to_le_bytes()); // TYPE_UINT32
        out.extend_from_slice(&value.to_le_bytes());
    }
}
