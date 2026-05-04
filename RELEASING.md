# Releasing modeltap

Maintainer-facing runbook. Every cut of `vX.Y.Z` follows the 10 numbered
steps below. Pipeline behaviour is owned by `.github/workflows/release.yml`
and `xtask`; this file documents how a human drives that pipeline.

**Quick reference (one-liner):**

```sh
cargo xtask release-prep --version X.Y.Z && \
  gh pr create --fill && gh pr merge --squash --auto && \
  git checkout main && git pull && \
  git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin vX.Y.Z
```

## Release steps

1. Branch off `main`: `git checkout -b release/vX.Y.Z`.
2. Run `cargo xtask release-prep --version X.Y.Z` (bumps Cargo.toml + CHANGELOG).
3. Open the prep PR (`gh pr create --fill`); CI must go green.
4. Merge the prep PR with squash; delete the prep branch.
5. `git checkout main && git pull --ff-only`.
6. Tag: `git tag -a vX.Y.Z -m "vX.Y.Z"` (signed if you have GPG configured).
7. Push the tag: `git push origin vX.Y.Z` — this triggers `release.yml`.
8. Watch `gh run watch` until publish + tap-bump jobs are green (~10–15 min).
9. Verify the tap PR auto-merged and `brew install jeffabailey/modeltap/modeltap` works on macOS.
10. Append a row to the release-log table below (timestamps in UTC ISO-8601).

## Release log (K-T2T tracking)

| version | tag-pushed-at | release-published-at | tap-merged-at | time-to-tap | platforms-verified | provenance-verified | notes |
|---------|---------------|----------------------|---------------|-------------|--------------------|---------------------|-------|
| _example_ | 2026-01-01T00:00:00Z | 2026-01-01T00:08:12Z | 2026-01-01T00:14:33Z | 14m33s | mac-arm,mac-x86,linux-x86,linux-arm | yes | template row — replace |

## First-time setup

One-time tasks before the first release:

- **Create the tap repo** at `jeffabailey/homebrew-modeltap` (public, empty
  `main` with a `Formula/` directory). The bump job pushes `bump/v<version>`
  branches here and opens PRs against `main`.
- **Set `GH_TAP_TOKEN`** as a repo secret on `jeffabailey/modeltap`. Mint a
  fine-scoped PAT with `contents:write` + `pull_requests:write` on
  `jeffabailey/homebrew-modeltap` only. Default 1-year expiry.
- **Configure tap branch protection** on `jeffabailey/homebrew-modeltap`
  `main`: require the `brew test-bot` status check and enable repo-level
  "Allow auto-merge" so step 9 above lands without a human click.

## Operational notes

- **`GH_TAP_TOKEN` rotation procedure.** Mint a new PAT with the same scopes,
  update the `GH_TAP_TOKEN` secret on `jeffabailey/modeltap`, then revoke the
  old PAT. The `token-expiry-warning` workflow (US-13 follow-up) opens an
  issue 30 days before expiry; rotate within that window. After rotation,
  re-run any in-flight `bump-tap-formula` job — `--force-with-lease` makes
  the retry idempotent (US-12, see manual-edit note below).
- **Manual-edit-clobber trade-off (bump branches).** Direct edits to
  `Formula/modeltap.rb` on `bump/v<version>` are SAFE for one-off fixes
  during a release but WILL be overwritten on the next `bump-tap-formula`
  invocation (idempotent retry force-pushes the freshly rendered formula).
  If you need a non-formula change in the tap PR, leave a PR comment instead.
- **macOS Gatekeeper xattr workaround.** First run of an unsigned `modeltap`
  binary on macOS may be quarantined with "cannot be opened because the
  developer cannot be verified". Clear the quarantine attribute once:
  `xattr -dr com.apple.quarantine "$(brew --prefix)/bin/modeltap"`. Document
  this in the GitHub Release body for end users.
