//! Loose-GGUFs plugin for modeltap (per ADR-001).
//!
//! Walks the configured search paths (default: `~/llms/`, `~/models/`) for
//! `.gguf` files and parses each header for `general.architecture` and the
//! quantization label. Corrupt/truncated files are surfaced with
//! `Format::Other` + `ModelStatus::Corrupt` rather than silently dropped, so
//! the TUI can render them as `[format: corrupt]` (US-07 AC-4).
//!
//! ## Why "Loose GGUFs", not "llama-cli"
//!
//! The earlier name "llama-cli" was misleading: llama-cli's *managed cache*
//! (populated by `llama-cli -hf` / `-mu` / `-dr` and listed via `--cache-list`)
//! actually lives inside the Hugging Face cache (`~/.cache/huggingface/hub/`)
//! — which is the HF plugin's territory. What this plugin *actually* surfaces
//! is **loose GGUF files the user has piled into convention directories**
//! (`~/llms/`, `~/models/`, or whatever the user adds via TOML config).
//! Renaming makes that honest.
//!
//! Configuration sources (highest priority first):
//!   1. `MODELTAP_LOOSE_GGUF_DIRS` env (colon-separated; test seam).
//!   2. `~/.modeltap/config.toml` `[plugins.loose-gguf] search_paths`
//!      (overridable via `MODELTAP_CONFIG_PATH` for tests).
//!   3. Defaults: `$HOME/llms`, `$HOME/models` (same on macOS + Linux).
//!
//! `accepted_formats()` reports `[Gguf]`.

#![forbid(unsafe_code)]

pub mod config;
pub mod delete;
pub mod discover;
pub mod gguf_header;
pub mod link;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, PluginFactory, Tool, ToolId,
};

pub const TOOL_NAME: ToolId = ToolId("Loose GGUFs");
const ACCEPTED: &[Format] = &[Format::Gguf];

/// The Loose-GGUFs plugin instance. Holds the resolved search paths so
/// `discover()` can run without re-reading env/config on every call.
pub struct LooseGgufPlugin {
    search_paths: Vec<PathBuf>,
}

impl LooseGgufPlugin {
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

impl Default for LooseGgufPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for LooseGgufPlugin {
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
                    "loose-gguf discovery task panicked: {join_err}"
                )))
            })?
    }

    async fn link(
        &self,
        canonical_src: &Path,
        model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        // Loose GGUFs have no manifest indirection — `target` is the model's
        // on_disk_path itself. Per ADR-004 OQ-1, this is a direct file
        // replacement via the atomic-rename pattern.
        let target = model.on_disk_path.clone();
        let canonical = canonical_src.to_path_buf();
        let id = model.id_in_tool.clone();
        tokio::task::spawn_blocking(move || link::link_at(&canonical, &target, &id))
            .await
            .map_err(|join_err| {
                LinkError::Io(std::io::Error::other(format!(
                    "loose-gguf link task panicked: {join_err}"
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
                    "loose-gguf delete_one task panicked: {join_err}"
                )))
            })?
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "loose-gguf delete_all arrives in a future step".to_string(),
        ))
    }
}

inventory::submit! {
    PluginFactory {
        make: || Box::new(LooseGgufPlugin::new()),
    }
}
