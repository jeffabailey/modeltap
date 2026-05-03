//! GPT4All plugin for modeltap (per ADR-001).
//!
//! GPT4All is a local LLM inference app from Nomic AI. It stores models under
//! a per-platform default directory (typically `~/.gpt4all/` or the app's
//! configured downloads path) as flat `.gguf` files.
//!
//! Per ADR-001 (frozen `Tool` trait surface), this crate implements all six
//! methods. Step 01-01 is pure scaffolding: every behavior method returns
//! `NotYetImplemented` so the trait bound compiles cleanly. Subsequent steps
//! in the gpt4all-plugin roadmap fill in the semantics:
//!
//!   - 01-02 / 01-03: `discover()` (path resolution + directory walk)
//!   - 01-05+:        `link()` / `delete_one()` / `delete_all()`
//!
//! `accepted_formats()` reports only `Format::Gguf` — GPT4All's runtime is
//! llama.cpp-derived and only loads GGUF blobs.
//!
//! ## Inventory registration
//!
//! The `inventory::submit!` block lives in this `lib.rs` (NOT a submodule) so
//! that a single `use modeltap_plugin_gpt4all as _;` in `modeltap-app/main.rs`
//! (added in step 01-04) is sufficient to force linker inclusion of the
//! factory. This crate is intentionally NOT yet wired into the composition
//! root — that arrives in step 01-04. Until then, `cargo build --workspace`
//! will compile this crate but no production binary will register its factory.

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

/// Display name for the GPT4All plugin. Matches the app's own branding —
/// distinct from the four sibling plugins' `ToolId`s so all five can coexist
/// in the inventory without colliding on the key.
pub const TOOL_NAME: ToolId = ToolId("gpt4all");

const ACCEPTED: &[Format] = &[Format::Gguf];

/// The GPT4All plugin instance. Holds the resolved search paths produced by
/// `config::load_from_process()` at construction time; `discover()` walks
/// these paths via `discover::discover_in`.
pub struct Gpt4AllPlugin {
    search_paths: Vec<PathBuf>,
}

impl Gpt4AllPlugin {
    /// Construct using the production config resolution: reads
    /// `MODELTAP_GPT4ALL_DIRS` and falls back to per-OS defaults when unset.
    pub fn new() -> Self {
        let cfg = config::load_from_process();
        Self {
            search_paths: cfg.search_paths,
        }
    }

    /// Test-only constructor with explicit search paths. Mirrors the seam
    /// `AtomicChatPlugin::new_with_search_paths` exposes — used by integration
    /// tests that need a deterministic root without going through env vars.
    pub fn new_with_search_paths(search_paths: Vec<PathBuf>) -> Self {
        Self { search_paths }
    }

    /// Read-only access to the resolved search paths (for diagnostics/tests).
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }
}

impl Default for Gpt4AllPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for Gpt4AllPlugin {
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
                    "gpt4all discovery task panicked: {join_err}"
                )))
            })?
    }

    async fn link(
        &self,
        canonical_src: &Path,
        model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        // GPT4All's target IS the model's existing on-disk path — flat-file
        // direct replacement (no manifest, no content-addressing). Per
        // ADR-005 the hardlink + rename calls are sync, so wrap in
        // spawn_blocking to avoid stalling the runtime thread.
        let target = model.on_disk_path.clone();
        let canonical = canonical_src.to_path_buf();
        let id = model.id_in_tool.clone();
        tokio::task::spawn_blocking(move || link::link_at(&canonical, &target, &id))
            .await
            .map_err(|join_err| {
                LinkError::Io(std::io::Error::other(format!(
                    "gpt4all link task panicked: {join_err}"
                )))
            })?
    }

    async fn delete_one(&self, model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        // Per ADR-005: unlink is sync; wrap in spawn_blocking.
        let target = model.on_disk_path.clone();
        let id = model.id_in_tool.clone();
        tokio::task::spawn_blocking(move || delete::delete_one_at(&target, &id))
            .await
            .map_err(|join_err| {
                DeleteError::Io(std::io::Error::other(format!(
                    "gpt4all delete_one task panicked: {join_err}"
                )))
            })?
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        // Per ADR-005: directory walk + unlink loop is sync; wrap in
        // spawn_blocking. Roots are cloned so the closure owns its inputs.
        let roots = self.search_paths.clone();
        tokio::task::spawn_blocking(move || delete::delete_all_at(&roots))
            .await
            .map_err(|join_err| {
                DeleteError::Io(std::io::Error::other(format!(
                    "gpt4all delete_all task panicked: {join_err}"
                )))
            })?
    }
}

inventory::submit! {
    PluginFactory {
        make: || Box::new(Gpt4AllPlugin::new()),
    }
}
