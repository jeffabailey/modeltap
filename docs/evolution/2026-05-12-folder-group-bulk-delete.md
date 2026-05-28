# Evolution Archive — folder-group-bulk-delete

**Feature**: folder-group-bulk-delete (US-05c — Shift+F deletes an entire Hugging Face `<author>/<repo>/` folder with all its quants + sidecars in a single typed-confirm dialog)
**Wave**: DELIVER (full DISCUSS → DESIGN → DISTILL → DELIVER cycle, single-story brownfield extension of `modeltap-tui`)
**Date completed**: 2026-05-12
**Date finalized**: 2026-05-27
**Status**: APPROVED — adversarial review zero defects; mutation kill-rate ≥80% gate met (100% production-only after Phase 5b test-gap closure); shipped in v0.2.x line.
**Step-IDs**: 16 steps across 6 phases (01-01 → 06-02). All COMMIT/PASS.
**ADR**: ADR-010 — Folder-Group Delete — HF Capability via Trait Default-Method.

## Outcome

modeltap now exposes a third model-delete granularity for the Hugging Face plugin: pressing `Shift+F` on a folder-header row opens a typed-confirm dialog that, on a byte-exact match of `<author>/<repo>` (e.g., `bartowski/Llama-3.2-1B-Instruct-GGUF`), deletes the entire folder — every quant variant, every sidecar (`README.md`, `LICENSE`, `*.imatrix`, `*.gguf.urls`, `refs/`, `blobs/`) — and reports `Reclaimed: <X> GB` + `Retained: <Y> GB` in parent vocabulary. Per-file unique-vs-shared classification routes through the parent's US-09 `compute_indicator` engine; cross-tool hardlinks survive (only the HF-side snapshot symlink is unlinked, blob ref-counting from ADR-009 keeps the inode alive for the other tool). Partial failures are continue-and-report — no rollback, no journal — and the post-action summary itemises both deleted and failed files so the user can `[F]` again after addressing the cause.

The HF right pane now always groups files under a collapsible folder-header row (`[+]` collapsed / `[-]` expanded, default collapsed per recovery step 01-07). Folder headers are cursor-targetable; sidecar children are dim-prefixed `.` and not cursor-targetable. The bottom bar dims `[F]` when the active tool is non-HF or the cursor is not on a folder-header row (US-08 dimmed-shortcut convention). The keystroke budget — `<= 40` keys total for any folder size, asserted by `K-FGD-2` — is independent of file count: this is the entire point of the feature versus today's US-05b loop of `~22 × N_files`.

The four non-HF plugins (Ollama, llama-cli, lm-studio, atomic-chat) inherit `Tool::delete_folder`'s default body returning `Err(DeleteError::Unsupported)` with zero source changes — the trait-default pattern from ADR-010.

## Why It Was Needed

modeltap already exposed two model-delete granularities: `[z]` zap a whole tool (US-05) and `[d]` delete a single model file (US-05b). For Hugging Face specifically, neither matched the most common cleanup task: discarding a whole `<author>/<repo>/` folder, which typically holds 5–20 quant variants plus sidecars. Devon's only path was the US-05b loop — open dialog, type model id, confirm, repeat — `~22 × N_files` keystrokes (≈440 for a 20-quant repo), and the K-FGD-1 wall-clock estimate of 60–180 seconds. The feature targets a p50 ≤ 15 s and the keystroke count flat at `~35`, independent of file count.

The feature is intentionally **HF-only in v1**. Ollama uses content-addressed blobs with no folder semantics; llama-cli uses flat directories; LM Studio has no canonical repo-folder unit. ADR-010 closes the trait-shape question with a default-body `Tool::delete_folder` so the four non-HF plugins compile without modification and the contract-test guard catches a future plugin that forgets to override when it gains real folder semantics.

## Implementation Summary

16 TDD cycles, sequential by phase, each producing one COMMIT/PASS event in `deliver/execution-log.json`. Walking-skeleton-first per parent's Strategy B (real I/O against fixture-populated temp dirs — `tests/src/fixtures/cache_fixtures.rs`).

**Trait + domain types (modeltap-core)**:

- `Tool::delete_folder(&self, plan: &FolderDeletePlan) -> Result<FolderDeleteOutcome, DeleteError>` with default body returning `Err(DeleteError::Unsupported)` (Step 01-01).
- New domain types: `FolderGroup`, `FolderDeletePlan`, `FolderDeleteOutcome`, `SidecarKind`, `DeleteError::Unsupported` (Step 01-01).
- Pure logic functions: `group_by_hf_repo`, `classify_unique_vs_shared`, `build_folder_delete_plan` in `crates/modeltap-core/src/logic/folder_group.rs` (Step 01-02). `classify_unique_vs_shared` calls the parent's `compute_indicator` per file — single-engine invariant enforced at the call site (AC-13).
- Proptests for `@property` ACs landed in Step 03-03 (`crates/modeltap-core/tests/folder_group_proptest.rs`) covering INT-FGD-2 (file_count = models + sidecars), INT-FGD-3 (total_bytes = reclaim + retain), and AC-13 (Tentative dedup keys never produce Shared classification → R1 mitigation).

**HF plugin overrides**:

- `plugins/hf/src/folder_delete.rs`: full override implementing `delete_folder`, `enumerate_sidecars`, `delete_folder_at`, `remove_empty_repo_tree` (Step 01-03 happy path; Step 03-02 mixed-folder hardlink survival; Step 04-01 partial-failure handling + EBUSY simulation seam; Step 04-02 idempotent retry; Step 04-03 read-only and vanished-folder pre-flight refusals).
- The `MODELTAP_TEST_EBUSY_PATHS` env-var seam is gated under `#[cfg(any(test, feature = "test-harness"))]` — production builds do not ship the env-var name string. Phase 4 review confirmed zero leakage.

**TUI + composition root**:

- `crates/modeltap-tui/src/render/folder_confirm_dialog.rs` + Shift+F keymap binding + `[F]` bottom-bar entry with two-arm gating (active tool is HF AND cursor is on a folder header) (Step 01-04).
- HF right-pane grouping with folder-header rows, indentation, dim-prefixed sidecar rows (Step 01-06 recovery + Step 01-07 collapse default).
- Mixed-folder dialog itemisation (N unique + M shared + K sidecars; Reclaim/Retained accounting; running-tool warning slot) in Step 03-01.
- Wrong-path + trailing-slash + Esc cancel paths (Step 02-01) using a byte-exact comparator and `DirManifest` pre/post-assertion. Step 02-02 dims `[F]` on non-HF tools.
- `crates/modeltap-app::orchestration::execute_folder_delete` wires the dialog → plan-build → plugin dispatch → JSONL `action.folder_delete` event chain (Step 01-05 walking-skeleton + Step 06-01 KPI instrumentation).

**KPI + invariants (Phase 06)**:

- JSONL `action.folder_delete` instrumentation; K-FGD-2 keystroke-count `<= 40` assertion; K-FGD-3 mis-target invariant (no string literal matching `<author>/<repo>` appears inline in dispatch code — `crates/modeltap-tui/tests/lint.rs` lint test) (Step 06-01).
- Cross-cutting integration scenarios INT-FGD-1, INT-FGD-5, INT-FGD-6, INT-FGD-7, plus INT-FGD-8 regression gate (Step 06-02). All 93 parent-feature scenarios continue to pass.

Post-DELIVER fix:

- `a33c23c fix(tui): keep bottom-bar within 80 cols by shortening [F] label` — drift caught by AC-19's bar-truncation invariant.

## Key Decisions

1. **Trait default-method, not a downcast or capability subtrait (ADR-010 / Q-FGD-1 closure).** `Tool::delete_folder` lives on the existing trait with a default body returning `Err(DeleteError::Unsupported)`. Rejected: a capability subtrait would force a runtime `Any::downcast_ref` in the orchestrator (back-door coupling); a plugin-private method would put the dispatch logic inside `modeltap-app` and lose the trait-level test surface. The default-body approach keeps the 5-tool extensibility property (a new plugin compiles without override) while letting the M5 contract test assert "every plugin returns the right variant in the first place" — Layer A (orchestrator-observable) + Layer B (trait-direct) split per D5.

2. **Per-file unique-vs-shared classification, single-engine invariant (D-FGD-4 / AC-13).** `classify_unique_vs_shared` calls the parent's US-09 `compute_indicator` per file inside the folder. No parallel implementation. A folder can mix unique and shared files (e.g., user `unify`'d the q4_K_M but not the other 19 quants) and the indicator-vs-dialog itemisation cannot drift. The R1 mitigation proptest in Step 03-03 asserts `Tentative` dedup keys never produce `Shared`.

3. **Sidecar enumeration owned by the HF plugin (D-FGD-5 / AC-14).** The sidecar suffix list (`README.md`, `LICENSE`, `LICENSE.md`, `.imatrix`, `.gguf.urls`, `.urls`, `refs/`, `blobs/`) lives only in `plugins/hf/src/folder_delete.rs`. `modeltap-core` contains only the `SidecarKind` enum. HF version changes that add new sidecar conventions don't propagate to the core crate. Partial sweeps (model files only) are NOT offered — stranded sidecars are cosmetic junk with no other cleanup path.

4. **Byte-exact typed confirmation of `<author>/<repo>` (D-FGD-2).** Case-sensitive, byte-exact comparator. Trailing slash is **mismatch → cancel** (D2 — DISTILL refusal to normalize on the user's behalf). The dialog tells the user the exact string; type what's shown. Rejected: `[y/n]` (appropriate for US-05b shared-single-file but not for a bulk operation that may unlink dozens of inodes across multiple sidecars); typing just the repo name (ambiguous — multiple authors publish repos with the same name).

5. **Partial-failure continue-and-report, no rollback (D-FGD-6 / AC-12).** If any individual unlink fails, modeltap continues with the remaining files. Successfully-deleted files stay deleted. Failed files remain on disk with their reason captured. The post-action summary itemises both. User retries `[F]` after addressing the cause. Rationale: rollback requires a journal of unlinks, which is itself fragile and out of scope for the stateless-rediscovery design (intake Q7); re-run is cheap because the inventory rebuilds on next launch.

6. **Cross-tool hardlink survival via ADR-009 ref-counting (Phase 4 strength #5).** `delete_one_hf_side_only_at` unlinks only the HF-side snapshot symlink. Blob ref-counting from ADR-009 keeps the inode alive when Ollama (or any other tool) still hardlinks to it. modeltap does NOT touch the other tool's registration. Hardlink preservation is guaranteed by construction, not by assertion — INT-FGD-4 verifies it end-to-end against a real tempdir.

7. **`MODELTAP_TEST_EBUSY_PATHS` env-var seam over a port adapter (D4).** Three approaches were considered: real `flock(LOCK_EX)` from a sibling process (Linux-only semantics; macOS unlink-while-open differs; brittle cross-platform CI); a `FakeFsOps` adapter behind a new port (most pure-architecturally; disproportionate overhead for one scenario family); env-var seam (one line in the plugin, zero impact on production, cfg-gated). The env-var approach was chosen. Permission-denied via `chmod 0555` is portable and is used for the contract-test 3.11.S.4 (Layer B) instead of EBUSY.

8. **HF right pane grouping is always on, not a toggle (D-FGD-3).** Grouping is a property of the HF cache layout, not a display mode. Folders with one file collapse to a one-row form indistinguishable from the parent's pre-existing US-04 row format — backward-compatible. Aggregates roll up into the existing `tool.disk_usage`, so the summary bar remains a single source of truth.

9. **Walking-skeleton Strategy B, inherited from parent (D1).** Real `unlink(2)` against a real tempdir. The M1 walking-skeleton litmus passes — deleting the HF adapter and substituting an `InMemoryHfPlugin` would not mutate the filesystem; the test would fail. Strategy B is load-bearing for this feature.

10. **`@property` tags → DELIVER discretion (D6).** Tags signal universal invariants; the choice of "proptest vs concrete-example E2E" was deferred to DELIVER. The pure-function invariants (INT-FGD-2, INT-FGD-3, AC-13) became proptests in `crates/modeltap-core/tests/folder_group_proptest.rs`; the AC-8 mutation-class invariant (wrong prefix, wrong case, extra char, missing char) stayed as concrete E2E scenarios in cucumber-rs; INT-FGD-7 became a hybrid (E2E + a lint test asserting no inline `<author>/<repo>` literal).

## Lessons Learned

- **Mutation testing surfaces test gaps that adversarial review misses.** Phase 4 review found zero defects, but `cargo-mutants` on `plugins/hf/src/folder_delete.rs` initially landed at 63.33% (spec) / 59.26% (strict behavioral) — well under the 80% gate. The misses clustered in `classify_sidecar` and `path_starts_with_subdir` where the integration tests exercised the outer behavior but never directly asserted per-suffix classification (no fixture contained a literal `README.md`; tests fed paths under `refs/` but never the contrastive "path NOT under `refs`/`blobs`" branch). Phase 5b closed the gap with a single parametrised unit test file (`plugins/hf/tests/folder_delete_classify_sidecar.rs`) covering 10 `(path, expected-kind)` tuples — production-only kill rate jumped to 100%. **Pattern to keep:** for pure classification functions, write a small parametrised table-driven unit test at the function port; don't rely on integration tests to exercise every branch.
- **`is_test_ebusy_path` is the canonical example of "out-of-scope for mutation kill-rate".** Six surviving mutants inside the cfg-gated test seam are not production code; the function does not ship in release builds. The fix was to document the exclusion in the mutation report and (optionally) add `exclude_re = ["is_test_ebusy_path"]` to `mutants.toml`. This is the pattern for any future test-only seam — gate it under `#[cfg(any(test, feature = "test-harness"))]`, document why mutants inside it don't count, and (if noise becomes a problem) configure `mutants.toml` to skip it.
- **Bottom-bar width is a real invariant, not a style nit.** Step 06-02 shipped clean but the next-day fix `a33c23c` had to shorten the `[F]` label because the bar overflowed 80 cols. The parent feature has had two more bar-width fixes since (89a9e50 cascading drop + conditional `[r]` label). **Treat bar-width as a per-feature acceptance criterion** when adding a new hotkey, and consider testing it explicitly against an 80-col `TestBackend` frame — the existing `crates/modeltap-acceptance` cache-kpi-style scenarios are the right pattern.
- **A trait default-method is a better deprecation story than a downcast.** Future capability boundaries should follow the ADR-010 pattern: add the method to the trait with a sensible default; rely on Layer A (orchestrator-observable) + Layer B (trait-direct) contract tests to enforce that non-supporting plugins return the right variant; let the override land on plugins that genuinely have the capability. No `Any::downcast_ref` in the orchestrator; no plugin-specific code paths in `modeltap-app`.
- **`@property` is a planning tag, not an implementation directive.** The DISTILL decision to leave proptest-vs-E2E discretion to DELIVER worked well: the hybrid INT-FGD-7 (E2E for the comparator + lint test for the literal-absence invariant) was the right shape, and it would have been wrong to lock that in at acceptance-design time. Keep this delegation pattern.

## Risks / Follow-ups Deferred

- **K-FGD-1 wall-clock target (`p50 ≤ 15 s, p90 ≤ 30 s` for a 21-file repo)** is asserted in `outcome-kpis.md` but is NOT a Layer A scenario in DISTILL — wall-clock latency depends on per-machine perf; it's a quarterly aggregate target tracked via JSONL `action.folder_delete` events. The M6 keystroke-count scenario covers the K-FGD-2 measurable surface. Per D10 escalation note, a reviewer wanting K-FGD-1 in a scenario must `@escalate:po-reviewer`.
- **`llama-cli` plugin substitution.** The roadmap specified contract tests for `ollama / llama-cli / lm-studio`. `plugins/llama-cli` does not exist in this workspace, so the contract test substitutes `atomic-chat` (third existing non-HF plugin). The substitution is documented in Step 05-01's commit message body. If `llama-cli` is added later, its contract test follows the same Layer A + Layer B pattern with no behavioral change to the trait.
- **DEVOPS wave skipped for this feature** (per D8). The parent `modeltap-tui` inherits Strategy B and the fixture mechanism; this feature added 6 new named fixtures within the same env contract. No new environment targets, no platform-specific code beyond what the parent already handles (macOS / Linux / WSL).
- **K-FGD-2 may tighten to 36 after first measurement** of typical user corrections (per D3 note). The 40-key bound is conservative; the typical path is 34 keys (33 chars + Enter). Reducing the assertion bound is a small post-launch follow-up if telemetry confirms the gap.

## Steps Completed

| Step | Title | Closed by |
|---|---|---|
| 01-01 | Core types + `Tool::delete_folder` default | `e3996d1` |
| 01-02 | Pure folder-group logic | `400f63d` |
| 01-03 | HF plugin override (happy path) | `dce769f` |
| 01-04 | TUI folder-header + Shift+F + confirm dialog | `8796beb` |
| 01-05 | Orchestrator + walking-skeleton green | `af258a8` |
| 01-06 | (recovery) HF right-pane grouping | `4bf492a` |
| 01-07 | (recovery) Default-collapsed folder rows | `2ddf865` |
| 02-01 | Wrong-path + trailing-slash + Esc cancel | `59e14c0` |
| 02-02 | Shift+F no-op on non-HF + dim `[F]` | `ece09b6` |
| 03-01 | Mixed-folder dialog itemisation | `149d383` |
| 03-02 | Cross-tool hardlink survival (INT-FGD-4) | `39b0072` |
| 03-03 | @property proptests for INT-FGD-2/-3 + AC-13 | `2eb2da5` |
| 04-01 | Partial-failure + EBUSY seam | `e4b88d6` |
| 04-02 | Idempotent retry after busy resolved | `37c5d31` |
| 04-03 | Pre-flight refusals (read-only + vanished) | `254fe17` |
| 05-01 | Plugin-contract Layer B + M5 Layer A | `05fc344` |
| 06-01 | KPI instrumentation + K-FGD-2 + K-FGD-3 | `12e5852` |
| 06-02 | Integration checkpoints + INT-FGD-8 regression gate | `49ffe78` |
| (fix) | Bottom-bar `[F]` 80-col shortening | `a33c23c` |

Phase 4 adversarial review: zero defects, APPROVED → PROCEED_TO_MUTATION.
Phase 5 mutation testing: 100% production-only kill rate after Phase 5b test-gap closure.
Phase 06 exit gate: all 93 parent-feature scenarios continue to pass.

## Links

- **Migrated artifacts** (per nWave destination map):
  - Architecture (4 docs): [`docs/architecture/folder-group-bulk-delete/`](../architecture/folder-group-bulk-delete/)
  - UX (3 docs): [`docs/ux/folder-group-bulk-delete/`](../ux/folder-group-bulk-delete/)
  - ADR-010: [`docs/adrs/ADR-010-folder-group-delete-hf-capability.md`](../adrs/ADR-010-folder-group-delete-hf-capability.md) (already in permanent location pre-finalize)
- **Supporting artifact** (this archive): [`folder-group-bulk-delete/mutation-report.md`](./folder-group-bulk-delete/mutation-report.md) — Phase 5 + 5b cargo-mutants results, surviving-mutant analysis, and the parametrised-test pattern used to close the gap.
- **Workspace** (preserved per nWave convention so the wave matrix derives status): `docs/feature/folder-group-bulk-delete/`. Wave-decisions, peer reviews, requirements, story-maps, acceptance-criteria, distill features, and plugin-contract-spec remain in the workspace as historical context.
- **Feature commit range**: `e3996d1..a33c23c` (17 commits — 16 DELIVER steps + 1 post-DELIVER bar-width fix).
- **Related feature**: [`modeltap-tui-v1-evolution.md`](./modeltap-tui-v1-evolution.md) — the parent feature whose US-09 `compute_indicator`, US-05/US-05b delete patterns, and shared-artifacts-registry vocabulary this feature extends.
