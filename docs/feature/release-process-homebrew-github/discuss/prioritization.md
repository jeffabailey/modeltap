# Prioritization: release-process-homebrew-github

Outcome-driven prioritization following the formula: **Value × Urgency / Effort = Priority Score** (1-5 scale per dimension), with tie-breaking by Walking Skeleton > Riskiest Assumption > Highest Value.

## Release Priority

| Priority | Release | Target Outcome | KPI(s) targeted | Rationale |
|---|---|---|---|---|
| 1 | Walking Skeleton | One-target end-to-end pipeline proves: tag → build → publish → tap-bump → `brew install` works at all | K-T2T (Tag-To-Tap latency, single-arch baseline), K-PIPE (release workflow success rate) | Validates the riskiest assumption: cross-repo automation against a real Homebrew tap. If this fails, every later release is a fantasy. |
| 2 | Release 1 — Multi-arch real release | Maintainer ships v0.1.0 to all 4 supported platforms (2 macOS, 2 Linux) with atomic guarantees and SLSA attestations | K-T2T (full target matrix), K-COVER (platform coverage), K-PROV (supply-chain provenance) | Brings the pipeline to "real release" parity. Atomic publish guard prevents half-shipped releases — a CRITICAL guardrail. |
| 3 | Release 2 — Hands-off automation | Maintainer can push a tag and walk away: tap-bump auto-merges, retry is idempotent, contributors can read the workflow | K-TOIL (manual steps per release), K-CONTRIB (contributor workflow comprehension) | Polish that converts a working pipeline into a boring one. Lower urgency — the maintainer can babysit a release in v1. |

## Riskiest Assumption Rationale (Maurya)

The riskiest assumption embedded in this feature: **"GitHub Actions can reliably open a PR against a separate Homebrew tap repo, the formula renders correctly, and `brew test-bot` runs against a fresh-installed binary to validate the round trip."**

If this fails, the maintainer falls back to manual `brew bump-formula-pr` invocations, which defeats the entire feature's value proposition. The walking skeleton MUST validate this end-to-end before any multi-arch effort.

## Backlog (Story-Level Priorities)

| Story | Release | Priority | Outcome KPI Link | Dependencies |
|---|---|---|---|---|
| US-A1 | WS | P1 | K-TOIL (one-command prep) | None |
| US-A2 | WS | P1 | K-PIPE (validate-tag prevents version drift) | US-A1 |
| US-A3 | WS | P1 | K-PIPE (CI parity prevents bad releases) | US-A2 |
| US-A4 | WS | P1 | K-T2T (single-target build proves the path) | US-A2, US-A3 |
| US-A5 | WS | P1 | K-T2T (GitHub Release exists) | US-A4 |
| US-A6 | WS | P1 | K-T2T (tap PR exists) | US-A5 |
| US-A15 | WS | P1 | K-T2T (end-user installs and runs) | US-A6 |
| US-A7 | R1 | P2 | K-COVER (4 targets) | US-A4 |
| US-A8 | R1 | P2 | K-PIPE (atomic-publish guard) | US-A7 |
| US-A9 | R1 | P2 | K-PROV (SLSA attestations) | US-A4 |
| US-A10 | R1 | P2 | K-COVER (formula has 4 blocks) | US-A6, US-A7 |
| US-A11 | R2 | P3 | K-TOIL (auto-merge eliminates manual step) | US-A6, US-A10 |
| US-A12 | R2 | P3 | K-PIPE (idempotent retry) | US-A6 |
| US-A13 | R2 | P3 | K-CONTRIB (runbook) | All WS stories merged |
| US-A14 | R2 | P3 | K-CONTRIB (workflow readability) | All other release.yml stories |

## MoSCoW Classification

| Category | Stories | Why |
|---|---|---|
| **Must Have (v1 of this feature)** | US-A1, US-A2, US-A3, US-A4, US-A5, US-A6, US-A7, US-A8, US-A10, US-A15 | Without these, the feature does not deliver: maintainer cannot ship a real multi-arch release, OR end users cannot install. |
| **Should Have (v1 of this feature)** | US-A9, US-A11, US-A12 | Significant value (supply chain trust, hands-off cuts, retry hygiene). Workarounds exist (no attestation, manual merge, manual cleanup of duplicate PRs). |
| **Could Have (v1 of this feature)** | US-A13, US-A14 | Documentation and code-quality polish. Releases work without them; contributor experience suffers slightly. |
| **Won't Have (v1 of this feature)** | macOS notarization, multi-binary support (modeltap-cli), homebrew-core submission, automated yank, release-please/cargo-release | Tracked in story-map.md "Release 3 — Future-proofing" section. Out of scope for this feature. |

## Value × Urgency / Effort Scores

Per release (composite of constituent stories):

| Release | Value | Urgency | Effort | Score | Rank |
|---|---|---|---|---|---|
| Walking Skeleton | 5 (validates riskiest assumption + delivers minimal end-to-end value) | 5 (blocks all later work) | 3 (multi-step but each step well-understood) | 8.3 | 1 |
| Release 1 (multi-arch + atomic + SLSA) | 5 (real multi-platform release) | 4 (needed before announcing v0.1.0) | 3 (matrix expansion + guard + attestation step) | 6.7 | 2 |
| Release 2 (hands-off polish) | 3 (maintainer can babysit v1, polish reduces toil) | 2 (no external deadline) | 2 (small workflow changes + docs) | 3.0 | 3 |

## Dependency Order (Critical Path)

```
US-A1 (release-prep tool)
  -> US-A2 (validate-tag job)
       -> US-A3 (CI parity gates in release.yml)
            -> US-A4 (single-target build)
                 -> US-A5 (publish-github-release)
                      -> US-A6 (bump-tap-formula opens PR)
                           -> US-A15 (end-user install verifies)  [WALKING SKELETON DONE]
                                -> US-A7 (multi-arch matrix)
                                     -> US-A8 (atomic-publish guard)
                                     -> US-A9 (SLSA attestation per archive)
                                     -> US-A10 (formula 4-platform render)  [RELEASE 1 DONE]
                                          -> US-A11 (auto-merge)
                                          -> US-A12 (idempotent retry)
                                          -> US-A13 (RELEASING.md)
                                          -> US-A14 (workflow readability pass)  [RELEASE 2 DONE]
```

US-A1 is the only story with no dependencies. Everything else chains from the walking skeleton.

## Open Decisions Affecting Prioritization

| Decision | If decided differently, what changes? |
|---|---|
| D1 (tap repo location) | If a `modeltap` GitHub org appears mid-feature, US-A6 and US-A10 acquire a token-rotation step; estimate +0.5 day. |
| D2 (release-cut trigger) | If `release-please` is adopted, US-A1 is replaced by a different prep automation; story count unchanged. |
| D6 (notarization) | If notarization is brought into v1, add a story to Release 1 (between US-A4 and US-A5); estimate +1 day; requires Apple Developer account. |
| D7 (auth mechanism) | If GitHub App is chosen instead of PAT, US-A6 grows by ~0.5 day for app installation/setup. |

## Note on Story IDs

Stories are labeled US-A1..US-A15 in this prioritization document. Final IDs (US-01..US-15 in `user-stories.md`) are assigned in Phase 4 and may be reordered by dependency or alphabetically. The "A" prefix in this doc means "release-process Activity" and is a Phase-2.5 placeholder.
