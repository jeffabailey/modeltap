# Phase 4 Adversarial Review — folder-group-bulk-delete

**Reviewer:** nw-software-crafter-reviewer (orchestrator-dispatched)
**Scope:** 17 commits `e3996d1..a33c23c` covering 16 roadmap steps + 1 regression fix
**Verdict:** **APPROVED**
**Recommendation:** **PROCEED_TO_MUTATION**

## Defects Found

**Zero.** No critical, high, medium, or low issues identified.

## Strengths (Verified)

1. **Trait extension with default method (ADR-010)** — `Tool::delete_folder` default body returns `Err(DeleteError::Unsupported)`; non-HF plugins inherit zero source changes; contract-test guard protects against missing override in future plugins.

2. **Single-engine invariant (AC-13)** — `classify_unique_vs_shared` is the only producer of `FolderClassification`. Every model routes through `compute_indicator`. Proptest in `folder_group_logic.rs` validates `Tentative` dedup keys never produce `Shared` classification (R1 mitigation). No parallel dedup logic anywhere.

3. **Sidecar ownership (AC-14)** — Hermetically sealed in `plugins/hf/src/folder_delete.rs`. modeltap-core contains only the `SidecarKind` enum type definition; suffix list (`README.md`, `.imatrix`, `.gguf.urls`, `refs/`, `blobs/`) lives only in the plugin.

4. **MODELTAP_TEST_EBUSY_PATHS seam** — Gated behind `cfg(any(test, feature = "test-harness"))`. Production binary will not contain the env-var name string. Seam used only within the per-file unlink loop, also cfg-guarded.

5. **Hardlink survival via ADR-009 ref-counting** — `delete_one_hf_side_only_at()` unlinks only the HF-side snapshot symlink; blob ref-counting from ADR-009 keeps the inode alive when Ollama still hardlinks to it. Cross-tool hardlink preservation guaranteed by construction, not by assertion.

6. **Port-to-port testing discipline** — Acceptance tests drive the `modeltap` binary through the composition root entry point. Unit tests on pure functions are port-to-port at domain scope. Plugin contract tests invoke `Box<dyn Tool>`. Zero internal-class tests.

7. **Cargo-flock fix** — Acceptance tests invoke `target/debug/modeltap-app` directly via the `modeltap_app_in` helper, NOT `cargo run`. The previously-observed 21-minute flock deadlock is avoided.

8. **Bottom-bar regression fix (a33c23c)** — `[F]` label shortened to fit 80 cols. AC-19 invariant preserved (single source SHORTCUT_TABLE drives both bottom-bar render and dispatch). No hardcoded `<author>/<repo>` literal in `keymap.rs` (INT-FGD-7 lint test passes).

## Testing Theater 7-Pattern Scan

| Pattern | Result |
|---|---|
| Zero-assertion tests | NONE found |
| Tautological tests | NONE found |
| Mock-dominated SUT | NONE found |
| Circular verification | NONE found |
| Always-green tests | NONE found |
| Fully-mocked SUT | NONE found (acceptance uses real HF plugin + tempdir) |
| Assertion-free smoke tests | NONE found |

## AC Traceability

All 20 US-05c ACs + 8 integration ACs traced to a passing scenario in either `folder-group-delete.feature` or `integration-checkpoints.feature`. Zero unmapped ACs.

## Walking-Skeleton Litmus (D1)

The M1 acceptance test asserts on `path.exists() == false` against a real tempdir HF cache tree after delete. Deleting the real `HfPlugin` adapter and substituting an `InMemoryHfPlugin` would not mutate the filesystem; the test would fail. Strategy B (real I/O) is load-bearing.

## Plugin-Contract Substitution Note

`plugins/llama-cli` does not exist in this workspace. The roadmap specified contract tests for `ollama / llama-cli / lm-studio`. Substituted `atomic-chat` (third existing non-HF plugin). The substitution is documented in step 05-01's commit message body and does not break M5 spec intent — the trait-level default-body assertion is identical across any non-HF plugin.

## Quality Gates (Crafter Reviewer Standard)

| Gate | Status |
|---|---|
| G1 — Single acceptance test active | PASS |
| G2 — Acceptance test fails for valid reason during RED | PASS |
| G3 — Unit test fails on assertion during RED | PASS |
| G4 — No mocks inside hexagon | PASS |
| G5 — Business language in tests | PASS |
| G6 — All tests green | PASS |
| G7 — 100% passing before commit | PASS |
| G8 — Test count within budget | PASS |
| G9 — No test weakening | PASS |

## Recommendation

**PROCEED_TO_MUTATION.** Feature is architecturally sound, TDD-disciplined, theater-free, and ready for `cargo-mutants` (≥80% kill-rate per CLAUDE.md per-feature gate).

## 5-Line Summary

1. Verdict: APPROVED.
2. Counts: Critical 0 / High 0 / Medium 0 / Low 0.
3. Most important finding: zero defects; ADR-010, AC-13 single-engine, AC-14 sidecar confinement, and INT-FGD-4 cross-tool hardlink semantics all verified by inspection plus tests.
4. Walking-skeleton litmus passes; deleting the HF adapter would break the M1 test (strategy B real-I/O is load-bearing).
5. Recommendation: PROCEED_TO_MUTATION → finalize → merge to main.
