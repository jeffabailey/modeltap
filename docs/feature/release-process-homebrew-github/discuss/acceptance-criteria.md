# Acceptance Criteria — release-process-homebrew-github

Consolidated AC index across all 15 stories. Each AC is observable, testable, and traces back to a UAT scenario in `user-stories.md` or `journey-tag-to-brew-install.feature`.

## US-01: `cargo xtask release-prep` automates the prep cycle

| AC | Criterion | Source |
|---|---|---|
| US-01.AC-1 | `cargo xtask release-prep --version <X.Y.Z>` exists as a workspace xtask | UAT US-01 prep |
| US-01.AC-2 | Bumps `Cargo.toml [workspace.package].version` and updates `Cargo.lock` | UAT US-01 prep |
| US-01.AC-3 | Generates `CHANGELOG.md` `[X.Y.Z]` section from conventional commits via `git-cliff` | UAT US-01 prep |
| US-01.AC-4 | Runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked` and fails on any failure | UAT US-01 ci-gate-fail |
| US-01.AC-5 | Refuses on dirty working tree with clear message | UAT US-01 dirty-tree |
| US-01.AC-6 | Refuses non-monotonic version bumps with clear message | UAT US-01 non-monotonic |
| US-01.AC-7 | Exits zero with next-step instructions on success | UAT US-01 prep |

## US-02: `release.yml` triggers on `v*.*.*` tag and validates tag matches workspace version

| AC | Criterion | Source |
|---|---|---|
| US-02.AC-1 | `release.yml` exists at `.github/workflows/release.yml` | UAT US-02 happy |
| US-02.AC-2 | Trigger is `on: push: tags: ['v*.*.*']` | UAT US-02 non-version-tag |
| US-02.AC-3 | First job is `validate-tag` | UAT US-02 happy |
| US-02.AC-4 | `validate-tag` reads `Cargo.toml [workspace.package].version` and asserts tag == `v` + version | UAT US-02 happy |
| US-02.AC-5 | On mismatch, job fails with message `tag <X> does not match workspace version <Y>` | UAT US-02 mismatch |
| US-02.AC-6 | All subsequent jobs use `needs: validate-tag` so they never run on mismatch | UAT US-02 mismatch |

## US-03: `release.yml` runs CI parity gates before building

| AC | Criterion | Source |
|---|---|---|
| US-03.AC-1 | Build job's first three steps are `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked` | UAT US-03 gates-first |
| US-03.AC-2 | Toolchain action: `dtolnay/rust-toolchain@stable` (matches ci.yml exactly) | UAT US-03 toolchain |
| US-03.AC-3 | All three gates use the same flags as `ci.yml` | UAT US-03 gates-first |
| US-03.AC-4 | Failure of any gate fails the build job (no `continue-on-error: true`) | UAT US-03 clippy-warning |
| US-03.AC-5 | `cargo build --release --locked --target <T>` runs ONLY after all gates pass | UAT US-03 gates-first |

## US-04: Single-target build produces a stripped archive and sha256

| AC | Criterion | Source |
|---|---|---|
| US-04.AC-1 | Build runs `cargo build --release --locked --target <T> --package modeltap-app` | UAT US-04 happy |
| US-04.AC-2 | Resulting binary is stripped before packaging | UAT US-04 happy |
| US-04.AC-3 | Archive filename is exactly `modeltap-${version}-${target}.tar.gz` | UAT US-04 happy |
| US-04.AC-4 | Archive contains a single file named `modeltap` (no nested directories) | UAT US-04 happy |
| US-04.AC-5 | Sidecar `.sha256` file is written with `sha256sum < archive > archive.sha256` | UAT US-04 happy |
| US-04.AC-6 | Both files uploaded as workflow artifact named `release-${target}` | UAT US-04 happy |
| US-04.AC-7 | Pre-release suffixes (e.g., `-rc1`) are preserved in the archive name | UAT US-04 prerelease |

## US-05: `publish-github-release` job creates the GitHub Release

| AC | Criterion | Source |
|---|---|---|
| US-05.AC-1 | `publish-github-release` job runs with `needs: [validate-tag, build]` | UAT US-05 happy |
| US-05.AC-2 | All build artifacts (archives + .sha256s) are downloaded into the workspace | UAT US-05 happy |
| US-05.AC-3 | `CHANGELOG.md` `[X.Y.Z]` section is extracted into `RELEASE_NOTES.md` | UAT US-05 happy |
| US-05.AC-4 | Missing `[X.Y.Z]` section fails the job with `CHANGELOG.md has no [X.Y.Z] section` | UAT US-05 missing-section |
| US-05.AC-5 | `gh release create` is invoked with the tag, all archives, all sha256s, the title, and `--notes-file` | UAT US-05 happy |
| US-05.AC-6 | Pre-release tags (containing `-`) are marked as pre-release via `--prerelease` | UAT US-05 prerelease |

## US-06: `bump-tap-formula` job opens a PR against the tap repo

| AC | Criterion | Source |
|---|---|---|
| US-06.AC-1 | `bump-tap-formula` job runs with `needs: publish-github-release` | UAT US-06 atomic |
| US-06.AC-2 | Checks out `jeffabailey/homebrew-modeltap` using `${{ secrets.GH_TAP_TOKEN }}` | UAT US-06 happy |
| US-06.AC-3 | Renders `Formula/modeltap.rb` from a template using `${version}`, `${release-url}` per target, `${sha256}` per target | UAT US-06 happy |
| US-06.AC-4 | sha256 values are read from the `.sha256` artifact files (NOT recomputed) | UAT US-06 happy |
| US-06.AC-5 | Commits to branch `bump/v${version}` and pushes | UAT US-06 happy |
| US-06.AC-6 | Opens PR titled `modeltap ${version}` against the tap's `main` branch | UAT US-06 happy |
| US-06.AC-7 | Token failures fail the job visibly (no silent continuation) | UAT US-06 token-fail |

## US-07: Build matrix expands to all 4 supported targets

| AC | Criterion | Source |
|---|---|---|
| US-07.AC-1 | Build job uses `strategy.matrix.target` with 4 entries: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` | UAT US-07 parallel |
| US-07.AC-2 | Each cell is mapped to the correct runner: `macos-14`, `macos-13`, `ubuntu-22.04`, `ubuntu-22.04` | UAT US-07 parallel |
| US-07.AC-3 | aarch64-linux cell successfully cross-compiles or natively builds | UAT US-07 cross-compile |
| US-07.AC-4 | All 4 archives + sha256s uploaded as artifacts | UAT US-07 parallel |
| US-07.AC-5 | Matrix uses `fail-fast: false` so a single failure does not cancel still-running cells | UAT US-07 parallel |

## US-08: Atomic-publish guard ensures all-or-nothing releases

| AC | Criterion | Source |
|---|---|---|
| US-08.AC-1 | `publish-github-release` declares `needs: [validate-tag, build]` | UAT US-08 all-pass |
| US-08.AC-2 | `bump-tap-formula` declares `needs: publish-github-release` | UAT US-08 all-pass |
| US-08.AC-3 | No `if: always()` or `if: failure()` overrides bypass the guard for `publish` or `bump` | Code review |
| US-08.AC-4 | Workflow run UI clearly shows "skipped" status for downstream jobs when a build cell fails | UAT US-08 one-fails |

## US-09: SLSA build provenance attestation per archive

| AC | Criterion | Source |
|---|---|---|
| US-09.AC-1 | `actions/attest-build-provenance@v2` runs in the build job after the tar.gz step | UAT US-09 every-attested |
| US-09.AC-2 | One attestation per archive | UAT US-09 every-attested |
| US-09.AC-3 | Workflow has `permissions: id-token: write` and `attestations: write` | UAT US-09 every-attested |
| US-09.AC-4 | `gh attestation verify <archive> --owner jeffabailey` returns success for every published archive | UAT US-09 every-attested |
| US-09.AC-5 | README troubleshooting section documents the verify command | UAT US-09 every-attested |

## US-10: Formula renders all 4 platform blocks with sha256s read from artifact files

| AC | Criterion | Source |
|---|---|---|
| US-10.AC-1 | Rendered formula has 4 platform blocks: `on_macos.on_arm`, `on_macos.on_intel`, `on_linux.on_arm`, `on_linux.on_intel` | UAT US-10 four-blocks |
| US-10.AC-2 | Each block's `url` and `sha256` are derived from `${release-url}` and the contents of the `.sha256` artifact file | UAT US-10 four-blocks |
| US-10.AC-3 | sha256 values are read from `.sha256` files; never recomputed by the bump job | UAT US-10 four-blocks |
| US-10.AC-4 | Missing `.sha256` artifact fails the job with clear "expected <file>, not found" error | UAT US-10 missing-artifact |
| US-10.AC-5 | `brew test-bot audit` passes on the resulting formula | UAT US-10 brew-audit |

## US-11: Auto-merge tap-bump PR when `brew test-bot` is green

| AC | Criterion | Source |
|---|---|---|
| US-11.AC-1 | `bump-tap-formula` job runs `gh pr merge --auto --squash` on the tap PR | UAT US-11 green-merges |
| US-11.AC-2 | Tap repo `main` branch has protection rule requiring `brew test-bot` status check | UAT US-11 green-merges (precondition) |
| US-11.AC-3 | Auto-merge fires only when test-bot is green | UAT US-11 green-merges |
| US-11.AC-4 | Failed test-bot leaves the PR open (no force-merge) | UAT US-11 fail-withholds |
| US-11.AC-5 | Documented in `RELEASING.md`: branch protection setup is a one-time precondition | UAT US-13 (runbook) |

## US-12: `bump-tap-formula` is idempotent on retry

| AC | Criterion | Source |
|---|---|---|
| US-12.AC-1 | Job checks for existing `bump/v${version}` branch before creating | UAT US-12 first-run, re-run |
| US-12.AC-2 | Force-pushes to existing branch (no duplicate branches) | UAT US-12 re-run |
| US-12.AC-3 | One PR per release version (no duplicates) | UAT US-12 re-run |
| US-12.AC-4 | `RELEASING.md` documents the manual-edit-clobber trade-off | UAT US-12 manual-edit-clobber |

## US-13: `RELEASING.md` runbook exists, ≤10 numbered steps

| AC | Criterion | Source |
|---|---|---|
| US-13.AC-1 | `RELEASING.md` exists at repo root | UAT US-13 short |
| US-13.AC-2 | Contains ≤10 numbered steps | UAT US-13 short |
| US-13.AC-3 | Contains a release-log table (per-release rows) | UAT US-13 release-log |
| US-13.AC-4 | Documents `GH_TAP_TOKEN` rotation procedure | UAT US-13 (cross-ref US-06) |
| US-13.AC-5 | Documents the manual-edit-to-bump-branch clobber trade-off (cross-ref US-12) | UAT US-13 (cross-ref US-12) |
| US-13.AC-6 | Documents the macOS Gatekeeper `xattr` workaround (cross-ref D6) | UAT US-13 |
| US-13.AC-7 | Total file size ≤ 80 lines | UAT US-13 short |

## US-14: `release.yml` is ≤250 lines, every job has a purpose comment

| AC | Criterion | Source |
|---|---|---|
| US-14.AC-1 | `release.yml` is ≤250 lines (including comments and blank lines) | UAT US-14 line-limit |
| US-14.AC-2 | Every job declaration has a `# Purpose: <one sentence>` comment immediately above | UAT US-14 every-comment |
| US-14.AC-3 | A `cargo xtask lint-workflows` (or shell equivalent) enforces both constraints | UAT US-14 lint-catches |
| US-14.AC-4 | Lint runs in `ci.yml` | UAT US-14 lint-catches |

## US-15: Devon installs modeltap with `brew install` and verifies the version

| AC | Criterion | Source |
|---|---|---|
| US-15.AC-1 | `brew install jeffabailey/modeltap/modeltap` succeeds on macOS Sonoma (Apple Silicon) | UAT US-15 macos-arm |
| US-15.AC-2 | `brew install jeffabailey/modeltap/modeltap` succeeds on macOS Ventura (Intel) | journey .feature macos-intel |
| US-15.AC-3 | `brew install jeffabailey/modeltap/modeltap` succeeds on Ubuntu 22.04 (x86_64) | UAT US-15 linux-x86 |
| US-15.AC-4 | `brew install jeffabailey/modeltap/modeltap` succeeds on Ubuntu 22.04 (aarch64) | journey .feature linux-arm |
| US-15.AC-5 | `modeltap --version` prints exactly `modeltap ${version}` matching the installed version | UAT US-15 macos-arm, linux-x86 |
| US-15.AC-6 | `modeltap` launches the TUI (delegates to modeltap-tui feature for TUI behavior) | UAT US-15 macos-arm |
| US-15.AC-7 | Median tag-to-install latency ≤ 15 minutes | UAT US-15 SLA |
| US-15.AC-8 | Install during tap-update window installs the previous version informatively | UAT US-15 tap-window |

## Cross-Story Acceptance: Integration Invariants

These ACs are not owned by a single story but emerge from the integration. Verified during peer review and DESIGN.

| AC-ID | Criterion | Source |
|---|---|---|
| INT.AC-1 | `${version}` is the same string in: `Cargo.toml`, the git tag (with `v` prefix), every archive name, the GitHub Release title, the Homebrew formula, and the `modeltap --version` output | shared-artifacts-registry.md `version` row |
| INT.AC-2 | For each target T: GitHub Release `.sha256` content == Homebrew formula `sha256` field for T | shared-artifacts-registry.md `sha256` row |
| INT.AC-3 | For each target T: GitHub Release URL for the archive == Homebrew formula `url` field for T | shared-artifacts-registry.md `release-url` row |
| INT.AC-4 | The release pipeline from `git push origin v0.x.0` to a clean machine `brew install` succeeding completes in ≤15 minutes (median) | journey .feature SLA scenario |
| INT.AC-5 | Either ALL targets build AND the GitHub release publishes AND the tap PR opens, OR none of these visible-to-users effects happen | US-08 + journey .feature atomic scenarios |
| INT.AC-6 | The release workflow runs the SAME CI parity gates as `ci.yml` | US-03 + workflow file diff |
