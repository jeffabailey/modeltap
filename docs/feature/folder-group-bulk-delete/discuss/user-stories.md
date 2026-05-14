# User Stories — folder-group-bulk-delete

Single story extending the parent `modeltap-tui` feature. Persona is shared with the parent: **Devon Park**, local-AI power user on macOS or Linux who runs at least two of {Ollama, llama-cli, Hugging Face cache, LM Studio}, comfortable with vim-style keys.

Story ID `US-05c` chosen per intake brief — sits between US-05 (whole-tool zap) and US-05b (single-model delete), the third delete granularity.

---

## US-05c: Delete a whole Hugging Face folder group

### Problem

Devon Park audits multiple quant variants of one logical model by downloading them from a Hugging Face repo such as `bartowski/Llama-3.2-1B-Instruct-GGUF/`. A typical repo contains ~20 `.gguf` files (different quantisations) plus sidecar files: `README.md`, `.imatrix`, `.gguf.urls`, and HF-internal `refs/`, `blobs/` entries. After he settles on the q4_K_M variant he ends up wanting the entire repo gone. The existing US-05b (`[d]` delete-one) forces him through 20 typed-confirmation dialogs — and even then sidecars are left stranded on disk. The whole-tool zap (US-05) is too coarse (would wipe his other 30 HF repos he wants to keep). He needs the missing middle granularity: delete the whole `<author>/<repo>/` folder as a unit.

### Who

- **Devon Park** — multi-tool local-AI power user, macOS or Linux, keyboard-first, uses HF cache as his primary download path for GGUF quants. Typically downloads 5-10 new HF repos per month, discards 1-2 of them after auditioning.

### Solution

When the Hugging Face plugin is the selected tool, the right pane groups files by their parent `<author>/<repo>/` directory under a collapsible folder header (`[+]`/`[-]`). With the cursor on a folder header row, pressing `Shift+F` opens a folder-delete dialog showing:

- The folder path (also the string the user must type to confirm)
- Per-file breakdown: count unique to HF, count shared with other tools, count of sidecars
- Disk impact: `Reclaim: <X> GB` + `Retained: <Y> GB`
- Running-tool warning if any file is open

Devon types the folder path `<author>/<repo>` exactly and presses Enter. modeltap unlinks each file:

- **Unique files**: fully deleted (inode freed)
- **Shared files** (hardlinked into another tool): HF-path removed only; the other tool's hardlink keeps the inode alive
- **Sidecars**: deleted as part of the folder sweep

After all files, the now-empty `models--<author>--<repo>/` directory tree is removed. On partial failure (a file held open, permission denied), modeltap continues with the remaining files and reports successes and failures in the post-action summary. Successfully-deleted files stay deleted (no rollback).

### Domain Examples

#### 1: Happy path — Devon deletes the bartowski Llama-3.2 repo, one file shared with Ollama

Devon's HF cache contains `bartowski/Llama-3.2-1B-Instruct-GGUF/` with 20 `.gguf` files (657 MB ... 2.5 GB; total 14.7 GB across the .gguf files) plus 3 sidecars (`README.md` 24 KB, `Llama-3.2-1B-Instruct.imatrix` 1.3 MB, `Llama-3.2-1B-Instruct-Q4_K_M.gguf.urls` 8 KB). One of the .gguf files (`Llama-3.2-1B-Instruct-Q4_K_M.gguf`, 808 MB) is hardlinked into Ollama from a prior unify. Devon selects Hugging Face, navigates to the folder header `[+] bartowski/Llama-3.2-1B-Instruct-GGUF (21 files, 14.7 GB, 20 unique, 1 shared)`, presses Shift+F. The dialog opens showing the breakdown. Devon types `bartowski/Llama-3.2-1B-Instruct-GGUF` and presses Enter. modeltap unlinks 19 unique .gguf files, removes the HF path for the shared .gguf (Ollama's hardlink survives), unlinks all 3 sidecars, removes the now-empty `models--bartowski--Llama-3.2-1B-Instruct-GGUF/` directory tree. Post-action: `Reclaimed 14.0 GB, Retained 0.7 GB (1 file also linked in Ollama)`. The summary bar's total disk usage decreases by 14.0 GB within 500ms.

#### 2: Edge — Devon wrong-types the path, dialog cancels

Devon presses Shift+F on `bartowski/Llama-3.2-1B-Instruct-GGUF`. The dialog opens. Devon types `Llama-3.2-1B-Instruct-GGUF` (forgot the `bartowski/` author prefix) and presses Enter. The dialog closes with no changes; the folder remains intact. The right pane returns to its prior state with the folder header still visible.

#### 3: Error — Ollama holds 2 files open; partial failure handled

Devon's `ollama serve` is running (PID 4421) with two of the .gguf files in the folder open (e.g., the Q4_K_M he's currently using and a Q4_0 he hasn't unloaded). The dialog shows `Running tools detected: ollama (PID 4421) -- file open: 2 of 21`. Devon proceeds anyway (soft-warning per US-17). modeltap unlinks 19 of the 21 files successfully (including all sidecars). The 2 EBUSY files remain on disk. Post-action: `partial: 19 of 21 files removed. Reclaimed: 13.0 GB. Retained: 0 GB. Failed (2 files, 1.6 GB remain on disk): Llama-3.2-1B-Instruct-Q4_K_M.gguf reason: file open by ollama. Llama-3.2-1B-Instruct-Q4_0.gguf reason: file open by ollama. Press [F] again after closing ollama to finish.` Devon closes Ollama, presses Shift+F again on the (now 2-file) folder header, types the path, and the remaining 2 files are deleted.

### UAT Scenarios (BDD)

#### Scenario: Hugging Face right pane groups files under repo folder headers

Given Devon's HF cache contains `bartowski/Llama-3.2-1B-Instruct-GGUF` with 20 .gguf files and 3 sidecars
When Devon launches modeltap and selects Hugging Face in the left pane
Then the right pane shows a folder header `[+] bartowski/Llama-3.2-1B-Instruct-GGUF`
And the header line reads `21 files, 14.7 GB (20 unique, 1 shared)`
And expanding the header shows 20 model rows (with `*`/`o`/`!`/`?` indicators) and 3 dim `.`-prefixed sidecar rows

#### Scenario: Pressing Shift+F on a folder header opens the typed-confirmation dialog

Given Devon's cursor is on the folder header `bartowski/Llama-3.2-1B-Instruct-GGUF`
When Devon presses Shift+F
Then a modal dialog opens titled `Delete folder group: bartowski/Llama-3.2-1B-Instruct-GGUF`
And the dialog itemises 19 unique + 1 shared + 3 sidecars
And the dialog shows `Reclaim: 14.0 GB` and `Retained: 0.7 GB`
And the dialog asks Devon to type the folder path to confirm

#### Scenario: Correct typed-confirmation executes the folder-delete

Given the folder-delete dialog is open for `bartowski/Llama-3.2-1B-Instruct-GGUF`
And no tool is holding any file in the folder open
When Devon types `bartowski/Llama-3.2-1B-Instruct-GGUF` exactly and presses Enter
Then 19 unique .gguf files are unlinked from the HF cache
And the 1 shared .gguf has only its HF path unlinked (Ollama-side hardlink survives)
And 3 sidecar files are unlinked
And the now-empty `models--bartowski--Llama-3.2-1B-Instruct-GGUF/` directory tree is removed
And modeltap reports `Reclaimed 14.0 GB, Retained 0.7 GB`

#### Scenario: Wrong typed path cancels the folder-delete

Given the folder-delete dialog is open for `bartowski/Llama-3.2-1B-Instruct-GGUF`
When Devon types `Llama-3.2-1B-Instruct-GGUF` (missing the author prefix) and presses Enter
Then the dialog closes with no changes
And no files are deleted
And the folder header still appears in the right pane

#### Scenario: Partial failure when Ollama holds files open

Given Devon has confirmed the folder-delete
And ollama is running and holds 2 of the 21 files open
When modeltap attempts to unlink each file
Then 19 files are successfully unlinked
And 2 files fail with reason `file open by ollama` and remain on disk
And the post-action summary reads `partial: 19 of 21 files removed`
And modeltap does NOT roll back the 19 successful deletions
And the folder-delete operation can be re-run after closing Ollama to finish the remaining 2

#### Scenario: Shift+F is a no-op when the active tool is not Hugging Face

Given Devon's cursor is on a model row in the Ollama right pane
When Devon presses Shift+F
Then no dialog opens
And the `[F]` shortcut in the bottom bar is dimmed when the active tool is not Hugging Face

#### Scenario: Shared file's other-tool hardlink survives folder-delete

Given `Llama-3.2-1B-Instruct-Q4_K_M.gguf` is hardlinked into both HF and Ollama, both paths stat() to the same inode
When Devon successfully folder-deletes the HF repo containing this file
Then the HF path no longer exists
And the Ollama path still exists and still stat()s to the original inode
And `ollama run llama3.2:1b` still succeeds

### Acceptance Criteria

- [ ] US-05c.AC-1 — The Hugging Face right pane groups files by their parent `<author>/<repo>/` directory under collapsible folder header rows
- [ ] US-05c.AC-2 — Each folder header shows: repo path, total file count (models + sidecars), total bytes, unique/shared split, `[+]`/`[-]` indicator
- [ ] US-05c.AC-3 — Folder header rows are cursor-targetable; sidecar child rows are not
- [ ] US-05c.AC-4 — `Shift+F` on a folder header opens the folder-delete dialog within 200ms
- [ ] US-05c.AC-5 — `Shift+F` on a non-folder row OR when the active tool is not Hugging Face is a no-op (no dialog, no destructive action)
- [ ] US-05c.AC-6 — The dialog shows: folder path, absolute on-disk path, count of unique files, count of shared files (and which other tools), count of sidecars, Reclaim bytes, Retained bytes, running-tool warning if any
- [ ] US-05c.AC-7 — `Reclaim + Retained == folder_group.total_bytes` (within rounding) in the dialog and the post-action summary
- [ ] US-05c.AC-8 — The user must type the folder path `<author>/<repo>` exactly (case-sensitive, byte-exact); anything else cancels with no destructive action
- [ ] US-05c.AC-9 — `Esc` cancels the dialog at any point with no destructive action
- [ ] US-05c.AC-10 — On execution, unique files are fully unlinked; shared files have only their HF path unlinked (other tool's hardlink keeps the inode alive); sidecars are unlinked
- [ ] US-05c.AC-11 — After all files, the now-empty `models--<author>--<repo>/` directory tree is removed
- [ ] US-05c.AC-12 — On partial failure, successfully-deleted files stay deleted; failed files remain on disk with their reason; the post-action summary itemises both
- [ ] US-05c.AC-13 — Per-file unique-vs-shared classification uses `compute_compatibility()` (US-09 machinery) — no parallel implementation
- [ ] US-05c.AC-14 — Sidecar enumeration (README.md, .imatrix, .gguf.urls, plus HF-internal sidecars) is owned by the HF plugin, not hardcoded in modeltap-core
- [ ] US-05c.AC-15 — If the HF cache directory is read-only, modeltap refuses BEFORE opening the dialog with a clear message
- [ ] US-05c.AC-16 — The post-action summary shows: action name, success/partial status, `N of M files removed`, Reclaimed bytes, Retained bytes (with which other tools), per-file failures
- [ ] US-05c.AC-17 — The summary bar's `total.disk_usage` refreshes within 500ms of action completion (per US-11 invariant)
- [ ] US-05c.AC-18 — The bottom bar shows `[F] folder-delete` shortcut, dimmed when not applicable to current focus
- [ ] US-05c.AC-19 — `[F]` is part of `ui::shortcuts::SHORTCUT_TABLE` — the bottom bar render and dispatch table share a single source

### Outcome KPIs

See `outcome-kpis.md` for the full table. This story drives:

- **K-FGD-1** (`time_to_reclaim_repo_p50_seconds`) — primary
- **K-FGD-2** (`keystrokes_per_repo_delete`) — primary
- **K-FGD-3** (`mis_target_rate`) — guardrail

Also contributes to the parent feature's existing K1 (`bytes_reclaimed_per_session`) and K5 (`zero_accidental_loss`).

### Technical Notes (Constraints, not solutions)

- Persists no state across launches (parent invariant: stateless rediscovery per intake Q7)
- Concurrency: detect-and-prompt-then-retry per intake Q5 — soft warning, user can proceed (mirrors US-17 + US-05b error scenario)
- Per intake scope constraint #1: HF plugin only in v1. Trait extension shape (Q-FGD-1) belongs to DESIGN.
- Per ADR-009 safety rubric: typed-confirmation for irreversible delete (matches US-05's whole-tool zap), NOT `[y/n]` (which US-05b uses only for shared-single-file delete). Folder-delete is irreversible across many files even when some are individually shared.
- Cross-platform: macOS and Linux (HF cache layout identical per parent's US-12 + US-20)
- Sidecar enumeration owned by HF plugin (not modeltap-core); HF version changes may add new sidecar types

### Dependencies

**Parent feature stories (already PASSED DoR and either in DELIVER or shipped):**

- US-03 (two-pane layout) — cursor targeting
- US-04 (model row format) — extended with folder header row type
- US-05 (typed-confirmation pattern) — pattern copied
- US-05b (single-model delete) — coexists; same `[d]` discipline preserved
- US-06 (post-action message) — reused with new file-count consumers
- US-08 (bottom bar) — gains `[F]` shortcut
- US-09 (compatibility engine) — drives per-file classification
- US-11 (summary bar refresh) — reused
- US-12 (HF cache discovery) — extended with folder enumeration
- US-17 (running-tool detection) — reused in dialog

**DESIGN-must-close (does not block DISCUSS handoff; blocks DESIGN handoff to DEVOPS):**

- Q-FGD-1 (trait extension shape: third method vs HF-only capability)
- Q-FGD-2 (concurrency model: per-file detect-retry vs folder-level lock)
- Q-FGD-3 (folder dedup-key analogue: needed or not)

**External: none.**
