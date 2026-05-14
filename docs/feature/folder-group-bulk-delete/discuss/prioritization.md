# Prioritization: folder-group-bulk-delete

## Summary

Single user story, single release, single bounded context (HF plugin). MoSCoW: **Must Have** within the v0.3.x roadmap because the existing `[d]`-per-file workflow makes Devon's most common HF-cleanup task (auditioning quants and discarding the repo) feel like a 21-step penalty.

## Release Priority

| Priority | Release | Target Outcome | KPI | Rationale |
|---|---|---|---|---|
| 1 | R1: US-05c folder-delete | Devon deletes one HF repo in 1 keystroke + 1 typed-confirm | K-FGD-2 (keystrokes_per_repo_delete: O(N) -> O(1)) | Only release; cannot be split smaller without losing user value. Walking skeleton is the full story. |

## Backlog Suggestions

| Story | Release | Priority | Outcome Link | Dependencies |
|---|---|---|---|---|
| US-05c | R1 | P1 (Must Have) | K-FGD-1, K-FGD-2, K-FGD-3 | Parent: US-12 (HF discovery), US-09 (compatibility engine), US-05b (delete-one pattern); DESIGN: Q-FGD-1 closed |

> Note: Story ID `US-05c` chosen per intake brief's suggested slot. It sits between US-05 (whole-tool zap) and US-05b (single-model delete), and the naming `c` (not 6) signals "third delete granularity, not third feature epic."

## Value / Effort Matrix

|  | Low Effort | High Effort |
|---|---|---|
| **High Value** | **US-05c** (~2-3 days, eliminates a 21-keystroke ceremony for Devon's most common HF cleanup) |  |
| **Low Value** |  |  |

Classic quick-win: low effort, immediate visible value. Compounding effect: every future HF repo cleanup is now O(1) instead of O(N).

## MoSCoW Classification

| Category | Stories | Notes |
|---|---|---|
| **Must Have** | US-05c | Required for the feature to exist at all |
| **Should Have** | (none) | |
| **Could Have** | (none in v1) | See "Out of Scope" below |
| **Won't Have** | Folder-delete for non-HF plugins (Ollama, llama-cli, LM Studio); folder-undo; trash-bin; dry-run preview; multi-folder bulk select | See "Out of Scope" below |

## Riskiest Assumption First

The riskiest assumption in this slice is **per-file shared/unique classification correctness inside a folder**. If `compute_compatibility()` (the parent's US-09 engine) returns a wrong classification for even one file inside the folder, the user might delete an HF path that was the LAST hardlink reference for a model another tool depends on — silent data loss.

**Mitigation:** the dialog must show the per-file classification (the `[+]`/`[-]` expanded list with each file's indicator), AND the dialog must call the same `compute_compatibility()` function as the row indicator (no parallel implementation). The integration checkpoint in `shared-artifacts-registry.md` enforces this.

This is not a separate spike — it's enforced at code-review time by the registry invariant: "Per-file shared/unique classification uses `compute_compatibility()` (US-09 machinery)."

## Why This Is Priority 1

- **Pain frequency**: Devon downloads new HF GGUF quants weekly. Each download is ~20 files. The current path (`[d]` -> typed confirm -> press 20 more times) is the dominant pain in his cleanup workflow.
- **Effort minimal**: 2-3 days. Reuses 90% of existing machinery (compatibility engine, summary refresh, post-action message format, typed-confirmation pattern from US-05).
- **No risk to other features**: HF-only in v1; the `Tool` trait may or may not change (DESIGN decides). Even if the trait grows a method, it's additive — non-HF plugins inherit a default no-op.
- **Compound value**: Every future HF cleanup is now O(1).

## Out of Scope (Won't Have in v1)

| Feature | Reason |
|---|---|
| Folder-delete for Ollama / llama-cli / LM Studio | Per intake scope constraint #1: those plugins do not expose a meaningful repo-folder structure. Ollama's manifests + blobs are content-addressed and shared; "folder" is not a unit there. llama-cli and LM Studio use flat directories. The feature does not generalise. |
| Undo / trash-bin | Parent feature already chose no soft-delete (US-05 technical notes). Consistency wins; if v2 introduces a trash, both US-05 and US-05c adopt it together. |
| Dry-run preview before folder-delete | Parent has dry-run only on unify (US-14) because unify is non-destructive but rewrites inodes. Folder-delete is destructive; the dialog itself IS the preview (it itemises every file with classification before the typed confirm). A separate `[n]` dry-run shortcut would just add a keystroke for no new information. |
| Multi-folder bulk select | YAGNI. If the user wants to clear multiple HF repos, they run `F` multiple times. The typed confirmation prevents accidental cascade. |
| Folder-delete from within a model detail screen (US-13) | The folder header in the right pane is the canonical entry point. Adding a second entry point fragments the UX. |
| `--folder-delete <author>/<repo>` CLI flag (non-interactive) | Parent feature has no non-interactive CLI in v1. Defer with the rest of the CLI work. |

## Validation Plan (when shipped)

Track for 30 days post-release:

1. **K-FGD-1 baseline** — `time_to_reclaim_repo_p50_seconds`: instrument the dialog-open-to-summary-shown latency. Target: < 5 seconds for a 21-file repo (excluding the time the user spends typing the path).
2. **K-FGD-2 measurement** — `keystrokes_per_repo_delete`: should be exactly 1 (Shift+F) + length-of-folder-path (typed confirm) + 1 (Enter). Compare to baseline `4 + 1 + length-of-id + 1` per file in the existing US-05b path.
3. **K-FGD-3 measurement** — `mis_target_rate`: count typed-confirmation mismatches (the user typed something other than `<author>/<repo>`). Target: 0. Any mismatches above 1% suggest the dialog text is misleading and a UX revision is warranted.
