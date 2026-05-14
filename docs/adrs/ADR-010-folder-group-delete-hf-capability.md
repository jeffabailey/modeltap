# ADR-010: Folder-Group Delete — HF Capability via Trait Default-Method

## Status

Accepted (2026-05-11). Closes Q-FGD-1 (trait shape) and Q-FGD-2 (concurrency model) from `docs/feature/folder-group-bulk-delete/discuss/wave-decisions.md`.

## Context

DISCUSS for the `folder-group-bulk-delete` feature locks the following constraints (see `wave-decisions.md` / `requirements.md`):

- **HF plugin only in v1** (B-FGD-1 / intake scope constraint #1). Ollama uses content-addressed blobs with no folder semantics; llama-cli uses flat directories; LM Studio has no canonical repo-folder unit.
- **Hotkey `Shift+F`, byte-exact typed confirmation of `<author>/<repo>`** (D-FGD-1, D-FGD-2).
- **Per-file unique/shared classification via the parent's US-09 `compute_indicator` engine** (D-FGD-4 / AC-13). No parallel implementation.
- **Sidecar enumeration owned by the HF plugin** (D-FGD-5 / AC-14 / B-FGD-2).
- **Partial-failure continue-and-report; no rollback** (D-FGD-6 / AC-12).

The existing `Tool` trait (ADR-001 + ADR-009) defines 6 methods and is FROZEN in the sense that "adding a 5th tool requires zero changes to `modeltap-core`." Folder-delete is a destructive capability that maps cleanly onto the `Tool` abstraction (it is "delete a unit of work from THIS tool"), but it is meaningful in v1 only for the HF plugin.

Two open architectural questions had to be closed before this feature can move into DELIVER:

- **Q-FGD-1:** Does the `Tool` trait grow `delete_folder`, or does the HF plugin expose this via a downcast / capability subtrait / plugin-private method?
- **Q-FGD-2:** Concurrency model — per-file detect-and-prompt-then-retry (matching ADR-009 / intake Q5), or a stricter folder-level lock?

## Decision

### Q-FGD-1: Add `delete_folder` to `Tool` with a default body returning `Err(DeleteError::Unsupported)`.

```rust
// crates/modeltap-core/src/tool.rs — added to the existing trait

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    // ... existing 6 methods unchanged ...

    /// Delete every file in a folder-group from this tool's storage.
    /// Default returns `Err(DeleteError::Unsupported)`; plugins with a
    /// folder-grouped layout override.
    async fn delete_folder(
        &self,
        plan: &FolderDeletePlan,
    ) -> Result<Vec<DeleteOutcome>, DeleteError> {
        let _ = plan;
        Err(DeleteError::Unsupported { tool: self.name() })
    }
}
```

The HF plugin overrides; Ollama / llama-cli / LM Studio inherit the default and compile without source changes.

### Q-FGD-2: Per-file detect-and-prompt-then-retry, inherited from ADR-009.

No folder-level lock. The HF plugin's `delete_folder` iterates the plan's paths sequentially. The pre-action soft warning from US-17 runs once (over all paths in the folder); per-file EBUSY surfaces as a per-entry failure in the returned `Vec<DeleteOutcome>` and is rendered in the post-action partial-failure summary. The user closes the offending tool and retries `Shift+F` on the (now-shorter) folder.

This concurrency decision is captured inline in this ADR rather than as ADR-011 because it is "the same pattern as ADR-009" — no new architectural commitment.

## Alternatives Considered (for Q-FGD-1)

### A (CHOSEN) — Trait method with default body

```rust
async fn delete_folder(&self, plan: &FolderDeletePlan)
    -> Result<Vec<DeleteOutcome>, DeleteError>
{
    Err(DeleteError::Unsupported { tool: self.name() })
}
```

**Pros:**

- Preserves the object-safe `Box<dyn Tool>` discipline from ADR-001. No downcast machinery, no `dyn Any`, no marker subtrait.
- Source-compatible with existing plugins: Ollama / llama-cli / LM Studio compile without changes (the default body covers them).
- The trait remains the single port the orchestrator dispatches against. `modeltap-app::orchestration::execute_folder_delete` calls `tool.delete_folder(plan)` symmetrically with how `execute_zap` calls `tool.delete_all()`.
- Plugin contract test extends cleanly: each plugin's contract test asserts either "returns `Unsupported`" or "honors the folder-delete contract."
- Rust-idiomatic — default-body trait methods are the standard escape hatch for adding capabilities to a trait without forcing every implementer to write a stub.

**Cons:**

- The `Tool` trait grows from 6 to 7 methods. ADR-001's "FROZEN SURFACE" comment in `tool.rs` is mildly weakened — the trait IS being extended.
- Plugin authors who add a new folder-aware tool must remember to override; a missing override silently falls through to "Unsupported," which is detectable via the contract test but is not a compile-time error.

**Why this is the right trade-off:** the "FROZEN" property in ADR-001 was always "frozen against breaking changes to existing methods" rather than "frozen against any extension forever." Default-body methods are the most common, most-Rust-idiomatic way to extend a trait without breaking existing implementations. The contract-test guard catches the missing-override case at the testing layer, which is where this discipline belongs (an ad-hoc "MUST override" requirement at the type system would not be more correct).

### B (REJECTED) — Capability subtrait `FolderDeleteCapable: Tool`

```rust
#[async_trait::async_trait]
pub trait FolderDeleteCapable: Tool {
    async fn delete_folder(&self, plan: &FolderDeletePlan)
        -> Result<Vec<DeleteOutcome>, DeleteError>;
}

// HF plugin impl FolderDeleteCapable for HfPlugin { ... }

// Orchestrator dispatch:
let tool: &dyn Tool = registry.get("hf");
let folder_capable: &dyn FolderDeleteCapable = downcast_to_folder_capable(tool)?;
// ...
```

**Pros:**

- The `Tool` trait stays at 6 methods. ADR-001's FROZEN property is technically preserved.
- A subtrait communicates "this is an optional capability" more loudly than a default-body method.

**Cons:**

- **Breaks object-safety in the way the orchestrator dispatches.** `&dyn Tool` cannot be downcast to `&dyn FolderDeleteCapable` directly in Rust without either (a) a `dyn Any` escape hatch the codebase doesn't have, (b) a duplicate `Tool` registry indexed by capability, or (c) a per-trait-method bespoke downcast helper. Each option introduces complexity that the default-body approach avoids.
- Adding more optional capabilities later (e.g., a future "snapshot" or "verify" capability) multiplies the downcast machinery. Linear growth in trait count = quadratic growth in downcast logic.
- The orchestrator's call site becomes asymmetric: `delete_one`, `delete_all`, `link` go through `&dyn Tool`; `delete_folder` goes through a different cast path. Tests have two dispatch shapes to cover.
- Rust ecosystems that adopted "capability subtrait" patterns (e.g., some async-IO library designs) have generally migrated AWAY from this style as default-body methods stabilized. The 2024-2025 Rust trend is "default-body trait method" for optional capabilities.

**Rejected** for object-safety friction and orchestrator-asymmetry.

### C (REJECTED) — Plugin-private API; modeltap-app knows HfPlugin by name

```rust
// plugins/hf/src/lib.rs
impl HfPlugin {
    pub async fn delete_folder(&self, plan: &FolderDeletePlan)
        -> Result<Vec<DeleteOutcome>, DeleteError> { ... }
}

// modeltap-app/src/orchestration/execute_folder_delete.rs
use modeltap_plugin_hf::HfPlugin;
let hf: &HfPlugin = registry.get_concrete::<HfPlugin>()?;
hf.delete_folder(plan).await?
```

**Pros:**

- Zero pollution of the `Tool` trait. Other plugins do not even SEE the new capability.
- Most "honest" — folder-delete genuinely is HF-only in v1.

**Cons:**

- **Violates component-boundaries §R5 / the parent's architecture lint.** Only `modeltap-app` may depend on concrete plugin crates, but the orchestrator currently dispatches through the `Tool` trait OBJECT — never by concrete type. This option would require either (a) a new "concrete plugin downcast" registry, or (b) a direct `use modeltap_plugin_hf::HfPlugin` in the orchestrator, which other plugins can imitate and which destroys the "5th tool = zero core changes" property.
- A second plugin family that gains folder-delete in v2 must also be known by concrete type — the orchestrator becomes a per-plugin `match`.
- The plugin contract test cannot exercise folder-delete uniformly; every folder-aware plugin needs bespoke acceptance test infrastructure.

**Rejected** for boundary violation and v2 scalability.

## Consequences

### Positive

- **The plugin port stays the plugin port.** Adding a 5th tool is still "implement `Tool` and register; default-body methods handle anything you don't support." US-18 / K4 preserved.
- **Source-compatible extension.** No `Cargo.toml` change in non-HF plugins; no `unimplemented!()` stubs; no test churn.
- **Object-safety preserved.** `Vec<Box<dyn Tool>>` continues to work without downcast machinery.
- **Architecture lint unchanged.** R1–R6 in `component-boundaries.md` pass without modification (verified in `docs/feature/folder-group-bulk-delete/design/component-boundaries.md` §"Build-time enforcement").
- **Symmetric dispatch.** `execute_folder_delete` calls `tool.delete_folder(plan)` the same way `execute_zap` calls `tool.delete_all()`. Tests mock the same shape.

### Negative

- **Trait grows from 6 to 7 methods.** ADR-001's FROZEN comment in `tool.rs` needs a clarifying note: "frozen against breaking changes; extensions via default-body methods are permitted with an ADR."
- **Silent fall-through risk if a future folder-aware plugin misses the override.** Mitigated by the plugin contract test in `crates/modeltap-core/tests/folder_delete_contract.rs`, which asserts each `T: Tool` either returns `Unsupported` OR honors the folder-delete contract — no third state.
- **The default body's `DeleteError::Unsupported` variant adds one enum variant.** Existing exhaustive matches on `DeleteError` must add an arm (`Unsupported { tool } => ...`). This is a one-time surface change; the compiler enforces the update.

### Concurrency (Q-FGD-2 inheritance)

- **No file-locking crate** added (see `technology-stack.md`).
- **No PID-detection complexity** beyond the soft pre-action check (US-17, intake Q5). The existing `FsProbe::processes_holding` port is the only running-tool detection mechanism used.
- **Partial-failure UX is the contract.** AC-12, AC-16, and the journey's Step 4 partial-failure mockup all describe the user-facing behavior. The plugin returns `Vec<DeleteOutcome>` and the orchestrator's `LastAction.detail` renders per-file failures.
- **Re-run after closing the offending tool is the recovery path.** The folder header re-appears in the right pane on the next inventory rebuild (stateless rediscovery — parent constraint C7).

## Implementation Guidance (for DELIVER)

This ADR specifies WHAT, not HOW. Software-crafter owns the unlink-loop body, the sidecar enumeration order, the empty-tree cleanup heuristic, and the per-file `tracing` span shape. The following are constraints, not implementations:

- The trait method signature is fixed at `async fn delete_folder(&self, plan: &FolderDeletePlan) -> Result<Vec<DeleteOutcome>, DeleteError>`.
- The default body MUST return `Err(DeleteError::Unsupported { tool: self.name() })`.
- The HF plugin's override MUST iterate the plan's `paths_to_unlink_fully` and `paths_to_unlink_hf_only` separately and produce one `DeleteOutcome` per file.
- The HF plugin's override SHOULD reuse `crate::delete::delete_one_at` for model files to inherit the blob ref-counting from ADR-009; sidecars use direct `std::fs::remove_file`.
- After per-file unlinks complete, the override MUST attempt `remove_dir` on the now-empty subdirectories in the `models--<author>--<repo>/` tree. Failure to remove a non-empty subdir is silently absorbed (it represents the partial-failure case).
- Pre-flight checks (cache-writeable, folder-exists) live in `modeltap-app::orchestration::execute_folder_delete`, not in the plugin. The plugin assumes a valid plan against an existing folder.

## Test Scenarios

The plugin contract test in `crates/modeltap-core/tests/folder_delete_contract.rs` covers (for each `T: Tool`):

1. **`delete_folder` on a non-folder-aware plugin** (Ollama / llama-cli / LM Studio): returns `Err(DeleteError::Unsupported { tool: <plugin> })`. No filesystem mutation.
2. **`delete_folder` on a folder-aware plugin** (HF): the body of the contract test runs against `tempfile`-backed fixtures and asserts the contract from `data-models.md` §7. Sub-scenarios:
   - Empty folder (only sidecars): all sidecars unlinked; folder dir removed.
   - All-unique folder: all model files + sidecars unlinked; `bytes_freed` sum matches plan's `bytes_to_reclaim`.
   - Mixed unique+shared folder: unique files unlinked fully; shared files have only HF path unlinked; cross-tool hardlink (verified via post-condition stat) survives.
   - Partial failure: one model file set read-only; the rest unlink successfully; failed entry's `registration_removed: false`.
   - Idempotent retry: after partial failure, re-running `delete_folder` on the (smaller) folder completes the remaining work.

These are scenarios — `acceptance-designer` (DISTILL wave) writes the executable form.

## Cross-references

- ADR-001 (Plugin dispatch) — establishes the `Box<dyn Tool>` object-safe trait pattern this ADR extends.
- ADR-009 (Single-model delete) — establishes the `delete_one` / `delete_all` symmetry that `delete_folder` follows. Establishes the per-file detect-and-retry concurrency pattern Q-FGD-2 inherits.
- `docs/feature/folder-group-bulk-delete/design/architecture-design.md` — full feature design referencing this ADR.
- `docs/feature/folder-group-bulk-delete/design/data-models.md` — `FolderGroup`, `FolderDeletePlan`, `DeleteError::Unsupported` shapes.
- `docs/feature/folder-group-bulk-delete/design/component-boundaries.md` — closure of Q-FGD-3 (no new folder dedup-key artifact).
