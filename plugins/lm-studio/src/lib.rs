//! LM Studio plugin for modeltap (per ADR-001).
//!
//! Walks both LM Studio default-path conventions:
//!
//!   `~/.cache/lm-studio/models/`  — newer (LM Studio 0.3.x+, XDG-compliant).
//!   `~/.lmstudio/models/`         — older (LM Studio 0.2.x and earlier).
//!
//! Plus an optional `[plugins.lm-studio] search_paths` override via
//! `~/.modeltap/config.toml` (mirror llama-cli pattern from US-07).
//!
//! Configuration sources (highest priority first; env + TOML are UNIONED):
//!   1. `MODELTAP_LMSTUDIO_DIRS` env (colon-separated; test seam).
//!   2. `~/.modeltap/config.toml` `[plugins.lm-studio] search_paths`
//!      (overridable via `MODELTAP_CONFIG_PATH` for tests).
//!   3. Defaults: `$HOME/.cache/lm-studio/models`, `$HOME/.lmstudio/models`
//!      (same on macOS + Linux per US-20).
//!
//! Per intake C3 / ADR-004 OQ-3, MLX is OUT OF SCOPE for v1, so
//! `accepted_formats()` reports only `Format::Gguf`. When MLX support lands
//! (v1.x), the Format enum gains a parallel slot and this list is updated.
//!
//! `link()` / `delete_one()` / `delete_all()` are stubbed; they land in
//! steps 03-02 / 03-06 / 03-04. The link strategy spike (ADR-004 OQ-2) is
//! documented in `plugins/lm-studio/PATHS.md`.

#![forbid(unsafe_code)]

pub mod config;
pub mod delete;
pub mod discover;
pub mod link;
pub mod paths;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, PluginFactory, Tool, ToolId,
};

pub const TOOL_NAME: ToolId = ToolId("lm-studio");
const ACCEPTED: &[Format] = &[Format::Gguf];

/// The LM Studio plugin instance. Holds the resolved search paths so
/// `discover()` can run without re-reading env/config on every call.
pub struct LmStudioPlugin {
    search_paths: Vec<PathBuf>,
}

impl LmStudioPlugin {
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

impl Default for LmStudioPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for LmStudioPlugin {
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
                    "lm-studio discovery task panicked: {join_err}"
                )))
            })?
    }

    async fn link(
        &self,
        canonical_src: &Path,
        model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        // Per ADR-004 OQ-2 / `plugins/lm-studio/PATHS.md`, the target path is
        // the model's existing `on_disk_path` — LM Studio re-reads the file
        // on next selection, so atomic-replace at that exact path is correct.
        let target = model.on_disk_path.clone();
        let canonical = canonical_src.to_path_buf();
        let id = model.id_in_tool.clone();
        tokio::task::spawn_blocking(move || link::link_at(&canonical, &target, &id))
            .await
            .map_err(|join_err| {
                LinkError::Io(std::io::Error::other(format!(
                    "lm-studio link task panicked: {join_err}"
                )))
            })?
    }

    async fn delete_one(&self, model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        let target = model.on_disk_path.clone();
        let id = model.id_in_tool.clone();
        // Per ADR-005: fs::remove_file is sync; wrap in spawn_blocking.
        tokio::task::spawn_blocking(move || delete::delete_one_at(&target, &id))
            .await
            .map_err(|join_err| {
                DeleteError::Io(std::io::Error::other(format!(
                    "lm-studio delete_one task panicked: {join_err}"
                )))
            })?
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "lm-studio delete_all arrives in step 03-04".to_string(),
        ))
    }
}

inventory::submit! {
    PluginFactory {
        make: || Box::new(LmStudioPlugin::new()),
    }
}
