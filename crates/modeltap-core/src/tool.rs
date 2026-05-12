//! The `Tool` trait — the plugin port (per ADR-001 + ADR-009).
//!
//! **FROZEN SURFACE.** Per ADR-001, this trait shape is contract: subsequent
//! steps may NOT add or remove methods. They may only implement the trait for
//! new plugins. ADR-001 §"Enforcement" guarantees that adding a 5th tool
//! requires zero changes to `modeltap-core`.
//!
//! Six methods total (per ADR-001 §"Decision" + ADR-009 §"Decision"):
//! 1. `name()` — `ToolId` for the left pane / typed-confirm string.
//! 2. `accepted_formats()` — `&'static [Format]` declaring host capability.
//! 3. `discover()` — async; returns `Vec<DiscoveredModel>` or `DiscoverError`.
//! 4. `link()` — async; hardlinks a canonical file into this tool's tree.
//! 5. `delete_one()` — async; removes one model from this tool (per ADR-009).
//! 6. `delete_all()` — async; removes every model from this tool (per US-05).
//!
//! Step 01-02 implements `name`, `accepted_formats`, `discover`. The other
//! three are stubbed in plugins (returning `Err(NotYetImplemented(...))`).
//! That stub-shape IS deliberate: it forces 01-02 to lock the trait surface
//! while 03-02 / 03-06 fill in the semantics.

use std::path::Path;

use async_trait::async_trait;

use crate::types::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, FolderDeletePlan, Format,
    LinkError, LinkOutcome, ModelMeta, ToolId,
};

/// The plugin port. Object-safe (per ADR-001) so `Vec<Box<dyn Tool>>` works
/// for runtime dispatch. `Send + Sync` so plugin orchestration can run
/// across tokio tasks.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Stable, human-readable identifier. MUST be deterministic across calls
    /// — used as the dictionary key for inventory and as the typed-confirm
    /// string for `z`/`d` actions.
    fn name(&self) -> ToolId;

    /// Formats this tool can host. MUST be a non-empty `&'static` slice (the
    /// plugin contract test enforces this; see `plugin-contract-spec.md` §3.7).
    fn accepted_formats(&self) -> &'static [Format];

    /// Walk this tool's on-disk tree and return one `DiscoveredModel` per
    /// model the tool would load. Idempotent (no side-effects, per ADR-003).
    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError>;

    /// Hardlink the canonical file at `canonical_src` into this tool's tree
    /// such that the tool would now load FROM that inode. Implemented in
    /// step 03-02; stubbed in 01-02 with `LinkError::NotYetImplemented`.
    async fn link(&self, canonical_src: &Path, model: &ModelMeta)
        -> Result<LinkOutcome, LinkError>;

    /// Remove one model's registration from this tool (per ADR-009). If the
    /// model's content is unique-to-this-tool the file is deleted; otherwise
    /// only the registration is removed and `bytes_freed == 0`. Implemented
    /// in step 03-06; stubbed in 01-02.
    async fn delete_one(&self, model: &ModelMeta) -> Result<DeleteOutcome, DeleteError>;

    /// Remove every model registered with this tool. Default impl could loop
    /// over `delete_one`; tools with batch-optimized delete may override.
    /// Implemented in step 03-04; stubbed in 01-02.
    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError>;

    /// Delete every file in a folder-group from this tool's storage.
    ///
    /// Per ADR-010, the default body returns
    /// `Err(DeleteError::Unsupported { tool: self.name() })` so plugins that
    /// do not have a folder-grouped layout (Ollama, llama-cli, LM Studio,
    /// atomic-chat) compile without an override and the orchestrator can
    /// surface a coherent no-op-with-message at the UI layer when (somehow)
    /// folder-delete is dispatched to a non-folder-aware plugin. The HF
    /// plugin overrides this default in step 01-03.
    ///
    /// Contract (when overridden):
    /// - Iterates the plan's paths and returns one `DeleteOutcome` per file.
    /// - On per-file failure: continues; failed entry has
    ///   `registration_removed: false`, `file_deleted: false`,
    ///   `bytes_freed: 0`.
    /// - Cross-tool hardlinks must survive: shared model files have only
    ///   the plugin-side path unlinked.
    /// - On full success: the now-empty repo directory tree is removed.
    /// - Idempotent on retry against a partial folder.
    async fn delete_folder(
        &self,
        plan: &FolderDeletePlan,
    ) -> Result<Vec<DeleteOutcome>, DeleteError> {
        let _ = plan;
        Err(DeleteError::Unsupported { tool: self.name() })
    }
}
