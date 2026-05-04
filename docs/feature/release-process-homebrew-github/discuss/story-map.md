# Story Map: release-process-homebrew-github

## User: Jeff Bailey (maintainer) — primary

## Goal: One `git push origin v0.x.0` produces a fully-published release that lands in Homebrew within 15 minutes, with no manual steps after the tag push.

## Backbone

The maintainer's end-to-end activities, left-to-right:

| Activity 1: PREP | Activity 2: TAG | Activity 3: BUILD | Activity 4: PUBLISH | Activity 5: TAP-BUMP | Activity 6: USER-INSTALL |
|---|---|---|---|---|---|
| Bump `Cargo.toml` workspace version | Push annotated `v0.x.0` tag | Build per-target release binaries | Create GitHub Release with archives + notes | Open auto-merging PR against tap repo | End user runs `brew install` and `modeltap --version` |
| Generate `CHANGELOG.md` section from conventional commits | Workflow validates tag matches version | Run CI parity gates inside release.yml | Attach `.sha256` + SLSA attestation per archive | Render `Formula/modeltap.rb` from template | Brew downloads correct per-platform archive |
| Run CI gates locally (`cargo xtask release-prep`) | | Strip + tar each binary | Extract release notes from changelog | `brew test-bot` runs audit + install + test | Binary prints expected version string |
| Open prep PR for human review | | Cross-compile aarch64-linux | | Auto-merge on green test-bot | |
| | | (Future) Notarize macOS archives | | (Future) Maintainer-yank flow | |

---

### Walking Skeleton (the minimum end-to-end slice)

The thinnest path that connects all six activities for ONE target:

| Activity | Skeleton task |
|---|---|
| PREP | `cargo xtask release-prep` exists and bumps `Cargo.toml` + appends to `CHANGELOG.md` |
| TAG | `git push origin v0.0.1-rc1` triggers `release.yml` |
| BUILD | One target builds: `x86_64-unknown-linux-gnu` on `ubuntu-22.04`, with fmt+clippy+test gates |
| PUBLISH | `gh release create` uploads the one archive + its `.sha256` |
| TAP-BUMP | `bump-tap-formula` writes a one-platform `Formula/modeltap.rb` and opens a PR (does NOT need auto-merge) |
| USER-INSTALL | A clean Linux x86_64 box can `brew install jeffabailey/modeltap/modeltap` and `modeltap --version` prints `0.0.1-rc1` |

**Walking skeleton scope: US-A1, US-A2, US-A3, US-A4, US-A5, US-A6 (one slice each).** Approximately 2-3 days of build effort. No multi-arch matrix, no SLSA attestation, no auto-merge, no signing — those land in later releases.

> **Why a release-candidate (`-rc1`) tag for the skeleton**: lets the maintainer prove the pipeline end-to-end against a real Homebrew install without committing to a "real" v0.1.0 number. RC tags follow the same workflow trigger filter (`v*.*.*`) and are a normal part of the release vocabulary.

### Release 1: Multi-arch real release

Adds the remaining three build targets, atomic publish gating, and SLSA attestations. After Release 1, the maintainer can ship a real v0.1.0 to all four supported platforms.

| Story | Description |
|---|---|
| US-A7 | Build matrix expands to all 4 targets (aarch64-apple-darwin, x86_64-apple-darwin, aarch64-unknown-linux-gnu) |
| US-A8 | Atomic-publish guard: `publish-github-release` runs only if ALL build cells succeed |
| US-A9 | SLSA build provenance attestation per archive (`actions/attest-build-provenance@v2`) |
| US-A10 | Formula renders all 4 platform blocks with correct sha256s read from artifact files |

### Release 2: Hands-off automation

Auto-merge, idempotent retry, end-user upgrade path, contributor-readable workflow.

| Story | Description |
|---|---|
| US-A11 | Auto-merge tap-bump PR when `brew test-bot` passes |
| US-A12 | `bump-tap-formula` is idempotent — re-running updates the existing PR rather than opening a duplicate |
| US-A13 | `RELEASING.md` runbook exists, ≤10 numbered steps |
| US-A14 | Workflow file is ≤250 lines including comments; every job has a one-sentence purpose comment |

### Release 3: Future-proofing (deferred — out of scope for this feature)

Tracked but not committed for v1 of this feature:

| Story | Why deferred |
|---|---|
| (Future) Apple Developer ID notarization | Requires Apple Developer account + signing secrets in CI; D6 deferred |
| (Future) `modeltap-cli` binary added to the same pipeline | `modeltap-cli` crate does not yet exist; ship through this pipeline when it does |
| (Future) Yank workflow automation | Manual `gh release delete` + tap revert is acceptable for v1 (low frequency expected) |
| (Future) Submit to homebrew-core | Requires sustained user base + maintainer commitment; revisit after 6 months / 100+ stars |
| (Future) `cargo-release` or `release-please` driven cuts | Manual tag push for v1; D2 deferred |

## Scope Assessment: PASS — 14 stories, 1 bounded context (release pipeline + tap repo), estimated 5-7 days

Per the Elephant Carpaccio gate (Phase 2.7):

- 14 stories ≤ 10-story oversized signal? **YES, 14 > 10 — flagged.**
- Cross bounded contexts > 3? **NO** — single bounded context (release pipeline; tap repo is its mirror).
- Walking skeleton requires > 5 integration points? **YES** (Cargo.toml → tag → workflow → GitHub Release → tap PR → brew install) — but this is the inherent shape of the feature; collapsing integration points would defeat the goal.
- Estimated total effort > 2 weeks? **NO** — estimated 5-7 days for all 14 stories; walking skeleton alone is 2-3 days.
- Multiple independent user outcomes that could ship separately? **YES** — Walking Skeleton (one-arch proof), Release 1 (real multi-arch), Release 2 (hands-off polish) are independently demonstrable. The release-slice structure already implements Elephant Carpaccio.

**Verdict:** Right-sized as a single feature with three slices. The walking skeleton itself is shippable as a release-candidate proof; subsequent releases each add a coherent outcome. NOT proposing a split — the slices are already thin and outcome-coherent. Story count slightly above the 10-story signal but offset by single bounded context and short total effort.

## Story Inventory (preliminary IDs — finalized in Phase 4)

| ID | Story title | Activity | Release |
|---|---|---|---|
| US-A1 | `cargo xtask release-prep` bumps version + generates changelog + runs CI gates locally | PREP | WS |
| US-A2 | `release.yml` triggers on `v*.*.*` tag push and validates tag matches workspace version | TAG | WS |
| US-A3 | `release.yml` build job runs CI parity gates (fmt, clippy, test) before building | BUILD | WS |
| US-A4 | `release.yml` build job builds one target (x86_64-linux) and produces archive + `.sha256` | BUILD | WS |
| US-A5 | `publish-github-release` job creates GitHub Release with archive, sha256, and changelog notes | PUBLISH | WS |
| US-A6 | `bump-tap-formula` job opens a PR against the tap repo with the rendered formula | TAP-BUMP | WS |
| US-A7 | Build matrix expands to all 4 targets (3 additional: 2 macOS, 1 aarch64-linux) | BUILD | R1 |
| US-A8 | Atomic-publish guard: publish runs only if ALL build cells succeed | PUBLISH | R1 |
| US-A9 | SLSA build provenance attestation attached to every archive | PUBLISH | R1 |
| US-A10 | Formula renders all 4 platform blocks with sha256s read from `.sha256` artifact files | TAP-BUMP | R1 |
| US-A11 | Auto-merge enabled on tap-bump PR when `brew test-bot` is green | TAP-BUMP | R2 |
| US-A12 | `bump-tap-formula` is idempotent on retry (updates existing PR, no duplicates) | TAP-BUMP | R2 |
| US-A13 | `RELEASING.md` runbook exists at repo root with ≤10 numbered steps | PREP | R2 |
| US-A14 | `release.yml` is ≤250 lines, every job has a purpose comment | BUILD | R2 |

## End-User Story (cross-cutting)

| ID | Story title | Activity | Release |
|---|---|---|---|
| US-A15 | Devon Park installs modeltap with `brew install jeffabailey/modeltap/modeltap` and verifies version | USER-INSTALL | WS (proves the skeleton works) |

US-A15 is the user-facing end-to-end validation story; it lands in the walking skeleton because it IS the skeleton's success criterion.
