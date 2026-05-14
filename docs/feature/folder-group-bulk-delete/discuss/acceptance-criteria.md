# Acceptance Criteria — folder-group-bulk-delete

Consolidated AC index for the single story (US-05c). Each AC is observable, testable, and traces back to a UAT scenario in `user-stories.md` or a scenario in `journey-folder-group-delete.feature`.

## US-05c: Delete a whole Hugging Face folder group

| AC | Criterion | Source |
|---|---|---|
| US-05c.AC-1 | The Hugging Face right pane groups files by their parent `<author>/<repo>/` directory under collapsible folder header rows | UAT: "right pane groups files under repo folder headers" |
| US-05c.AC-2 | Each folder header shows: repo path, total file count (models + sidecars), total bytes, unique/shared split, `[+]`/`[-]` indicator | UAT: "right pane groups files under repo folder headers" |
| US-05c.AC-3 | Folder header rows are cursor-targetable with up/down arrows; sidecar child rows are not | feature: "Expanding a folder header shows model and sidecar children" |
| US-05c.AC-4 | `Shift+F` on a folder header opens the folder-delete dialog within 200ms | UAT: "Pressing Shift+F on a folder header opens the typed-confirmation dialog" |
| US-05c.AC-5 | `Shift+F` on a non-folder row OR when the active tool is not Hugging Face is a no-op (no dialog, no destructive action) | UAT: "Shift+F is a no-op when the active tool is not Hugging Face"; feature: "Pressing [F] on a non-folder row is a no-op" |
| US-05c.AC-6 | The folder-delete dialog shows: folder path, absolute on-disk path, count of unique files, count of shared files (and which other tools), count of sidecars, Reclaim bytes, Retained bytes, running-tool warning if any | UAT: "Pressing Shift+F on a folder header opens the typed-confirmation dialog" |
| US-05c.AC-7 | `Reclaim + Retained == folder_group.total_bytes` (within rounding) in the dialog AND the post-action summary | shared-artifacts-registry integration checkpoint |
| US-05c.AC-8 | The user must type the folder path `<author>/<repo>` exactly (case-sensitive, byte-exact) and press Enter; anything else cancels with no destructive action | UAT: "Correct typed-confirmation executes the folder-delete"; UAT: "Wrong typed path cancels the folder-delete" |
| US-05c.AC-9 | `Esc` cancels the dialog at any point with no destructive action | feature: "Esc cancels the folder delete at any point" |
| US-05c.AC-10 | On execution, unique files are fully unlinked; shared files have only their HF path unlinked (other tool's hardlink keeps the inode alive); sidecars are unlinked | UAT: "Correct typed-confirmation executes the folder-delete"; UAT: "Shared file's other-tool hardlink survives folder-delete" |
| US-05c.AC-11 | After all files are processed, the now-empty `models--<author>--<repo>/` directory tree is removed | UAT: "Correct typed-confirmation executes the folder-delete" |
| US-05c.AC-12 | On partial failure, successfully-deleted files stay deleted; failed files remain on disk with their reason; the post-action summary itemises both | UAT: "Partial failure when Ollama holds files open"; feature: "Permission failure on individual file" |
| US-05c.AC-13 | Per-file unique-vs-shared classification uses `compute_compatibility()` (US-09 machinery) — no parallel implementation | shared-artifacts-registry integration checkpoint; requirements F-FGD-1, F-FGD-5 |
| US-05c.AC-14 | Sidecar enumeration (README.md, .imatrix, .gguf.urls, plus HF-internal sidecars) is owned by the HF plugin, not hardcoded in modeltap-core | requirements F-FGD-3, F-FGD-5; B-FGD-2 |
| US-05c.AC-15 | If the HF cache directory is read-only, modeltap refuses BEFORE opening the dialog with a clear message ("Hugging Face cache is read-only -- cannot delete folder") | feature: "HF cache read-only refuses before opening the dialog" |
| US-05c.AC-16 | The post-action summary shows: action name, success/partial status, `N of M files removed`, Reclaimed bytes, Retained bytes (with which other tools), per-file failures | UAT: "Post-action summary shows reclaim and retain bytes"; UAT: "Partial failure when Ollama holds files open" |
| US-05c.AC-17 | The summary bar's `total.disk_usage` refreshes within 500ms of action completion (extends US-11 invariant) | feature: "Summary bar totals refresh consistently after folder delete" |
| US-05c.AC-18 | The bottom bar shows `[F] folder-delete` shortcut, dimmed when not applicable to current focus | requirements F-FGD-1, F-FGD-2 |
| US-05c.AC-19 | `[F]` is registered in `ui::shortcuts::SHORTCUT_TABLE` — the bottom bar render and dispatch table share a single source (extends US-08 invariant) | shared-artifacts-registry: `keyboard_shortcuts` parent artifact updated |
| US-05c.AC-20 | If the folder's `absolute_path` no longer exists at execution time (deleted out-of-band between launch and Shift+F), the dialog refuses with "folder no longer exists -- inventory will refresh" and triggers a re-discovery; no destructive action occurs | requirements F-FGD-8 second pre-flight check (RF-1) |

## Cross-Story / Integration ACs (folder-delete extending parent journey)

| AC | Criterion |
|---|---|
| INT-FGD-1 | `total.disk_usage` (summary bar) == sum of `tool.disk_usage` (left pane) at all times — including after folder-delete (extends parent INT-1) |
| INT-FGD-2 | `folder_group.unique_count + folder_group.shared_count + len(folder_group.sidecars) == folder_group.file_count` |
| INT-FGD-3 | `folder_group.bytes_to_reclaim + folder_group.bytes_to_retain == folder_group.total_bytes` (within rounding) |
| INT-FGD-4 | After a successful folder-delete, every previously-shared file's other-tool path still stat()s to a live inode (cross-tool hardlink preservation) |
| INT-FGD-5 | After a successful folder-delete, the HF plugin's `list_models()` and `list_folder_groups()` do not list the deleted folder or its files |
| INT-FGD-6 | After folder-delete: `new total.disk_usage == old total.disk_usage - last_action.bytes_reclaimed` (within rounding) — extends parent INT-5 |
| INT-FGD-7 | Folder-delete dialog `typed_input` string compared byte-exact to `folder_group.path` (the canonical artifact) — no hardcoded literal in dispatch |
| INT-FGD-8 | Existing parent ACs (US-01..US-20 + US-05b, INT-1..INT-7) continue to pass after this feature is introduced (regression gate) |

## Total: 1 story, 20 ACs, 8 cross-story integration ACs.
