# Wave Decisions — folder-group-bulk-delete (DISCUSS)

Wave: **DISCUSS** (wave 2 of 6) — single-story brownfield extension of `modeltap-tui`.
Persona: **Devon Park** (shared with parent feature).
Status: **APPROVED** by peer review (`peer-review.md` iteration 2).
Handoff target: **solution-architect (DESIGN wave)**.

## Feature recap

modeltap currently exposes two model-delete granularities: **`[z]` zap a whole tool** (US-05) and **`[d]` delete a single model file** (US-05b). For the Hugging Face plugin specifically, neither is right for the most common cleanup task: discarding a whole `<author>/<repo>/` folder (e.g., `bartowski/Llama-3.2-1B-Instruct-GGUF/` with ~20 quant variants plus sidecars). This feature adds the third granularity, **US-05c folder-delete**, as a single-story HF-only extension.

## Decisions made in this wave

### D-FGD-1: Hotkey is Shift+F (`[F]`)

**Chosen:** Shift+F, rendered as `[F]` in the bottom bar.

**Rationale:**

1. No collision with parent's `[d]` (single-file delete) or `[z]` (whole-tool zap).
2. Uppercase signals "bulk operation, more powerful than single but not the entire tool" — symmetric to how parent uses lowercase for individual-file operations.
3. Mnemonic: **F** for "folder."

**Rejected alternatives:**

- `[D]` (Shift+D, "delete folder") — proposed in intake brief. Rejected because the visual distance to `[d]` (single-file delete) is too small and a hand on Shift can accidentally apply to either. `[F]` carries no such adjacency.
- `[B]` (bulk) — rejected as less mnemonic; "bulk" is not the user's mental model.
- `[Z]` (Shift+Z, "big zap") — rejected as it implies tool-wide behaviour, not folder-scoped.

Other rendering rules: bottom bar shows `[F] folder-delete` dimmed when (a) active tool is not Hugging Face, OR (b) cursor is not on a folder header row. This follows parent US-08's dimmed-shortcut convention.

### D-FGD-2: Typed confirmation = full `<author>/<repo>` path

**Chosen:** the user must type the full folder path `<author>/<repo>` (e.g., `bartowski/Llama-3.2-1B-Instruct-GGUF`) exactly to confirm. Case-sensitive, byte-exact.

**Rationale:**

- Mirrors US-05's whole-tool zap typed-confirmation pattern (strongest cheap guard for irreversible bulk operations) per ADR-009 safety rubric.
- The string is the same string the user reads at the top of the dialog — recognition over recall (Nielsen #6).
- Includes the author prefix to prevent ambiguous matches (Hugging Face has multiple repos named `Llama-3.2-1B-Instruct-GGUF` from different authors).

**Rejected alternatives:**

- `[y/n]` single-key confirmation — appropriate for US-05b shared-single-file (registration-only removal) but NOT for bulk operation that may unlink dozens of inodes across multiple sidecars. Even when all files are individually shared, the registration removals are collectively irreversible.
- Type just the repo name (without author) — rejected as ambiguous and shorter-than-the-displayed-string.

### D-FGD-3: Discovery affordance is collapsible folder header

**Chosen:** the HF plugin's right-pane listing **always groups** files by their parent `<author>/<repo>/` directory under a collapsible folder header row. `[+]` (collapsed) / `[-]` (expanded) is the leftmost indicator. Children indented one level. Sidecar children dim-prefixed `.` and not cursor-targetable. Folder headers ARE cursor-targetable.

**Rationale:**

- Grouping is a property of the HF cache layout, not a display mode toggle. Always on.
- Folders with one file collapse to a one-row form indistinguishable from the parent's pre-existing US-04 row format — backward-compatible.
- Aggregates roll up into the existing `tool.disk_usage` (parent registry artifact), so the summary bar remains a single source of truth.

### D-FGD-4: Shared-model semantics — per-file, parent compatibility engine

**Chosen:** unique-vs-shared classification is computed **per file inside the folder** using the parent's `compute_compatibility()` from US-09. No parallel implementation. Shared files have only their HF path unlinked; the other tool's hardlink keeps the inode alive. modeltap does NOT touch the other tool's registration.

**Rationale:**

- Single-engine invariant prevents drift between row indicator and dialog itemisation.
- Per-file classification (not per-folder) accurately reflects reality: a folder can mix unique and shared files (e.g., user ran `unify` on the q4_K_M but not the other 19 quants).
- Cross-tool hardlink preservation is the riskiest invariant; it is captured both in the shared-artifacts-registry integration checkpoints AND in AC-13.

### D-FGD-5: Sidecar sweep is mandatory, owned by HF plugin

**Chosen:** `README.md`, `.imatrix`, `.gguf.urls`, and HF-internal files exclusive to this repo's snapshot (refs/, blobs/) are swept with the folder. Sidecar enumeration is owned by the HF plugin (not hardcoded in modeltap-core). Partial sweeps (model files only) are NOT offered.

**Rationale:**

- Stranded sidecars are cosmetic junk the user has no other way to clean — sweeping them avoids an obvious follow-up complaint.
- HF version changes may add new sidecar conventions; owning the list in the HF plugin keeps modeltap-core stable.

### D-FGD-6: Partial-failure handling — continue and report, no rollback

**Chosen:** if any individual unlink fails, modeltap continues with the remaining files. Successfully-deleted files stay deleted. Failed files remain on disk with their reason captured. The post-action summary itemises both. User retries `[F]` after addressing the failure cause.

**Rationale:**

- Mirrors parent's US-19 cross-fs fallback philosophy: never leave the filesystem in a worse state than starting.
- Rollback would require a journal of unlinks, which itself is fragile and out of scope for a stateless-rediscovery design (intake Q7).
- Re-run is cheap because the inventory rebuilds on next launch and the remaining files are still grouped under the same folder header.

### D-FGD-7: Reclaim accounting matches parent vocabulary

**Chosen:** post-action summary uses `Reclaimed: <X> GB` (unique files + sidecars) and `Retained: <Y> GB` (shared files whose HF path was removed but whose inode survives). Same vocabulary as US-05 and US-05b.

**Rationale:**

- Vocabulary consistency across the three delete granularities is critical for user trust (parent's `cli_vocabulary` invariant).
- "Retained" is the parent's term for "bytes kept alive by another tool's hardlink." Reuse, don't re-invent.

## Open questions kicked to DESIGN

| ID | Question | Why DESIGN must close |
|---|---|---|
| Q-FGD-1 | Does the `Tool` trait grow `list_folder_groups()` + `delete_folder(folder_path)`, or does only the HF plugin expose this via downcast / capability interface? | Trait shape is DESIGN's responsibility; intake scope locks HF-only for v1 |
| Q-FGD-2 | Concurrency: same detect-and-prompt-then-retry as ADR-009 (per-file), or stricter folder-level lock (fails fast if ANY file is busy)? | Has implications for trait method signature and error variants |
| Q-FGD-3 | Does folder-group need its own dedup-key analogue (e.g., canonical HF repo id), or is `folder_group.path` already canonical at display time? | Has implications for whether `modeltap-core` grows a new value type |

None of these block DISCUSS handoff. All three should be closed in DESIGN before DEVOPS handoff (platform-architect needs concrete trait shape for instrumentation planning).

## Outcome KPIs defined

Three story-level KPIs (see `outcome-kpis.md` for full smell-test table and measurement plan):

| KPI | Target | Baseline |
|---|---|---|
| K-FGD-1 `time_to_reclaim_repo_p50_seconds` | p50 ≤ 15 s, p90 ≤ 30 s for a 21-file repo | Today's US-05b loop: 60-180 s for 20 files |
| K-FGD-2 `keystrokes_per_repo_delete` | ~35 keystrokes total, independent of file count | Today's US-05b loop: ~22 × N_files (≈440 for 20 files) |
| K-FGD-3 `mis_target_rate` (guardrail) | < 1% of dialog opens; 0 accidental wrong-folder deletes in 90 days | N/A (feature doesn't exist) |

This feature also drives parent KPIs K1 (bytes reclaimed per session) and K5 (no accidental loss).

## DoR status

**PASSED** (9/9 items for US-05c). See `dor-checklist.md`.

## Peer review status

**APPROVED** after one iteration. Six required fixes (RF-1 through RF-6) all RESOLVED. See `peer-review.md`.

## Handoff package for solution-architect (DESIGN wave)

Artifacts produced (all in `docs/feature/folder-group-bulk-delete/discuss/`):

1. `journey-folder-group-delete-visual.md` — ASCII flow + TUI mockups + emotional arc
2. `journey-folder-group-delete.yaml` — structured journey schema
3. `journey-folder-group-delete.feature` — 17 Gherkin scenarios
4. `shared-artifacts-registry.md` — new `folder_group.*` artifacts + updated parent-artifact consumers
5. `story-map.md` — single-story backbone within the parent journey, with elephant-carpaccio scope assessment
6. `prioritization.md` — single-release prioritisation, out-of-scope list
7. `requirements.md` — functional + non-functional + business rules
8. `user-stories.md` — US-05c (single story)
9. `acceptance-criteria.md` — 20 ACs + 8 cross-story integration ACs
10. `dor-checklist.md` — 9/9 PASS
11. `outcome-kpis.md` — 3 KPIs with measurement definitions
12. `peer-review.md` — review iterations + resolution log
13. `wave-decisions.md` — this file

Open questions for DESIGN: Q-FGD-1, Q-FGD-2, Q-FGD-3 (see above).

Regression surface flagged for solution-architect:
- Parent US-04 row format extended with folder-header row type
- Parent US-12 HF discovery extended with folder enumeration + sidecar enumeration
- Parent US-08 bottom bar gains `[F]` entry
- Parent US-09 compatibility engine called per-file inside `classify_unique_vs_shared()`
- Parent ADR-009 trait pattern likely extended (Q-FGD-1)

Suggested ADR(s) for DESIGN to author: ADR-010 (or similar) covering the trait shape decision from Q-FGD-1.
