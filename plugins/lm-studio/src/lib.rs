//! LM Studio plugin stub for modeltap (per ADR-001).
//!
//! Step 01-03 ships this as a stub so the left pane has 4 tool slots from
//! day one; the real discovery / link / delete implementation lands in
//! step 02-04. The stub returns `DiscoverError::NotInstalled` from
//! `discover()`.

#![forbid(unsafe_code)]

use std::path::Path;

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, PluginFactory, Tool, ToolId,
};

pub const TOOL_NAME: ToolId = ToolId("lm-studio");
const ACCEPTED: &[Format] = &[Format::Gguf, Format::Safetensors];

pub struct LmStudioPluginStub;

#[async_trait]
impl Tool for LmStudioPluginStub {
    fn name(&self) -> ToolId {
        TOOL_NAME
    }

    fn accepted_formats(&self) -> &'static [Format] {
        ACCEPTED
    }

    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        Err(DiscoverError::NotInstalled)
    }

    async fn link(
        &self,
        _canonical_src: &Path,
        _model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        Err(LinkError::NotYetImplemented(
            "lm-studio link arrives in step 03-02".to_string(),
        ))
    }

    async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "lm-studio delete_one arrives in step 03-06".to_string(),
        ))
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "lm-studio delete_all arrives in step 03-04".to_string(),
        ))
    }
}

inventory::submit! {
    PluginFactory {
        make: || Box::new(LmStudioPluginStub),
    }
}
