# ADR-013: Tap-repo credential — fine-grained PAT (`GH_TAP_TOKEN`)

## Status

Proposed (2026-05-03 — DESIGN wave for `release-process-homebrew-github`).

Confirms DISCUSS decision D7 (default stood; DESIGN was empowered to revisit).

## Context

The `bump-tap-formula` job in `.github/workflows/release.yml` (in `jeffabailey/modeltap`) must check out, commit to, and open a PR against `jeffabailey/homebrew-modeltap` — a separate GitHub repository. This requires cross-repo write authentication.

The default `GITHUB_TOKEN` provided to GitHub Actions workflows is **scoped to the repository running the workflow** and cannot write to any other repository. So we need an explicit cross-repo credential.

Three credential mechanisms exist:

1. **Fine-grained Personal Access Token (PAT)**: a maintainer-issued token scoped to specific repos and permissions, stored as a GH Actions secret.
2. **GitHub App**: an installation-based identity with org/repo permissions, authenticated via a private key + JWT exchange.
3. **Deploy key (SSH)**: a per-repo SSH key with read-or-write access; less common for PR-creating workflows because `gh pr create` needs HTTPS+token auth, not SSH.

Constraints:

- **Single maintainer** today (no team coordination overhead).
- **Reversibility**: if credential mechanism changes, RELEASING.md operational steps change but no architectural restructure.
- **Privacy / supply-chain**: token must not have access beyond what the bump job needs.
- **Operational simplicity**: maintainer should be able to rotate without consulting docs.
- **C8 no-silent-skips**: token failures must surface visibly.

## Decision

**Use a fine-grained Personal Access Token, stored as the `GH_TAP_TOKEN` Actions secret on `jeffabailey/modeltap`, scoped exclusively to `jeffabailey/homebrew-modeltap` with permissions `Contents: Read+Write` and `Pull Requests: Read+Write`.**

Token configuration (one-time setup, documented in `RELEASING.md`):

```
Name:        GH_TAP_TOKEN (modeltap → homebrew-modeltap bump)
Owner:       jeffabailey
Repository:  jeffabailey/homebrew-modeltap (only this one)
Permissions:
  - Contents:        Read and write
  - Pull requests:   Read and write
  - Metadata:        Read-only (always required)
Expiration:  365 days (GitHub max for fine-grained PATs)
```

Stored as `Settings → Secrets and variables → Actions → Repository secrets → GH_TAP_TOKEN` on `jeffabailey/modeltap`.

Used in workflow as:

```yaml
- uses: actions/checkout@v4
  with:
    repository: jeffabailey/homebrew-modeltap
    token: ${{ secrets.GH_TAP_TOKEN }}
    path: tap-repo

- run: gh pr create --repo jeffabailey/homebrew-modeltap ...
  env:
    GH_TOKEN: ${{ secrets.GH_TAP_TOKEN }}
```

Rotation procedure (in `RELEASING.md`): at 30 days before expiry (warned by DEVOPS-side workflow, out of scope for this feature), maintainer creates a replacement token, updates the Actions secret, deletes the old token. Both old and new tokens work briefly; a failed bump-tap-formula job after rotation is a clear signal the secret update was missed.

## Alternatives Considered

### Alternative 1: GitHub App

- **Pros**:
  - **Better for multi-maintainer scenarios**: an App's installation persists across maintainer changes; no token-rotation handoff.
  - **Auditability**: App-issued tokens are short-lived (1h) and traceable to the App identity.
  - **No personal account dependency**: PATs disappear when the issuing user leaves an org; an App outlives its creator.
  - **Permissions model is more granular**: App permissions can include "Issues: Read" without "Contents: Write" etc.
- **Cons**:
  - **Setup complexity**: registering an App, generating a private key, installing it on the tap repo, generating an installation token in CI via JWT exchange — a meaningful one-time cost.
  - **No team coordination problem to solve today**: single maintainer means PAT rotation is one person's concern; no org politics.
  - **Marginal supply-chain benefit at this scale**: the App still needs `Contents: Write + PRs: Write` on the tap repo; the blast radius is the same.
  - **Out-of-band private-key handling**: the App's private key must be stored as an Actions secret (just like the PAT), so the secret-management surface is similar.
- **Migration path documented**: when a co-maintainer joins, migrate to a GitHub App. ADR-013 will be superseded at that time.
- **Rejection rationale**: appropriate complexity for the team size. Single maintainer = PAT.

### Alternative 2: SSH deploy key on tap repo

- **Pros**: no PAT in the modeltap repo; SSH auth is well-understood.
- **Cons**:
  - **`gh pr create` does not work with SSH-only auth**: it requires an HTTPS token to call the GitHub REST API. We'd still need a PAT for the PR creation step, defeating the purpose.
  - **Workflow becomes a hybrid** (SSH for git operations, HTTPS+token for `gh` operations) — confusing and surfaces two different failure modes.
- **Rejection rationale**: the cross-repo work involves both git push AND `gh pr create`, and `gh pr create` requires HTTPS+token. Hybrid auth adds confusion without removing the PAT.

### Alternative 3: Classic (legacy) PAT scoped to all-of-public-repos

- **Pros**: simplest to set up.
- **Cons**:
  - **Massively over-scoped**: a classic PAT with `repo` scope grants read+write to every public repo the maintainer can access. Catastrophic blast radius if leaked.
  - **GitHub is deprecating classic PATs** in favor of fine-grained PATs.
- **Rejection rationale**: violates least-privilege principle; deprecated.

### Alternative 4: `actions/create-github-app-token@v1` (GH App with installation token in CI)

- **Pros**: GitHub-maintained action that handles JWT exchange; combines App's auditability with workflow simplicity.
- **Cons**: same as Alternative 1 — GitHub App setup is meaningful overhead for a single-maintainer project.
- **Rejection rationale**: see Alternative 1. Worth revisiting on multi-maintainer migration.

## Consequences

### Positive

- **Smallest possible blast radius for a working credential**: scoped to one repo, two permissions. Cannot read code in `jeffabailey/modeltap`, cannot write to any other repo, cannot manage org members.
- **One-time setup, low ongoing cost**: 5-minute setup; rotate annually.
- **C8 compliance**: token failures (HTTP 401) surface in `gh` output and fail the job (US-06.AC-7).
- **Reversible**: migration to GitHub App is documented; no architectural change to release.yml structure required.

### Negative

- **Annual rotation toil**: maintainer must remember to rotate before expiry. Mitigation: DEVOPS-side workflow (out of scope for this feature) warns 30 days before expiry per DEVOPS handoff.
- **PAT is tied to maintainer's GitHub identity**: if Jeff Bailey ever transferred ownership of `jeffabailey/modeltap` to a `modeltap` org, the PAT would need to be reissued by the new owner. Acceptable risk for v1.
- **No fine-grained audit log per workflow run**: PAT activity logs the user, not the workflow run-id. Less granular than App-installation-token activity logs. Acceptable.
- **Documented rotation responsibility**: lives only with the maintainer. Bus factor = 1. Acceptable for single-maintainer OSS.

### Quality attribute impact

| Attribute | Impact |
|---|---|
| Security | **Positive** — least-privilege scoping vs classic PAT |
| Operability | **Slightly negative** — annual rotation; mitigated by warning workflow |
| Maintainability | **Positive** — standard, well-documented pattern |
| Reversibility | **Positive** — migration to App is a one-time secret-and-checkout change |

## References

- DISCUSS `wave-decisions.md` D7
- DISCUSS `requirements.md` (NFR security: tap-bump credential storage)
- DISCUSS `user-stories.md` US-06 (token-fail scenario), US-13 (rotation procedure in RELEASING.md)
- DESIGN `architecture-design.md` §3 (Conway's Law future evolution)
- DESIGN `component-boundaries.md` §9
- GitHub fine-grained PAT docs: <https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens>
