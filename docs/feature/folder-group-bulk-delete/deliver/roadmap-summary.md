# DELIVER Roadmap Summary — folder-group-bulk-delete

**Feature:** US-05c — Delete a whole Hugging Face folder group
**Total phases:** 6
**Total steps:** 14
**Total estimated hours:** 62
**Source roadmap:** `docs/feature/folder-group-bulk-delete/deliver/roadmap.json`

## Phase List

| Phase | Name | Steps | Est. hours | Exit gate |
|---|---|:---:|:---:|---|
| 01 | Walking Skeleton — Devon deletes an all-unique HF folder end-to-end | 5 | 24 | M1 `@walking-skeleton` scenario green against real HF plugin + real tempdir fixture; parent regression passes; CI lint clean |
| 02 | Confirmation safety — typed-confirm guardrails | 2 | 8 | All 4 M2 scenarios green; K-FGD-3 mis-target invariant holds |
| 03 | Mixed shared/unique — per-file classification + hardlink survival | 3 | 10 | All 4 M3 scenarios green; INT-FGD-4 hardlink survival green; `@property` proptest green |
| 04 | Partial failure — per-file detect-and-retry + pre-flight refusal | 3 | 10 | All 3 M4 scenarios green; AC-15 and AC-20 pre-flight scenarios green; plugin-contract 3.11.S.4 / 3.11.S.5 green |
| 05 | Plugin-contract boundary — Unsupported for Ollama / llama-cli / lm-studio | 1 | 4 | M5 Scenario Outline green for 3 plugins; plugin-contract 3.11.U.1 green per plugin |
| 06 | KPI guardrails + regression gate | 2 | 6 | Both M6 scenarios green; INT-FGD-1/5/6/7 green; INT-FGD-8 parent regression green; mutation kill-rate >= 80% |
| **Total** | | **14** | **62** | |

## Step-ID Convention

`NN-NN` format (e.g. `01-01`, `01-05`, `06-02`). Mirrors parent feature roadmap convention; DES validator-compatible.

## Walking Skeleton (Phase 01) Steps

| Step | Name | Hours |
|---|---|:---:|
| 01-01 | Core types + Tool trait extension | 5 |
| 01-02 | Pure folder-group logic + single-engine invariant | 4 |
| 01-03 | HF plugin happy-path override | 6 |
| 01-04 | TUI folder-header row + Shift+F + dialog view | 5 |
| 01-05 | Orchestration + walking-skeleton acceptance test green | 4 |

## Milestone -> Phase Mapping (from DISTILL features/folder-group-delete.feature)

| Milestone | Phase | Notes |
|---|---|---|
| M1 walking skeleton | 01 | Single all-unique scenario; exit gate |
| M2 confirmation safety | 02 | 4 scenarios across 2 steps |
| M3 mixed shared/unique | 03 | 4 scenarios + `@property` proptest |
| M4 partial failure | 04 | 3 scenarios + 2 pre-flight refusal scenarios |
| M5 capability boundary | 05 | Scenario Outline x 3 plugins + Layer B contract |
| M6 KPI guardrails | 06 | 2 KPI scenarios + 5 integration-checkpoint scenarios |

## Scenarios Driven Per Step (all sourced from DISTILL feature files)

Every step's `criteria` array references at least one specific scenario name from `folder-group-delete.feature` or `integration-checkpoints.feature`, plus the originating AC IDs (AC-N, INT-FGD-N). No private-method references in any criteria; every criterion is user-observable.

## Files-per-Step (sanity check)

Mean: ~3.3 files per step. Two steps approach 5-7 files (01-01, 01-05, 04-01) — these are the load-bearing composition-root and instrumentation steps; further decomposition would create artificial seams that the architecture does not justify. No step touches more than 2 distinct architectural concerns (e.g. 04-01 = HF plugin failure loop + orchestrator aggregation + post-action summary view, all on the partial-failure thread).

## Integration with Parent Feature

Phase 01 walking-skeleton step 01-05 takes a partial preview of INT-FGD-8 (parent regression). Phase 06 step 06-02 closes the full INT-FGD-8 regression gate as the final exit before peer review and merge.

## CI Lint Discipline (CLAUDE.md MANDATORY)

Every step's DoD includes `cargo clippy --workspace --all-targets -- -D warnings clean`. Phase 06 step 06-02 explicitly requires `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` clean before push, per CLAUDE.md.

## Mutation Testing Gate (CLAUDE.md per-feature)

- Phase 06 step 06-01: mutation kill-rate >= 80% on `modeltap-core::logic::folder_group` (the pure-logic surface)
- Phase 06 step 06-02: mutation kill-rate >= 80% on `plugins/hf` folder-delete contract test surface

## Risks Flagged for User Before TDD Kicks Off

1. The `MODELTAP_TEST_EBUSY_PATHS` env-var seam (D4) must be gated behind `cfg(any(test, feature = "test-harness"))` so release builds do not ship the test seam. Phase 04-01 includes a release-build absence test (`strings target/release/modeltap | grep MODELTAP_TEST_EBUSY_PATHS` must be empty) as evidence.
2. The sidecar suffix list in `enumerate_sidecars` is software-crafter-owned during phase 01-03 GREEN. Future HF version changes adding new sidecar types are a maintenance risk (R2 in architecture-design.md §10); mitigated by HF plugin ownership.
3. The cross-tool hardlink survival assertion (INT-FGD-4 / phase 03-02) requires the test fixture to set up a real hardlink between HF blob and Ollama tree; the fixture helper must verify pre-delete `stat().st_ino` equality as a precondition or the test silently degrades to a tautology.
4. The lint test in `crates/modeltap-tui/tests/lint.rs` (phase 01-04 and 06-02) is a regex-based grep over source files — it can produce false positives on innocent strings; software-crafter may need to scope the regex tightly to keymap/dispatch files only.
5. K-FGD-1 (latency p50 <= 15s) is NOT a Layer A scenario per D10 — it is escalated to PO-reviewer at the DELIVER post-merge gate. Acknowledge this so it is not relitigated mid-TDD.
