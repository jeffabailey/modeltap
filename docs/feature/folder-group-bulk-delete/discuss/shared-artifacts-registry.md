# Shared Artifacts Registry — folder-group-bulk-delete

Brownfield extension of `docs/feature/modeltap-tui/discuss/shared-artifacts-registry.md`. This file lists only the **new** artifacts introduced by US-05c (folder-delete) plus the **updated reclaim-math rows** where the parent artifacts now also have a folder-scoped consumer.

## Conventions

- **Source of truth** is the canonical producer (a function, file, or process).
- **Consumers** are every place the value is displayed or referenced.
- **Integration risk** is the impact of inconsistency.

## New Artifacts (folder_group.*)

| Artifact | Source of Truth | Consumers | Integration Risk |
|---|---|---|---|
| `folder_group.path` | `core::group_by_hf_repo(plugin.list_models())` — canonical form `<author>/<repo>` | folder header row, folder-delete dialog title, typed-confirmation comparator, post-action summary | CRITICAL — typed confirmation depends on byte-exact match; drift = either deletes the wrong repo or never confirms |
| `folder_group.absolute_path` | HF plugin: `<HF_HOME>/hub/models--<author>--<repo>/` (default `~/.cache/huggingface/`) | folder-delete dialog body, execution unlink loop, directory removal step | CRITICAL — execution targets a path; wrong path = wrong delete |
| `folder_group.file_count` | `len(folder_group.models + folder_group.sidecars)` | folder header row, folder-delete dialog header, post-action summary "N of N files removed" | HIGH — drift hides files or invents them |
| `folder_group.total_bytes` | `sum(model.size for m in models) + sum(sidecar.size for s in sidecars)` | folder header row, folder-delete dialog | HIGH — incorrect total misleads user about disk impact |
| `folder_group.unique_count` | `core::classify_unique_vs_shared(folder_group, inventory).unique.len()` | folder header row, folder-delete dialog "unique to HF" line | HIGH — wrong classification destroys data not retained anywhere |
| `folder_group.shared_count` | `core::classify_unique_vs_shared(folder_group, inventory).shared.len()` | folder header row, folder-delete dialog "also in: <tool>" line | HIGH — wrong classification might claim a hardlinked file is safe to delete when it isn't |
| `folder_group.unique_files[]` | `core::classify_unique_vs_shared(folder_group, inventory).unique` (list of paths) | folder-delete dialog itemisation, execution loop (full unlink), reclaim math | HIGH |
| `folder_group.shared_files[]` | `core::classify_unique_vs_shared(folder_group, inventory).shared` (list of paths + other tools each is also in) | folder-delete dialog "also in" itemisation, execution loop (HF-path-only unlink), retain math | HIGH |
| `folder_group.sidecars[]` | HF plugin: non-model files inside the repo directory tree (README.md, .imatrix, .gguf.urls, refs/<name>, blobs/<hash> exclusive to this repo's snapshot) | folder header sidecar rows, folder-delete dialog sidecar list, execution loop | MEDIUM — sidecar miss = orphan files left behind (cosmetic, not safety) |
| `folder_group.bytes_to_reclaim` | `sum(f.size for f in unique_files) + sum(s.size for s in sidecars)` | folder-delete dialog Reclaim line, post-action message | HIGH — promised reclaim must match actual reclaim |
| `folder_group.bytes_to_retain` | `sum(f.size for f in shared_files)` | folder-delete dialog Retained line, post-action message | HIGH — promised retain must match actual retain |
| `last_action.files_deleted_count` | result of execution loop | post-action summary "N of N files removed" | MEDIUM |
| `last_action.files_failed[]` | result of execution loop — list of `{path, reason}` for files that did not unlink | post-action partial-failure summary, "press [F] again to retry" hint | HIGH — silent partial failure is the worst-case UX |

## Updated parent artifacts (folder-scoped consumers added)

| Artifact (parent registry) | New consumer this feature adds | Integration Risk |
|---|---|---|
| `last_action.bytes_reclaimed` | folder-delete post-action message; summary bar delta after folder-delete | MEDIUM (same as parent) |
| `last_action.bytes_retained` | folder-delete post-action message | MEDIUM (same as parent) |
| `total.disk_usage` | refresh path now also triggered by folder-delete completion (US-11 invariant extended) | HIGH (same as parent) |
| `tool.disk_usage` (Hugging Face) | folder aggregates roll up into this; refresh path triggered after folder-delete | HIGH (same as parent) |
| `keyboard_shortcuts` | gains `[F] folder-delete` entry in `ui::shortcuts::SHORTCUT_TABLE` | HIGH (same as parent) |
| `running_tools[]` | reused unchanged in folder-delete dialog (per-file open detection) | LOW (same as parent) |
| `model.compatible_tools` | drives per-file unique-vs-shared classification inside `folder_group.classify_unique_vs_shared()` | HIGH — same engine, MUST not have a parallel implementation |
| `cli_vocabulary` | gains new terms: "folder group", "folder-delete", "sidecar", "retained" (already in parent), "folder header" | HIGH — terminology drift erodes user trust |

## Open Source-of-Truth Questions (DESIGN must close)

| ID | Artifact | Open question |
|---|---|---|
| Q-FGD-1 | `folder_group.*` | Does the `Tool` trait grow a third method (e.g., `list_folder_groups()` and `delete_folder(folder_path)`), or does the HF plugin alone expose this via a downcast / capability interface? Per intake scope constraint #1, the feature is HF-only in v1 — but trait design is DESIGN's call. |
| Q-FGD-2 | execution loop concurrency | Same detect-and-prompt-then-retry pattern as ADR-009 (intake Q5), or stricter (folder-level lock that fails fast if ANY file is busy)? |
| Q-FGD-3 | `folder_group.dedup_key` analogue | Does the folder-group concept need its own dedup-key (e.g., canonical HF repo id), or is `folder_group.path` already canonical at display time and no second key is needed? |

## Validation Plan (during DESIGN review)

1. Every `${variable}` in `journey-folder-group-delete-visual.md` and `journey-folder-group-delete.yaml` MUST appear in either this table or the parent registry.
2. `folder_group.classify_unique_vs_shared()` MUST be expressed in terms of the parent's `model.compatible_tools` engine (US-09) — no parallel implementation.
3. Typed-confirmation comparator MUST read from `folder_group.path` (no hardcoded literal in the dialog code).
4. Sidecar enumeration MUST come from the HF plugin (not modeltap-core hardcoding `README.md`, `.imatrix`, `.gguf.urls`) — different HF versions may add new sidecar conventions.
5. Open questions Q-FGD-1, Q-FGD-2, Q-FGD-3 must be closed before any code is written for the corresponding artifacts.

## Integration Checkpoints (cross-step invariants)

| Invariant | Steps involved | Failure mode |
|---|---|---|
| `folder_group.file_count == count(folder_group.models) + count(folder_group.sidecars)` | 1, 2 | Header row promises N, dialog itemises M; user confidence broken |
| `folder_group.total_bytes == sum(child.size)` (for both models and sidecars) | 1, 2 | Disk-impact math wrong; user reclaims less than promised |
| `folder_group.bytes_to_reclaim + folder_group.bytes_to_retain == folder_group.total_bytes` | 2 | Dialog math wrong; either promises too much reclaim or strands bytes |
| `last_action.bytes_reclaimed == folder_group.bytes_to_reclaim` (on full success) | 2, 3, 4 | Dialog promised more than was delivered — user cannot trust future dialogs |
| Post-folder-delete: every shared file's other-tool hardlink still resolves via `stat` to a live inode | 3, 4 | Cross-tool inode preservation broken — Ollama (or other tool) copy silently corrupted |
| Post-folder-delete: `~/.cache/huggingface/hub/models--<author>--<repo>/` is gone OR contains only the failed files | 3, 4 | Orphan empty directory tree, or partial state masquerading as clean |
| Folder aggregates roll up into `tool.disk_usage` (Hugging Face) | 1, 4 | Summary bar drift — total != sum of tools (existing parent invariant) |
| `keyboard_shortcuts` displayed in bottom bar matches the actual key handler dispatch table, including new `[F]` | all | App feels buggy / undiscoverable (existing parent invariant) |
| Per-file unique-vs-shared classification uses `compute_compatibility()` (US-09 machinery) | 1, 2 | Two indicator engines, two truths, drift inevitable |
