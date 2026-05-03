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
/// `config::load_from_process()` at construction time; `discover()` (step
/// 01-03) will walk these paths.
pub struct Gpt4AllPlugin {
    #[allow(dead_code)] // wired into discover() in step 01-03
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
        Err(DiscoverError::Io(std::io::Error::other(
            "gpt4all discover arrives in step 01-02",
        )))
    }

    async fn link(
        &self,
        _canonical_src: &Path,
        _model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        Err(LinkError::NotYetImplemented(
            "gpt4all link arrives in a follow-up step".to_string(),
        ))
    }

    async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "gpt4all delete_one arrives in a follow-up step".to_string(),
        ))
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "gpt4all delete_all arrives in a follow-up step".to_string(),
        ))
    }
}

inventory::submit! {
    PluginFactory {
        make: || Box::new(Gpt4AllPlugin::new()),
    }
}
