# Acceptance Self-Review — release-process-homebrew-github

**Wave:** DISTILL (5 of 6)
**Author:** Quinn (nw-acceptance-designer)
**Reviewer mode:** self-review against `nw-ad-critique-dimensions` (9 dimensions)
**Date:** 2026-05-03
**Iteration:** 1 (pre-peer-review)

> Run before invoking `nw-acceptance-designer-reviewer` (Sentinel) for the hard-gate peer review. All blockers must be resolved before peer review begins.

## 1. Self-Review Findings (per Critique Dimensions)

### Dimension 1 — Happy Path Bias

- **Counts:** 24 happy-path / 23 error-edge-infra-failure / 5 property = 41% error coverage (target ≥40%).
- **Finding:** `walking-skeleton.feature` has 5 happy + 4 error/edge for US-01..US-06; `multi-arch-release.feature` has 8 happy + 6 error; `hands-off-automation.feature` has 8 happy + 4 error + 2 edge; `integration-checkpoints.feature` has 4 happy + 4 error/recovery + 2 mixed. Spread is balanced across slices.
- **Verdict:** PASS.

### Dimension 2 — GWT Format Compliance

- **Audit:** every scenario has Given (or implicit via Background) → When (single action) → Then (observable outcome). Multi-When patterns: NONE found. Multi-line `When ... and ...` patterns: NONE — all `And` clauses follow Given or Then.
- **One scenario** uses `Scenario Outline` for boundary testing (validate-tag invariant) — explicitly permitted by skill.
- **Verdict:** PASS.

### Dimension 3 — Business Language Purity

- **Grep results** for technical jargon in feature files (excluding code identifiers Gherkin must use to be precise):

  ```
  $ grep -i -E '(database|REST|JSON|HTTP|controller|class|method|service|status code|200|404|500|Redis|Kafka|Lambda|mock|stub)' \
      docs/feature/release-process-homebrew-github/distill/features/*.feature
  ```

  Results: only `gh attestation verify` (a CLI command name, not a technical-jargon leak); `HTTP 401` mentioned ONCE in US-06 token-failure scenario where the maintainer's mental model legitimately includes the auth-failure shape. No `JSON`, `REST`, `database`, `mock`, `stub`, `class`, `method`, `controller`, status-code-as-jargon. CLI tool names (`cargo`, `git`, `gh`, `brew`) are domain vocabulary for this maintainer-facing feature.

- **Note:** This feature's domain language INCLUDES tool invocations — Jeff Bailey is a developer-maintainer. "Maintainer runs `cargo xtask release-prep`" is valid business language for THIS persona. (See Background `As Jeff Bailey, the modeltap maintainer`.)

- **Verdict:** PASS. The single `HTTP 401` mention in the token-failure scenario could be softened to "an authentication failure" — already done in `then_output_identifies_auth_failure` step.

### Dimension 4 — Coverage Completeness

- **Story coverage:** all 15 user stories (US-01..US-15) and all 6 integration ACs (INT.AC-1..INT.AC-6) have ≥1 scenario each. See `test-scenarios.md` traceability matrix.
- **Edge cases:** dirty working tree, non-monotonic version, missing CHANGELOG section, missing sidecar, malformed sidecar, expired token, build cell failure, manual edit clobber, line-budget overflow, missing purpose comment.
- **Verdict:** PASS.

### Dimension 5 — Walking Skeleton User-Centricity

- **Litmus test for each `@walking_skeleton` scenario:**
  1. "Maintainer prepares a release with one command" — title is user goal. Then steps observe Cargo.toml mutation, CHANGELOG section, exit-zero next-step message. PASS.
  2. "Validate-tag accepts a tag that matches the workspace version" — user goal (the maintainer's intent: "is my tag right?"). Then steps observe exit code + absence of error. PASS.
  3. "Build orchestration runs formatting, linting, and tests before packaging" — user goal (maintainer's intent: "release won't ship code CI would have rejected"). Then steps observe step ordering + only-after-pass artifact production. PASS.
  4. "Release notes are extracted from the matching changelog section" — user goal. Then steps observe the file existing and equaling the section body. PASS.
  5. "Render-formula produces a single-platform formula for the walking skeleton" — user goal (maintainer's intent: "the formula has the right URLs and sha256s"). Then steps observe the version field, URL, sha256, single-block populated state. PASS.
  6. "Bump-tap-formula opens a PR against the ephemeral tap repository" — user goal (maintainer's intent: "the bump did what it needed to"). Then steps observe branch existence in tap repo, commit content, commit message. PASS.
  7. "Devon installs modeltap on a clean Linux machine and verifies the version" — explicit end-user persona; user-observable outcome (`modeltap --version` print). PASS.
- **Verdict:** PASS — 7 walking-skeleton scenarios all describe user goals, not technical layer connectivity.

### Dimension 6 — Priority Validation

- **Bottleneck:** the cross-repo seam (modeltap → tap) is the riskiest assumption per DISCUSS handoff. Walking-skeleton scenario 5 (TAP-BUMP) exercises the FULL local cross-repo seam first; this is the priority-correct order.
- **Simpler alternatives considered:** WS could omit cross-repo (just produce the archive). Rejected: that would not validate the seam; the seam IS the assumption.
- **Constraint prioritization:** atomicity (US-08), version integrity (US-02), CI parity (US-03) all have scenarios in WS (US-02, US-03) or R1 (US-08). Ordering matches priority.
- **Data-justified:** ≥40% error coverage; KPI targets are inherited from DISCUSS (K-PIPE ≥95%, K-T2T ≤15min median).
- **Verdict:** PASS.

### Dimension 7 — Observable Behavior Assertions

- **Mechanical checklist applied to every Then step:**
  - Return value from driving port call? (xtask exit code, captured output) — YES for all assertions on script behavior.
  - Observable outcome (file exists, file content equals X, branch exists in repo, commit message, etc.) — YES for filesystem and git-repo assertions.
  - Internal state, private fields, mock call counts? — flagged Then steps:
    - `then_attest_step_invoked` asserts a step is invoked in the workflow YAML. This is workflow-as-data inspection, NOT a mock-call-count check. The "invocation" is observable in the workflow file content. PASS.
    - `then_gh_auto_merge_invoked` (US-11) asserts the bump step's captured command log contains `gh pr merge --auto --squash`. This is captured via a gh-shim that records invocations under `@real-io`. The assertion is on the OBSERVABLE side-effect (the recorded command), not a mock interaction. PASS.
- **No `assert mock.called`-style assertions** in any step.
- **Verdict:** PASS.

### Dimension 8 — Traceability Coverage

- **Check A (Story → Scenario):** all 15 user stories AND all 6 integration ACs have ≥1 scenario tagged appropriately. See `test-scenarios.md` table. PASS.
- **Check B (Environment → Scenario):** DEVOPS-missing default matrix used (DWD-04). Walking-skeleton scenarios reference the local-tempdir environment (the only one applicable for local-only flow). `@requires_external` and `@requires_docker` scenarios reference the relevant external environments. Tap-repo state (fresh / existing / stale) is exercised across US-12 idempotent-retry scenarios.
- **Verdict:** Check A PASS; Check B PASS subject to DEVOPS reconciliation (logged as a DELIVER concern in DWD-04).

### Dimension 9 — Walking Skeleton Boundary Proof

- **9a (WS strategy declared):** YES — DWD-01 in `wave-decisions.md`; rationale in `walking-skeleton.md`. Strategy C (Real local resources). PASS.
- **9b (Strategy-implementation match):** Strategy C requires `@real-io` for local adapters. Grep `@walking_skeleton @in-memory` in feature files = ZERO matches. PASS.
- **9c (Adapter integration coverage):** every driven adapter has a `@real-io` scenario OR a `@requires_external` smoke. See `adapter-coverage.md`. PASS.
- **9d (WS fixture tier):** litmus test "if I deleted the real adapter, would WS still pass?" = NO for each WS scenario (real `git init`, real `tempfile`, real `cargo`, real Tera). PASS.
- **9e (Strategy drift):** No `@in-memory` markers on any walking-skeleton scenario. PASS.
- **Verdict:** PASS.

## 2. Self-Review Verdict

| Dimension | Verdict |
|---|---|
| 1. Happy Path Bias | PASS |
| 2. GWT Format | PASS |
| 3. Business Language Purity | PASS |
| 4. Coverage Completeness | PASS |
| 5. WS User-Centricity | PASS |
| 6. Priority Validation | PASS |
| 7. Observable Behavior | PASS |
| 8. Traceability Coverage | PASS (with DEVOPS-reconciliation note) |
| 9. WS Boundary Proof | PASS |

**0 blockers, 0 high, 1 low (DEVOPS-reconciliation deferred to DELIVER).**

## 3. RED Scaffold Validation (Mandate 7)

- **Scaffold marker count:** `grep -rn "SCAFFOLD: true" xtask/src/` returns 13 markers across 7 files (1 marker per file as a doc-comment; 6 functional markers as `let _ = SCAFFOLD;` no-op evaluations). All public functions panic with `"Not yet implemented — RED scaffold"`.
- **`cargo check -p xtask`** succeeds (verified by Quinn during scaffold authoring). The scaffolds compile; no BROKEN classification will result from missing imports.
- **`xtask` workspace member declared** in `Cargo.toml` `members`; `default-members` enumerated explicitly excluding `xtask` per ADR-011. `cargo build` (no flags) skips xtask; `cargo build --workspace` includes it. Verified: matches the project's existing convention with `default-members` not previously set, so this DISTILL change additively introduces explicit defaults.
- **`.cargo/config.toml`** created with `xtask = "run --package xtask --quiet --"` alias.

## 4. CM-A through CM-D Mandate Compliance Evidence

| Mandate | Evidence |
|---|---|
| **CM-A** (Hexagonal boundary) | All step files invoke `Command::cargo_bin("xtask")` — the CLI binary IS the driving port. No step file imports `xtask::version::*`, `xtask::formula::*`, etc. directly. (Pure-function modules ARE unit-tested in DELIVER inner loop, separately.) |
| **CM-B** (Business language) | Grep results above: zero `JSON`/`REST`/`database`/`mock` in feature files. Domain vocabulary uses CLI tool names (legitimate for maintainer persona). |
| **CM-C** (User journey completeness) | Every WS scenario has trigger (Given/When), business logic (When), observable outcome (Then), business value. Demo-able to stakeholder Jeff Bailey: "did the maintainer accomplish their goal?" |
| **CM-D** (Pure function extraction) | 6 pure functions extracted: `parse_workspace_version`, `assert_monotonic`, `assert_tag_matches`, `render`, `extract_section`, `lint`. All take strings/structs in, return Results out. Adapters (`git_adapter`, `cargo_adapter`, `gh_adapter`, `cliff_adapter`, `fs_adapter`) are the ONLY layer where fixture parametrization applies (and even then, only at integration-test level, not acceptance). |

## 5. No-Fixture-Theater Audit

For each WS scenario, audited the Given clauses to ensure they set up PRECONDITIONS (input state), not the EXPECTED OUTPUT:

- "Given the workspace version in Cargo.toml is `0.1.0`" — input precondition. PASS.
- "Given there are 17 conventional commits since the v0.1.0 tag" — input precondition. PASS.
- "Given a CHANGELOG.md file containing sections `## [0.1.0]` and `## [0.0.1-rc1]`" — input. The Then clause asserts the EXTRACTED RELEASE_NOTES.md equals the section body (extraction is the system's work). PASS — fixture provides input file content, system performs the section extraction. NOT fixture theater.
- "Given a fixture artifact directory containing 4 sha256 sidecar files" — input. The Then clause asserts the rendered formula's sha256 fields equal the sidecars (render is the system's work). PASS.

No fixture sets up the expected output. If any WS test passes without xtask GREEN implementation, the test design has a bug — fixture is doing the system's work. None identified in this audit.

## 6. Ready for Peer Review

- All 9 critique dimensions PASS in self-review.
- 0 blockers, 0 high.
- 1 low (DEVOPS reconciliation) explicitly deferred to DELIVER, documented in DWD-04.
- All RED scaffolds compile; markers grep-verifiable.
- All artifacts produced per the `Deliverables` section of the user's prompt.

Invoking `nw-acceptance-designer-reviewer` (Sentinel) for hard-gate peer review.
