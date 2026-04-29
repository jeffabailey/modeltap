//! Ollama plugin for modeltap (per ADR-001 + ADR-009).
//!
//! Walks `~/.ollama/models/manifests/<registry>/<repo>/<tag>` and resolves
//! blob references at `~/.ollama/models/blobs/sha256-<hash>`. Returns one
//! `DiscoveredModel` per manifest entry. Per ADR-002, SHA256 is the primary
//! dedup key but lazy-computed; this plugin does NOT compute it.
//!
//! The discovery root is configurable via the `MODELTAP_OLLAMA_DIR` env var
//! (the test seam declared in `acceptance-test-plan.md` §3). When unset,
//! the production default is `$HOME/.ollama/models/`.
//!
//! Step 01-02 implements `name`, `accepted_formats`, `discover` per the
//! frozen `Tool` trait surface; `link` / `delete_one` / `delete_all` are
//! stubbed with `NotYetImplemented`.

#![forbid(unsafe_code)]

pub mod delete;
pub mod discovery;
pub mod link;
pub mod manifest;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, PluginFactory, Tool, ToolId,
};

pub const TOOL_NAME: ToolId = ToolId("ollama");
const ACCEPTED: &[Format] = &[Format::OllamaBlob];

/// The Ollama plugin instance. Holds the discovery root resolved at
/// construction time; production code calls `OllamaPlugin::new()` which
/// reads `MODELTAP_OLLAMA_DIR` or falls back to `$HOME/.ollama/models`.
pub struct OllamaPlugin {
    /// Path to the Ollama models root (i.e. the directory containing
    /// `manifests/` and `blobs/`).
    models_root: PathBuf,
}

impl OllamaPlugin {
    /// Construct using the production-default discovery root resolution.
    /// `MODELTAP_OLLAMA_DIR` (test seam) takes precedence; otherwise
    /// `$HOME/.ollama/models`. If neither is resolvable, the resulting
    /// `discover()` will return `DiscoverError::NotInstalled` (no panic).
    pub fn new() -> Self {
        let models_root = std::env::var_os("MODELTAP_OLLAMA_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ollama/models")))
            // Fall back to a path that does not exist; `discover()` will
            // observe the absence and return `NotInstalled`.
            .unwrap_or_else(|| PathBuf::from("/nonexistent/no-such-ollama"));
        Self { models_root }
    }

    /// Test-only constructor with an explicit discovery root.
    pub fn new_with_root(models_root: PathBuf) -> Self {
        Self { models_root }
    }

    pub fn models_root(&self) -> &Path {
        &self.models_root
    }
}

impl Default for OllamaPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for OllamaPlugin {
    fn name(&self) -> ToolId {
        TOOL_NAME
    }

    fn accepted_formats(&self) -> &'static [Format] {
        ACCEPTED
    }

    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        let root = self.models_root.clone();
        // Per ADR-005: parallel discovery; the directory walk uses
        // `spawn_blocking` because walkdir + std::fs::metadata are sync.
        tokio::task::spawn_blocking(move || discovery::discover_in(&root))
            .await
            .map_err(|join_err| {
                DiscoverError::Io(std::io::Error::other(format!(
                    "ollama discovery task panicked: {join_err}"
                )))
            })?
    }

    async fn link(
        &self,
        _canonical_src: &Path,
        _model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        Err(LinkError::NotYetImplemented(
            "Tool::link arrives in step 03-02".to_string(),
        ))
    }

    async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "Tool::delete_one arrives in step 03-06".to_string(),
        ))
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        let root = self.models_root.clone();
        // The directory walk + unlink loop is sync; wrap in spawn_blocking so
        // we don't stall the runtime thread (per ADR-005).
        tokio::task::spawn_blocking(move || delete::delete_all_at(&root))
            .await
            .map_err(|join_err| {
                DeleteError::Io(std::io::Error::other(format!(
                    "ollama delete_all task panicked: {join_err}"
                )))
            })?
    }
}

// Plugin registration (per ADR-001 §"Decision"). Plugins self-register into
// a static linker section via `inventory::submit!` against the `PluginFactory`
// slot defined in `modeltap-core`. The composition root (`modeltap-app`)
// iterates this section to assemble `Vec<Box<dyn Tool>>` without any plugin
// crate depending on another plugin crate.

inventory::submit! {
    PluginFactory {
        make: || Box::new(OllamaPlugin::new()),
    }
}
