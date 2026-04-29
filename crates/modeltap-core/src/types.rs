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
    HardLinked {
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
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum DeleteError {
    #[error("not yet implemented in step 01-02: {0}")]
    NotYetImplemented(String),
    #[error("model not found in this tool: {0}")]
    NotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
