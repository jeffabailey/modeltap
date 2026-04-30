//! Atomic Chat plugin for modeltap (per ADR-001).
//!
//! Atomic Chat is a Jan-derived inference app. It stores models under:
//!
//!   - macOS:     `~/Library/Application Support/Atomic Chat/data/llamacpp/models/<id>/`
//!   - Linux/WSL: `~/.config/Atomic Chat/data/llamacpp/models/<id>/`
//!
//! Each model dir contains:
//!   - `model.yml`   — YAML manifest (`name`, `size_bytes`, `model_path`, …)
//!   - `model.gguf`  — the GGUF blob
//!   - `mmproj.gguf` — optional multimodal projector
//!
//! Plus an optional `[plugins.atomic-chat] search_paths` override via
//! `~/.modeltap/config.toml` (mirrors LM Studio's resolution).
//!
//! Configuration sources (highest priority first; env + TOML are UNIONED):
//!   1. `MODELTAP_ATOMIC_CHAT_DIRS` env (colon-separated; test seam).
//!   2. `~/.modeltap/config.toml` `[plugins.atomic-chat] search_paths`
//!      (overridable via `MODELTAP_CONFIG_PATH` for tests).
//!   3. Per-OS default (see `paths::default_paths_from_home`).
//!
//! Per intake C3 / ADR-004 OQ-3, MLX is OUT OF SCOPE for v1, so
//! `accepted_formats()` reports only `Format::Gguf`. The plugin does NOT
//! walk `<data>/mlx/models/`.
//!
//! `link()` / `delete_one()` / `delete_all()` are stubbed — they would follow
//! the same direct-file-replacement pattern LM Studio uses, but the user's v1
//! ask is "show models", not full mutation. They are explicit
//! `NotYetImplemented` so the orchestrator surfaces a coherent message rather
//! than a panic.
//!
//! ## Plugin name disambiguation
//!
//! `Tool::name()` returns `ToolId("Atomic Chat")` — the human-readable form
//! with a space, as it appears in the Atomic Chat app's window title. The
//! existing `plugins/atomic-chat-fixture/` registers as
//! `ToolId("atomic-chat")` (the contract-test placeholder); the two are
//! intentionally distinct so both can coexist in the inventory without
//! colliding on the `ToolId` key. The fixture stays for the R1 architecture
//! lint and US-18 panic-isolation scenario; THIS crate is the production
//! plugin a real user actually wants.

#![forbid(unsafe_code)]

pub mod config;
pub mod discover;
pub mod manifest;
pub mod paths;

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome,
    ModelMeta, PluginFactory, Tool, ToolId,
};

/// Display name for the Atomic Chat plugin. With a space, matching the app's
/// own window title — distinct from the `atomic-chat-fixture` crate's
/// `ToolId("atomic-chat")` so both can coexist in the inventory.
pub const TOOL_NAME: ToolId = ToolId("Atomic Chat");
const ACCEPTED: &[Format] = &[Format::Gguf];

/// The Atomic Chat plugin instance. Holds the resolved search paths so
/// `discover()` can run without re-reading env/config on every call.
pub struct AtomicChatPlugin {
    search_paths: Vec<PathBuf>,
}

impl AtomicChatPlugin {
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

impl Default for AtomicChatPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for AtomicChatPlugin {
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
                    "atomic-chat discovery task panicked: {join_err}"
                )))
            })?
    }

    async fn link(
        &self,
        _canonical_src: &Path,
        _model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        // The production link path mirrors LM Studio's: hardlink the canonical
        // blob over `<data>/llamacpp/models/<id>/model.gguf`. Atomic Chat
        // re-reads model.yml + the file on next launch, so atomic-replace at
        // that path is correct. Deferred until a follow-up — the user's v1
        // ask was "show models", not full mutation support.
        Err(LinkError::NotYetImplemented(
            "atomic-chat link arrives in a follow-up step".to_string(),
        ))
    }

    async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "atomic-chat delete_one arrives in a follow-up step".to_string(),
        ))
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "atomic-chat delete_all arrives in a follow-up step".to_string(),
        ))
    }
}

inventory::submit! {
    PluginFactory {
        make: || Box::new(AtomicChatPlugin::new()),
    }
}
