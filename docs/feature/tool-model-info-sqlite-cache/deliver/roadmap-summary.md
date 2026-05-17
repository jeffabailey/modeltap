# DELIVER Roadmap Summary — tool-model-info-sqlite-cache

**Feature:** US-21..US-26 — Tool/model inspection + SQLite-backed cache + pre-mutate revalidation
**Total phases:** 6
**Total steps:** 22
**Total estimated hours:** 120 (24+18+18+26+22+12; the earlier "100" total was an architect arithmetic error noted in Phase 1c review)
**Source roadmap:** `docs/feature/tool-model-info-sqlite-cache/deliver/roadmap.json`

## Phase List

| Phase | Name | Steps | Est. hours | Exit gate |
|---|---|:---:|:---:|---|
| 01 | Walking Skeleton — Devon's second launch shows yesterday's inventory instantly from cache | 5 | 24 | M1 `@walking-skeleton` scenario green: process A writes cache, process B paints from cache in <=150ms; parent + sibling regression green; CI lint clean |
| 02 | Tool detail view (US-21) — inspect_tool overrides + INT-INFO-8 panic isolation | 3 | 18 | All 5 tool-detail.feature scenarios green; plugin-contract 3.12 green for 6 plugins; INT-INFO-8 scenario green for inspect_tool |
| 03 | Model detail view (US-22) — inspect_model overrides + graceful degradation | 3 | 18 | All 5 model-detail.feature scenarios green; plugin-contract 3.13 green for 6 plugins; INT-INFO-8 scenario green for inspect_model |
| 04 | Cache state model (US-23 + US-25) — recovery, opt-out, TTL, concurrency, KPI | 5 | 26 | 8 cache-state-model.feature scenarios green; INT-INFO-5 / INT-INFO-6 green; modeltap-store/tests/{corruption,migration,concurrent}.rs green; K-INFO-1 and K-INFO-7 budgets met |
| 05 | Reconcile + revalidator + manual refresh (US-24 + US-26) — cache safety contract | 4 | 22 | 5 cache-state-model + 4 manual-refresh + INT-INFO-3 / INT-INFO-4 / INT-INFO-7 green; revalidator wired into all 4 mutation sites |
| 06 | Architecture lints (R7/R8/R9) + mutation gate + lat.md + final CI | 2 | 12 | R7/R8/R9 tests green; mutation kill-rate >= 80% on modeltap-store and modeltap-core inspect+diff; parent + sibling regression green; lat.md + CHANGELOG updated; final CI clean |
| **Total** | | **22** | **120** | |

## Step-ID Convention

`NN-NN` format (e.g. `01-01`, `04-05`, `06-02`). Mirrors parent + sibling roadmap convention; orchestrator validator-compatible.

## Walking Skeleton (Phase 01) Steps

| Step | Name | Hours |
|---|---|:---:|
| 01-01 | Tool trait extension and inspect domain types | 4 |
| 01-02 | modeltap-store crate skeleton + migration v0->v1 + minimal repos | 6 |
| 01-03 | In-process TestTool plugin + plugin-registry seam | 3 |
| 01-04 | Warm-start orchestration + cache-path resolver | 5 |
| 01-05 | Walking-skeleton acceptance scenario green end-to-end | 6 |

## User Story to Phase Mapping

| Story | Release | Phase(s) | Notes |
|---|---|---|---|
| US-21 Tool detail screen | R1 | 02 | inspect_tool overrides for Ollama + HF; default-Unsupported for others |
| US-22 Model detail with metadata | R1 | 03 | inspect_model overrides for llama-cli (GGUF), lm-studio, Ollama, HF |
| US-23 Cache schema + recovery + WAL | R2 | 01 (basic), 04 (full) | Migration runs in 01-02; recovery/opt-out/concurrency in 04 |
| US-24 Manual refresh + provenance | R2 | 05 | [r] / [Shift+R] reuse the reconcile dispatcher from US-26 |
| US-25 Warm-start cache read | R2 | 01 (basic), 04 (TTL) | Warm-start orchestration in 01-04; per-tool TTL eligibility in 04-03 |
| US-26 Background reconcile + revalidation | R2 | 05 | Reconcile in 05-01; pre-mutate revalidator (R9 safety lint) in 05-02 |
| US-27 SHA256 persistence | R3 | (none) | DEFERRED per ADR-018; `@release-3 @skip` scenarios remain in the feature suite |

## Scenarios Driven Per Step

Every step's `criteria` array references at least one specific scenario name from the 7 feature files (`walking-skeleton.feature`, `tool-detail.feature`, `model-detail.feature`, `cache-state-model.feature`, `manual-refresh.feature`, `integration-checkpoints.feature`, plus the deferred `sha256-persistence.feature`) plus the originating AC IDs (AC-21-N..AC-26-N, INT-INFO-N). No criterion references a private method (underscore-prefix). Every criterion is user-observable through TUI, JSONL, filesystem, or `@cache-introspection` SQLite read.

## Files-per-Step (sanity check)

Mean: ~3.9 files per step (86 file-touch entries across 22 steps). Two justified outliers:

- **01-02 (7 files):** new `modeltap-store` crate scaffold — workspace `Cargo.toml` + crate `Cargo.toml` + `lib.rs` + `open.rs` + `migrate.rs` + `types.rs` + `migrations/0001_initial.sql` form an inseparable atomic commit. Cannot be split without breaking compilation.
- **05-02 (6 files):** pre-mutate revalidator is the load-bearing R9 safety choke point. The single seam touches `modeltap-store::revalidate` + `modeltap-app::orchestration::revalidate` + 3 mutation sites (execute_unify, execute_delete_one, execute_folder_delete) + the test. R9 architecture lint requires every mutation site to call `pre_mutate` in the same commit or the lint fails the next CI run.

All other steps land within 3-5 files. The sibling folder-group roadmap averaged ~3.5 with similar outliers (its phase 04 step had 7 files for partial-failure orchestration). This roadmap's mean is consistent with the project's established decomposition discipline.

## Identical-pattern Batching

The 6 plugin overrides for `inspect_tool` are NOT split into 6 steps; they are batched into:
- **02-02:** Ollama + HF overrides (the two that have real introspectable state) in one step
- **02-03:** the contract test 3.12 runs over all 6 plugins (Ollama + HF Supported; 4 others Unsupported) in one step

Same pattern for `inspect_model` in phase 03:
- **03-02:** all 4 supporting plugin overrides (llama-cli, lm-studio, Ollama, HF) in one step
- **03-03:** contract test 3.13 across all 6 plugins in one step

This honors the "3+ identical AC structure must be batched" gate.

## Integration with Parent + Sibling Features

- **Parent (modeltap-tui):** every phase preserves the 93-scenario master-acceptance suite. Step 01-05 is the first partial regression gate; step 06-02 is the final regression gate.
- **Sibling (folder-group-bulk-delete):** the `Tool::inspect_*` extensions are ADDITIVE — `delete_folder` (from ADR-010) and `inspect_*` (from ADR-016) coexist. The HF plugin gains `plugins/hf/src/inspect.rs` as a sibling to `plugins/hf/src/folder_delete.rs` (no merge conflict). INT-INFO-7 (folder-delete runs the revalidator) lands in step 05-02.
- **Deferred (US-27 SHA256 persistence):** `sha256-persistence.feature` scenarios remain `@release-3 @skip`; ADR-018 records the seam. Migration `0002_add_sha256_persistence.sql` is documented in data-models.md but not landed here.

## CI Lint Discipline (CLAUDE.md MANDATORY)

Every step's description embeds the per-crate CI gate: `cargo fmt -p <crate> && cargo clippy -p <crate> --all-targets -- -D warnings && cargo test -p <crate>`. Step 06-02 explicitly requires the workspace-wide gate `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` clean before push, per CLAUDE.md.

## Mutation Testing Gate (CLAUDE.md per-feature)

Phase 06 step 06-02 closes both kill-rate gates:
- `cargo mutants -p modeltap-store` >= 80% on `types::FileStat::matches`, `repo::{tools,models,files}`, `revalidate::verify_against_fs`
- `cargo mutants -p modeltap-core` >= 80% on `domain::inspect::{ToolDetail, ModelDetail}` builders and `logic::compute_inventory_diff`

## Risks Flagged for User Before TDD Kicks Off

1. **MODELTAP_TEST_PLUGINS env-var seam (R3 in acceptance-test-plan.md):** must be cfg-gated behind `cfg(any(test, feature = "test-harness"))` so release builds do not ship the test plugin registration code. Step 06-01 lands the release-build absence check (`strings target/release/modeltap | grep MODELTAP_TEST_PLUGINS` must be empty).
2. **MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS test seam (step 04-04):** same cfg-gating discipline required.
3. **The `gguf` crate vs hand-rolled parser decision (step 03-02):** ADR-016 implementation guidance defers to software-crafter. If the `gguf` crate adds a non-trivial dep, hand-rolling the minimal GGUF header parser is acceptable. Either choice is recorded inline in `plugins/llama-cli/src/inspect.rs`.
4. **R9 AST lint (step 06-01):** a future contributor who adds a new `Tool::replace_model` method (or any new destructive method) must extend the R9 lint to cover it. The lint is currently hard-coded to recognize the 4 known destructive methods; adding a 5th is a load-bearing maintenance step the architect must remember to do.
5. **Concurrent-process scenarios on macOS CI (step 04-04):** per CLAUDE.md §Running Tests Fast on macOS, the two-process tests are at risk of the Gatekeeper scan tax. Tag them `@concurrent` and consider running them in a serial CI job to avoid file-descriptor exhaustion.
6. **K-INFO-1 / K-INFO-7 perf assertions (step 04-05):** the `@perf`-tagged scenarios run in `--release` build only. Debug-build latencies routinely exceed 150 ms and would produce false positives. CI must run these in a dedicated `cargo test --release` invocation.
