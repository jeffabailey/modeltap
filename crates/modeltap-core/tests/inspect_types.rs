//! Unit tests for the inspect domain types and the `Tool` trait extension
//! introduced by step 01-01 of the `tool-model-info-sqlite-cache` feature.
//!
//! These tests verify the load-bearing source-compatibility guarantee of
//! ADR-016: a minimal `impl Tool` whose body uses NONE of the new methods
//! still receives the documented `Err(InspectError::Unsupported { tool })`
//! behavior via the trait's default bodies.
//!
//! Test budget: 5 distinct behaviors x 2 = 10 unit tests max. This file
//! holds 5 tests (one per behavior) — no Testing Theater inflation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use async_trait::async_trait;

use modeltap_core::domain::inspect::{
    InspectError, ModelDetail, ModelId, SearchPathEntry, SearchPathSource, ToolDetail,
};
use modeltap_core::types::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, ToolId,
};
use modeltap_core::Tool;

/// Minimal Tool impl that overrides only the required methods. The inspect
/// methods are deliberately NOT overridden — the test asserts the default
/// trait bodies fire.
struct StubTool;

#[async_trait]
impl Tool for StubTool {
    fn name(&self) -> ToolId {
        ToolId("stub")
    }

    fn accepted_formats(&self) -> &'static [Format] {
        &[Format::Gguf]
    }

    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        Ok(vec![])
    }

    async fn link(
        &self,
        _canonical_src: &Path,
        _model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        Err(LinkError::NotYetImplemented("stub".into()))
    }

    async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        Err(DeleteError::NotYetImplemented("stub".into()))
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented("stub".into()))
    }
}

#[tokio::test]
async fn default_inspect_tool_returns_unsupported_with_self_name() {
    let tool = StubTool;
    let err = tool
        .inspect_tool()
        .await
        .expect_err("default body must return Err");
    match err {
        InspectError::Unsupported { tool } => {
            assert_eq!(
                tool,
                ToolId("stub"),
                "Unsupported.tool must equal self.name()",
            );
        }
        other => panic!("expected InspectError::Unsupported, got {other:?}"),
    }
}

#[tokio::test]
async fn default_inspect_model_returns_unsupported_with_self_name() {
    let tool = StubTool;
    let model_id = ModelId::from("any-model");
    let err = tool
        .inspect_model(&model_id)
        .await
        .expect_err("default body must return Err");
    match err {
        InspectError::Unsupported { tool } => {
            assert_eq!(
                tool,
                ToolId("stub"),
                "Unsupported.tool must equal self.name()",
            );
        }
        other => panic!("expected InspectError::Unsupported, got {other:?}"),
    }
}

#[test]
fn inspect_error_unsupported_carries_tool_id() {
    let err = InspectError::Unsupported {
        tool: ToolId("ollama"),
    };
    let rendered = format!("{err}");
    assert!(
        rendered.contains("ollama"),
        "Display impl must mention the ToolId; got {rendered:?}",
    );
}

#[test]
fn tool_detail_can_be_constructed_with_required_fields() {
    // Compile-check + smoke assertion that the public field surface matches
    // architecture-design.md §5.2 and data-models.md §"In-memory mirror types".
    let detail = ToolDetail {
        tool_id: ToolId("ollama"),
        install_path: PathBuf::from("/home/dev/.ollama"),
        detected_version: Some("0.6.4".into()),
        plugin_version: "modeltap-plugin-ollama 0.2.6".into(),
        search_paths: vec![SearchPathEntry {
            path: PathBuf::from("/home/dev/.ollama/models"),
            source: SearchPathSource::Default,
        }],
        model_count: 3,
        disk_usage_bytes: 7_500_000_000,
        largest_model: Some(ModelId::from("llama3:8b")),
        last_scan_at: Some(SystemTime::UNIX_EPOCH),
        last_scan_duration_ms: Some(42),
        last_error: None,
        last_error_at: None,
    };
    assert_eq!(detail.tool_id, ToolId("ollama"));
    assert_eq!(detail.model_count, 3);
    assert_eq!(detail.search_paths.len(), 1);
    assert_eq!(detail.search_paths[0].source, SearchPathSource::Default);
}

#[test]
fn model_detail_can_be_constructed_with_required_fields() {
    let mut kv = BTreeMap::new();
    kv.insert("general.architecture".into(), "llama".into());

    let detail = ModelDetail {
        model_id: ModelId::from("llama3:8b"),
        format: Some("GGUF v3".into()),
        quantisation: Some("Q4_K_M".into()),
        architecture: Some("llama".into()),
        parameters: Some(8.03),
        context_length: Some(8192),
        metadata_kv: kv,
        introspected_at: Some(SystemTime::UNIX_EPOCH),
    };
    assert_eq!(detail.model_id, ModelId::from("llama3:8b"));
    assert_eq!(detail.metadata_kv.len(), 1);
    assert_eq!(
        detail
            .metadata_kv
            .get("general.architecture")
            .map(String::as_str),
        Some("llama")
    );
}
