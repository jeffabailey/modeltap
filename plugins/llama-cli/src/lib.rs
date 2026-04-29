//! llama-cli plugin stub for modeltap (per ADR-001).
//!
//! Step 01-03 ships this as a stub so the left pane has 4 tool slots from
//! day one; the real discovery / link / delete implementation lands in
//! step 02-02. The stub returns `DiscoverError::NotInstalled` from
//! `discover()` so the production left pane shows
//! "llama-cli  0  (not installed)" — matching the @walking-skeleton @us-02
//! "Devon has only Ollama installed" scenario.

#![forbid(unsafe_code)]

use std::path::Path;

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, PluginFactory, Tool, ToolId,
};

pub const TOOL_NAME: ToolId = ToolId("llama-cli");
const ACCEPTED: &[Format] = &[Format::Gguf];

pub struct LlamaCliPluginStub;

#[async_trait]
impl Tool for LlamaCliPluginStub {
    fn name(&self) -> ToolId {
        TOOL_NAME
    }

    fn accepted_formats(&self) -> &'static [Format] {
        ACCEPTED
    }

    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        // Step 02-02 fills in the real walker over llama-cli's search paths.
        // Until then, report not-installed so the left pane shows the
        // canonical "(not installed)" annotation.
        Err(DiscoverError::NotInstalled)
    }

    async fn link(
        &self,
        _canonical_src: &Path,
        _model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        Err(LinkError::NotYetImplemented(
            "llama-cli link arrives in step 03-02".to_string(),
        ))
    }

    async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "llama-cli delete_one arrives in step 03-06".to_string(),
        ))
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "llama-cli delete_all arrives in step 03-04".to_string(),
        ))
    }
}

inventory::submit! {
    PluginFactory {
        make: || Box::new(LlamaCliPluginStub),
    }
}
