//! llama-cli plugin for modeltap (per ADR-001).
//!
//! Walks the configured search paths (default: `~/llms/`, `~/models/`) for
//! `.gguf` files and parses each header for `general.architecture` and the
//! quantization label. Corrupt/truncated files are surfaced with
//! `Format::Other` + `ModelStatus::Corrupt` rather than silently dropped, so
//! the TUI can render them as `[format: corrupt]` (US-07 AC-4).
//!
//! Configuration sources (highest priority first):
//!   1. `MODELTAP_LLAMACLI_DIRS` env (colon-separated; test seam).
//!   2. `~/.modeltap/config.toml` `[plugins.llama-cli] search_paths`
//!      (overridable via `MODELTAP_CONFIG_PATH` for tests).
//!   3. Defaults: `$HOME/llms`, `$HOME/models` (same on macOS + Linux).
//!
//! `link()` / `delete_one()` / `delete_all()` are stubbed; they land in
//! steps 03-02 / 03-06 / 03-04. `accepted_formats()` reports `[Gguf]`.

#![forbid(unsafe_code)]

pub mod config;
pub mod discover;
pub mod gguf_header;
pub mod link;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, PluginFactory, Tool, ToolId,
};

pub const TOOL_NAME: ToolId = ToolId("llama-cli");
const ACCEPTED: &[Format] = &[Format::Gguf];

/// The llama-cli plugin instance. Holds the resolved search paths so
/// `discover()` can run without re-reading env/config on every call.
pub struct LlamaCliPlugin {
    search_paths: Vec<PathBuf>,
}

impl LlamaCliPlugin {
    /// Construct using the production config resolution (env -> TOML -> defaults).
    pub fn new() -> Self {
        let env = config::from_process_env();
        let cfg = config::load_config(&env);
        Self {
            search_paths: cfg.search_paths,
        }
    }

    /// Test-only constructor with explicit search paths.
    pub fn new_with_search_paths(search_paths: Vec<PathBuf>) -> Self {
        Self { search_paths }
    }

    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }
}

impl Default for LlamaCliPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for LlamaCliPlugin {
    fn name(&self) -> ToolId {
        TOOL_NAME
    }

    fn accepted_formats(&self) -> &'static [Format] {
        ACCEPTED
    }

    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        let roots = self.search_paths.clone();
        // Per ADR-005: directory walk is sync, so wrap in spawn_blocking to
        // avoid stalling the runtime thread.
        tokio::task::spawn_blocking(move || discover::discover_in(&roots))
            .await
            .map_err(|join_err| {
                DiscoverError::Io(std::io::Error::other(format!(
                    "llama-cli discovery task panicked: {join_err}"
                )))
            })?
    }

    async fn link(
        &self,
        canonical_src: &Path,
        model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        // llama-cli's `target` is the model's on_disk_path itself — there's
        // no manifest indirection. Per ADR-004 OQ-1, this is a direct
        // file replacement via the atomic-rename pattern.
        let target = model.on_disk_path.clone();
        let canonical = canonical_src.to_path_buf();
        let id = model.id_in_tool.clone();
        tokio::task::spawn_blocking(move || link::link_at(&canonical, &target, &id))
            .await
            .map_err(|join_err| {
                LinkError::Io(std::io::Error::other(format!(
                    "llama-cli link task panicked: {join_err}"
                )))
            })?
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
        make: || Box::new(LlamaCliPlugin::new()),
    }
}
