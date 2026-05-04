# KPI Instrumentation — release-process-homebrew-github

**Wave:** DEVOPS (4 of 6)
**Author:** Apex (nw-platform-architect)
**Date:** 2026-05-03

Per-KPI instrumentation: where the data lives, how it's collected, who reviews it, and how often. Implements the data-collection plan in DISCUSS `outcome-kpis.md` "Measurement Plan" section.

## 1. Posture

All KPIs are GitHub-native (per D5, C5). No external telemetry. Three data surfaces:

| Surface | KPIs sourced | Volatility |
|---|---|---|
| GH Actions API (`gh run list`, `gh run view`) | K-PIPE, K-T2T (partial) | 90-day retention; ephemeral beyond |
| GH Releases + Sigstore attestations | K-PROV | Permanent |
| `RELEASING.md` release-log table | K-T2T (canonical), K-COVER (recorded), K-TOIL (recorded) | Permanent (in git) |
| GH Issues with labels | K-PIPE failures, K-CONTRIB | Permanent (until issue deleted) |

## 2. Per-KPI Instrumentation Detail

### K-T2T — Tag-to-Tap latency (NORTH STAR)

**Definition**: Median ≤15 min, p90 ≤25 min, from `git push origin v0.x.0` to a clean machine successfully running `brew install jeffabailey/modeltap/modeltap`.

**Data sources**:

1. `gh run view <run-id> --json createdAt,updatedAt` for the `release.yml` run (gives release.yml duration)
2. `gh pr list --repo jeffabailey/homebrew-modeltap --state merged ...` for the tap PR merge timestamp
3. Maintainer's stopwatch / clock for the final `brew install` verification

**Collection method**: per release, maintainer records ALL THREE timestamps in the `RELEASING.md` release-log table:

```markdown
| version | tag-pushed-at        | release-published-at | tap-merged-at        | T2T duration | platforms verified | provenance verified | notes |
|---------|----------------------|----------------------|----------------------|--------------|--------------------|---------------------|-------|
| v0.2.0  | 2026-05-15 14:32 UTC | 2026-05-15 14:46 UTC | 2026-05-15 14:51 UTC | 19m          | mac-arm64,...      | yes                 | ...   |
```

**Helper command** (to be added to `RELEASING.md` "operational notes"):

```bash
# After release.yml succeeds, fetch the timestamps:
RUN_ID=$(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh run view "${RUN_ID}" --json createdAt,updatedAt --jq '
  "tag-pushed-at:        \(.createdAt[:16] | sub("T"; " ")) UTC
release-published-at: \(.updatedAt[:16] | sub("T"; " ")) UTC"'

# Then the tap PR merge timestamp:
gh pr list --repo jeffabailey/homebrew-modeltap --state merged --limit 1 \
  --json title,mergedAt --jq '.[0] | "tap-merged-at:        \(.mergedAt[:16] | sub("T"; " ")) UTC"'
```

The maintainer copies these three lines, computes T2T duration mentally, and appends a row.

**Review cadence**: per release (immediate); aggregated quarterly to compute median + p90.

**Quarterly review query** (against the markdown table):

```bash
# Crude awk on the release-log section of RELEASING.md
awk -F '|' '/^\| v[0-9]/ {
  gsub(/[ m]/, "", $6);
  print $6
}' RELEASING.md | sort -n | awk '
  { n++; vals[n] = $1 }
  END {
    print "n=" n
    print "median=" vals[int((n+1)/2)] "m"
    print "p90=" vals[int(n * 0.9)] "m"
  }'
```

Owner: maintainer. Cadence: per release + quarterly aggregate.

**Guardrail**: K-PIPE is the guardrail. If K-T2T is fast but K-PIPE drops, the release pipeline is "fast but flaky" — not actually meeting the maintainer-can-walk-away promise.

---

### K-PIPE — Pipeline success rate

**Definition**: ≥95% success rate over rolling last 10 releases. 100% of failures have documented root cause within 24h.

**Data sources**:

1. `gh run list --workflow=release.yml` (success/failure conclusions)
2. `gh issue list --label release-pipeline-failure` (root-cause records)

**Collection method**: AUTOMATIC for the failure-issue creation (`release-pipeline-alert.yml` — see `monitoring-alerting.md` §1). Manual for root-cause classification (maintainer fills triage checklist in issue body).

**Live-view query**:

```bash
gh run list --workflow=release.yml --limit 10 --json conclusion --jq '
  [.[] | select(.conclusion != null)] |
  (map(select(.conclusion == "success")) | length) as $ok |
  length as $n |
  "K-PIPE: \($ok)/\($n) = \( ($ok * 100 / $n) | floor )%"
'
```

**Open-issue check**:

```bash
gh issue list --label release-pipeline-failure --state open
```

**Review cadence**:

- Per release: failure issue auto-opens; maintainer triages within 24h per outcome-kpis.md target
- Monthly: review open issues; close any resolved ones
- Quarterly: aggregate root-cause classes from closed issues; identify systemic risks (e.g., "3 of 5 K-PIPE failures this quarter were toolchain regressions → consider pinning rust-toolchain to a specific version")

Owner: maintainer.

**Guardrail**: must NOT degrade below 90% (per outcome-kpis.md). 90% would mean 1 in 10 releases needs manual fix-and-retry — at that rate, the pipeline is a hazard not an asset.

---

### K-COVER — Platform install coverage

**Definition**: 100% of the 4 supported platform/arch combinations install successfully on a clean reference machine for every released version.

**Data sources**:

1. `brew test-bot` results in the tap-repo PR's status checks (mac-arm64, mac-x86, linux-x86; linux-arm64 via QEMU)
2. Maintainer's manual K-COVER spot-check on at least one platform per release (the "clean reference machine" run)

**Collection method**:

- AUTOMATIC: `brew test-bot` runs on every tap-bump PR. Cells visible at `gh pr view --repo jeffabailey/homebrew-modeltap <pr> --json statusCheckRollup`.
- MANUAL: maintainer records the platforms-verified column in `RELEASING.md` release-log table per release.

**Per-release spot-check command**:

```bash
# After tap PR merges, on a clean reference machine (or via brew --rebuild):
brew tap jeffabailey/modeltap   # if not already added
brew install modeltap
modeltap --version
# Confirm output matches expected version
```

**Review cadence**: per release. Quarterly aggregate review for any platform that has had a K-COVER failure.

Owner: maintainer (CI runs the brew test-bot).

**Guardrail**: must NOT degrade below 100%. Any platform failure is a release-blocking issue (auto-merge withholds; tap PR sits open visibly per US-11).

---

### K-TOIL — Manual steps per release

**Definition**: ≤1 manual step per release (the tag push itself); zero in prep beyond reviewing the prep PR.

**Data source**: `RELEASING.md` numbered-step list (the runbook itself is the spec).

**Collection method**: audit on every release. The maintainer mentally counts: "did I do exactly the steps in RELEASING.md, or did I do extra?" If extra, it's a K-TOIL regression — file an issue and bake the extra step into automation.

**Review cadence**: per release.

Owner: maintainer.

**Guardrail**: K-PIPE catches "zero toil but broken." A pipeline that's fully automated but fails 50% of the time would have great K-TOIL and terrible K-PIPE.

**Helper instruction in RELEASING.md** (recommended addition):

```markdown
## After every release: K-TOIL check

Did you perform exactly these steps and no others?

1. cargo xtask release-prep --version X.Y.Z
2. open prep PR
3. merge prep PR after CI passes
4. git checkout main && git pull
5. git tag -a vX.Y.Z -m "..."
6. git push origin vX.Y.Z
7. (wait for release.yml + brew test-bot to complete)
8. (verify brew install on a clean reference machine)
9. (record the release-log row using the helper command in operational notes)

If you did anything else (manual file edit, extra git operation, copy-paste sha256), file a "release-process-toil" issue describing the manual step. Each new manual step is a target for automation.
```

---

### K-PROV — SLSA L3 build provenance

**Definition**: 100% of release archives carry verifiable SLSA L3 build provenance attestation.

**Data sources**:

1. Per release: `actions/attest-build-provenance@v2` output in `release.yml` build job logs
2. Per archive: `gh attestation verify <archive> --owner jeffabailey` exit code

**Collection method**:

- AUTOMATIC: attestation is produced by `attest-build-provenance@v2` step. Failure of that step fails the build cell, blocking publish (atomic per US-08).
- MANUAL: per-release spot-check by maintainer — verify at least 1 of the 4 archives. Quarterly: full audit (all 4 of the most recent release).

**Spot-check command** (per release):

```bash
LATEST=$(gh release list --limit 1 --json tagName --jq '.[0].tagName')
mkdir -p /tmp/prov-check && cd /tmp/prov-check
gh release download "${LATEST}" --pattern '*.tar.gz' --repo jeffabailey/modeltap
for f in *.tar.gz; do
  gh attestation verify "${f}" --owner jeffabailey || echo "FAILED: ${f}"
done
echo "K-PROV check complete; see above for any FAILED entries"
```

**Review cadence**: per release (spot-check 1 of 4); quarterly (audit all 4 of most recent).

Owner: maintainer.

**Guardrail**: must NOT degrade below 100%. Any missing attestation = supply-chain regression = P0 incident.

---

### K-CONTRIB — Workflow comprehension

**Definition**: 0 release-process-question issues per quarter (target). Contributors who ask "how do releases work?" find their answer in `RELEASING.md` (≤10 numbered steps) and `release.yml` (≤250 lines, every job commented).

**Data source**: `gh issue list --label release-process-question`

**Collection method**: MANUAL. Maintainer applies the `release-process-question` label when triaging incoming issues that contain release-process confusion.

**Quarterly review query**:

```bash
# Issues opened in the last quarter with the label:
gh issue list --label release-process-question --state all \
  --search "created:>=$(date -v-3m +%Y-%m-%d)" \
  --json number,title,createdAt,closedAt
```

**Review cadence**: quarterly.

Owner: maintainer.

**Guardrail**: indirect — K-PIPE is the offset. A comprehensible workflow that breaks weekly is worse than an opaque one that never breaks.

**Action on each occurrence**: every `release-process-question` issue should result in a doc improvement (clarify RELEASING.md, add a comment to release.yml, or both). The label converts contributor confusion into a documentation-improvement signal.

**Label setup** (one-time):

```bash
gh label create release-process-question \
  --color "0E8A16" \
  --description "K-CONTRIB: contributor needed clarification about the release process" \
  --repo jeffabailey/modeltap
```

---

## 3. Aggregate Cadence Calendar

| Cadence | Activities |
|---|---|
| **Per release** (~1-4/month) | Append RELEASING.md row (K-T2T); spot-check K-PROV; spot-check K-COVER (verify on at least one platform); audit K-TOIL (count steps performed); auto: release-pipeline-alert.yml fires if K-PIPE failure |
| **Weekly** (Monday 13:00 UTC) | token-expiry-warning.yml runs; K-CONTRIB issue triage (assign label if applicable) |
| **Monthly** | Review open release-pipeline-failure issues; close any resolved; recompute K-PIPE rolling-10 |
| **Quarterly** | Aggregate K-T2T (median + p90); aggregate K-PIPE root-cause classes; full K-PROV audit (all 4 archives of most recent release); count K-CONTRIB issues opened; review pinned action versions (annual but checked here) |
| **Annual** | Rotate GH_TAP_TOKEN (60 days before expiry); review pinned action versions (proper review per ci-cd-pipeline.md §7); review the KPI definitions themselves (still measuring the right things?) |

## 4. Dashboard Mockup

Re-printed from `observability-design.md` §6 for convenience; the maintainer can build this as a one-page Bash script:

```
=== modeltap release pipeline status ===

RECENT RELEASES (last 10):
  2026-05-15  v0.2.0   success  19m  mac-arm64,mac-x86,linux-x86,linux-arm64  prov:yes
  2026-05-08  v0.1.1   FAILURE  -    -                                          -
  ... (8 more)

K-PIPE (last 10):     9/10 = 90%   [TARGET ≥95%]   ⚠ below target
K-T2T  (median):      19m         [TARGET ≤15m median, ≤25m p90]   ⚠ above median target
K-COVER (last 10):    9/9 = 100%  [TARGET 100%]   ✓
K-PROV  (last 10):    9/9 = 100%  [TARGET 100%]   ✓

OPEN ISSUES:
  release-pipeline-failure: 1 (#42 — v0.1.1 cargo-deny advisory regression)
  tap-token-expiry:         0
  release-process-question: 0

NEXT TOKEN ROTATION:
  Last rotated: 2026-02-15 (per RELEASING.md operational notes)
  Days until next: ~290 (annual cycle)
```

## 5. Future Enhancements (out of scope v1)

| Enhancement | Triggered by | Scope |
|---|---|---|
| Automated K-T2T row insertion | If maintainer manually appends rows often enough to be tedious | A 3rd workflow that polls release.yml + tap PR + opens an auto-PR adding the row to RELEASING.md |
| Automated weekly K-PIPE summary issue | If maintainer wants a regular review forcing function | A scheduled workflow that opens an issue with the K-PIPE 30-day numbers |
| Token expiry days-until-expiry warning | If GH ever exposes PAT expiry via API | Update token-expiry-warning.yml to compare vs. now; warn at 30 days |
| `cargo xtask audit-workflows` | If `release.yml` and `ci.yml` ever drift on toolchain pin | Extend lint-workflows to assert byte-equality of the toolchain step across both files |
| Automated K-PROV verification on every release | If manual spot-check is missed often | A 4th workflow that downloads each archive and runs `gh attestation verify`, adding a `prov-verified` label to the release |

All of these are tracked but explicitly NOT BUILT in v1 per scope discipline.

## 6. Cross-Reference

- DISCUSS `outcome-kpis.md` (KPI definitions, baselines, targets, guardrails)
- DEVOPS `observability-design.md` (where to query data)
- DEVOPS `monitoring-alerting.md` (alert workflows feeding K-PIPE + K-COVER)
- DEVOPS `ci-cd-pipeline.md` (release-pipeline-alert.yml + token-expiry-warning.yml specs)
