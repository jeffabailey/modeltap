//! Hugging Face cache plugin for modeltap (per ADR-001).
//!
//! Walks `<HF_HOME>/hub/` (default `~/.cache/huggingface/hub/`) for
//! `models--<org>--<repo>/snapshots/<rev>/<file>` symlinks and resolves each
//! to its blob target under `models--<org>--<repo>/blobs/<sha256>`. Each
//! snapshot file becomes one `DiscoveredModel`. Broken snapshot symlinks
//! are surfaced with `ModelStatus::BrokenSymlink` and `size_bytes = 0` —
//! never silently dropped (US-12 AC-5).
//!
//! Discovery root resolution:
//! 1. `HF_HOME` env var (the `huggingface_hub` standard) → `<HF_HOME>/hub/`.
//! 2. Default: `$HOME/.cache/huggingface/hub/`. Same on macOS + Linux per
//!    US-20; HF uses XDG conventions on macOS too.
//!
//! `accepted_formats()` reports the union of formats HF can host:
//! `[Gguf, Safetensors, Bin, Awq, Gptq]`.
//!
//! `link()` / `delete_one()` / `delete_all()` are stubbed; they land in
//! steps 03-02 / 03-06 / 03-04. The link strategy spike (ADR-004 OQ-1) is
//! documented in `plugins/hf/LINKING.md`.

#![forbid(unsafe_code)]

pub mod cache_walk;
pub mod discover;
pub mod symlink_resolve;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, PluginFactory, Tool, ToolId,
};

pub const TOOL_NAME: ToolId = ToolId("hf");
const ACCEPTED: &[Format] = &[
    Format::Gguf,
    Format::Safetensors,
    Format::Bin,
    Format::Awq,
    Format::Gptq,
];

/// The Hugging Face plugin instance. Holds the resolved hub root so
/// `discover()` can run without re-reading env on every call.
pub struct HfPlugin {
    hub_root: PathBuf,
}

impl HfPlugin {
    /// Construct using the production-default discovery root resolution
    /// (HF_HOME → $HOME/.cache/huggingface).
    pub fn new() -> Self {
        Self {
            hub_root: discover::resolve_hub_root(),
        }
    }

    /// Test-only constructor with an explicit hub root (the directory
    /// that contains `models--*` subdirs, i.e. `<HF_HOME>/hub/`).
    pub fn new_with_hub_root(hub_root: PathBuf) -> Self {
        Self { hub_root }
    }

    pub fn hub_root(&self) -> &Path {
        &self.hub_root
    }
}

impl Default for HfPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HfPlugin {
    fn name(&self) -> ToolId {
        TOOL_NAME
    }

    fn accepted_formats(&self) -> &'static [Format] {
        ACCEPTED
    }

    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        let root = self.hub_root.clone();
        // Per ADR-005: directory walk + symlink resolution are sync; wrap
        // in spawn_blocking so we don't stall the runtime thread.
        tokio::task::spawn_blocking(move || discover::discover_in(&root))
            .await
            .map_err(|join_err| {
                DiscoverError::Io(std::io::Error::other(format!(
                    "hf discovery task panicked: {join_err}"
                )))
            })?
    }

    async fn link(
        &self,
        _canonical_src: &Path,
        _model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        // Per ADR-004 OQ-1, the link strategy for HF is documented in
        // `plugins/hf/LINKING.md` (numbered fs ops). Implementation lands
        // in step 03-02.
        Err(LinkError::NotYetImplemented(
            "hf link arrives in step 03-02 (see plugins/hf/LINKING.md)".to_string(),
        ))
    }

    async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "hf delete_one arrives in step 03-06".to_string(),
        ))
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "hf delete_all arrives in step 03-04".to_string(),
        ))
    }
}

inventory::submit! {
    PluginFactory {
        make: || Box::new(HfPlugin::new()),
    }
}
