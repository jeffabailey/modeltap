//! Ollama manifest JSON parser.
//!
//! An Ollama manifest is a Docker-image-style JSON document at
//! `manifests/<registry>/<repo>/<tag>` whose `layers[*]` entries describe
//! the model blob(s) under `blobs/sha256-<hash>`. modeltap reads ONLY the
//! single model layer (`mediaType = application/vnd.ollama.image.model`)
//! and ignores config/template/license layers — those are tiny metadata
//! blobs, not the content we account for.
//!
//! Pure function module: no I/O. Caller passes raw bytes; we return a
//! parsed view or a `ParseError`. This is deliberately separate from
//! `discovery.rs` so the parser is unit-testable in isolation.

use serde::Deserialize;
use thiserror::Error;

/// Parsed view of one Ollama manifest. Only the fields modeltap needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// SHA256 hash of the model blob (the value AFTER `sha256:` in the
    /// manifest's model layer digest). Used to resolve the on-disk blob
    /// path at `blobs/sha256-<blob_sha>`.
    pub blob_sha: String,
    /// Declared size of the model layer in bytes. Used as the model's
    /// `size_bytes` for inventory accounting.
    pub size_bytes: u64,
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("manifest is not valid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("manifest has no model layer (mediaType application/vnd.ollama.image.model)")]
    NoModelLayer,
    #[error("manifest model layer digest is not sha256-prefixed: {0}")]
    BadDigest(String),
}

/// Parse a raw Ollama manifest JSON string into the fields modeltap needs.
///
/// Strategy: deserialize into a permissive structural shape, then locate
/// the single layer entry whose mediaType is the Ollama model layer. If
/// no such layer exists, return `NoModelLayer`.
pub fn parse_manifest(raw: &str) -> Result<ManifestEntry, ParseError> {
    let doc: ManifestDoc = serde_json::from_str(raw)?;
    let model_layer = doc
        .layers
        .into_iter()
        .find(|l| l.media_type == OLLAMA_MODEL_MEDIA_TYPE)
        .ok_or(ParseError::NoModelLayer)?;
    let blob_sha = strip_sha256_prefix(&model_layer.digest)?;
    Ok(ManifestEntry {
        blob_sha,
        size_bytes: model_layer.size,
    })
}

const OLLAMA_MODEL_MEDIA_TYPE: &str = "application/vnd.ollama.image.model";

fn strip_sha256_prefix(digest: &str) -> Result<String, ParseError> {
    digest
        .strip_prefix("sha256:")
        .map(|s| s.to_string())
        .ok_or_else(|| ParseError::BadDigest(digest.to_string()))
}

#[derive(Debug, Deserialize)]
struct ManifestDoc {
    #[serde(default)]
    layers: Vec<LayerDoc>,
}

#[derive(Debug, Deserialize)]
struct LayerDoc {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
    size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MANIFEST: &str = r#"{
  "schemaVersion": 2,
  "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
  "config": {
    "mediaType": "application/vnd.docker.container.image.v1+json",
    "digest": "sha256:cafebabe",
    "size": 412
  },
  "layers": [
    {
      "mediaType": "application/vnd.ollama.image.model",
      "digest": "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
      "size": 4700000000
    }
  ]
}"#;

    #[test]
    fn valid_manifest_yields_blob_sha_and_size() {
        let entry = parse_manifest(VALID_MANIFEST).expect("valid manifest must parse");
        assert_eq!(
            entry.blob_sha,
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        );
        assert_eq!(entry.size_bytes, 4_700_000_000);
    }

    #[test]
    fn malformed_json_returns_invalid_json_error() {
        let raw = "{ not json";
        let err = parse_manifest(raw).expect_err("malformed JSON must error");
        assert!(matches!(err, ParseError::InvalidJson(_)));
    }

    #[test]
    fn manifest_without_model_layer_returns_no_model_layer() {
        // Has only a config layer, no `application/vnd.ollama.image.model` layer.
        let raw = r#"{
            "schemaVersion": 2,
            "layers": [
                {
                    "mediaType": "application/vnd.ollama.image.template",
                    "digest": "sha256:deadbeef",
                    "size": 12
                }
            ]
        }"#;
        let err = parse_manifest(raw).expect_err("no model layer must error");
        assert!(matches!(err, ParseError::NoModelLayer));
    }

    #[test]
    fn manifest_with_non_sha256_digest_returns_bad_digest() {
        let raw = r#"{
            "schemaVersion": 2,
            "layers": [
                {
                    "mediaType": "application/vnd.ollama.image.model",
                    "digest": "md5:deadbeef",
                    "size": 100
                }
            ]
        }"#;
        let err = parse_manifest(raw).expect_err("non-sha256 digest must error");
        assert!(matches!(err, ParseError::BadDigest(_)));
    }
}
