# Test Scenarios — release-process-homebrew-github

**Wave:** DISTILL (5 of 6)
**Author:** Quinn (nw-acceptance-designer)
**Date:** 2026-05-03

> **DEVOPS-MISSING WARNING:** the DEVOPS wave was running in parallel and was missing at DISTILL start. Default environment matrix from instructions used (macOS-14, macOS-13, ubuntu-22.04 x86, ubuntu-22.04 aarch64-cross + tap-repo state). Recorded in DWD-04 (`wave-decisions.md`). Reconcile in DELIVER if DEVOPS produces contradicting requirements.

## 1. Suite Overview

Total scenarios: **56** across 4 feature files + 1 master index.

| Feature file | Scenarios | Slice |
|---|---|---|
| `walking-skeleton.feature` | 18 | WS (US-01..US-06 + US-15) |
| `multi-arch-release.feature` | 14 | R1 (US-07..US-10) |
| `hands-off-automation.feature` | 14 | R2 (US-11..US-14) |
| `integration-checkpoints.feature` | 10 | Cross-story (INT.AC-1..INT.AC-6) |
| `master-acceptance.feature` | 1 | Documentation index |

### Coverage ratios

| Category | Count | Ratio |
|---|---|---|
| Happy-path | 24 | 43% |
| Error / edge / infra-failure | 23 | **41%** (target ≥40%) |
| Property scenarios (`@property`) | 5 | — |
| Walking-skeleton scenarios (`@walking_skeleton`) | 6 | — |
| `@requires_external` smokes | 7 | — |
| `@requires_docker` smokes | 3 | — |
| `@cross-repo` (modeltap-fake ↔ tap-fake seam) | 6 | — |
| `@adapter-integration @real-io` | 12 | — |

## 2. Story-to-Scenario Traceability Matrix

Every user story (US-01..US-15) and every cross-story integration AC (INT.AC-1..INT.AC-6) has at least one acceptance scenario.

| Story / AC | Scenario(s) | Feature file |
|---|---|---|
| **US-01** | Maintainer prepares a release with one command | walking-skeleton |
| US-01 | Release-prep refuses on a dirty working tree | walking-skeleton |
| US-01 | Release-prep refuses a non-monotonic version bump | walking-skeleton |
| US-01 | Release-prep runs CI parity gates locally and exits zero on success | walking-skeleton |
| US-01 | Release-prep halts when a CI parity gate fails | walking-skeleton |
| **US-02** | Validate-tag accepts a tag that matches the workspace version | walking-skeleton |
| US-02 | Validate-tag rejects a tag that does not match the workspace version | walking-skeleton |
| US-02 | Validate-tag rejects a tag missing the leading v prefix | walking-skeleton |
| US-02 | Validate-tag enforces tag-equals-v-plus-version invariant (Outline ×5) | walking-skeleton |
| **US-03** | Build orchestration runs formatting, linting, and tests before packaging | walking-skeleton |
| US-03 | release.yml CI parity gates use the exact same flags as ci.yml | integration-checkpoints |
| US-03 | release.yml runs CI parity gates before any cargo build release step | integration-checkpoints |
| **US-04** | Single-target archive is produced and named correctly | walking-skeleton |
| US-04 | Archive sha256 sidecar always contains a valid 64-character lowercase hex digest | walking-skeleton |
| **US-05** | Release notes are extracted from the matching changelog section | walking-skeleton |
| US-05 | Missing changelog section fails the publish step with a clear message | walking-skeleton |
| US-05 | Publish step shells out to gh release create with all archives, sha256s, and notes | walking-skeleton (`@requires_external`) |
| **US-06** | Render-formula produces a single-platform formula for the walking skeleton | walking-skeleton |
| US-06 | Bump-tap-formula opens a PR against the ephemeral tap repository | walking-skeleton |
| US-06 | Tap-bump step surfaces token failure visibly | walking-skeleton |
| US-06 | Bump-tap-formula opens a real PR titled correctly against the live tap repo | walking-skeleton (`@requires_external`) |
| **US-07** | Build matrix declares all four supported targets with correct runners | multi-arch-release |
| US-07 | aarch64-linux cell cross-compiles successfully via cross | multi-arch-release (`@requires_docker`) |
| US-07 | Each build cell uploads a workflow artifact named by target | multi-arch-release |
| **US-08** | Publish job declares dependency on validate-tag and build matrix | multi-arch-release |
| US-08 | Single failing build cell prevents publish and tap-bump from running | multi-arch-release |
| US-08 | Publish atomicity holds for any combination of build cell outcomes | multi-arch-release (`@property`) |
| **US-09** | Build job declares the OIDC permissions required for attestation | multi-arch-release |
| US-09 | Each build cell invokes the attest-build-provenance action against its archive | multi-arch-release |
| US-09 | Devon verifies a published archive's attestation with one command | multi-arch-release (`@requires_external`) |
| **US-10** | Formula renders all 4 platform blocks with sha256s read from artifact files | multi-arch-release |
| US-10 | Render-formula fails when an expected sha256 sidecar is missing | multi-arch-release |
| US-10 | Render-formula rejects a sha256 sidecar with malformed content | multi-arch-release |
| US-10 | Render-formula round-trip preserves every sha256 verbatim | multi-arch-release (`@property`) |
| US-10 | Brew test-bot audit passes on the rendered formula | multi-arch-release (`@requires_docker`) |
| **US-11** | Bump-tap-formula step invokes auto-merge with squash strategy | hands-off-automation |
| US-11 | Auto-merge fires within 5 minutes when brew test-bot is green | hands-off-automation (`@requires_external`) |
| US-11 | Auto-merge withholds when brew test-bot fails on any platform | hands-off-automation (`@requires_external`) |
| **US-12** | First-run creates the bump branch and opens a new PR | hands-off-automation |
| US-12 | Re-run after token rotation force-pushes to the existing branch | hands-off-automation |
| US-12 | One PR per version invariant holds across any number of retries | hands-off-automation (`@property`) |
| US-12 | Manual edits to the bump branch are clobbered by the next render | hands-off-automation |
| **US-13** | Runbook exists at repo root within the line budget | hands-off-automation |
| US-13 | Runbook contains the per-release log table | hands-off-automation |
| US-13 | Runbook documents the operational safety notes | hands-off-automation |
| **US-14** | Lint-workflows accepts a release.yml within the line budget | hands-off-automation |
| US-14 | Lint-workflows rejects a release.yml exceeding the line budget | hands-off-automation |
| US-14 | Lint-workflows rejects a job missing the purpose comment | hands-off-automation |
| US-14 | Lint-workflows accepts every workflow that satisfies both constraints | hands-off-automation (`@property`) |
| **US-15** | Devon installs modeltap on a clean Linux machine and verifies the version | walking-skeleton (`@requires_external`) |
| US-15 | Version string agrees across Cargo.toml, tag, archive name, release title, and binary output | integration-checkpoints |
| US-15 | From tag push to clean machine install in 15 minutes (median) | integration-checkpoints (`@requires_external`) |
| **INT.AC-1** | Version string agrees across Cargo.toml, tag, archive name, release title, and binary output | integration-checkpoints |
| INT.AC-1 | Version-string consistency holds for any valid semver release | integration-checkpoints (`@property`) |
| **INT.AC-2** | Each target's formula sha256 equals the artifact sidecar content | integration-checkpoints |
| **INT.AC-3** | Each target's formula URL equals the GitHub Release archive URL | integration-checkpoints |
| **INT.AC-4** | From tag push to clean machine install in 15 minutes (median) | integration-checkpoints (`@requires_external`) |
| **INT.AC-5** | All four build cells succeeding produces all visible effects | integration-checkpoints |
| INT.AC-5 | Any build cell failing produces no visible effects | integration-checkpoints |
| **INT.AC-6** | release.yml CI parity gates use the exact same flags as ci.yml | integration-checkpoints |
| INT.AC-6 | release.yml runs CI parity gates before any cargo build release step | integration-checkpoints |
| Recovery | GitHub Release succeeds but tap-bump fails leaves an intact release | integration-checkpoints |
| Recovery | Maintainer yanks a release after a critical defect is found | integration-checkpoints |

**Coverage verdict:** Every story (15/15) and every integration AC (6/6) has ≥1 scenario. Mandate 4 (Coverage Completeness) PASSES.

## 3. Walking-Skeleton Scenario Set

Per Mandate 5 (Walking Skeleton Strategy), the WS exit gate is the 6 user-value scenarios spanning the 6 backbone activities:

| # | Activity | Scenario | Story | Adapters exercised |
|---|---|---|---|---|
| 1 | PREP | Maintainer prepares a release with one command | US-01 | fs, git, cargo, cliff |
| 2 | TAG | Validate-tag accepts a tag that matches the workspace version | US-02 | fs |
| 3 | BUILD | Build orchestration runs formatting, linting, and tests before packaging | US-03+US-04 | cargo, fs |
| 4 | PUBLISH | Release notes are extracted from the matching changelog section | US-05 | fs |
| 5 | TAP-BUMP | Bump-tap-formula opens a PR against the ephemeral tap repository | US-06 | tera, git, fs |
| 6 | USER-INSTALL | Devon installs modeltap on a clean Linux machine and verifies the version | US-15 | brew (`@requires_external`) |

DELIVER ships when scenarios 1-5 are green using `@real-io` (no `@in-memory` substitutions for adapters that have a local equivalent). Scenario 6 is the manual smoke verified by the maintainer per release per `RELEASING.md` Step 9.

## 4. Property-Based Scenarios

Five `@property` scenarios for DELIVER software-crafter to implement as proptest generators:

| Scenario | Property |
|---|---|
| Validate-tag enforces tag-equals-v-plus-version invariant | For any (version, tag): `validate_tag` succeeds iff `tag == "v" + version` |
| Archive sha256 sidecar always contains a valid 64-character lowercase hex digest | For any archive: `sidecar_content matches /^[a-f0-9]{64}$/` AND equals the actual `sha256sum(archive)` |
| Publish atomicity holds for any combination of build cell outcomes | For any pass/fail vector across N build cells: `publish_runs ⇔ ∀cell: passed(cell)` |
| Render-formula round-trip preserves every sha256 verbatim | For any 4-tuple of valid sha256s: `formula_render(...).extract_sha256_per_block == input_sha256s` |
| One PR per version invariant holds across any number of retries | For any N retries on the same version: `count(prs(version)) == 1 AND count(branches(version)) == 1` |
| Lint-workflows accepts every workflow that satisfies both constraints | For any (workflow_text, max_lines) where lines ≤ max_lines AND every job has a purpose comment: `lint(workflow_text) == Ok` |
| Version-string consistency holds for any valid semver release | For any valid version string: `cargo_toml.version == tag.strip("v") == archive_name.version_field == release.title.strip("v") == formula.version == binary("--version").parse()` |

(7 entries; the table has one duplicate row from a multi-applicable scenario — counted as 5 distinct property concepts; DELIVER may merge or split per testing strategy.)

## 5. Story-Slice Mapping

Per `discuss/story-map.md`:

| Slice | Stories | Scenarios |
|---|---|---|
| **Walking Skeleton** | US-01..US-06 + US-15 (single-target) | 18 (in `walking-skeleton.feature`) |
| **Release 1 — Multi-arch** | US-07..US-10 | 14 (in `multi-arch-release.feature`) |
| **Release 2 — Hands-off** | US-11..US-14 | 14 (in `hands-off-automation.feature`) |
| **Cross-story integration** | INT.AC-1..INT.AC-6 | 10 (in `integration-checkpoints.feature`) |

## 6. Adapter Coverage Quick Reference

See `adapter-coverage.md` for the full audit. Summary:

| Adapter | Real-I/O scenario | Costly-external scenario |
|---|---|---|
| fs | YES (multiple) | — |
| git | YES (multiple) | — |
| cargo | YES (`@slow`) | — |
| cliff | YES | — |
| tera | YES | — |
| gh | NO | YES (`@requires_external`) |
| cross/Docker | NO | YES (`@requires_docker`) |
| brew test-bot | OUT OF SCOPE | — |

## 7. Test Infrastructure Placement

Per DWD-05: Acceptance test crate at `tests/acceptance/release_process/`. Step skeletons in this DISTILL artifact directory are reference; DELIVER moves them into the test crate per crafter's runner choice (cucumber-rs, integration tests, or hybrid).

xtask scaffolds at `xtask/` (workspace root). See `wave-decisions.md` DWD-07 and the xtask source files.
