# Observability Design — release-process-homebrew-github

**Wave:** DEVOPS (4 of 6)
**Author:** Apex (nw-platform-architect)
**Date:** 2026-05-03

## 1. Posture: GitHub-Native, Privacy-First

Per DISCUSS C5 (privacy by default), `outcome-kpis.md` ("GitHub-native KPIs, no external telemetry"), and DEVOPS D5: **no observability data leaves GitHub**. There is no Datadog, no ELK, no Sentry, no Loki, no Prometheus. The substrate is:

- **GitHub Actions logs** (per-step stdout/stderr; queryable via `gh run view --log` or web UI; ~90 day retention by default for public repos)
- **GitHub Actions API** (`gh api /repos/{owner}/{repo}/actions/runs/...` for run metadata)
- **GitHub Releases API** (`gh release view --json` for asset metadata)
- **`RELEASING.md` release-log table** (the durable, human-curated record; the source of truth for K-T2T and historical analysis)

This document specifies what gets observed, where, and how the maintainer queries it.

## 2. Three-Pillar Mapping

The conventional three pillars (logs, metrics, traces) map onto a GitHub-native pipeline as:

| Pillar | Conventional store | GitHub-native equivalent | Cardinality |
|---|---|---|---|
| Logs | structured logs in ELK/Loki | GH Actions run logs (per step, per job, per cell) | High; per-step granularity; ~90d retention |
| Metrics | Prometheus / Datadog gauges | GH Actions API `runs` endpoint (timestamps, conclusions, durations) + RELEASING.md release-log table | Medium; per-run aggregates |
| Traces | distributed tracing (Tempo, Jaeger) | Job DAG within a single workflow run = the "trace" (no cross-process spans here; everything is one workflow) | Single workflow run shows entire trace via web UI |

The pipeline is small enough that the GH Actions web UI for one run IS the trace view. No additional tooling needed.

## 3. What Gets Observed

### 3.1 Per workflow run (real-time, ~5 sec lag)

Available in `gh run view <run-id>`:

- Trigger event (tag push, with ref name + actor)
- Per-job conclusion (success / failure / skipped / cancelled)
- Per-step duration
- Per-step stdout/stderr
- Artifact list (with sizes)
- Annotations (warnings + errors emitted via `::warning::` and `::error::` markers)

### 3.2 Per release (durable, hand-curated)

Appended to `RELEASING.md` release-log table after every release. Schema (from DISCUSS handoff §2):

| version | tag-pushed-at | release-published-at | tap-merged-at | T2T duration | platforms verified | provenance verified | notes |
|---|---|---|---|---|---|---|---|

Example row:

```markdown
| v0.2.0 | 2026-05-15 14:32 UTC | 2026-05-15 14:46 UTC | 2026-05-15 14:51 UTC | 19m | mac-arm64, mac-x86, linux-x86, linux-arm64 | yes | first warm-cache release; 5min faster than v0.1.0 |
```

This table is the K-T2T data source. Maintainer appends one row per release using the helper instructions in `kpi-instrumentation.md` §K-T2T-collection.

### 3.3 Per-issue (alerting state)

Available via `gh issue list`:

- `release-pipeline-failure` label → opened issues are unresolved K-PIPE failures
- `tap-token-expiry` label → opened issues are pending GH_TAP_TOKEN rotations
- `release-process-question` label → contributor confusion; K-CONTRIB signal

### 3.4 Per attestation (provenance)

Available via `gh attestation verify`:

- For every `modeltap-${VERSION}-${TARGET}.tar.gz`, `gh attestation verify <archive> --owner jeffabailey` returns success/failure
- Maintainer spot-checks per release; quarterly full audit

## 4. The "Dashboard"

A real dashboard would be overkill at 1-4 releases per month. Instead, define **standardized terminal commands** the maintainer runs ad-hoc. These ARE the dashboard:

### 4.1 Recent releases

```bash
gh run list --workflow=release.yml --limit 10 \
  --json databaseId,displayTitle,conclusion,createdAt,updatedAt,event,headBranch \
  --jq '.[] | "\(.createdAt[:16]) \(.conclusion // "running") \(.displayTitle)"'
```

Output (one line per recent release):

```
2026-05-15T14:32 success v0.2.0
2026-05-08T11:04 failure v0.1.1
2026-05-01T16:20 success v0.1.0
```

### 4.2 K-PIPE rolling-10 success rate

```bash
gh run list --workflow=release.yml --limit 10 --json conclusion --jq '
  [.[] | select(.conclusion != null)] |
  (map(select(.conclusion == "success")) | length) as $ok |
  length as $n |
  if $n == 0 then "n=0 (no completed runs)"
  else "K-PIPE: \($ok)/\($n) = \( ($ok * 100 / $n) | floor )%"
  end
'
```

### 4.3 Open release-failure issues

```bash
gh issue list --label release-pipeline-failure --state open
```

### 4.4 K-T2T per recent release

```bash
gh run list --workflow=release.yml --limit 10 --status success \
  --json displayTitle,createdAt,updatedAt --jq '
  .[] | {
    version: .displayTitle,
    duration_min: (((.updatedAt | fromdateiso8601) - (.createdAt | fromdateiso8601)) / 60 | floor)
  }
'
```

This gives only "release.yml duration", which excludes brew test-bot time on the tap repo. The full K-T2T calculation requires combining this with the tap repo PR merge timestamp:

```bash
gh pr list --repo jeffabailey/homebrew-modeltap --state merged --limit 10 \
  --json number,title,mergedAt --jq '
  .[] | "\(.mergedAt[:16]) \(.title)"
'
```

The maintainer correlates these manually when appending to `RELEASING.md`. A future enhancement (out of scope) could automate this via a third workflow that polls and appends.

### 4.5 K-PROV spot-check

```bash
# For the most recent release
LATEST=$(gh release list --limit 1 --json tagName --jq '.[0].tagName')
mkdir -p /tmp/prov-check && cd /tmp/prov-check
gh release download "${LATEST}" --pattern '*.tar.gz' --repo jeffabailey/modeltap
for f in *.tar.gz; do
  echo "Verifying ${f}..."
  gh attestation verify "${f}" --owner jeffabailey || echo "FAILED: ${f}"
done
```

### 4.6 K-COVER status

`brew test-bot` results are visible in tap-repo PR status checks. Per release:

```bash
gh pr view --repo jeffabailey/homebrew-modeltap <pr-number> --json statusCheckRollup
```

The maintainer eyeballs the macos-14 / macos-13 / ubuntu-22.04 cells; all-green = K-COVER 100%.

## 5. Logs — Structure and Retention

### 5.1 Structure

GH Actions logs are inherently structured by:

- workflow → run → job → step → line

Each level is queryable via `gh` CLI with JSON output. No custom log format needed.

For one-off log search across recent runs:

```bash
gh run list --workflow=release.yml --limit 20 --json databaseId,displayTitle --jq '.[].databaseId' | \
  while read id; do
    gh run view "${id}" --log 2>/dev/null | grep -H -i "${SEARCH_TERM:-error}" | head -5
  done
```

### 5.2 Retention

- **GH Actions logs**: 90 days for public repos (GitHub default; not configurable via Actions API)
- **GH Releases**: indefinite (until release deleted)
- **`RELEASING.md` release-log table**: indefinite (in git history)
- **Issues**: indefinite (until issue deleted)

For data older than 90 days, `RELEASING.md` is the only durable record. This is acceptable because K-T2T / K-COVER / K-PROV are recorded per-release in the table; the underlying run logs are not needed retroactively.

### 5.3 Sensitive-data hygiene

Steps that handle secrets must use `${{ secrets.X }}` referencing, not echo the secret value. GitHub Actions automatically masks known secret values in logs. Spot-checked at PR review for new workflow steps.

`bump-tap-formula` job is the highest-risk step (uses `GH_TAP_TOKEN`). All `gh` invocations use `env: GH_TOKEN: ${{ secrets.GH_TAP_TOKEN }}` rather than passing on command line — keeps token out of the process listing.

## 6. Dashboard Mockup (text-art)

If a maintainer ran a hypothetical `modeltap-status` script, this is what they'd see — and it composes the queries above:

```
=== modeltap release pipeline status ===

RECENT RELEASES (last 10):
  2026-05-15  v0.2.0   success  19m  mac-arm64,mac-x86,linux-x86,linux-arm64  prov:yes
  2026-05-08  v0.1.1   FAILURE  -    -                                          -
  2026-05-01  v0.1.0   success  24m  mac-arm64,mac-x86,linux-x86,linux-arm64  prov:yes
  ... (7 more)

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

The maintainer can build this script as a one-page Bash file when needed; it is NOT mandatory infra. Documented in `kpi-instrumentation.md`.

## 7. SLO / Error Budget Framing

Re-frame the KPIs as SLO+budget pairs:

| SLO | Target | Error budget |
|---|---|---|
| K-PIPE: release.yml runs succeed | ≥95% rolling-10 | 5% (1 in 20 runs may need manual fix-and-retry) |
| K-T2T: tag-to-tap latency ≤15 min median | 50% of releases meet median; p90 ≤25 min | 10% may exceed 25 min (cold cache, transient slowness) |
| K-COVER: all 4 platforms install | 100% | 0% (any platform failure is a release-blocking issue) |
| K-PROV: every archive has valid attestation | 100% | 0% (any missing attestation is a supply-chain regression) |
| K-TOIL: ≤1 manual step per release | 100% (only `git push origin v*.*.*`) | 0% (any new manual step is a regression of automation) |
| K-CONTRIB: 0 release-process-question issues per quarter | quarterly count | n/a (qualitative; degrades organically) |

Burn-rate alerting (skill SLO design pattern):

| Burn rate | Trigger | Action |
|---|---|---|
| K-PIPE single failure | one `release-pipeline-failure` issue | Triage within 24h (per outcome-kpis.md target) |
| K-PIPE 2 failures in 5 runs | 2+ open `release-pipeline-failure` issues | Pause new releases; root-cause spike before next tag |
| K-COVER < 100% | brew test-bot cell red on tap PR | Block merge; investigate platform-specific failure |
| K-PROV failure | `gh attestation verify` returns non-zero on spot-check | Treat as P0 supply-chain incident; investigate before next release |

## 8. What This Design Does NOT Do (Explicit)

- **No real-time alerting**. Releases are infrequent enough that the next-business-day issue notification is fine.
- **No cross-release trending dashboard**. Quarterly aggregate review by the maintainer using the RELEASING.md table.
- **No external uptime monitoring** (StatusPage, Pingdom). Brew install is verified per-release; persistent end-user reports come via GitHub issues.
- **No log shipping**. GH Actions logs stay in GH Actions.
- **No metrics export**. GH Actions API IS the metric source; no Prometheus exporter to maintain.

These are deliberate per privacy stance + maintainer-effort stance. Adding any of them is a future enhancement out of scope for v1.

## 9. Cross-Reference

- DISCUSS `outcome-kpis.md` (KPI definitions; data sources; collection methods)
- DEVOPS `kpi-instrumentation.md` (per-KPI implementation detail)
- DEVOPS `monitoring-alerting.md` (alert workflow specifications)
- DEVOPS `ci-cd-pipeline.md` §4 (`release-pipeline-alert.yml`)
- DEVOPS `ci-cd-pipeline.md` §5 (`token-expiry-warning.yml`)
