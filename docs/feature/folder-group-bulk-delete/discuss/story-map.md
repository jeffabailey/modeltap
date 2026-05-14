# Story Map: folder-group-bulk-delete

## User: Devon Park (multi-tool local-AI power user, audits many HF quants per repo)

## Goal: Reclaim disk by deleting a whole `<author>/<repo>/` HF folder in one keystroke + typed confirmation, preserving cross-tool hardlinks and sweeping sidecars.

## Brownfield Context

This feature is a **single new user story** (US-05c) that extends the parent `modeltap-tui` feature. The parent feature is already in DELIVER wave with 21 stories (US-01..US-20 + US-05b) live. The full multi-activity story-map exercise from the parent is not re-done; this map shows the new backbone slice within the parent journey.

## Backbone (within the parent journey)

The parent journey backbone is: `Launch -> Browse -> Inspect -> Decide -> Execute -> Verify`. This feature adds tasks **at the Browse and Decide steps**, plus a parallel Execute path.

| Browse | Decide | Execute | Verify |
|---|---|---|---|
| **(NEW)** Right pane shows HF folder-group headers `[+]/[-]` above their child files | **(NEW)** Press `[F]` on folder header opens folder-delete dialog | **(NEW)** Per-file unlink loop (unique files fully unlinked; shared files HF-path-only; sidecars unlinked) | **(NEW)** Post-action summary shows `bytes_reclaimed` + `bytes_retained` + per-file failures |
| (existing) Expand/collapse with Enter | (existing) Dialog itemises unique/shared/sidecars | (existing) Remove now-empty `models--<author>--<repo>/` directory tree | (existing) Summary bar refreshes per US-11 |
|  | (existing) Typed-confirmation = `<author>/<repo>` path | (existing) Partial-failure: continue + report, no rollback |  |
|  | (existing) Running-tool warning shown |  |  |

## Walking Skeleton

A walking-skeleton is NOT required for this feature (per orchestrator config: `walking_skeleton=No`). The parent feature already established the end-to-end TUI flow; this feature is a single additive slice.

The minimum end-to-end slice for THIS feature is **the full US-05c story** — the folder-delete operation cannot be split smaller without losing user value (deleting 18 of 21 files and leaving sidecars is not a useful intermediate; that's just the existing US-05b loop).

## Releases (only one)

### Release 1: US-05c — Delete a Hugging Face folder group

**Stories:** US-05c (single story)

**Target outcome:** Devon presses one Shift+F + typed confirmation and reclaims a whole HF repo, with sidecars swept and cross-tool hardlinks preserved.

**KPI targeted:** `keystrokes_per_repo_delete` reduced from O(N_files) to O(1) per K-FGD-2 (see `outcome-kpis.md`).

**Rationale for not splitting:** The full operation is one user outcome. Splitting by technical layer (e.g., "first detect folder groups", "later add delete") violates the elephant-carpaccio rule of slicing by user outcome. Detecting folder groups without offering bulk delete delivers no new user value — the user can already see all the files; the value is the bulk delete itself.

## Scope Assessment: PASS — 1 story, 1 bounded context (HF plugin), estimated 2-3 days

Per Phase 2.7 (Elephant Carpaccio Gate):

- Story count: **1** (target: ≤10) ✓
- Bounded contexts touched: **1** — HF plugin only, per intake scope constraint #1. The `Tool` trait may or may not change (DESIGN's Q-FGD-1 to close); even if it does, the change is additive and localised. ✓
- Integration points: **1** — the existing right-pane row renderer accepts a new row type (folder header). The summary bar refresh path is reused unchanged. ✓
- Estimated effort: **2-3 days** (folder grouping logic + dialog + unlink loop + sidecar enumeration) ✓
- Independent user outcomes: **1** — the folder-delete operation. ✓

Right-sized. No splitting required.

## Dependencies on Parent Feature (modeltap-tui)

This story depends on the following parent stories already being in place (all are PASSED DoR and either in DELIVER or shipped):

- US-03 (two-pane layout) — for cursor targeting and pane focus
- US-04 (model row format with indicators) — extended with folder header row type
- US-05 (typed-confirmation pattern) — copied for the folder-path typed confirmation
- US-05b (single-model delete) — coexists with this feature; same hotkey discipline
- US-06 (post-action message) — reused, new `files_deleted_count` / `files_failed[]` consumers
- US-08 (bottom bar) — gains `[F] folder-delete` entry
- US-09 (compatibility engine) — drives per-file unique-vs-shared classification
- US-11 (summary bar refresh after action) — reused unchanged
- US-12 (HF cache discovery) — extended with folder-group enumeration
- US-17 (running-tool detection) — reused in folder-delete dialog

No new dependencies on stories outside the parent feature. No new ADRs required beyond ADR-009-style trait extension that DESIGN must close (see Q-FGD-1).

## Why This Is One Story, Not Many

Common bad splits to avoid:

| Bad split | Why it's bad |
|---|---|
| US-05c-a "Group HF files into folder headers" + US-05c-b "Implement folder delete" | Layer-based, not outcome-based. Headers without delete deliver no user value (the user can already see all files). |
| US-05c-a "Folder delete without sidecar sweep" + US-05c-b "Add sidecar sweep" | Sidecars stranded after the first slice is a defect, not an intermediate. Sweeping sidecars is part of the same user outcome. |
| US-05c-a "Folder delete without partial-failure handling" + US-05c-b "Add partial-failure UI" | Partial-failure handling is mandatory for any destructive operation per the parent's emotional-arc rule ("user must feel in control, not surprised"). Cannot be deferred. |
| US-05c-a "Folder delete only when all files are unique" + US-05c-b "Add shared-file classification" | The shared-file case is the most realistic case (Devon uses unify regularly). Shipping without it would mislead the user. |

The story is right-sized as a single 2-3 day unit. It is demonstrable in one session: launch -> Hugging Face -> expand a repo -> press F -> type path -> see reclaim message.
