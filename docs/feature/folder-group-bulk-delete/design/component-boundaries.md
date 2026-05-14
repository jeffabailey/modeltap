# Component Boundaries — folder-group-bulk-delete

**Wave:** DESIGN (3 of 6) — brownfield extension
**Parent boundaries:** `docs/feature/modeltap-tui/design/component-boundaries.md`

The parent's six dependency rules (R1–R6) and crate layout remain authoritative. This document specifies (a) which existing modules are touched, (b) which new sub-modules are added, (c) how the single-engine invariant from D-FGD-4 is enforced at the call-site, and (d) the closure of Q-FGD-3 (no new folder dedup-key artifact).

## Module-level deltas

### `crates/modeltap-core/`

```
crates/modeltap-core/
└── src/
    ├── tool.rs                              # MODIFIED — adds Tool::delete_folder with default body
    ├── types.rs                             # MODIFIED — adds FolderGroup, FolderDeletePlan,
    │                                        #             FolderDeleteOutcome, Sidecar, DeleteError::Unsupported
    └── logic/
        ├── mod.rs                           # MODIFIED — registers folder_group module
        ├── compatibility.rs                 # UNCHANGED — read-only by classify_unique_vs_shared
        └── folder_group.rs                  # NEW — pure: group_by_hf_repo, classify_unique_vs_shared,
                                             #             build_folder_delete_plan
```

**Boundary rules unchanged.** `folder_group.rs` is a pure-logic module: depends on `domain::*`, `types::*`, `logic::compatibility::compute_indicator`. No I/O, no async, no tokio.

### `plugins/hf/`

```
plugins/hf/
└── src/
    ├── lib.rs                               # MODIFIED — adds delete_folder trait impl (delegating to folder_delete)
    └── folder_delete.rs                     # NEW — sidecar enumeration, per-file unlink loop,
                                             #         empty-tree cleanup; reuses delete_one_at for model files
```

**Boundary rules unchanged.** `folder_delete.rs` depends on `modeltap-core` (types only) and re-uses `crate::delete::delete_one_at` + `crate::cache_walk::list_snapshot_files`. Does not import any other plugin.

### `crates/modeltap-tui/`

```
crates/modeltap-tui/
└── src/
    ├── input/
    │   └── keymap.rs                        # MODIFIED — adds Shift+F to SHORTCUT_TABLE (the single source);
    │                                        #             dispatches Msg::RequestFolderDelete
    └── view/
        ├── two_pane.rs                      # MODIFIED — adds folder header row type, [+]/[-] indicator,
        │                                    #             indented children, dim sidecar rows
        └── folder_confirm_dialog.rs         # NEW — typed-confirm dialog for folder-delete; mirrors confirm_dialog
```

**Boundary rules unchanged.** The TUI still has no I/O deps and no plugin deps (R3, R4).

### `crates/modeltap-app/`

```
crates/modeltap-app/
└── src/
    └── orchestration/
        ├── execute_folder_delete.rs         # NEW — builds FolderDeletePlan, runs pre-flight checks
                                             #         (cache-writeable + folder-exists), calls Tool::delete_folder,
                                             #         aggregates outcomes into LastAction
```

**Boundary rules unchanged.** Only the app crate composes plugins and pure logic (R5).

## New module: `modeltap-core::logic::folder_group`

This is the load-bearing addition. Its public surface (signatures only — code-as-documentation; software-crafter writes the bodies) is:

```rust
// crates/modeltap-core/src/logic/folder_group.rs

use crate::types::{FolderGroup, FolderDeletePlan, FolderClassification, ModelMeta, Sidecar, ToolId};
use crate::logic::compatibility::{compute_indicator, PluginCapabilityMap};
use crate::domain::RowIndicator;

/// Partitions HF ToolInventory.models by the `<author>/<repo>` prefix of
/// `id_in_tool`. Sidecars are supplied by the caller (the HF plugin owns
/// sidecar enumeration per AC-14 / B-FGD-2).
///
/// Pure function; no I/O. Deterministic order.
pub fn group_by_hf_repo(
    hf_models: &[ModelMeta],
    sidecars_by_repo: &std::collections::BTreeMap<String, Vec<Sidecar>>,
) -> Vec<FolderGroup>;

/// Per-child-model classification using the parent's US-09 compatibility
/// engine. THE SINGLE SOURCE OF TRUTH for shared/unique decisions
/// (D-FGD-4 / AC-13). Does NOT re-implement dedup-key comparison or
/// hardlink-presence detection.
///
/// Pure function; no I/O.
pub fn classify_unique_vs_shared(
    folder: &FolderGroup,
    inventory: &[ModelMeta],
    capabilities: &PluginCapabilityMap,
) -> FolderClassification;

/// Builds the immutable plan the orchestrator passes to the plugin.
/// Freezes bytes_to_reclaim (unique + sidecars) and bytes_to_retain (shared).
///
/// Pure function; no I/O.
pub fn build_folder_delete_plan(
    folder: &FolderGroup,
    classification: &FolderClassification,
) -> FolderDeletePlan;
```

**Single-engine invariant enforced at the call-site.** `classify_unique_vs_shared` is the ONLY function in the codebase that produces a `FolderClassification`. Its body MUST call `compute_indicator` for each child model. The peer reviewer (and the unit tests software-crafter will write) checks this by inspection: any classification path that does not route through `compute_indicator` is a bug.

A property test in DELIVER asserts: for every synthetic `Inventory` where `compute_indicator(m, _, _) == Compatible | FormatLocked` (single-tool), `classify_unique_vs_shared(folder, _, _).unique.contains(m)`. And: for every `m` where `compute_indicator(m, _, _) == Shared`, `classify_unique_vs_shared(folder, _, _).shared` contains `m` paired with its other-tool names.

## Trait extension: dependency-inversion seam preserved

The added `Tool::delete_folder` method has a **default body**. Three consequences for boundaries:

1. **Plugins not implementing folder-delete need no changes.** Ollama, llama-cli, LM Studio inherit the default and compile. This preserves the parent's R5 invariant ("only app composes plugins"): no plugin source change means no `Cargo.toml` change means no dependency-graph regression.
2. **The HF plugin override is the only plugin-specific surface.** It depends on `modeltap-core::types::{FolderGroup, FolderDeletePlan, DeleteOutcome, DeleteError}` plus its own internal modules. No new cross-plugin dependency.
3. **Architecture lint coverage unchanged.** The parent's `tests/architecture.rs` (R1–R6) does not need updating; the new modules sit inside existing crates and inherit those crates' dependency rules.

## Plugin contract test extension

A new parameterized contract test in `crates/modeltap-core/tests/folder_delete_contract.rs` asserts, for every `T: Tool`:

- `delete_folder` is either (a) the unmodified default — returns `Err(DeleteError::Unsupported)` — OR (b) honors the folder-delete contract.
- Contract (b) — folder-aware plugins:
  - Returns `Vec<DeleteOutcome>` with one entry per file attempted (model files + sidecars).
  - Each entry's `registration_removed` + `file_deleted` + `bytes_freed` accurately describes the post-state.
  - `bytes_freed` summed over successful unique-file deletions matches the plan's `bytes_to_reclaim`.
  - Cross-tool hardlinks (other tools' paths to the same inode) survive the delete — verified by post-condition `stat()` returning a live inode for any shared-file path outside the HF cache.
  - Partial failure: if any unlink fails (simulated via read-only file), other unlinks succeed and the returned `Vec` contains the per-failure entry with `registration_removed: false`.
  - Idempotence on retry: re-running `delete_folder` on a now-partial folder removes the remaining files without surprising errors.

The HF plugin's contract test runs path (b). Each of Ollama / llama-cli / LM Studio plugins' contract tests run path (a) — they each add a one-line test asserting their `delete_folder` returns `Err(Unsupported)`.

## Closure of Q-FGD-3 — no new folder dedup-key artifact

**Decision: no new artifact materialized. `folder_group.path` IS canonical by construction.**

Rationale:

1. The HF cache layout uses `models--<author>--<repo>/` as its directory naming convention. The `<author>/<repo>` string is computable directly from the directory name and is identical to the HF repo identifier on the hub. There is no possible ambiguity at the source-of-truth level.
2. The typed-confirmation comparator already reads `folder_group.path` (the canonical artifact — INT-FGD-7 in shared-artifacts-registry). A second key would either (a) duplicate this, creating a drift risk, or (b) shadow it, creating a question of which is authoritative.
3. The feature is HF-only in v1 (B-FGD-1). There are no cross-tool folder operations to futureproof — a folder dedup key would only matter if a future plugin family also had folder-grouped delete semantics, at which point the design can introduce the artifact with the cross-tool data it actually needs.
4. The parent's `ModelMeta.dedup_key` exists because two `ModelMeta` values with the same SHA256 are the same logical model across different tools. There is no analogous identity question for folders: an HF repo folder is a single in-cache concept, not a cross-tool one.

This decision is captured in `shared-artifacts-registry.md` § "Open Source-of-Truth Questions" as RESOLVED. No new artifact row is added. The existing `folder_group.path` row remains the canonical entry.

## Closure of Q-FGD-2 — concurrency model

**Decision: Option A — per-file detect-and-prompt-then-retry, inherited from ADR-009 / intake Q5.**

The DISCUSS partial-failure semantics (D-FGD-6) already commit to per-file continuation with reporting. A folder-level lock (Option B) would either (a) require atomicity over many unlinks that the filesystem does not support, or (b) refuse the entire operation if any single file is busy — both contradict the user-facing contract in AC-12. Option A keeps the existing one-pattern-fits-all behavior and shares the running-tool detection (US-17) the parent already implements. No new ADR is required; the inheritance is captured inline in ADR-010 § "Concurrency model".

## Testing surfaces summary

| Layer | What's testable | Tools |
|---|---|---|
| `modeltap-core::logic::folder_group` (pure) | Grouping invariants, classification correctness, plan-building math | `cargo test -p modeltap-core` + `proptest` |
| `Tool::delete_folder` (trait) | Default behavior; per-plugin overrides | `crates/modeltap-core/tests/folder_delete_contract.rs` parameterized |
| `plugins/hf::folder_delete` | Sidecar enumeration, unlink loop, empty-tree cleanup, ref-count preservation | `plugins/hf/tests/folder_delete_contract.rs` with `tempfile` fixtures |
| `modeltap-app::orchestration::execute_folder_delete` | Pre-flight checks, plan dispatch, post-action aggregation | Acceptance tests in `tests/acceptance/` with a fake HF plugin |
| `modeltap-tui::view::folder_confirm_dialog` | Dialog content, typed-input handling, byte-exact comparison | `insta` snapshots + `ratatui::backend::TestBackend` |
| `modeltap-tui::view::two_pane` | Folder header row rendering, [+]/[-] indicator, cursor targetability | `insta` snapshots |
| `modeltap-tui::input::keymap` | Shift+F dispatch; dim when not applicable | Unit tests against `SHORTCUT_TABLE` |

## Build-time enforcement

The parent's architecture-lint integration test (`tests/architecture.rs`) parses `cargo metadata --format-version 1` and enforces R1–R6. This feature passes all six without modification because:

- R1 (core has no plugin deps): `folder_group.rs` depends only on `crate::*` and `std`.
- R2 (plugins don't depend on each other): `plugins/hf/folder_delete.rs` depends only on `modeltap-core` + plugin-internal modules.
- R3 (TUI has no plugin deps): `folder_confirm_dialog.rs` depends only on `modeltap-core::types` + `ratatui` + `crossterm`.
- R4 (TUI has no I/O deps): no new I/O dep introduced.
- R5 (app is the only assembler): only `modeltap-app::orchestration::execute_folder_delete` imports concrete plugin crates (and even then, only via the `Tool` trait object — same pattern as `execute_zap`).
- R6 (core is leaf-y): no tokio / ratatui / crossterm / reqwest / nix added to `modeltap-core`.

## Open items deferred to DELIVER

| Item | Where it's resolved |
|---|---|
| Exact sidecar suffix list (`.md`, `.imatrix`, `.gguf.urls`, plus HF-internal refs/blobs heuristic) | Software-crafter writes the body of `enumerate_sidecars` against the DISCUSS examples and verifies against `tempfile` fixtures |
| Progress-bar render details for ≥10-file folders | `modeltap-tui::view` already has progress-bar primitives from `delete_all`; reuse during DELIVER |
| The `Sidecar` type's exact field set (likely just `path: PathBuf`, `size_bytes: u64`, `kind: SidecarKind`) | See `data-models.md` for the proposed shape; software-crafter finalizes during the first GREEN |
| Truncation threshold for the failed-file list in post-action summary (NF-FGD-1 RF-2 informational allowance) | DELIVER UX choice; documenting "first 10, then 'and N more'" is a reasonable starting point |

None of these are architectural decisions — they are surface-level details software-crafter owns during GREEN + REFACTOR.
