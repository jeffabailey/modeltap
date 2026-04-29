# Data Models — modeltap-tui

This document defines the algebraic types in `modeltap-core::domain::types`. These are the shapes that flow through every layer (plugins → app → tui). All types are `Send + Sync + Clone` unless noted.

## Naming convention

- `Foo` — owned value type
- `FooId` — opaque identifier (newtype around `String` or `[u8; 32]`)
- `FooMeta` — metadata-only projection (for UI display)
- `FooOutcome` — result of an action

## Identity types

```rust
/// Stable, human-readable identifier for a tool.
/// Comes from `Tool::name()`. Used as the key for the left-pane selection
/// and the Zap typed-confirmation string.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize)]
pub struct ToolId(pub &'static str);

/// SHA-256 of file content. The PRIMARY dedup identity (per ADR-002).
/// 32 bytes (256 bits). Display as lowercase hex.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
pub struct ContentHash(pub [u8; 32]);

/// Display-only secondary identity from HF-style metadata.
/// Used as a label when SHA256 is not yet computed.
/// Examples: "mistralai/Mistral-7B-v0.3 q4_K_M GGUF".
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize)]
pub struct DisplayLabel(pub String);

/// The dedup key. SHA256 is the primary identity; if not yet computed,
/// label-based grouping is shown to the user as "tentative" until upgraded.
#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize)]
pub enum DedupKey {
    /// Authoritative — content hash known.
    Content(ContentHash),
    /// Tentative — same display label, hashes not yet computed.
    /// May refine to two distinct Content keys after hashing.
    Tentative(DisplayLabel),
}
```

## Format and capability

```rust
/// On-disk format of a model file. Open enum (Other for unknown).
/// Plugins consume `&[Format]` from `accepted_formats()`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
pub enum Format {
    Gguf,
    Safetensors,
    Bin,            // PyTorch .bin
    Awq,
    Gptq,
    OllamaBlob,     // Ollama's manifest+blob layout (single-tool format)
    Mlx,            // out of scope for v1 per C3, but the variant exists
    Other,          // unrecognized; renders `?`
}

/// Capability declared by a plugin via `accepted_formats()`.
/// A plugin saying "I accept GGUF" means: a file in GGUF format CAN be
/// hardlinked into this tool's directory and the tool will be able to load it.
pub type Capability = &'static [Format];
```

## Discovered models and metadata

```rust
/// What a plugin's `discover()` returns. SHA256 is NOT computed here.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredModel {
    /// Plugin-supplied stable id within the tool, e.g. "mistral:7b-instruct-q4_K_M".
    pub id_in_tool: String,
    /// Absolute path to the actual file the tool would read (resolved through any symlinks).
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

/// `ModelMeta` = the cross-plugin canonical projection of a model.
/// One `ModelMeta` exists per (ToolId, on-disk path) pair. Two ModelMetas
/// with the same `dedup_key` represent the same logical model in two tools.
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
```

## Inventory and groups

```rust
/// Result of a single full discovery pass. Owned by the App.
#[derive(Debug, Clone, Serialize)]
pub struct Inventory {
    pub generated_at: SystemTime,
    pub by_tool: HashMap<ToolId, ToolInventory>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolInventory {
    pub tool: ToolId,
    pub status: ToolStatus,
    pub models: Vec<ModelMeta>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum ToolStatus {
    /// Plugin reported successfully.
    Ok,
    /// Plugin determined the tool is not installed (no directory).
    NotInstalled,
    /// Plugin failed; `(error)` shown in left pane.
    Error { reason: String },
}

/// One group of models that share a dedup_key.
/// Built by `logic::dedup::group_by_dedup_key(&Inventory)`.
#[derive(Debug, Clone, Serialize)]
pub struct DedupGroup {
    pub key: DedupKey,
    /// Members. Always >= 1.
    pub members: Vec<ModelMeta>,
}

impl DedupGroup {
    pub fn is_unified(&self) -> UnifyState {
        // UNIFIED if all members' on-disk paths point to the same inode.
        // PARTIAL if some do.
        // NOT_UNIFIED if all are independent.
        // SINGLE_TOOL if members.len() == 1.
        unimplemented!() // pure fn, defined in logic::dedup
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum UnifyState {
    Unified,         // 1 inode, N hardlinks across N tool paths
    PartiallyUnified { unified_count: usize, total: usize },
    NotUnified,
    SingleTool,      // not eligible for unify
}
```

## Compatibility indicator

```rust
/// The o / * / ! / ? indicator on each row.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub enum Indicator {
    /// `*` — currently registered with 2+ tools (multi-tool).
    Star,
    /// `o` — only in 1 tool but >=1 other tool accepts the format.
    Open,
    /// `!` — only in 1 tool and no other supported tool accepts the format.
    Bang,
    /// `?` — format unknown or capability metadata missing.
    Question,
}
```

## Plans

```rust
/// What a unify operation will do. Built by logic::plan::build_unify_plan.
/// Identical for dry-run and real-run (single source of truth, US-14 invariant).
#[derive(Debug, Clone, Serialize)]
pub struct UnifyPlan {
    pub group: DedupGroup,
    /// The one path that becomes the canonical inode. Per Q1 override:
    /// chosen from the existing tool paths (largest copy by default;
    /// see ADR-004). NOT a path under ~/.modeltap/store/.
    pub canonical: PathBuf,
    /// Per-target operations. One per group member (excluding canonical).
    pub targets: Vec<UnifyTarget>,
    /// Total bytes that disappear from disk if all targets succeed.
    pub estimated_reclaim_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnifyTarget {
    pub tool: ToolId,
    pub current_path: PathBuf,
    /// Operation to perform.
    pub action: UnifyAction,
    /// True if current_path's filesystem differs from canonical's.
    pub crosses_filesystem: bool,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum UnifyAction {
    /// Hardlink current_path → canonical. Default for same-fs.
    HardLink,
    /// Copy canonical → current_path. Used when same-fs is impossible
    /// AND user chose 'copy' fallback (US-19).
    Copy,
    /// Leave alone. Used when user chose 'skip cross-fs' fallback.
    Skip { reason: String },
}

/// What a zap operation will do. Built by logic::plan::build_zap_plan.
#[derive(Debug, Clone, Serialize)]
pub struct ZapPlan {
    pub tool: ToolId,
    /// All models registered with this tool.
    pub all_models: Vec<ModelMeta>,
    /// Subset whose dedup_key matches at least one model in another tool.
    /// These get the registration removed but the file is preserved (because
    /// another tool still references the same inode).
    pub shared_models: Vec<ModelMeta>,
    /// Subset whose dedup_key is unique to this tool — file gets deleted.
    pub unique_models: Vec<ModelMeta>,
    pub estimated_reclaim_bytes: u64,
    pub estimated_retained_bytes: u64,  // shared bytes that stay on disk
}

/// The single-model variant (F4 delta — supports US-05b / US-05 expansion).
#[derive(Debug, Clone, Serialize)]
pub struct ZapOnePlan {
    pub tool: ToolId,
    pub model: ModelMeta,
    /// True if at least one other tool's dedup_key matches.
    /// If true: only the registration is removed; file preserved.
    /// If false: registration removed AND file deleted.
    pub also_in_other_tools: bool,
    pub estimated_reclaim_bytes: u64,
}
```

## Outcomes (what plugins return)

```rust
#[derive(Debug, Clone, Serialize)]
pub struct LinkOutcome {
    pub tool: ToolId,
    pub model_id_in_tool: String,
    pub result: LinkResult,
}

#[derive(Debug, Clone, Serialize)]
pub enum LinkResult {
    HardLinked { canonical: PathBuf, target: PathBuf, inode: u64 },
    Copied { from: PathBuf, to: PathBuf, bytes: u64 },
    Skipped { reason: String },
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteOutcome {
    pub tool: ToolId,
    pub model_id_in_tool: String,
    pub bytes_freed: u64,         // 0 if file was preserved (shared)
    pub registration_removed: bool,
    pub file_deleted: bool,
}
```

## Last action (UI feedback)

```rust
/// What the right pane shows after a mutating action (US-06).
#[derive(Debug, Clone, Serialize)]
pub struct LastAction {
    pub kind: ActionKind,
    pub target: String,             // e.g. "llama-cli" or "mistral:7b"
    pub status: ActionStatus,
    pub bytes_reclaimed: u64,
    pub bytes_retained: u64,        // for zap: shared bytes preserved
    pub finished_at: SystemTime,
    pub detail: Option<String>,     // partial-success specifics
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum ActionKind {
    Zap,
    ZapOne,
    Unify,
    UnifyDryRun,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum ActionStatus {
    Success,
    PartialSuccess { succeeded: usize, total: usize },
    Failed,
    Cancelled,  // e.g., user typed wrong tool name
}
```

## Errors

```rust
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("tool not installed (no expected directory)")]
    NotInstalled,
    #[error("permission denied reading {path}: {source}")]
    PermissionDenied { path: PathBuf, source: io::Error },
    #[error("unexpected layout in {path}: {reason}")]
    UnexpectedLayout { path: PathBuf, reason: String },
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("cross-filesystem hardlink not possible: canonical={canonical:?} target={target:?}")]
    CrossFilesystem { canonical: PathBuf, target: PathBuf },
    #[error("target file in use; close {tool} and retry")]
    InUse { tool: ToolId },
    #[error("permission denied: {0}")]
    PermissionDenied(io::Error),
    #[error("manifest update failed: {0}")]
    ManifestUpdate(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    #[error("model not found in this tool: {0}")]
    NotFound(String),
    #[error("file in use; close {tool} and retry")]
    InUse { tool: ToolId },
    #[error("permission denied: {0}")]
    PermissionDenied(io::Error),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}
```

## Secondary ports (driven, in `modeltap-core::ports`)

```rust
/// SHA256 hashing port. Mockable in tests.
#[async_trait::async_trait]
pub trait Hasher: Send + Sync {
    async fn sha256(&self, path: &Path) -> Result<ContentHash, io::Error>;
}

/// Filesystem probe — same-fs check, lsof. Mockable.
#[async_trait::async_trait]
pub trait FsProbe: Send + Sync {
    async fn same_filesystem(&self, a: &Path, b: &Path) -> Result<bool, io::Error>;
    /// Returns names of tool processes holding any file under `paths` open.
    /// Returns `Unavailable` if lsof or equivalent is not present.
    async fn processes_holding(&self, paths: &[PathBuf]) -> Result<LsofResult, io::Error>;
}

#[derive(Debug, Clone)]
pub enum LsofResult {
    Found(Vec<HoldingProcess>),
    Empty,
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct HoldingProcess {
    pub pid: u32,
    pub command: String,
    pub paths: Vec<PathBuf>,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}
```

## Type-level invariants (enforced by construction)

- A `ContentHash` is always 32 bytes. Always.
- A `DedupKey::Content(_)` is authoritative — never downgraded to `Tentative`.
- A `ZapOnePlan` always references exactly one model.
- A `UnifyPlan` always has at least one `target` (a single-member group is not eligible — `build_unify_plan` returns `None`).
- `UnifyState::Unified` ⇒ all member paths stat to the same inode.
- A plugin returning `ToolStatus::NotInstalled` returns an empty `models` vec.

These invariants hold at the type level (sum types make illegal states unrepresentable where possible) and at the construction-function level (smart constructors validate before returning).
