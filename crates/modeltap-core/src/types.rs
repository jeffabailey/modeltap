//! Core algebraic types used across plugin, app, and TUI layers.
//!
//! Per `docs/feature/modeltap-tui/design/data-models.md`. Step 01-02 introduces
//! the slice needed by the Ollama plugin's `discover()` and the stub
//! signatures for `link()` / `delete_one()` / `delete_all()`. The full
//! `Inventory`, `DedupGroup`, `UnifyPlan`, etc. land in subsequent steps —
//! their absence here is intentional, not an oversight.
//!
//! Wraps primitives where it matters (Object Calisthenics #3): `ToolId` is a
//! newtype around `&'static str`, `DisplayLabel` around `String`, `ContentHash`
//! around `[u8; 32]`. `DedupKey` is a sum type so illegal states (e.g., a
//! tentative key being mistaken for an authoritative one) are unrepresentable.

#![allow(clippy::module_name_repetitions)]

use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Stable, human-readable identifier for a tool. Comes from `Tool::name()`.
/// Used as the key for the left-pane selection and the Zap typed-confirmation
/// string. Newtype around `&'static str` so plugin authors cannot accidentally
/// construct one from runtime data.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct ToolId(pub &'static str);

impl std::fmt::Display for ToolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// SHA-256 of file content. The PRIMARY dedup identity (per ADR-002). Not yet
/// produced by 01-02 (lazy hashing arrives in 01-05) but the type lives here
/// so trait method signatures don't churn when hashing lands.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
pub struct ContentHash(pub [u8; 32]);

/// Display-only secondary identity from filename / manifest metadata. Used as
/// a label until SHA256 is computed.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize)]
pub struct DisplayLabel(pub String);

impl DisplayLabel {
    pub fn from(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// The dedup key. SHA256 is the primary identity; if not yet computed,
/// label-based grouping is shown to the user as "tentative" until upgraded.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize)]
pub enum DedupKey {
    /// Authoritative — content hash known.
    Content(ContentHash),
    /// Tentative — same display label, hashes not yet computed.
    Tentative(DisplayLabel),
}

// ---------------------------------------------------------------------------
// Format
// ---------------------------------------------------------------------------

/// On-disk format of a model file. Open enum (Other for unknown). Plugins
/// declare which formats they can host via `accepted_formats()`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
pub enum Format {
    Gguf,
    Safetensors,
    Bin,
    Awq,
    Gptq,
    /// Ollama's manifest+blob layout (single-tool format). Distinct from
    /// `Gguf` because the file at the on-disk path is not directly loadable
    /// by other tools — it's just the blob portion.
    OllamaBlob,
    Mlx,
    Other,
}

// ---------------------------------------------------------------------------
// DiscoveredModel and ModelMeta
// ---------------------------------------------------------------------------

/// What a plugin's `discover()` returns. SHA256 is NOT computed here.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredModel {
    /// Plugin-supplied stable id within the tool, e.g. "mistral:7b-instruct-q4_K_M".
    pub id_in_tool: String,
    /// Absolute path to the actual file the tool would read (resolved through
    /// any symlinks).
    pub on_disk_path: PathBuf,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Detected format (best-effort; `Format::Other` if unknown).
    pub format: Format,
    /// Display-only secondary label from filename / manifest / metadata.
    pub display_label: DisplayLabel,
    /// True if the file is currently broken (missing target, truncated, etc.).
    pub status: ModelStatus,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum ModelStatus {
    Healthy,
    BrokenSymlink { reason: String },
    Corrupt { reason: String },
    Unreadable { reason: String },
}

/// `ModelMeta` = the cross-plugin canonical projection of a model. Step 01-02
/// uses the simpler `DiscoveredModel` for plugin output; `ModelMeta` is the
/// cross-tool view assembled by the app. Defined here so trait method
/// signatures (`delete_one(&ModelMeta)`) compile.
#[derive(Debug, Clone, Serialize)]
pub struct ModelMeta {
    pub tool: ToolId,
    pub id_in_tool: String,
    pub on_disk_path: PathBuf,
    pub size_bytes: u64,
    pub format: Format,
    pub display_label: DisplayLabel,
    pub status: ModelStatus,
    /// Initially `Tentative(label)`; upgraded to `Content(hash)` after SHA256.
    pub dedup_key: DedupKey,
}

// ---------------------------------------------------------------------------
// Tool status (left-pane annotation)
// ---------------------------------------------------------------------------

/// What the left pane shows next to a tool name. `(installed)` means the
/// plugin reported successfully; `(not installed)` means no expected directory;
/// `(error)` means a failure during discovery.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum ToolStatus {
    /// Plugin reported successfully.
    Ok,
    /// Plugin determined the tool is not installed (no directory).
    NotInstalled,
    /// Plugin failed; the left pane shows `(error)`. The reason is written to
    /// the diagnostics log per AC-4.
    Error { reason: String },
}

// ---------------------------------------------------------------------------
// Outcomes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LinkOutcome {
    pub tool: ToolId,
    pub model_id_in_tool: String,
    pub result: LinkResult,
}

#[derive(Debug, Clone, Serialize)]
pub enum LinkResult {
    /// The target was created (or replaced) as a hardlink to the canonical
    /// inode. The standard success outcome of `Tool::link` per ADR-002.
    HardLinked {
        canonical: PathBuf,
        target: PathBuf,
        inode: u64,
    },
    /// Idempotent re-invocation: the target already shared the canonical's
    /// inode, so no filesystem mutation occurred. Per ADR-002, `link()` is
    /// idempotent — calling it twice leaves the state identical.
    AlreadyLinked {
        canonical: PathBuf,
        target: PathBuf,
        inode: u64,
    },
    Copied {
        from: PathBuf,
        to: PathBuf,
        bytes: u64,
    },
    Skipped {
        reason: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteOutcome {
    pub tool: ToolId,
    pub model_id_in_tool: String,
    pub bytes_freed: u64,
    pub registration_removed: bool,
    pub file_deleted: bool,
}

// ---------------------------------------------------------------------------
// Errors (plugin -> app)
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum DiscoverError {
    /// Tool not installed: no expected directory. The app translates this to
    /// `ToolStatus::NotInstalled` (per AC-3).
    #[error("tool not installed (no expected directory)")]
    NotInstalled,
    #[error("permission denied reading {path}: {source}")]
    PermissionDenied {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unexpected layout in {path}: {reason}")]
    UnexpectedLayout { path: PathBuf, reason: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest parse error in {path}: {reason}")]
    ManifestParse { path: PathBuf, reason: String },
}

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("not yet implemented in step 01-02: {0}")]
    NotYetImplemented(String),
    #[error("cross-filesystem hardlink not possible: canonical={canonical:?} target={target:?}")]
    CrossFilesystem { canonical: PathBuf, target: PathBuf },
    /// The canonical's content sha256 does not match what the tool expects at
    /// `target`. Defensive check for content-addressed stores (Ollama, HF).
    /// Should be impossible given the dedup-key precondition, but enforced
    /// per ADR-008's "no partial-state corruption" rule.
    #[error("content mismatch at {target:?}: expected sha256 {expected}, canonical computes to {actual}")]
    ContentMismatch {
        target: PathBuf,
        expected: String,
        actual: String,
    },
    /// Permission denied accessing `path` (parent dir not writable, target
    /// unwritable, etc.). Surface so the UI can show a coherent message.
    #[error("permission denied: {path:?}: {source}")]
    PermissionDenied {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The model metadata supplied to `link()` did not give the plugin enough
    /// information to compute a target path (e.g., HF `ModelMeta` without a
    /// recognizable `<hub>/blobs/<sha256>` on_disk_path).
    #[error("malformed model metadata: {reason}")]
    MalformedMeta { reason: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum DeleteError {
    #[error("not yet implemented in step 01-02: {0}")]
    NotYetImplemented(String),
    #[error("model not found in this tool: {0}")]
    NotFound(String),
    /// The plugin does not implement folder-grouped delete. Returned by the
    /// default body of `Tool::delete_folder` (per ADR-010) so non-HF plugins
    /// compile without an override and the orchestrator can surface a
    /// coherent no-op when (somehow) folder-delete is dispatched to a
    /// non-folder-aware plugin. Defensive surface — the dispatch keymap
    /// should prevent this path per AC-5.
    #[error("{tool} does not support folder-delete")]
    Unsupported { tool: ToolId },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

// ---------------------------------------------------------------------------
// Folder-group bulk-delete types (per
// `docs/feature/folder-group-bulk-delete/design/data-models.md` §§1–4)
// ---------------------------------------------------------------------------

/// One Hugging Face repo folder, grouping model files and sidecars under a
/// canonical `<author>/<repo>` identifier. Built by
/// `logic::folder_group::group_by_hf_repo` from an HF `ToolInventory.models`
/// slice plus sidecars supplied by the HF plugin.
///
/// `path` is the canonical `<author>/<repo>` string the user types to
/// confirm a folder-delete (D-FGD-2). `absolute_path` is the on-disk root
/// the unlink loop walks.
///
/// Construct via [`FolderGroup::new`] — direct field initialization is
/// allowed (fields are `pub`) for ergonomic plugin code, but the smart
/// constructor validates the invariants and is the recommended path.
#[derive(Debug, Clone, Serialize)]
pub struct FolderGroup {
    /// Canonical `<author>/<repo>` identifier — e.g.,
    /// `"bartowski/Llama-3.2-1B-Instruct-GGUF"`. Source-of-truth for the
    /// typed-confirm comparator (INT-FGD-7).
    pub path: String,

    /// Absolute on-disk root — `<HF_HOME>/hub/models--<author>--<repo>/`.
    pub absolute_path: PathBuf,

    /// Owning tool. Always `ToolId("hf")` in v1 (B-FGD-1).
    pub tool: ToolId,

    /// Model files in this folder. One per `.gguf` / `.safetensors` / etc.
    /// Each carries its own dedup_key for per-file classification.
    pub models: Vec<ModelMeta>,

    /// Non-model files in this folder (README.md, .imatrix, .gguf.urls,
    /// plus HF-internal refs/, blobs/ entries exclusive to this repo's
    /// snapshot). Enumerated by the HF plugin (AC-14 / B-FGD-2).
    pub sidecars: Vec<Sidecar>,
}

/// Error returned by [`FolderGroup::new`] when smart-constructor invariants
/// are violated.
#[derive(Debug, Error)]
pub enum FolderGroupError {
    /// `path` does not match the canonical `^[^/]+/[^/]+$` regex: empty,
    /// no slash, or more than one slash.
    #[error("invalid folder path {path:?}: expected canonical <author>/<repo>")]
    InvalidPath { path: String },
    /// `tool` is not `ToolId("hf")`. In v1, only the HF plugin owns folder
    /// groups (B-FGD-1).
    #[error("folder groups are HF-only in v1, got tool {tool}")]
    WrongTool { tool: ToolId },
}

impl FolderGroup {
    /// Smart constructor enforcing the documented invariants:
    /// - `path` is non-empty and matches `^[^/]+/[^/]+$` (one author, one repo)
    /// - `tool == ToolId("hf")` (B-FGD-1)
    ///
    /// Per-model `tool` and `id_in_tool` invariants are checked at the
    /// higher-level call site in `logic::folder_group::group_by_hf_repo` —
    /// the type carries them by construction.
    pub fn new(
        path: String,
        absolute_path: PathBuf,
        tool: ToolId,
        models: Vec<ModelMeta>,
        sidecars: Vec<Sidecar>,
    ) -> Result<Self, FolderGroupError> {
        if !is_canonical_repo_path(&path) {
            return Err(FolderGroupError::InvalidPath { path });
        }
        if tool != ToolId("hf") {
            return Err(FolderGroupError::WrongTool { tool });
        }
        Ok(Self {
            path,
            absolute_path,
            tool,
            models,
            sidecars,
        })
    }

    /// Sum of model bytes + sidecar bytes. INT-FGD-2 invariant.
    pub fn total_bytes(&self) -> u64 {
        let model_bytes: u64 = self.models.iter().map(|m| m.size_bytes).sum();
        let sidecar_bytes: u64 = self.sidecars.iter().map(|s| s.size_bytes).sum();
        model_bytes + sidecar_bytes
    }

    /// Total file count = `models.len() + sidecars.len()`. INT-FGD-2.
    pub fn file_count(&self) -> usize {
        self.models.len() + self.sidecars.len()
    }
}

/// `^[^/]+/[^/]+$` without pulling a regex crate in (pure-domain crate is
/// `thiserror`/`serde`/`async-trait`/`inventory` only — no regex).
fn is_canonical_repo_path(path: &str) -> bool {
    let mut parts = path.split('/');
    let Some(author) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    !author.is_empty() && !repo.is_empty()
}

/// A non-model file that lives inside the repo directory tree. Swept with
/// the folder when `delete_folder` runs (B-FGD-2). The HF plugin owns
/// enumeration (AC-14); modeltap-core only holds the type.
#[derive(Debug, Clone, Serialize)]
pub struct Sidecar {
    /// Absolute path to the file inside the repo's on-disk tree.
    pub path: PathBuf,
    /// File size in bytes. Counted toward `folder.total_bytes` and
    /// `bytes_to_reclaim` (sidecars are never "shared" with another tool).
    pub size_bytes: u64,
    /// Best-effort classification for diagnostic output. The plugin's
    /// unlink loop treats every variant identically (unlink unconditionally).
    pub kind: SidecarKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub enum SidecarKind {
    /// `README.md`, `LICENSE`, etc. — text metadata.
    Readme,
    /// `.imatrix` calibration file used by some quants.
    Imatrix,
    /// `.gguf.urls` provenance / download manifests.
    Urls,
    /// HF-internal: a ref under `refs/<name>` or a blob entry under `blobs/`
    /// that is exclusive to this repo's snapshot.
    HfInternal,
    /// Anything else inside the folder that is not a model file (per HF
    /// plugin's discriminator).
    Other,
}

/// Output of `logic::folder_group::classify_unique_vs_shared`. Each model
/// in the folder maps to exactly one bucket. Sidecars are NOT classified
/// (they are unconditionally "unique" — never shared with another tool by
/// definition).
///
/// The single source of truth for shared-vs-unique decisions on folder
/// children (D-FGD-4 / AC-13). Built by routing each child through
/// `compatibility::compute_indicator` — no parallel implementation.
#[derive(Debug, Clone, Serialize)]
pub struct FolderClassification {
    /// Files only registered with HF (or whose dedup_key is `Tentative`
    /// per the conservative-when-uncertain rule). Will be fully unlinked.
    pub unique: Vec<ModelMeta>,
    /// Files also registered with another tool. Only the HF-side path is
    /// unlinked; the other tool's hardlink keeps the inode alive (B-FGD-3).
    pub shared: Vec<SharedModel>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SharedModel {
    pub model: ModelMeta,
    /// Which other tools also have this file (by inode equivalence, per
    /// US-09 compute_indicator). Non-empty (a file with zero other-tools
    /// would be classified `unique`, not `shared`).
    pub other_tools: Vec<ToolId>,
}

/// What `Tool::delete_folder` is asked to do. Built by
/// `logic::folder_group::build_folder_delete_plan` from a `FolderGroup` +
/// `FolderClassification`. Immutable once constructed (per data-models §4).
#[derive(Debug, Clone, Serialize)]
pub struct FolderDeletePlan {
    /// The folder being deleted.
    pub folder: FolderGroup,
    /// Per-file classification — the contract the user agreed to when
    /// they typed the path. The plugin MUST NOT recompute mid-execution;
    /// the plan IS the agreement.
    pub classification: FolderClassification,
    /// Files to unlink fully (unique models + all sidecars).
    pub paths_to_unlink_fully: Vec<PathBuf>,
    /// Files to unlink only the HF-side registration of (shared models).
    pub paths_to_unlink_hf_only: Vec<PathBuf>,
    /// Promised reclaim — must match the actual `bytes_freed` sum on full
    /// success (INT-FGD-3, INT-FGD-6).
    pub bytes_to_reclaim: u64,
    /// Promised retain — files whose HF registration is removed but whose
    /// inode is kept alive by another tool's hardlink.
    pub bytes_to_retain: u64,
}

/// Error returned by [`FolderDeletePlan::new`] when its invariants are
/// violated.
#[derive(Debug, Error)]
pub enum FolderDeletePlanError {
    /// `bytes_to_reclaim + bytes_to_retain` does not equal
    /// `folder.total_bytes()` within a 1-byte rounding tolerance.
    #[error(
        "reclaim ({reclaim}) + retain ({retain}) = {sum} does not match total ({total}) within \
         1-byte tolerance"
    )]
    ReclaimRetainMismatch {
        reclaim: u64,
        retain: u64,
        sum: u64,
        total: u64,
    },
}

impl FolderDeletePlan {
    /// Smart constructor enforcing the AC-7 / INT-FGD-3 rounding invariant:
    /// `|bytes_to_reclaim + bytes_to_retain - folder.total_bytes()| <= 1`.
    ///
    /// Path-count invariants (per data-models §4) are enforced by the
    /// higher-level `build_folder_delete_plan` and are not redundantly
    /// re-checked here — over-validation at type construction sites
    /// duplicates logic and produces noisy double-errors.
    pub fn new(
        folder: FolderGroup,
        classification: FolderClassification,
        paths_to_unlink_fully: Vec<PathBuf>,
        paths_to_unlink_hf_only: Vec<PathBuf>,
        bytes_to_reclaim: u64,
        bytes_to_retain: u64,
    ) -> Result<Self, FolderDeletePlanError> {
        let total = folder.total_bytes();
        let sum = bytes_to_reclaim.saturating_add(bytes_to_retain);
        let diff = sum.abs_diff(total);
        if diff > 1 {
            return Err(FolderDeletePlanError::ReclaimRetainMismatch {
                reclaim: bytes_to_reclaim,
                retain: bytes_to_retain,
                sum,
                total,
            });
        }
        Ok(Self {
            folder,
            classification,
            paths_to_unlink_fully,
            paths_to_unlink_hf_only,
            bytes_to_reclaim,
            bytes_to_retain,
        })
    }
}
