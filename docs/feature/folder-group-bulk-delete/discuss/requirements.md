# Requirements — folder-group-bulk-delete

Brownfield extension of `modeltap-tui`. Single user story (US-05c) adds a third deletion granularity between US-05 (whole-tool zap) and US-05b (single-model delete).

## Business Context

modeltap is a Rust TUI for managing local AI models across tools (Ollama, HF cache, LM Studio, Atomic Chat). The Hugging Face cache is the only plugin where users naturally accumulate many files (~20 quant variants) under one logical unit (a `<author>/<repo>/` folder). The existing two deletion granularities are too coarse (zap entire tool) or too fine (one file at a time). Devon's most common HF cleanup task — discarding a repo he auditioned — is a 21-keystroke penalty under the current UX.

This feature adds the missing middle tier: folder-group delete.

## Functional Requirements

### F-FGD-1: Folder-group recognition in the right pane

The HF plugin's right-pane listing groups files by their parent `<author>/<repo>/` directory under a collapsible folder header. **Grouping is always on for the HF plugin** (RF-3) — it is a property of the HF cache layout, not an opt-in display mode. Folders containing a single file collapse to a one-row form that reads identically to the parent's pre-existing US-04 row format (the folder header IS the file row, no `[+]`/`[-]` indicator needed when there is nothing to expand).

For folders with ≥2 files, the header shows:

- Repo path (`<author>/<repo>`)
- Aggregate file count (model files + sidecars)
- Aggregate total bytes
- Per-file unique/shared split (e.g., `(20 unique, 1 shared)`)
- A `[+]` (collapsed) / `[-]` (expanded) indicator on the leftmost column

Children of a folder header are indented one level. Model file children retain their `*/o/!/?` indicators (per US-04, US-09). Sidecar children show a dim `.` indicator and are not cursor-targetable. The folder header IS cursor-targetable.

### F-FGD-2: Shift+F hotkey on folder header opens delete dialog

When the cursor is on a folder header row in the right pane, pressing `Shift+F` opens a typed-confirmation dialog (the "folder-delete dialog"). Pressing `F` (or any other key) on a non-folder row is a no-op. Pressing `Shift+F` when the active tool is not Hugging Face is a no-op with the bottom-bar `[F]` shortcut dimmed.

### F-FGD-3: Folder-delete dialog content

The dialog displays:

- The folder path (`<author>/<repo>`) — same string the user must type to confirm
- The absolute on-disk path (`~/.cache/huggingface/hub/models--<author>--<repo>/`)
- Per-file breakdown — first reading prefers explicit phrasing, with the shorthand "unique" used only inside the count summary line (RF-4):
  - Count of model files **only registered with Hugging Face** (will be permanently deleted; shorthand: "unique")
  - Count of model files **also registered with another tool** (HF registration removed; other-tool hardlink preserved; shorthand: "shared")
  - Count of sidecar files (README.md, .imatrix, .gguf.urls, plus HF-internal refs/blobs exclusive to this repo)
- Disk impact: `Reclaim: <X> GB` (unique + sidecars) and `Retained: <Y> GB` (shared)
- Running-tool warning (if any tool process holds any file in the folder open)
- A typed input field for the folder path
- `[Esc] cancel`

The Reclaim + Retained MUST sum to the folder's total bytes (within rounding).

Vocabulary note: "unique" in this dialog means "only registered with Hugging Face," not "unique filename within the folder." The explicit phrasing on first reading prevents the overloaded-term confusion (RF-4).

### F-FGD-4: Typed confirmation

The user must type the folder path `<author>/<repo>` exactly (case-sensitive, byte-exact) and press Enter to execute. Any other input cancels the dialog with no destructive action. Esc cancels at any point.

### F-FGD-5: Per-file unlink semantics

On confirmation, modeltap executes per-file:

- **Unique file** (HF registration is the only reference, classified by `compute_compatibility()` US-09 engine): unlink the file. The inode is freed.
- **Shared file** (another tool's path stat()s to the same inode): unlink only the HF-side path. The other tool's hardlink keeps the inode alive. modeltap does NOT touch the other tool's registration.
- **Sidecar file** (non-model file in the repo directory): unlink the file. No cross-tool consideration.

After all files are processed, modeltap removes the now-empty `models--<author>--<repo>/` directory tree (snapshots/, refs/, blobs/ subdirectories scoped to this repo).

### F-FGD-6: Partial-failure handling

If any individual unlink fails (permission, EBUSY, file-held-open), modeltap continues with the remaining files. After all files are attempted:

- Successfully deleted files stay deleted (no rollback)
- Failed files remain on disk with their reason captured
- The post-action summary itemises successes and failures
- The user can re-run `F` after addressing the failure cause to finish

This mirrors the parent feature's US-19 cross-fs fallback philosophy: never leave the filesystem in a worse state than starting; always report.

### F-FGD-7: Post-action summary

After execution, the TUI returns to the main view. The right pane shows:

- Header: `Last action: folder-delete <author>/<repo> (<success|partial>)`
- `N of M files removed`
- `Reclaimed: <X> GB` (matches the dialog's promise on full success)
- `Retained: <Y> GB (<N> file(s) also linked in <tool>)`
- On partial failure: a per-failed-file list with reason

The summary bar (`total.disk_usage`, `total.model_count`) refreshes within 500ms per US-11.

### F-FGD-8: Pre-flight refusal

Two pre-flight checks run before the folder-delete dialog opens:

1. **Cache writeable check**: if the HF cache directory is read-only (entire cache unwriteable), modeltap refuses with "Hugging Face cache is read-only -- cannot delete folder". This prevents the user from typing a confirmation that cannot possibly succeed.

2. **Folder still exists check** (RF-1): if the folder's `absolute_path` no longer exists at execution time (deleted out-of-band between launch and `Shift+F` — a real edge case in the stateless-rediscovery model per intake Q7), modeltap refuses with "folder no longer exists -- inventory will refresh" and triggers a re-discovery. No destructive action occurs.

## Non-Functional Requirements

### NF-FGD-1: Performance

- Folder-grouping inventory pass adds ≤200ms to HF discovery (US-12) for ≤500 folder groups
- Folder-delete dialog opens within 200ms of `Shift+F` (per Nielsen #1, clig.dev <100ms ideal; <200ms acceptable for a dialog with classification work)
- Per-file unlink completes at filesystem speed; UI shows a progress bar for folders with ≥10 files (Nielsen #1; consistent with parent CLI/TUI patterns)
- Per-folder file count assumption (RF-2): typical HF repos contain 1-30 files. Folders with >100 files still work but use the progress bar without special handling; the post-action summary's failed-file list MAY truncate after a reasonable threshold (DELIVER decides; truncation is informational only).

### NF-FGD-2: Safety

- Typed confirmation is mandatory; no `[y/n]` shortcut (unlike US-05b's shared-single-file case, because the bulk operation is irreversible across many files, even if some are individually shared)
- Partial-state invariant: at every moment during execution, the filesystem is consistent (each file is either fully unlinked or unchanged; no half-deleted files)
- No rollback machinery — successfully unlinked files stay unlinked; user retries failures explicitly

### NF-FGD-3: Cross-platform

- macOS and Linux supported (HF cache layout is identical per the parent's US-12 + US-20)
- Windows: WSL-only per the project's existing constraint

### NF-FGD-4: Vocabulary consistency

New terms ("folder group", "folder-delete", "sidecar", "folder header") added to the parent's CLI vocabulary table. "Retained" already exists in the parent and is reused verbatim.

## Business Rules

### B-FGD-1: HF plugin only in v1

Per intake scope constraint #1: folder-delete applies only to the Hugging Face plugin. Ollama (content-addressed), llama-cli (flat directories), and LM Studio (varies) do not expose a meaningful repo-folder unit. Future extension to other plugins is out of scope for v1 and is not a blocker for this story.

### B-FGD-2: Sidecars are part of the folder

Per intake scope constraint #3: `README.md`, `.imatrix`, `.gguf.urls`, and HF-internal sidecars (refs/, blobs/ scoped to this repo) are swept with the folder. The folder is the unit; partial sweeps (model files only) are NOT offered. Rationale: stranded sidecars are cosmetic junk that the user has no other way to clean up; sweeping them avoids an obvious follow-up complaint.

### B-FGD-3: Cross-tool hardlinks survive

A `.gguf` file hardlinked into another tool (via unify, US-10) MUST survive a folder-delete that includes the HF path. The other tool's path keeps the inode alive. modeltap does NOT touch the other tool's registration.

### B-FGD-4: Per-file classification uses parent's compatibility engine

The unique-vs-shared classification inside `folder_group.classify_unique_vs_shared()` MUST be expressed in terms of `compute_compatibility()` from US-09. No parallel implementation. Drift between the row indicator and the folder-delete dialog destroys user trust.

## Constraints (Inherited from Parent and Intake)

| Constraint | Source |
|---|---|
| No central modeltap-owned model store | intake Q1 — modeltap reads each tool's directory directly |
| Stateless rediscovery on every launch | intake Q7 |
| Dedup key = SHA256 (primary), HF repo+quant (display fallback) | intake Q6 |
| Concurrency: detect-and-prompt-then-retry | intake Q5 |
| WSL-only on Windows | parent constraint |
| Plugin extensibility via `Tool` trait + `Box<dyn Tool>` | CLAUDE.md |

## Stakeholders

| Stakeholder | Need |
|---|---|
| Devon Park (primary persona) | Reclaim HF cache disk without 21-keystroke ceremony |
| Riley Chen (contributor persona) | Not directly impacted; the trait extension (if any, per Q-FGD-1) must remain optional for non-HF plugins |
| solution-architect (DESIGN wave) | Resolve Q-FGD-1 (trait extension vs HF-only capability), Q-FGD-2 (concurrency model), Q-FGD-3 (folder dedup key) |
| acceptance-designer (DISTILL wave) | Add `journey-folder-group-delete.feature` scenarios to master acceptance |
| platform-architect (DEVOPS) | Instrument K-FGD-1/2/3 KPIs |

## Open Questions for DESIGN

| ID | Question |
|---|---|
| Q-FGD-1 | Does the `Tool` trait grow `list_folder_groups()` + `delete_folder(folder_path)`, or does only the HF plugin expose this via downcast / capability interface? Per intake scope, HF-only in v1; trait shape is DESIGN's call. |
| Q-FGD-2 | Concurrency: same detect-and-prompt-then-retry as ADR-009 (per-file), or stricter folder-level lock (fails fast if ANY file is busy)? |
| Q-FGD-3 | Does folder-group need its own dedup-key analogue (e.g., canonical HF repo id), or is `folder_group.path` already canonical at display time? |

## Risk Assessment

| Risk | Category | Probability | Impact | Mitigation |
|---|---|---|---|---|
| Per-file classification drift (folder-dialog vs row indicator) | Technical | LOW | HIGH | Enforce single-engine invariant in shared-artifacts-registry; reviewer checks code-call-site |
| Sidecar enumeration incomplete (new HF version adds new sidecar type) | Technical | MEDIUM | LOW | HF plugin owns sidecar enumeration (not modeltap-core hardcoding); refresh on HF version detection |
| User types wrong path (mis-target) | Project | LOW | LOW | Typed confirmation is byte-exact; mismatch cancels with no action; K-FGD-3 tracks this |
| Ollama holds .gguf open during execution | Technical | MEDIUM | LOW | Partial-failure handling; user retries after closing Ollama (mirrors US-17 + intake Q5) |
| HF cache moves to a new layout in future versions | Business | LOW | HIGH | HF plugin owns layout knowledge; this feature inherits parent's US-12 stability assumptions |

## Acceptance & Done

See `acceptance-criteria.md` for the full AC table. The Definition of Done (owned by acceptance-designer DISTILL wave) requires:

- All UAT scenarios in `journey-folder-group-delete.feature` pass
- Folder-grouping inventory pass measured ≤200ms for ≤500 folder groups
- Bytes-reclaimed accounting verified against `du -sh` cross-check on a synthetic test cache
- Cross-tool hardlink survival verified by `stat`/`fstat` post-delete
- Parent journey's existing scenarios (US-01..US-20 + US-05b) still pass — regression gate
