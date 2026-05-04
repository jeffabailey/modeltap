# Monitoring and Alerting — release-process-homebrew-github

**Wave:** DEVOPS (4 of 6)
**Author:** Apex (nw-platform-architect)
**Date:** 2026-05-03

This document specifies the alerting workflows, rollback procedures, end-user-facing troubleshooting documentation, and rate-limiting advisory. It closes DEVOPS handoff items #1, #2, #3, #5 plus Eclipse-flagged rate-limit advisory.

## 1. Workflow-Failure Alerting (`release-pipeline-alert.yml`)

### 1.1 Closes

- DEVOPS handoff item #1: "K-PIPE alerting — workflow failures notify maintainer"
- DEVOPS handoff item #5: "Workflow-failure alerting routing"
- `outcome-kpis.md` "Handoff Notes for DEVOPS" §4: K-PIPE alert wiring

### 1.2 Specification

See `ci-cd-pipeline.md` §4 for the full YAML specification. Summary:

- **Trigger**: `workflow_run: { workflows: [release], types: [completed] }`
- **Filter**: `if: github.event.workflow_run.conclusion != 'success'`
- **Action**: open a GitHub Issue with label `release-pipeline-failure`, title containing the failed run's display title and conclusion, body containing run URL, run ID, triage checklist
- **Routing**: `--assignee github.repository_owner` ensures the maintainer receives the standard GH issue-mention email

### 1.3 Routing decision: Why GitHub Issues (and not email/Slack/Discord webhook)

Considered three alerting destinations:

| Destination | Pros | Cons | Decision |
|---|---|---|---|
| GitHub Issues (chosen) | Persistent (closeable, labelable, searchable); native GH notifications already in maintainer's inbox; no third-party | Slight delay (~1 min vs immediate webhook) | **Chosen** — fits D5 (GitHub-native), C5 (privacy), single-maintainer reality |
| Email via SMTP action | Immediate | Requires SMTP credentials; another secret to manage; harder to follow up on | Rejected |
| Slack/Discord webhook | Real-time chat surface | Requires webhook URL secret; couples release pipeline to chat tool that may not exist; D5 forbids external telemetry/integrations beyond GH | Rejected |

GitHub Issue is the **same surface where the maintainer files all other work** for this project. No new tab, no new app, no new account. Notification fan-out is GitHub's default behavior.

### 1.4 Failure-mode catalogue

When a `release-pipeline-failure` issue opens, the body's triage checklist guides classification. Common root-cause categories:

| Category | Symptom | Typical fix |
|---|---|---|
| Tag mismatch (US-02) | `validate-tag` fails within ~30s | Re-tag with correct version |
| Toolchain regression | clippy/fmt fails after `dtolnay/rust-toolchain@stable` updates | Fix-forward in source; new tag |
| Test failure | `cargo test --workspace --locked` fails in `build` cell | Fix in source; new tag |
| `cargo deny check` fails (license/advisory regression) | `build` cell fails on deny | Update `deny.toml` or rotate dependency |
| Network blip in `cross` Docker pull | `build` aarch64-linux cell fails on `cross` setup | Re-run failed cell |
| Provenance attestation fails | `attest-build-provenance@v2` returns non-zero | Investigate Sigstore status; re-run |
| `gh release create` fails | `publish-github-release` job fails | GitHub Releases API outage (rare); re-run |
| `GH_TAP_TOKEN` expired/invalid | `bump-tap-formula` 401 from `gh` | Rotate token; re-run failed job (US-12) |
| Tap-repo branch protection misconfigured | `bump-tap-formula` push refused | Reconfigure branch protection; re-run |
| `brew test-bot` fails in tap repo | tap PR red status check; auto-merge withholds | Investigate platform-specific failure; this is K-COVER not K-PIPE |

The triage checklist captures root cause in the issue body. Quarterly K-PIPE review (per `kpi-instrumentation.md`) aggregates root-cause classes.

### 1.5 Self-failure handling

If `release-pipeline-alert.yml` itself fails (e.g., GH Actions outage during issue creation), the failure is silent — no alert is fired about the alert workflow not firing. This is acceptable because:

1. Release frequency is low (~1-4/month); a missed alert is caught by the next manual K-PIPE audit (monthly per `kpi-instrumentation.md`).
2. The maintainer is also a watcher of `release.yml` and receives GH's default workflow-failure email already (independent of this follow-up workflow). The follow-up workflow's value is the *issue* — the persistent, classifiable record — not real-time notification.

### 1.6 Self-test (first-deploy validation)

Documented in `ci-cd-pipeline.md` §4.5 and `RELEASING.md` "First-time setup":

1. Push tag `v0.0.0-alert-test` (intentionally mismatches `Cargo.toml` version)
2. `validate-tag` fails within ~30s
3. `release-pipeline-alert.yml` fires within ~2 min
4. `release-pipeline-failure` issue appears, assigned to maintainer
5. Close issue, delete tag

This validates: workflow_run delivery, label existence, issue creation, assignee notification — end-to-end.

## 2. `GH_TAP_TOKEN` Expiry Monitoring (`token-expiry-warning.yml`)

### 2.1 Closes

DEVOPS handoff item #3: "GH_TAP_TOKEN expiry monitoring (tokens expire annually with fine-grained PATs — silent expiry would break releases)"

### 2.2 Specification

See `ci-cd-pipeline.md` §5 for the full YAML. Summary:

- **Trigger**: weekly schedule (Mondays 13:00 UTC) + `workflow_dispatch` for ad-hoc check
- **Probe**: `gh api /repos/jeffabailey/homebrew-modeltap` using `GH_TAP_TOKEN`
- **Failure action**: open a `tap-token-expiry`-labeled issue (idempotent — only one open at a time)

### 2.3 Limitation: no advance warning

GitHub does NOT expose PAT expiry-timestamp via any public API. The probe can only detect *already-expired* or *revoked* tokens, not *about-to-expire*.

**Mitigation**: maintainer records the rotation date in `RELEASING.md` "operational notes" section when rotating. The annual rotation reminder is the maintainer's calendar discipline, not pipeline automation.

**Future enhancement (out of scope)**: a `TOKEN_EXPIRY_DATE` repo variable updated on rotation; `token-expiry-warning.yml` could compare against `now()` and warn at <30 days. Tracked in `kpi-instrumentation.md` as a v2 enhancement.

### 2.4 Rotation procedure (one-time-per-year)

Documented in `RELEASING.md` "operational notes" section. Steps:

1. Generate replacement fine-grained PAT at https://github.com/settings/personal-access-tokens
   - Repository access: only `jeffabailey/homebrew-modeltap`
   - Repository permissions: Contents (Read+Write), Pull requests (Read+Write), Metadata (Read)
   - Expiration: 365 days
2. Update `Settings → Secrets and variables → Actions → Repository secrets → GH_TAP_TOKEN` on `jeffabailey/modeltap` with the new value
3. Trigger `token-expiry-warning.yml` via `workflow_dispatch` to confirm probe succeeds
4. Delete the old PAT at https://github.com/settings/personal-access-tokens
5. Append rotation date to `RELEASING.md` operational notes
6. If a `tap-token-expiry` issue is open, close it with comment "Rotated YYYY-MM-DD"

## 3. README Troubleshooting Section (End-User Documentation)

### 3.1 Closes

DEVOPS handoff item #2: "README troubleshooting section for end users (gatekeeper warning workaround per D3, attestation verify command)"

### 3.2 Recommended README content

To be added to `README.md` by software-crafter during DELIVER wave (per US-09.AC-5 and US-13). The content draft below SHOULD be incorporated verbatim or with minor edits; this section IS the deliverable.

````markdown
## Installation troubleshooting

### macOS Gatekeeper warning on first run

The first time you run `modeltap` on macOS 13 or 14, you may see a warning:

> "modeltap" cannot be opened because Apple cannot check it for malicious software.

This is because we do not currently sign or notarize the macOS binaries (tracked
as a future enhancement; see ADR roadmap). To proceed:

```sh
xattr -d com.apple.quarantine $(brew --prefix)/bin/modeltap
modeltap --version
```

After this one-time step, `modeltap` runs without further prompts.

### Verifying the binary's build provenance (optional, supply-chain conscious users)

Every release archive carries a SLSA Level 3 build provenance attestation,
proving the binary was built by the official GitHub Actions workflow on
`jeffabailey/modeltap`. To verify:

```sh
# Download the archive matching your platform
gh release download v0.2.0 --pattern 'modeltap-*-aarch64-apple-darwin.tar.gz'

# Verify the attestation
gh attestation verify modeltap-0.2.0-aarch64-apple-darwin.tar.gz --owner jeffabailey
```

A successful verification confirms:

- The archive was built by `.github/workflows/release.yml` in `jeffabailey/modeltap`
- The build was triggered by a tag push (not a manual workflow_dispatch)
- The archive's SHA-256 matches what was attested at build time

### Linux: aarch64 binaries are cross-compiled

Linux ARM64 binaries (used by Raspberry Pi 4/5, Apple Silicon Macs in Linux VMs,
ARM-based EC2 instances) are cross-compiled on x86_64 build runners using
`cross`. They run on native aarch64 Linux as well as via QEMU on x86_64 hosts.
WSL2 on Windows uses the Linux x86_64 binary (WSL2 transparently provides x86_64
compatibility regardless of the host CPU).

### Where do I report install problems?

File an issue at https://github.com/jeffabailey/modeltap/issues with:

- Your OS and architecture (`uname -s -m`)
- Your Homebrew version (`brew --version`)
- The exact command you ran
- The full error output
````

### 3.3 README integration plan

- Software-crafter adds the above section to `README.md` during DELIVER wave per US-09.AC-5 and US-13
- Section title: `## Installation troubleshooting`
- Placement: after `## Installation` section, before `## Usage` section (or wherever fits the existing README skeleton)

## 4. Rollback Procedures

The "deployment unit" is a tag/release per D6 (Recreate strategy). There is no traffic-shifting; rollback means reverting the discoverable artifacts.

### 4.1 Pre-deployment validation

Per `nw-deployment-strategies` skill:

| Validation | How performed | When |
|---|---|---|
| Build scripts tested | `cargo xtask release-prep` runs locally before tag push (US-01) | Pre-tag, by maintainer |
| Database migrations tested | n/a (no database) | n/a |
| Config consistency | n/a (no runtime config) | n/a |
| Health checks configured | n/a (binary has no health endpoint) | n/a |
| Monitoring prepared | `release-pipeline-alert.yml` + `token-expiry-warning.yml` deployed | Once, at feature merge |
| Backup & DR procedures | git history is the backup; release artifacts re-buildable from any tag | Always available |
| Rollback procedure tested | First release dry-run includes rollback rehearsal (per below) | Once, at first release |

### 4.2 Design Rollback First

Per `nw-deployment-strategies` skill principle: every deployment plan starts with rollback. For this pipeline:

#### Scenario A: Pipeline failed BEFORE GH Release was created

- **State**: tag exists; some build cells succeeded; `publish-github-release` did NOT run (atomic guard per US-08).
- **Rollback**: nothing to undo. The atomic guard prevented partial publish.
- **Recovery**: diagnose failure via `release-pipeline-failure` issue. If transient (network, etc.) re-run failed jobs in GH UI. If real bug, fix in source, retag (delete old tag, push new tag with same version IF nothing was published; otherwise bump to next patch).
- **Action**: zero customer-facing impact.

#### Scenario B: Pipeline failed AFTER GH Release published, BEFORE tap PR merged

- **State**: GH Release exists with all archives; tap repo is one version behind.
- **Rollback (option 1, preferred)**: re-run failed `bump-tap-formula` job in GH UI (US-12 idempotent retry). Tap catches up; no rollback needed.
- **Rollback (option 2, if tap-side bug discovered)**: delete the GH Release manually:

```sh
gh release delete v${VERSION} --yes --cleanup-tag --repo jeffabailey/modeltap
```

This deletes both the release and the tag. Then fix and retag.

#### Scenario C: Pipeline succeeded but bug discovered in published binary

- **State**: GH Release published; tap repo updated; users can now `brew install` a buggy binary.
- **Rollback**: bump version (NEVER edit a published release). Cut a new release with the fix. Optionally:

```sh
# Mark the buggy release as draft (hides it from end users; keeps it for forensics)
gh release edit v${VERSION} --draft --repo jeffabailey/modeltap
```

- **Tap revert**: the next release's bump-tap-formula PR overwrites the formula. End users on `brew upgrade` get the new version automatically. For urgent revert (rare), manually open a PR against the tap repo reverting Formula/modeltap.rb to the previous version's content; merge after `brew test-bot` passes.

#### Scenario D: SLSA attestation reveals tampering on an existing release

- **State**: `gh attestation verify` fails on a published archive.
- **Rollback**: P0 incident. Delete the release immediately. Investigate via Sigstore log + GH Audit Log. Re-build from source on a new tag.

### 4.3 Automated rollback triggers

This pipeline does NOT have automated rollback. The failure modes above all require human judgment (diagnose → decide → act). Per `nw-deployment-strategies`: automated rollback applies to traffic-shifting deployments, which this pipeline doesn't perform.

The closest equivalent: **brew test-bot in the tap repo IS the automatic rollback** — if it fails, auto-merge withholds, and the buggy formula never reaches end users.

### 4.4 Manual rollback decision criteria

Per skill — when to manually rollback:

- Stakeholder-reported functional issues: end-user issue filed within 24h of release with reproducible install failure on a supported platform → consider drafting (Scenario C) and bumping
- Security vulnerability discovered post-deploy: P0 — Scenario C action immediately
- Data integrity concerns: n/a (binary doesn't manage user data at this layer)
- Performance degradation below acceptable levels: assessed case-by-case; usually fix-forward via new release

## 5. Rate-Limiting Advisory (Eclipse-Flagged)

### 5.1 Closes

Eclipse (DISCUSS reviewer) low-severity flag: "Rate-limiting note for GH API"

### 5.2 Risk

The release pipeline makes ~30-50 GH API calls per release (checkouts, artifact ops, release create, attestation, PR ops, status check polling). At ~1-4 releases/month, total volume is well below GitHub's authenticated rate limits:

- `GITHUB_TOKEN`: 1,000 req/hour per repo (release.yml well under)
- `GH_TAP_TOKEN` (PAT): 5,000 req/hour for fine-grained PATs
- Unauthenticated: 60 req/hour (n/a — every call we make is authenticated)

### 5.3 When this becomes a concern

- Multiple releases triggered in close succession (e.g., 5 patch releases in an hour during emergency triage) — could approach 1,000 req/hour for a single repo
- Heavy `gh` polling in scheduled or follow-up workflows (currently we have weekly token-warning + per-failure alert; both well under)
- Large artifact uploads do NOT count against API rate limits (separate quota for asset upload)

### 5.4 Mitigation strategy (not actively applied)

If rate-limiting is ever observed:

1. Inspect `gh api -i /rate_limit` to confirm
2. Backoff: `gh` retries 503/429 automatically with exponential backoff; no client-side change needed
3. Spread emergency release retries with `--max-retries` flag if needed
4. As last resort: switch ad-hoc-query Bash scripts in `RELEASING.md` to `gh api --paginate` to reduce request count

This is documented as a known low-impact risk; no proactive mitigation is wired into the workflows. If observed in practice, file an enhancement issue.

## 6. Annual Maintenance Tasks

Calendar reminders the maintainer should set (documented in `RELEASING.md` "operational notes"):

| Task | Cadence | Owner | Procedure |
|---|---|---|---|
| Rotate `GH_TAP_TOKEN` | Annual (60 days before expiry) | Maintainer | §2.4 above |
| Review pinned action versions | Annual (January) | Maintainer | `ci-cd-pipeline.md` §7 |
| Audit `release-pipeline-failure` issues for K-PIPE root-cause patterns | Quarterly | Maintainer | `kpi-instrumentation.md` K-PIPE review section |
| Spot-check K-PROV verification on most recent release | Per release | Maintainer | `observability-design.md` §4.5 |
| Append release-log row to `RELEASING.md` | Per release | Maintainer | `kpi-instrumentation.md` K-T2T section |
| Self-test alert workflow (deliberate failure) | After any change to `release.yml` or `release-pipeline-alert.yml` | Maintainer | §1.6 above |

## 7. Cross-Reference

- DESIGN ADR-013 (PAT credential model)
- DEVOPS `ci-cd-pipeline.md` §4 (release-pipeline-alert.yml YAML)
- DEVOPS `ci-cd-pipeline.md` §5 (token-expiry-warning.yml YAML)
- DEVOPS `observability-design.md` (where to find what)
- DEVOPS `kpi-instrumentation.md` (per-KPI cadence)
- DISCUSS `outcome-kpis.md` (Handoff Notes for DEVOPS)
