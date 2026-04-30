//! `model.yml` parser for the Atomic Chat plugin.
//!
//! Each Atomic Chat model directory carries a YAML manifest with the shape:
//!
//! ```yaml
//! embedding: false
//! mmproj_path: llamacpp/models/<id>/mmproj.gguf   # optional
//! model_path:  llamacpp/models/<id>/model.gguf
//! name: <model id>
//! size_bytes: 22915306816
//! ```
//!
//! The parser is pure (`bytes -> Result<ModelYml, ParseError>`) so it tests
//! cleanly without filesystem setup. Callers in `discover.rs` open the file
//! and pass the bytes here; they own the I/O error reporting.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed — the parser is a tight transform
//! over bytes.

use serde::Deserialize;
use thiserror::Error;

/// Parsed `model.yml` content. Field names mirror the on-disk YAML keys so
/// `serde_yaml` deserializes directly without a custom visitor.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ModelYml {
    /// Display name (also used as the model id). e.g. `"Qwen3_5-35B-A3B-Q4_K_M"`.
    pub name: String,
    /// Apparent on-disk size in bytes (sum of `model.gguf` + `mmproj.gguf`
    /// when present). Authoritative for the right-pane size column.
    pub size_bytes: u64,
    /// Path to the GGUF model file, RELATIVE to the Atomic Chat data root
    /// (e.g. `~/Library/Application Support/Atomic Chat/data/`).
    pub model_path: String,
    /// Optional path to the multimodal projector. Some vision-capable models
    /// ship one alongside `model.gguf`. Absent for plain LLMs.
    #[serde(default)]
    pub mmproj_path: Option<String>,
    /// `true` when the model is an embedding model (not a chat model). The
    /// modeltap UI doesn't currently distinguish these, but the field is
    /// preserved for forward-compat.
    #[serde(default)]
    pub embedding: bool,
}

/// Error returned when a `model.yml` cannot be parsed. Caller decides whether
/// to surface this as a per-model `Corrupt` marker (preferred — keeps the
/// rest of discovery alive) or to fail the whole discovery.
#[derive(Debug, Error)]
pub enum ParseError {
    /// `serde_yaml` rejected the input (truncated, missing required keys, etc.).
    #[error("invalid model.yml: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Parse a `model.yml` byte buffer. Returns `Err` when the YAML is malformed
/// or missing a required key (`name`, `size_bytes`, `model_path`).
pub fn parse_model_yml(bytes: &[u8]) -> Result<ModelYml, ParseError> {
    let parsed: ModelYml = serde_yaml::from_slice(bytes)?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Behavior 1 — happy path: valid YAML produces a populated `ModelYml`.
    /// Mirrors the real on-disk shape from a Jan/Atomic Chat install.
    #[test]
    fn parse_returns_populated_struct_for_valid_yaml() {
        let yaml = br#"embedding: false
mmproj_path: llamacpp/models/Qwen3_5-35B-A3B-Q4_K_M/mmproj.gguf
model_path: llamacpp/models/Qwen3_5-35B-A3B-Q4_K_M/model.gguf
name: Qwen3_5-35B-A3B-Q4_K_M
size_bytes: 22915306816
"#;
        let m = parse_model_yml(yaml).expect("ok");
        assert_eq!(m.name, "Qwen3_5-35B-A3B-Q4_K_M");
        assert_eq!(m.size_bytes, 22_915_306_816);
        assert_eq!(
            m.model_path,
            "llamacpp/models/Qwen3_5-35B-A3B-Q4_K_M/model.gguf"
        );
        assert_eq!(
            m.mmproj_path.as_deref(),
            Some("llamacpp/models/Qwen3_5-35B-A3B-Q4_K_M/mmproj.gguf")
        );
        assert!(!m.embedding);
    }

    /// Behavior 1 (variant): `mmproj_path` is OPTIONAL — a plain text-only
    /// LLM has no projector and the YAML simply omits the key.
    #[test]
    fn parse_accepts_yaml_without_mmproj_path() {
        let yaml = br#"embedding: false
model_path: llamacpp/models/llama-3-8b/model.gguf
name: llama-3-8b
size_bytes: 4700000000
"#;
        let m = parse_model_yml(yaml).expect("ok");
        assert_eq!(m.name, "llama-3-8b");
        assert!(m.mmproj_path.is_none(), "no mmproj when key absent");
    }

    /// Behavior 2 — malformed input must Err, not panic. The discover walk
    /// uses this signal to skip the model dir without aborting the whole
    /// scan (tested at the discover layer).
    #[test]
    fn parse_returns_err_on_truncated_yaml() {
        // YAML cut off mid-key; serde_yaml returns a parse error.
        let bytes = b"name: Qwen3\nsize_byt";
        let res = parse_model_yml(bytes);
        assert!(res.is_err(), "truncated YAML must error; got {:?}", res);
    }

    /// Behavior 2 (variant): missing required key (`size_bytes`) → Err.
    #[test]
    fn parse_returns_err_when_required_field_missing() {
        let bytes = br#"name: only-name
model_path: llamacpp/models/x/model.gguf
"#;
        let res = parse_model_yml(bytes);
        assert!(
            res.is_err(),
            "missing required `size_bytes` must error; got {:?}",
            res
        );
    }
}
