# Journey: Delete a Hugging Face Folder Group — Visual

**Feature:** folder-group-bulk-delete
**Persona:** Devon Park — Local-AI power user on macOS/Linux. Uses the Hugging Face cache to download many quant variants of the same logical model (e.g., `bartowski/Llama-3.2-1B-Instruct-GGUF/` contains ~20 `.gguf` files plus `README.md`, `.imatrix`, `.gguf.urls`). Has been auditioning quants and now wants the whole repo gone.
**Goal:** Delete every file in a Hugging Face repo folder (`<author>/<repo>/`) in one keystroke + one typed confirmation, sweeping sidecars, with per-file shared/unique accounting that mirrors US-05b's safety rubric.

## Brownfield Context

This journey extends the parent feature's journey (`docs/feature/modeltap-tui/discuss/journey-cleanup-and-unify-visual.md`) at Step 2 (browse) and Step 4 (decide). It introduces a **third deletion granularity** between US-05 (`[z]` whole-tool zap) and US-05b (`[d]` single-model delete). All vocabulary, indicators, post-action message format, and emotional rules from the parent journey carry forward unchanged.

## Emotional Arc

Same proportions as the parent journey — developer-power-user tool, not a consumer flow. The new beat is the middle: the user must feel that "folder group" is a recognised unit, not a stunt.

| Phase | State | What drives it |
|---|---|---|
| Trigger | Mildly annoyed ("I have 20 quants of one model and want them gone") | HF cache bloat from auditioning quants |
| Recognise | Oriented ("oh — the TUI groups them for me") | Collapsible folder row in right pane labelled with repo + aggregate size |
| Deliberate | Deliberate ("I see exactly what disappears, including sidecars") | Folder-delete dialog shows file count, total bytes, per-file shared/unique split, sidecar list |
| Execute | In-control ("I typed the path, I authored this") | Typed confirmation = author/repo string |
| Verify | Satisfied ("21 files, 14.7 GB reclaimed, the Ollama hardlink survived") | Post-action message reuses US-05/US-05b vocabulary: `bytes_freed` + `bytes_retained` |

Failure-mode emotional rule (inherited from parent): every destructive action must end with the user feeling **in control**, not surprised. The folder-delete dialog must make sidecar handling explicit.

## Journey Flow (ASCII)

```
[Trigger: HF repo bloat]                                [End state: repo gone, hardlinks preserved]
"I auditioned 20 quants of                  21 files removed. Sidecars swept.
 Llama-3.2-1B and want the                  bytes_freed=14.7 GB unique, bytes_retained=0.7 GB shared
 whole bartowski/... repo gone."            (one .gguf still hardlinked from Ollama).
       |                                                        ^
       v                                                        |
+------+-------+   +-------+-------+   +-------+--------+   +---+----+-------+
| Step 1       |   | Step 2        |   | Step 3        |   | Step 4         |
| Recognise    |-->| Press [F] on  |-->| Confirm by    |-->| Verify post-   |
| folder group |   | folder header |   | typing path   |   | action summary |
+--------------+   +---------------+   +---------------+   +----------------+
 Feels:             Feels:              Feels:               Feels:
 "TUI groups        "I'm targeting      "I authored          "Bytes match what
  these"             the right thing"    this delete"         the dialog promised"
```

## Step-by-Step Detail

### Step 1: Recognise the folder group

**Context:** Devon has selected Hugging Face in the left pane. The right pane now shows folder-group headers above their child models. This is the **new visual affordance** vs the parent journey: previously each `.gguf` rendered as its own row; now siblings under the same `<author>/<repo>/` collapse under a header row by default.

**TUI mockup (Hugging Face selected, three repos visible, one expanded):**

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Models in Hugging Face (54 files, 92.4 GB)        |
|   Ollama        12   |                                                   |
|   llama-cli     6    | [-] bartowski/Llama-3.2-1B-Instruct-GGUF          |
| > Hugging Face  54   |     21 files, 14.7 GB  (20 unique, 1 shared)      |
|   LM Studio     9    |     * Llama-3.2-1B-Instruct-IQ3_M.gguf  657 MB    |
|                      |     * Llama-3.2-1B-Instruct-IQ4_XS.gguf 743 MB    |
|                      |     o Llama-3.2-1B-Instruct-Q4_K_M.gguf 808 MB    |
|                      |       (also in: Ollama)                           |
|                      |     o Llama-3.2-1B-Instruct-f16.gguf    2.5 GB    |
|                      |     ... 17 more files ...                         |
|                      |     . README.md                          24 KB    |
|                      |     . Llama-3.2-1B-Instruct.imatrix      1.3 MB   |
|                      |                                                   |
|                      | [+] meta-llama/Llama-3-8B-Instruct                |
|                      |     1 file, 16.0 GB                               |
|                      |                                                   |
|                      | [+] TheBloke/something-AWQ                        |
|                      |     1 file, 8.1 GB                                |
+--------------------------------------------------------------------------+
| [<-/->] tools  [up/down] rows  [Enter] expand  [u] unify                 |
| [d] delete-one  [F] folder-delete  [z] zap tool  [?] help [q] quit       |
+--------------------------------------------------------------------------+
```

**New row types in the right pane (extends US-04's row format):**

| Row type | Prefix | Cursor-targetable? | Notes |
|---|---|---|---|
| Folder-group header (collapsed) | `[+]` | Yes — `F` targets this | `<author>/<repo>` + aggregate file count + total bytes + unique/shared split |
| Folder-group header (expanded) | `[-]` | Yes — `F` targets this | Same content, children visible below |
| Model file (child of folder) | `*` / `o` / `!` / `?` | Yes — `d` targets this | Indented one level; same indicators as parent journey US-04/US-09 |
| Sidecar file (child of folder) | `.` (dim) | No — informational | `README.md`, `.imatrix`, `.gguf.urls`; counted in folder aggregates |

**Shared artifacts referenced:** `${folder_group.path}` (e.g., `bartowski/Llama-3.2-1B-Instruct-GGUF`), `${folder_group.file_count}`, `${folder_group.total_bytes}`, `${folder_group.unique_count}`, `${folder_group.shared_count}`, `${folder_group.sidecars[]}`. All sourced from the HF plugin's new `list_folder_groups()` capability (open question Q-FGD-1 — DESIGN may model this via downcast or a separate trait).

**Emotional state:** entry "the cache is a mess" → exit "the TUI sees what I see — one logical model, many quants." Recognition over recall (Nielsen #6): Devon does not have to remember which 21 filenames belong to one repo; the TUI groups them.

**Integration checkpoint:** `folder_group.file_count` MUST equal the number of child rows when expanded. `folder_group.total_bytes` MUST equal sum of child `model.size` plus sidecar sizes. Folder aggregates MUST also roll up into the existing `tool.disk_usage` (parent registry artifact).

---

### Step 2: Press `[F]` on the folder header

**Context:** Devon has navigated his cursor to the `[-] bartowski/Llama-3.2-1B-Instruct-GGUF` header row. He presses **Shift+F** (`F`).

**TUI mockup (folder-delete dialog opens, foregrounded):**

```
+- Delete folder group: bartowski/Llama-3.2-1B-Instruct-GGUF ---------------+
|                                                                           |
| THIS WILL DELETE 21 FILES (14.7 GB) FROM Hugging Face.                    |
|                                                                           |
| Folder path:  ~/.cache/huggingface/hub/models--bartowski--                |
|               Llama-3.2-1B-Instruct-GGUF/                                 |
|                                                                           |
| Contents:                                                                 |
|   20 model files (.gguf)                                                  |
|     -- 19 only registered with Hugging Face (will be permanently deleted) |
|     --  1 also registered with Ollama (HF registration removed; file      |
|         preserved, Ollama copy unaffected)                                |
|   3 sidecar files (README.md, .imatrix, .gguf.urls)                       |
|     -- all swept with the folder                                          |
|                                                                           |
| Disk impact:                                                              |
|   Reclaim:  14.0 GB (unique files + sidecars)                             |
|   Retained: 0.7 GB  (1 file kept alive by Ollama hardlink)                |
|                                                                           |
| Type the folder path to confirm:                                          |
|   [ bartowski/Llama-3.2-1B-Instruct-GGUF                              ]   |
|                                                                           |
| Running tools detected: ollama (PID 4421)  -- file open: 1 of 21          |
|                                                                           |
| [Esc] cancel                                                              |
+---------------------------------------------------------------------------+
```

User must type `bartowski/Llama-3.2-1B-Instruct-GGUF` exactly (case-sensitive). Anything else cancels with no destructive partial state.

**Shared artifacts:** `${folder_group.path}`, `${folder_group.absolute_path}` (the on-disk `~/.cache/huggingface/hub/models--<author>--<repo>/` path), `${folder_group.unique_files[]}`, `${folder_group.shared_files[]}`, `${folder_group.sidecars[]}`, `${folder_group.bytes_to_reclaim}`, `${folder_group.bytes_to_retain}`, `${running_tools[]}` (reused from parent).

**Emotional state:** entry "delete the whole thing" → exit "I see precisely what will and won't be lost, including the .gguf that Ollama hardlinks." Constraint principle (Norman): typing the full author/repo path is the strongest cheap guard for an irreversible 14 GB operation. This matches US-05's zap rubric, not US-05b's `[y/n]` for shared single-file delete — because the bulk operation is irreversible across many files, even if individual files are shared.

**Integration checkpoint:** `bytes_to_reclaim + bytes_to_retain == folder_group.total_bytes` (within rounding). Per-file classification (unique vs shared) MUST use the same `compute_compatibility()` machinery that drives the parent journey's row indicators — no second implementation, no drift.

---

### Step 3: Confirmation and execution

**Context:** Devon types `bartowski/Llama-3.2-1B-Instruct-GGUF` and presses Enter. modeltap performs the delete.

**Execution rules:**

1. **Per-file**, classified `unique` or `shared`:
   - `unique` (.gguf only in HF, or sidecar): unlink the file on disk; if it was the last reference, the inode is gone.
   - `shared` (.gguf hardlinked into another tool): unlink the HF-side path only. The inode survives via the other tool's link. modeltap does NOT touch the other tool's registration.
2. **Sidecars** (`README.md`, `.imatrix`, `.gguf.urls`, plus any HF-internal refs/blobs that belong exclusively to this repo's snapshot): treated as unique; unlinked.
3. **Directory cleanup**: after all files are unlinked, the now-empty `~/.cache/huggingface/hub/models--<author>--<repo>/` directory tree is removed (snapshots/, refs/, blobs/ subdirs — same scope as the HF plugin's discovery walk).
4. **Partial failure**: if any individual unlink fails (permission, file-open, EBUSY), modeltap continues with remaining files. The post-action summary itemises successes and failures. No rollback — successfully-deleted files stay deleted (consistent with US-19's "skip / copy / cancel" precedent: never leave the filesystem in a worse state than starting). Failed files remain on disk and are re-listed on next inventory rebuild.

**Mid-execution mockup (transient, ~200ms-2s):**

```
+- Deleting folder: bartowski/Llama-3.2-1B-Instruct-GGUF ------------------+
|                                                                          |
|  Removing files...                                                       |
|  [################################------]  16 / 21                       |
|                                                                          |
+--------------------------------------------------------------------------+
```

---

### Step 4: Verify outcome (post-action summary)

**TUI mockup (return to main view, folder header gone):**

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Last action: folder-delete                        |
|   Ollama        12   |   bartowski/Llama-3.2-1B-Instruct-GGUF (success)  |
|   llama-cli     6    |                                                   |
| > Hugging Face  33   |   21 of 21 files removed.                         |
|   LM Studio     9    |   Reclaimed: 14.0 GB                              |
|                      |   Retained: 0.7 GB (1 file also linked in Ollama) |
|  Total: 60 models    |                                                   |
|  Disk: 77.7 GB       |   [+] meta-llama/Llama-3-8B-Instruct              |
|  Dedup-able: ...     |       1 file, 16.0 GB                             |
|                      |   [+] TheBloke/something-AWQ                      |
|                      |       1 file, 8.1 GB                              |
+--------------------------------------------------------------------------+
| [<-/->] tools  [up/down] rows  [Enter] expand  [u] unify                 |
| [d] delete-one  [F] folder-delete  [z] zap tool  [?] help [q] quit       |
+--------------------------------------------------------------------------+
```

**Partial-failure mockup (2 of 21 failed because Ollama held a file open):**

```
+- modeltap ---------------------------------------------------------------+
| Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF          |
|   (partial: 19 of 21 files removed)                                      |
|                                                                          |
|   Reclaimed: 13.1 GB                                                     |
|   Retained: 0.7 GB (1 file also linked in Ollama)                        |
|   Failed (2 files, 1.0 GB remained on disk):                             |
|     - Llama-3.2-1B-Instruct-Q4_K_M.gguf   reason: file open by ollama    |
|     - Llama-3.2-1B-Instruct-Q4_0.gguf     reason: file open by ollama    |
|                                                                          |
|   Folder partially emptied; remaining 2 files re-listed on next launch.  |
|   Press [F] again after closing Ollama to finish.                        |
+--------------------------------------------------------------------------+
```

**Shared artifacts:** `${last_action.bytes_reclaimed}` (reused from parent), `${last_action.bytes_retained}` (reused), plus new `${last_action.files_deleted_count}`, `${last_action.files_failed[]}`. The summary bar (`total.disk_usage`, `total.model_count`) refreshes per US-11.

**Emotional state:** entry "did it work?" → exit "yes — and the Ollama side is intact, which I was worried about." Visibility-of-system-status (Nielsen #1). Same emotional landing as US-05's zap verify, scaled to a folder unit.

**Integration checkpoint:** New `total.disk_usage == old total.disk_usage - last_action.bytes_reclaimed` (within rounding). Folder-row disappears from right pane. Any `*`-marked file in another tool that was hardlinked to a deleted-but-shared .gguf MUST still resolve via `stat` (inode survives).

## Error Paths (acknowledged)

| Failure | UX response |
|---|---|
| User types wrong folder path | Dialog closes with no changes (same rule as US-05 wrong-name) |
| User presses `F` on a non-folder row (e.g., a single model file) | No action; bottom bar briefly highlights to indicate `F` applies to folder headers only |
| User presses `F` on a folder with zero files (rare; only sidecars) | Dialog still opens but reads "0 model files, N sidecar files. Confirm to sweep sidecars only." |
| Ollama holds one of the .gguf files open during execution | Partial-success path; 2 of 21 fail with `reason: file open by ollama`; user retries after closing Ollama (mirrors US-17 soft-detection + intake Q5 detect-and-prompt-then-retry) |
| HF cache directory is read-only | Pre-flight check refuses with "Hugging Face cache is read-only — cannot delete folder" before opening the typed-confirm dialog |
| Folder path includes characters that need shell-escaping (rare; HF repos don't have these) | Typed input field accepts raw string; comparison is byte-exact |
| modeltap is killed mid-execution (Ctrl+C) | Files already unlinked stay unlinked. No journal, no rollback. Inventory rebuild on next launch reflects the partial state. |

## CLI vocabulary (extends parent)

| Concept | Term used | Never call it |
|---|---|---|
| The new bulk-delete granularity | "folder-delete" or "delete folder group" | "bulk delete", "purge repo", "wipe folder", "rm -rf" |
| The unit being deleted | "folder group" or "HF repo folder" | "bundle", "package", "collection" |
| The header row in the right pane | "folder header" | "group row", "parent row" |
| The non-model files (README, imatrix, urls) | "sidecars" | "extras", "metadata", "junk" |
| Files preserved because another tool hardlinks them | "retained" (matches US-05) | "kept", "skipped", "left alone" |

## Material Honesty

The folder-delete dialog is a foreground modal (Norman: feedback before commitment). The keystroke is one Shift+F press (clig.dev: minimal input for power users). The typed confirmation matches the on-screen path string verbatim (recognition over recall). No mouse, no scroll-back, no leaving the TUI to verify with `du -sh`. CLI-native end to end.
