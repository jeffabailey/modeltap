# Outcome KPIs — release-process-homebrew-github

## Feature: release-process-homebrew-github

### Objective

The modeltap maintainer can cut a release with a single tag push, the release lands in Homebrew within fifteen minutes, and end users install modeltap with one `brew install` command — every release, predictably, with zero manual steps after the tag push.

### Outcome KPIs

| # | Who | Does What | By How Much | Baseline | Measured By | Type |
|---|---|---|---|---|---|---|
| K-T2T | Maintainer (Jeff) | Cuts a release that lands in Homebrew (tap PR merged + brew installable) | Median ≤ 15 minutes from `git push origin v0.x.0` to a clean machine successfully running `brew install jeffabailey/modeltap/modeltap`; p90 ≤ 25 minutes | N/A (no release pipeline exists today) | Workflow run timestamps (`gh run view --json createdAt,updatedAt`) plus tap PR merge timestamp; aggregated per release in `RELEASING.md` release log | Leading (outcome) |
| K-PIPE | Maintainer | Completes a release pipeline run successfully end-to-end (no manual intervention) | ≥ 95% pipeline success rate over rolling last 10 releases (i.e., at most 1 in 20 needs manual fix-and-retry); 100% of failed pipelines have a documented root cause within 24h | N/A | GitHub Actions run-history for `release.yml`; manual issue tagged `release-pipeline-failure` for tracking root causes | Leading (outcome) |
| K-COVER | End-user (Devon) | Successfully installs modeltap on their platform via `brew install` | 100% of the 4 supported platform/arch combinations (mac-arm64, mac-x86, linux-x86, linux-arm64) install successfully on a clean reference machine for every released version | N/A | Per-release post-publish verification: tap repo `brew test-bot` runs install on macos-14, macos-13, ubuntu-22.04 (x86 and arm via QEMU) and asserts `modeltap --version` matches | Leading (guardrail) |
| K-TOIL | Maintainer | Performs manual steps during a release | ≤ 1 manual step per release (the tag push itself); zero manual steps in the prep phase beyond reviewing the prep PR | N/A (current state would require: bump version, edit changelog, build per platform, upload, hand-edit formula = 8+ steps) | `RELEASING.md` enumerates exactly the manual steps; auditable | Leading (secondary) |
| K-PROV | End-user concerned about supply chain | Verifies the binary they installed was built by the official GitHub Actions workflow | 100% of release archives (every target, every release) carry a verifiable SLSA L3 build provenance attestation | N/A | `gh attestation verify <archive>` returns success for every published archive; spot-checked per release | Leading (guardrail) |
| K-CONTRIB | Open-source contributor (Riley) | Reads the release workflow and runbook end-to-end and can explain it correctly in 5 minutes | 100% of contributors who ask "how do releases work?" can find their answer in `RELEASING.md` (≤10 numbered steps) and `release.yml` (≤250 lines, every job commented) | N/A | Qualitative; tracked via "release-process-question" GitHub issue label — target zero confused-newcomer issues per quarter post-feature ship | Leading (secondary) |

### Metric Hierarchy

- **North Star**: K-T2T — median tag-to-tap latency. The whole feature exists so the maintainer can ship and walk away. If this is fast and reliable, everything else follows.
- **Leading Indicators**:
  - K-PIPE — pipeline success rate predicts whether K-T2T is reproducible vs lucky.
  - K-COVER — platform coverage predicts whether end users actually install successfully.
- **Guardrail Metrics**:
  - K-PIPE — must NOT degrade below 90% (a release pipeline that fails 1 in 5 times is not a pipeline, it is a hazard).
  - K-COVER — must NOT degrade below 100% (a single broken platform means real users are blocked).
  - K-PROV — must NOT degrade below 100% once introduced (skipping attestation silently is a supply-chain regression).
- **Secondary Indicators**:
  - K-TOIL — manual-step count; converts to maintainer satisfaction.
  - K-CONTRIB — workflow comprehension; converts to ecosystem health over time.

### Measurement Plan

| KPI | Data Source | Collection Method | Frequency | Owner |
|---|---|---|---|---|
| K-T2T | `gh run view` for `release.yml` + tap repo PR merge timestamp + manual reference-machine `brew install` log | Aggregated per release into `RELEASING.md` release log table | Per release | Maintainer |
| K-PIPE | GitHub Actions run history filter: `workflow:release.yml status:failure` over last 10 runs | Manual review on every release; tagged issue if failed | Per release + monthly review | Maintainer |
| K-COVER | Tap repo `brew test-bot` workflow run results | Automated; `brew test-bot` runs on every tap-bump PR | Per release | Maintainer (CI runs it) |
| K-TOIL | `RELEASING.md` numbered-step count vs actual steps performed | Audit on every release | Per release | Maintainer |
| K-PROV | `gh attestation verify <archive>` for each archive | Spot-check on every release; full audit quarterly | Per release | Maintainer |
| K-CONTRIB | GitHub issues with label `release-process-question` | Manual triage; aim for zero per quarter | Quarterly | Maintainer |

> No telemetry leaves the modeltap repo for these KPIs. All measurement uses GitHub-native data (Actions runs, PR timestamps, issue labels) plus a single hand-maintained release log in `RELEASING.md`. Consistent with C5 (privacy by default).

### Hypothesis

We believe that **a tag-triggered GitHub Actions workflow that builds 4 targets in parallel, atomically publishes a GitHub Release, and opens an auto-merging PR against a separate Homebrew tap repo** for **the modeltap maintainer (and through him, end users on macOS and Linux)** will achieve **a median 15-minute tag-to-tap latency, ≥95% pipeline success rate, and 100% platform coverage on every release**.

We will know this is true when **the maintainer can run `git push origin v0.x.0` on a Friday evening and, by the time he closes his laptop 20 minutes later, a clean machine running `brew install jeffabailey/modeltap/modeltap` succeeds with the new version — without the maintainer having performed any manual step beyond the tag push itself**.

### Smell Test (per KPI)

| KPI | Measurable today? | Rate not total? | Outcome not output? | Has baseline? | Team can influence? | Has guardrails? |
|---|---|---|---|---|---|---|
| K-T2T | Yes (after pipeline exists) | Time-bounded threshold per release; aggregate is a median | Yes — measures end-user-observable behavior (install works) | N/A — greenfield | Yes — the workflow design fully owns this | Yes — K-PIPE catches "fast but broken" |
| K-PIPE | Yes (GH Actions history) | Yes — % of runs that succeed | Yes — measures pipeline reliability behavior | N/A | Yes — the workflow design owns this | Itself a guardrail; K-T2T catches "reliable but slow" |
| K-COVER | Yes (`brew test-bot` results) | Yes — % of platforms working | Yes — measures install-success behavior | N/A | Yes — matrix targets and formula design own this | Itself a guardrail |
| K-TOIL | Yes (audit on each release) | Per-release count | Yes — measures maintainer-effort behavior | N/A | Yes — workflow automation owns this | K-PIPE catches "zero toil but broken" |
| K-PROV | Yes (`gh attestation verify`) | Per-archive % verified | Yes — measures supply-chain hygiene behavior | N/A | Yes — `attest-build-provenance@v2` step owns this | Itself a guardrail |
| K-CONTRIB | Partial (issue labels are a weak proxy) | Per-quarter count | Yes — measures contributor confidence behavior | N/A | Partial — workflow legibility + runbook quality own this; community size is exogenous | None directly; offset by K-PIPE (a comprehensible workflow that breaks weekly is worse than an opaque one that never breaks) |

All KPIs pass the smell test. K-CONTRIB is the weakest (qualitative proxy) — flagged for the platform-architect to consider whether issue-labeling is the right instrument or whether a periodic survey would be stronger.

### Handoff Notes for DEVOPS (platform-architect)

1. **Instrumentation is GitHub-native.** No external telemetry, no log shipping. All KPIs derive from `gh` CLI queries against existing GitHub data (Actions runs, PR timestamps, issue labels).
2. **`RELEASING.md` is the human-readable release log.** Each release adds a row: `| version | tag-pushed-at | release-published-at | tap-merged-at | T2T duration | platforms verified | provenance-verified | notes |`. This doubles as the runbook AND the data source for K-T2T and K-COVER.
3. **No real-time dashboards needed** — releases are too infrequent (estimated 1-4/month) to justify dashboards. Quarterly aggregate review is sufficient for K-PIPE and K-PROV.
4. **Alert thresholds**:
   - K-PIPE: a pipeline failure should automatically open a `release-pipeline-failure`-tagged issue with a link to the failing run (DEVOPS to wire via a small follow-up workflow on `workflow_run: completed` if status is failure).
   - K-COVER: `brew test-bot` failure on the tap-bump PR is the alert (auto-merge withholds, PR sits open visibly).
   - K-T2T regression: > 30 minutes triggers manual review (no automated alert; releases are infrequent enough to spot manually).
5. **Baseline establishment**: this is a greenfield feature. The first release IS the baseline; collect 3-5 releases of data before evaluating against targets.
6. **Provenance verification key reference**: end users who care about K-PROV can run `gh attestation verify <archive> --owner jeffabailey`. Document this in the README troubleshooting section.
