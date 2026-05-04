# Branching Strategy — release-process-homebrew-github

**Wave:** DEVOPS (4 of 6)
**Author:** Apex (nw-platform-architect)
**Date:** 2026-05-03
**Decision:** D8 — Trunk-Based Development

## 1. Decision Summary

**Trunk-Based Development (TBD)**: single `main` branch; short-lived feature branches (typically <1 day, occasionally up to a week for larger features); PRs with required CI gates; merge to main = ready-to-release. Releases cut from `main` via annotated tags `v*.*.*`.

This is D8 from `wave-decisions.md`, defaulted via auto-mode and validated against the project's actual behavior:

- Recent commit history (`facddef`, `4c8428f`, `bc1695e`, `a623467`, `f82cfd5`) shows direct commits to main from the maintainer, with CI gating every push.
- Project is single-maintainer; complex multi-version branching (GitFlow, release branching) would add coordination overhead with no consumer.
- DELIVER wave is in progress with clear test-and-merge cycles per CLAUDE.md "DELIVER wave (wave 6 of 6)".

## 2. Branch Topology

```
main ────●────●────●────●────●────●────●────●────●────●  (always releasable)
          \             \              \              \
           feat/A        fix/B          feat/C         feat/D
           (1d)          (2h)           (3d)           (1w)

Tags:                                         v0.1.0     v0.1.1
                                              ↓          ↓
                                              release.yml fires per tag
```

Properties:

- `main` is the only persistent branch
- Every commit to `main` MUST pass `ci.yml` (existing branch protection)
- Tags are signed annotated tags created from `main`; tag push triggers `release.yml`
- Feature branches are deleted after PR merge (GitHub auto-delete on merge enabled)

## 3. Pipeline Triggers (Per Branching Strategy)

Per skill: branching strategy determines pipeline triggers. For TBD:

| Event | Workflow | Purpose |
|---|---|---|
| `pull_request: [main]` | `ci.yml` | Pre-merge gates (fmt, clippy, test, deny, k3-bench, lint-workflows) |
| `push: [main]` | `ci.yml` | Post-merge re-validation (catches "merge skew" where two PRs that each passed individually conflict in main) |
| `push: tags: [v*.*.*]` | `release.yml` | Cut a release |
| `workflow_run: { workflows: [release], types: [completed] }` | `release-pipeline-alert.yml` | K-PIPE alerting |
| `schedule: weekly` | `token-expiry-warning.yml` | GH_TAP_TOKEN health |
| (no trigger) | `workflow_dispatch` available on all of the above | Manual ad-hoc runs |

### 3.1 Why no `release-please` / `cargo-release` automation

Per DISCUSS D2: deferred. Maintainer cuts releases manually:

1. Run `cargo xtask release-prep --version X.Y.Z` locally
2. Open PR with bumped Cargo.toml + CHANGELOG.md
3. Merge PR after CI passes
4. `git tag -a vX.Y.Z -m "..."`
5. `git push origin vX.Y.Z` → `release.yml` fires

This keeps the human-in-the-loop for the prep PR (review the version bump and changelog), but the human has zero involvement post-tag-push (per K-TOIL ≤1 manual step).

A future automation path (per OQ-2, OQ-1 in DESIGN architecture-design.md §11): add `release-please` as a separate enabling workflow. Out of scope for v1.

## 4. Branch Protection — Source Repo (`main`)

Per skill `nw-cicd-and-deployment` Branch Protection Rules section, adapted for TBD + single maintainer:

| Rule | Setting | Rationale |
|---|---|---|
| Require PR reviews | OPTIONAL (single maintainer; no second reviewer to require) | Future-proof: enable when co-maintainer joins |
| Require status checks: `ci-success` | YES | Existing — gate every PR + main push |
| Require branches up to date | YES | Forces rebase before merge; catches merge skew at PR time |
| Require linear history | YES | Easier git history; no merge commits |
| Require signed commits | OPTIONAL (recommended) | Maintainer's own discipline; not strictly required |
| Restrict force pushes | YES (disabled) | Cannot rewrite main history accidentally |
| Restrict deletions | YES (disabled) | Cannot delete main accidentally |
| Enforce on admins | YES | Maintainer also subject to rules; prevents accidental bypass |

### 4.1 Apply via API

```bash
gh api -X PUT \
  /repos/jeffabailey/modeltap/branches/main/protection \
  -f required_status_checks[strict]=true \
  -f required_status_checks[contexts][]='ci success' \
  -f required_pull_request_reviews=null \
  -f enforce_admins=true \
  -f required_linear_history=true \
  -f allow_force_pushes=false \
  -f allow_deletions=false \
  -f restrictions=null
```

Documented in `RELEASING.md` "First-time setup" if not already configured.

## 5. Tag Protection

GitHub does not provide automatic tag-pattern protection of the kind that would prevent non-maintainer pushes to `v*.*.*`. Two layers of defense:

1. **Repo-level push permission**: only collaborators with write access can push tags. Single maintainer = single tag-pusher.
2. **`validate-tag` job**: even if a malformed tag is pushed (e.g., `v0.0.0-typo`), the workflow fails within ~30s without producing artifacts. Per US-02 / C1.

For multi-maintainer future: GitHub's "tag protection rules" can restrict `v*.*.*` tag creation to specific actors; documented as a future-enable.

## 6. Tap Repo Branching

Tap repo `jeffabailey/homebrew-modeltap` follows the same TBD model with a critical difference: branch protection is STRICTER because the formula file directly affects end-user installs.

| Rule | Setting | Rationale |
|---|---|---|
| Require status check: `brew test-bot` | YES | Auto-merge gates here per US-11 |
| Require PR reviews | NO (single maintainer; PRs are bot-opened by `bump-tap-formula`) | Auto-merge from bot would block on review requirement |
| Enforce on admins | YES | Even maintainer cannot bypass `brew test-bot` |
| Restrict force pushes | YES (disabled on main; allowed on bump/* branches per US-12 force-push-with-lease) | Required for idempotent retry |
| Restrict deletions | YES (disabled on main) | |

Setup commands per `infrastructure-integration.md` §3.1. Documented in `RELEASING.md` "First-time setup".

### 6.1 The `bump/v${VERSION}` branches

`bump-tap-formula` creates short-lived branches named `bump/v${VERSION}`. Lifecycle:

1. Created or fast-forwarded by `bump-tap-formula` job
2. PR opened against tap-repo `main`
3. `brew test-bot` runs as required check
4. Auto-merge fires when check passes (`gh pr merge --auto --squash`)
5. Branch deleted automatically after merge (GitHub auto-delete on merge)

Idempotency (US-12): if `bump/v${VERSION}` already exists from a prior failed run, `git push --force-with-lease` updates it; the existing PR (if any) is updated; if no PR exists, a new one is created. Net result: exactly one PR per version. See DESIGN component-boundaries.md §3.3 step 5.

## 7. Versioning Strategy

Per D2 (DISCUSS) and US-04: semantic versioning (MAJOR.MINOR.PATCH). Versions strictly monotonic from a `0.x.x` pre-1.0 baseline.

| Version increment | When | Example |
|---|---|---|
| PATCH | Bug fix, dependency update without API impact | v0.1.0 → v0.1.1 |
| MINOR | New feature, backward-compatible | v0.1.x → v0.2.0 |
| MAJOR | Breaking change | v0.x.y → v1.0.0 (and beyond) |
| Pre-release suffix | Alpha/beta/RC for experimental tags | v0.2.0-rc1 |

The `validate-tag` job parses the tag with `semver` crate and asserts it matches `Cargo.toml [workspace.package].version`. Pre-release suffixes propagate: `v0.2.0-rc1` requires `Cargo.toml` version = `0.2.0-rc1`.

Pre-release tags get `--prerelease` flag on `gh release create` per US-05.AC-6, and the resulting tap-repo PR uses the same pre-release version in the formula (Homebrew handles pre-release versions but they require explicit `brew install <formula>@version`).

## 8. Release Cadence

No fixed cadence. Releases triggered by:

- Accumulated unreleased commits the maintainer judges worth shipping
- A bug fix that needs to ship (PATCH)
- A coordinated milestone (MINOR or MAJOR)

Estimated 1-4 releases per month per `outcome-kpis.md` baseline. K-PIPE (≥95% success rate) is measured over rolling-10 releases — at 1-4/month, this gives a 2-10 month K-PIPE window. Acceptable for a low-frequency-release project.

## 9. Hot-Fix Path

For an urgent fix (security, severe bug):

1. Direct commit or fast PR to `main` with the fix
2. `cargo xtask release-prep --version X.Y.(Z+1)` (PATCH bump)
3. Open PR, merge after CI passes
4. Tag and push as normal

There is NO separate hotfix branch (TBD doesn't have them). The `main` branch is always releasable; therefore a fix on main IS a hotfix.

If multiple in-progress branches make `main` unsuitable for an immediate release: revert the offending merges first (GitHub provides a "Revert" button on merged PRs), then ship the fix, then re-introduce the reverts as their own PRs. This is the standard TBD recovery flow.

## 10. Coexistence with `modeltap-tui` Feature

Per CLAUDE.md, the `modeltap-tui` feature is in DELIVER wave. Both features share the same `main` branch under TBD. Release process integration:

- The first release that ships `modeltap-tui` is the same release that ships `release-process-homebrew-github` (the latter ships the pipeline that ships the former).
- Pre-release tags during DELIVER may be cut as `v0.1.0-rc1` (or similar) to validate the pipeline end-to-end before the GA release.
- `RELEASING.md` is the single runbook for both features; this design's runbook content covers both.

No branch coordination needed — TBD already handles concurrent feature work via PR-and-merge serialization on main.

## 11. Cross-Reference

- DESIGN `architecture-design.md` §3 (Conway's Law — single maintainer)
- DEVOPS `ci-cd-pipeline.md` (workflow triggers + permissions)
- DEVOPS `infrastructure-integration.md` §5 (source-repo branch protection), §3.1 (tap-repo branch protection)
- DEVOPS `wave-decisions.md` D8
- DISCUSS `wave-decisions.md` D2 (manual tag cuts)
- Existing `.github/workflows/ci.yml`
