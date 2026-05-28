# Data Models — folder-group-bulk-delete

**Wave:** DESIGN (3 of 6) — brownfield extension
**Parent data models:** `docs/feature/modeltap-tui/design/data-models.md`

These are Rust algebraic-type **sketches** — code-as-documentation for the shapes the software-crafter will instantiate during DELIVER. Method bodies (`impl` blocks beyond constructors) are NOT specified here; the crafter owns implementation during GREEN. Trait-method signatures ARE specified because they are the contract.

All types are `Send + Sync + Clone + Debug + Serialize` unless noted. They live in `modeltap-core::types` (additive to the parent's existing module) and in `modeltap-core::logic::folder_group` (the pure-logic surface).

## Naming convention (inherited)

- `Foo` — owned value type
- `FooPlan` — frozen description of work to be done (input to a plugin)
- `FooOutcome` — result of an action (output from a plugin)
- `FooError` — typed error variant (`thiserror` enum)

## 1. FolderGroup — the unit being deleted

```rust
/// One Hugging Face repo folder, grouping model files and sidecars under a
/// `<author>/<repo>` identifier.
///
/// Built by `logic::folder_group::group_by_hf_repo` from an HF
/// `ToolInventory.models` slice plus sidecars supplied by the HF plugin.
///
/// `path` is the canonical `<author>/<repo>` string the user types to
/// confirm a folder-delete (D-FGD-2). `absolute_path` is the on-disk root
/// the unlink loop walks.
#[derive(Debug, Clone, Serialize)]
pub struct FolderGroup {
    /// Canonical `<author>/<repo>` identifier — e.g.,
    /// `"bartowski/Llama-3.2-1B-Instruct-GGUF"`.
    /// Source-of-truth for the typed-confirm comparator (INT-FGD-7).
    pub path: String,

    /// Absolute on-disk root —
    /// `<HF_HOME>/hub/models--<author>--<repo>/`.
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

impl FolderGroup {
    /// Sum of model bytes + sidecar bytes. INT-FGD-2 invariant.
    /// (Body omitted — software-crafter writes during GREEN.)
    pub fn total_bytes(&self) -> u64 { unimplemented!() }

    /// Total file count = models.len() + sidecars.len(). INT-FGD-2.
    pub fn file_count(&self) -> usize { unimplemented!() }
}
```

**Invariants enforced by construction (smart constructor in `group_by_hf_repo`):**

- `path` is non-empty and matches the regex `^[^/]+/[^/]+$` (one author, one repo).
- `absolute_path.file_name()` is `models--<author>--<repo>` where the dashes are the HF-canonical encoding.
- Every model in `models` has `tool == ToolId("hf")` and its `id_in_tool` starts with `path + "/"`.
- `sidecars` may be empty; `models` may be empty (rare; folder containing only sidecars per journey error path).

## 2. Sidecar — non-model files in the folder

```rust
/// A non-model file that lives inside the repo directory tree. Swept with
/// the folder when delete_folder runs (B-FGD-2). The HF plugin owns
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
```

**Why a typed enum vs. just `String`?** Diagnostics. The HF plugin's `tracing` span for folder-delete will carry the kind; future HF version changes that introduce a new sidecar type are caught by an exhaustive `match` warning rather than a silent miss.

## 3. FolderClassification — per-file shared/unique projection

```rust
/// Output of `logic::folder_group::classify_unique_vs_shared`. Each model
/// in the folder maps to exactly one bucket. Sidecars are NOT classified
/// (they are unconditionally "unique" — never shared with another tool by
/// definition).
///
/// THE SINGLE SOURCE OF TRUTH for shared-vs-unique decisions on folder
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
```

**Mapping from `compute_indicator` result to `FolderClassification` bucket:**

| `RowIndicator` returned by `compute_indicator` | Bucket |
|---|---|
| `Shared` | `shared` — with `other_tools` from the dedup-group |
| `Compatible` | `unique` — single-tool, file gets unlinked |
| `FormatLocked` | `unique` — single-tool, file gets unlinked |
| `Unknown` | `unique` — conservative: treat as unique-to-HF (loses no data because if some other tool also has it via a hardlink to the same inode, ADR-009 ref-counting in `delete_one_at` will detect it at execution time) |

The "Unknown → unique" projection deserves a comment in the source: it preserves the parent's conservative-when-uncertain rule (an `Unknown` indicator never claims sharing). The cross-tool hardlink survival guarantee (INT-FGD-4) holds via the HF plugin's existing blob ref-counting, not via classification.

## 4. FolderDeletePlan — frozen description of work

```rust
/// What `Tool::delete_folder` is asked to do. Built by
/// `logic::folder_group::build_folder_delete_plan` from a FolderGroup +
/// FolderClassification. Immutable once constructed.
///
/// The orchestrator (modeltap-app::orchestration::execute_folder_delete)
/// builds the plan; the plugin executes it. The plan + the returned
/// outcomes are the data the post-action LastAction renders from.
#[derive(Debug, Clone, Serialize)]
pub struct FolderDeletePlan {
    /// The folder being deleted. Carries `path`, `absolute_path`,
    /// `models`, `sidecars`.
    pub folder: FolderGroup,

    /// Per-file classification — the contract the user agreed to when
    /// they typed the path. Software-crafter MUST NOT recompute mid-
    /// execution; the plan IS the agreement.
    pub classification: FolderClassification,

    /// Files to unlink fully (unique models + all sidecars).
    /// Equivalent to `classification.unique + folder.sidecars` projected
    /// to paths. Pre-computed here so the plugin doesn't re-derive.
    pub paths_to_unlink_fully: Vec<PathBuf>,

    /// Files to unlink only the HF-side registration of (shared models).
    /// Equivalent to `classification.shared.iter().map(|s| s.model.on_disk_path)`.
    pub paths_to_unlink_hf_only: Vec<PathBuf>,

    /// Promised reclaim — must match the actual bytes_freed sum on full
    /// success (INT-FGD-3, INT-FGD-6).
    pub bytes_to_reclaim: u64,

    /// Promised retain — files whose HF registration is removed but whose
    /// inode is kept alive by another tool's hardlink.
    pub bytes_to_retain: u64,
}
```

**Invariants (enforced by `build_folder_delete_plan`):**

- `bytes_to_reclaim + bytes_to_retain == folder.total_bytes()` within rounding (AC-7 / INT-FGD-3).
- `paths_to_unlink_fully.len() + paths_to_unlink_hf_only.len() == folder.file_count()`.
- `classification.unique.len() + classification.shared.len() == folder.models.len()`.

## 5. FolderDeleteOutcome — per-file result

The plugin returns `Vec<DeleteOutcome>` from `delete_folder` — one entry per file attempted. Reusing the existing `DeleteOutcome` type (already in `modeltap-core::types`) rather than introducing a parallel `FolderDeleteOutcome` keeps the orchestrator's aggregation code path identical to the per-file `delete_one` path.

Each `DeleteOutcome` entry carries:

```rust
// EXISTING type — for reference, no change.
pub struct DeleteOutcome {
    pub tool: ToolId,
    pub model_id_in_tool: String,
    pub bytes_freed: u64,
    pub registration_removed: bool,
    pub file_deleted: bool,
}
```

For folder-delete, the conventions are:

- **Unique model successfully unlinked:** `registration_removed: true`, `file_deleted: true`, `bytes_freed: <size>`.
- **Shared model HF-path successfully unlinked:** `registration_removed: true`, `file_deleted: false`, `bytes_freed: 0`. (The other tool's hardlink keeps the inode; HF lost its reference.)
- **Sidecar successfully unlinked:** `registration_removed: true`, `file_deleted: true`, `bytes_freed: <size>`. (`model_id_in_tool` carries the sidecar's filename for diagnostic output; the orchestrator's `LastAction` aggregation routes sidecar entries to a separate counter so the user sees "21 of 21 files (20 models + 3 sidecars)" not "21 of 21 models".)
- **Failure (any kind):** `registration_removed: false`, `file_deleted: false`, `bytes_freed: 0`. The reason is captured in the surrounding `tracing` span; the orchestrator's `LastAction.detail` aggregates per-file reasons for the post-action partial-failure rendering.

A separate `FolderDeleteOutcome` type would create two `DeleteOutcome` shapes in the codebase — unnecessary duplication. Reuse is correct here.

## 6. DeleteError::Unsupported — new variant on existing enum

```rust
// MODIFIED: crates/modeltap-core/src/types.rs

#[derive(Debug, thiserror::Error)]
pub enum DeleteError {
    #[error("not yet implemented in step 01-02: {0}")]
    NotYetImplemented(String),

    #[error("model not found in this tool: {0}")]
    NotFound(String),

    /// NEW (this feature, per ADR-010). Returned by the default impl of
    /// `Tool::delete_folder` so non-HF plugins compile without an override
    /// and the orchestrator can surface the no-op-with-message at the UI
    /// layer when (somehow) folder-delete is dispatched to a non-folder-
    /// aware plugin.
    #[error("{tool} does not support folder-delete")]
    Unsupported { tool: ToolId },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

The `Unsupported` variant is reachable only if `modeltap-app::orchestration::execute_folder_delete` is called with a non-HF plugin (a path the dispatch keymap should not allow per AC-5). It is a defensive surface, not a primary error path.

## 7. Tool trait — added method (full signature)

```rust
// MODIFIED: crates/modeltap-core/src/tool.rs

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    // ... existing 6 methods (name, accepted_formats, discover, link,
    //     delete_one, delete_all) unchanged ...

    /// Delete every file in a folder-group from this tool's storage.
    ///
    /// Per ADR-010, the default impl returns `Err(DeleteError::Unsupported)`
    /// so plugins that do not have a folder-grouped layout (Ollama,
    /// llama-cli, LM Studio) compile and respond coherently. The HF plugin
    /// overrides.
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
```

## 8. Relationship to existing types

```
ModelMeta            (existing, in types.rs)
   |
   |  group_by_hf_repo(hf_models, sidecars_by_repo)
   v
FolderGroup          (NEW, in types.rs)
   |
   |  classify_unique_vs_shared(folder, inventory, capabilities)
   |     internally calls compute_indicator(per child)  ← THE SINGLE-ENGINE SEAM
   v
FolderClassification (NEW, in types.rs)
   |
   |  build_folder_delete_plan(folder, classification)
   v
FolderDeletePlan     (NEW, in types.rs)
   |
   |  Tool::delete_folder(plan)  ← async, plugin-side; HF overrides default
   v
Vec<DeleteOutcome>   (EXISTING — reused as-is)
   |
   |  aggregate into LastAction
   v
LastAction           (EXISTING — orchestration produces, TUI renders)
```

Every arrow in this pipeline is either a pure function (no side effects) or a single trait method call. There are no hidden dispatches, no global state, no implicit dependencies between steps. Every intermediate value is `Serialize` so the diagnostic-log path (parent feature) can capture the full transcript.

## 9. Test-fixture sketches (DELIVER will materialize)

For software-crafter's reference — the synthetic-inventory shapes the unit tests in `logic::folder_group` will use:

- **Empty inventory** — `group_by_hf_repo` returns `Vec::new()`.
- **One HF model, no sidecars** — single FolderGroup with `models.len() == 1`, `sidecars.len() == 0`. `file_count() == 1`.
- **One HF model, three sidecars** — single FolderGroup with `models.len() == 1`, `sidecars.len() == 3`. `file_count() == 4`.
- **20 HF models, 1 shared with synthetic Ollama, 3 sidecars** — single FolderGroup. `classify_unique_vs_shared` returns `unique.len() == 19`, `shared.len() == 1`, `shared[0].other_tools == vec![ToolId("ollama")]`.
- **Mixed authors** — two HF models, one under `bartowski/foo`, one under `meta-llama/bar`. `group_by_hf_repo` returns 2 FolderGroups.
- **Tentative dedup keys only** — 5 HF models, all with `DedupKey::Tentative(...)`. `compute_indicator` returns `Compatible` for each (conservative-when-uncertain — single tool, format-compatible elsewhere) or `FormatLocked`. `classify_unique_vs_shared` puts all 5 in `unique`. PROPERTY: tentative keys never yield `shared` classification (R1 mitigation from architecture-design §10).

These fixtures are described, not pre-written; software-crafter writes them during the first acceptance-driven RED → GREEN cycle in DELIVER.
