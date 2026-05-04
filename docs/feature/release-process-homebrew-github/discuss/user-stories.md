<!-- markdownlint-disable MD024 -->

# User Stories — release-process-homebrew-github

All maintainer-facing stories use the persona **Jeff Bailey**: single-maintainer of `jeffabailey/modeltap`, runs macOS Sonoma + Linux WSL, comfortable with `git`, GitHub Actions, and Homebrew formula authoring. Stories US-13 (runbook) and US-14 (workflow legibility) double-cast with **Riley Chen** (open-source contributor reused from `modeltap-tui`). The end-user install story US-15 uses **Devon Park** (multi-tool local-AI power user, also reused from `modeltap-tui`).

Story IDs are stable for cross-document referencing. Acceptance Criteria use Given/When/Then derived from `journey-tag-to-brew-install.feature`.

---

## US-01: `cargo xtask release-prep` automates the prep cycle

### Problem

Jeff Bailey wants to cut release v0.2.0. Today, he would have to: edit `Cargo.toml` by hand, run `cargo update --workspace`, hand-write a CHANGELOG.md section, run `cargo fmt && cargo clippy && cargo test` separately, and remember to commit before tagging. That's six failure-prone manual steps. He wants one command that does all six and refuses to proceed if any check fails.

### Who

- Jeff Bailey, single maintainer, runs `cargo xtask` subcommands routinely, owns 17+ commits since the last v0.1.0 tag.

### Solution

A `cargo xtask release-prep --version 0.2.0` subcommand in the workspace that bumps `Cargo.toml [workspace.package].version`, regenerates `CHANGELOG.md` from conventional commits between the previous tag and HEAD using `git-cliff`, runs the CI parity gates locally (`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`), and exits zero with instructions to open a `release: v0.2.0` PR. Refuses to proceed on a dirty working tree or a non-monotonic version bump.

### Domain Examples

#### 1: Happy path — Jeff bumps from 0.1.0 to 0.2.0

Jeff is on a clean `main` checkout of `jeffabailey/modeltap`. The workspace version is `0.1.0`. There are 17 commits since the v0.1.0 tag (4 `fix:`, 8 `feat:`, 3 `chore:`, 2 `refactor:`). Jeff runs `cargo xtask release-prep --version 0.2.0`. The command updates `Cargo.toml` to `0.2.0`, updates `Cargo.lock`, writes a new `## [0.2.0]` section to `CHANGELOG.md` with the 17 commits grouped by type, runs all three CI parity gates (332 tests pass), and prints `Done. Commit, push, open PR titled 'release: v0.2.0'.`

#### 2: Edge — dirty working tree

Jeff has uncommitted local changes (he was experimenting). He runs `cargo xtask release-prep --version 0.2.0`. The command exits non-zero immediately with `error: working tree is dirty: commit or stash first`. No files are modified.

#### 3: Error — non-monotonic version bump

Jeff fat-fingers `0.1.5` instead of `0.2.0` while the current version is already `0.2.0`. He runs `cargo xtask release-prep --version 0.1.5`. The command exits non-zero with `error: proposed version 0.1.5 is not greater than current 0.2.0`. No files are modified.

### UAT Scenarios (BDD)

#### Scenario: Jeff prepares a release with one command

Given the workspace version in Cargo.toml is 0.1.0
And there are 17 conventional commits since the v0.1.0 tag
When Jeff runs `cargo xtask release-prep --version 0.2.0`
Then Cargo.toml [workspace.package].version becomes 0.2.0
And Cargo.lock is updated to match
And CHANGELOG.md gains a `## [0.2.0]` section grouping commits by type
And cargo fmt, clippy, and test all pass locally
And the script exits zero with instructions to commit, push, and open a PR

#### Scenario: Release-prep refuses on dirty working tree

Given the workspace has uncommitted local changes
When Jeff runs `cargo xtask release-prep --version 0.2.0`
Then the script exits non-zero
And the message says "working tree is dirty: commit or stash first"
And no files are modified

#### Scenario: Release-prep refuses non-monotonic version

Given the workspace version in Cargo.toml is 0.2.0
When Jeff runs `cargo xtask release-prep --version 0.1.5`
Then the script exits non-zero
And the message says "proposed version 0.1.5 is not greater than current 0.2.0"

#### Scenario: CI gate failure halts prep

Given the workspace version in Cargo.toml is 0.1.0
And `cargo clippy` would emit a warning on current code
When Jeff runs `cargo xtask release-prep --version 0.2.0`
Then the script exits non-zero after the clippy step
And no version bump is committed
And the maintainer is told which gate failed

### Acceptance Criteria

- [ ] `cargo xtask release-prep --version <X.Y.Z>` exists as a workspace xtask
- [ ] Bumps `Cargo.toml [workspace.package].version` and updates `Cargo.lock`
- [ ] Generates `CHANGELOG.md` `[X.Y.Z]` section from conventional commits via `git-cliff`
- [ ] Runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked` and fails on any failure
- [ ] Refuses on dirty working tree with clear message
- [ ] Refuses non-monotonic version bumps with clear message
- [ ] Exits zero with next-step instructions on success

### Outcome KPIs

- **Who**: Jeff Bailey (maintainer)
- **Does what**: Performs zero manual prep steps beyond reviewing the prep PR (K-TOIL)
- **By how much**: Replaces 6+ manual steps with one command
- **Measured by**: Audit on every release: count of manual steps performed during prep
- **Baseline**: N/A (no release pipeline exists today)

### Technical Notes

- `cargo xtask` pattern (a `xtask/` crate alias for workspace tasks) — established Rust convention.
- `git-cliff` configured via `git-cliff.toml` at repo root with sections matching conventional commit types.
- The xtask must NOT push the tag — that is a separate maintainer action (US-02 is the trigger story).

### Dependencies

- Repo must adopt conventional commits (already in use per recent history).
- `git-cliff` dependency added to dev tooling.

---

## US-02: `release.yml` triggers on `v*.*.*` tag and validates tag matches workspace version

### Problem

Jeff Bailey just merged the prep PR. He's about to push `git tag -a v0.2.0`. Without a guard, if he forgets a step (or rebases away the version bump), he could push a `v0.2.0` tag pointing at code where `Cargo.toml` still says `0.1.0`. The release would ship a binary that prints `modeltap 0.1.0` from a `v0.2.0` archive — a release that lies about its identity. He needs the workflow to abort on mismatch BEFORE any artifact is built.

### Who

- Jeff Bailey, single maintainer, has been bitten by version drift in past projects.

### Solution

A `release.yml` workflow with `on: push: tags: ['v*.*.*']` trigger. Its first job, `validate-tag`, asserts that the tag string equals `v` + the workspace version read from `Cargo.toml`. Mismatch fails the job with a clear message; subsequent build/publish jobs are gated by `needs: validate-tag` and never run on mismatch.

### Domain Examples

#### 1: Happy path — tag and Cargo.toml agree

Jeff has merged the prep PR. `Cargo.toml [workspace.package].version` on `main` is `0.2.0`. He runs `git tag -a v0.2.0 -m "v0.2.0"` and `git push origin v0.2.0`. Within 30 seconds, GitHub Actions starts `release.yml`. The `validate-tag` job reads `Cargo.toml`, computes `v0.2.0`, compares to the pushed tag `v0.2.0`, succeeds, and the build matrix begins.

#### 2: Error — tag ahead of Cargo.toml

Jeff fat-fingered the prep: he created the tag `v0.3.0` but only bumped `Cargo.toml` to `0.2.0`. He pushes `v0.3.0`. The `validate-tag` job fails within 15 seconds with `error: tag v0.3.0 does not match workspace version 0.2.0`. No build job runs. Jeff deletes the tag (`git push --delete origin v0.3.0`), fixes `Cargo.toml`, retags.

#### 3: Edge — non-version tag is ignored

Jeff pushes a tag named `experiment-foo` for a personal experiment. The `release.yml` workflow does NOT run because the trigger filter `v*.*.*` excludes it. No false-positive release is cut.

### UAT Scenarios (BDD)

#### Scenario: Pushing a matching tag triggers the workflow

Given the prep PR has merged and Cargo.toml workspace version on main is 0.2.0
When Jeff pushes the annotated tag "v0.2.0" to origin
Then the release workflow starts within 30 seconds
And the validate-tag job confirms the tag matches "v" plus the workspace version

#### Scenario: Mismatched tag fails fast

Given Cargo.toml workspace version on main is 0.1.0
When Jeff mistakenly pushes the annotated tag "v0.2.0"
Then the validate-tag job fails before any build job starts
And the failure message says "tag v0.2.0 does not match workspace version 0.1.0"

#### Scenario: Non-version tag does not trigger the workflow

Given main is at any state
When Jeff pushes a tag "experiment-foo"
Then the release workflow does not run
Because the trigger filter is "v*.*.*"

### Acceptance Criteria

- [ ] `release.yml` exists at `.github/workflows/release.yml`
- [ ] Trigger is `on: push: tags: ['v*.*.*']`
- [ ] First job is `validate-tag`
- [ ] `validate-tag` reads `Cargo.toml [workspace.package].version` and asserts tag == `v` + version
- [ ] On mismatch, job fails with message `tag <X> does not match workspace version <Y>`
- [ ] All subsequent jobs use `needs: validate-tag` so they never run on mismatch

### Outcome KPIs

- **Who**: Jeff Bailey (maintainer)
- **Does what**: Avoids shipping a release that lies about its version (K-PIPE guardrail)
- **By how much**: 100% of pushed tags either match Cargo.toml or are caught before any build runs
- **Measured by**: Workflow run history; any `validate-tag` failure is logged and triaged
- **Baseline**: N/A

### Technical Notes

- `Cargo.toml` parsing in the validate-tag job can use `cargo metadata --format-version 1 | jq -r .workspace_root_manifest.workspace.package.version`, OR `grep -E '^version = "' Cargo.toml | head -1`. Choose simplest.
- The validate-tag step MUST be the first step of the first job; no parallel jobs precede it.

### Dependencies

- US-01 (release-prep) is the upstream guard; US-02 is the workflow-side guard. They are complementary, not redundant.

---

## US-03: `release.yml` runs CI parity gates before building

### Problem

Jeff Bailey has been burned before by releases that shipped code which a normal CI run would have rejected. The `ci.yml` runs `fmt`, `clippy`, and `test` on every PR — but `release.yml` is a separate workflow triggered by tag push, and there's no guarantee the tagged commit was the SAME commit that passed `ci.yml`. Even if the prep PR passed CI, force-pushes or long-lived branches could drift. The release workflow MUST re-run the CI parity gates before building any artifact.

### Who

- Jeff Bailey, single maintainer, treats CI gates as load-bearing.

### Solution

The `build` job in `release.yml` runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --locked` as its first three steps, BEFORE `cargo build --release`. Failure of any gate fails the job. Same toolchain pinning as `ci.yml` (`dtolnay/rust-toolchain@stable`).

### Domain Examples

#### 1: Happy path — gates pass

Jeff pushes `v0.2.0`. `validate-tag` passes. The build matrix starts. On each runner (`macos-14`, `macos-13`, `ubuntu-22.04`), the build job runs `cargo fmt --all -- --check` (passes), `cargo clippy --workspace --all-targets -- -D warnings` (passes, 0 warnings), `cargo test --workspace --locked` (passes, 332 tests). Then `cargo build --release --locked --target ${target}` runs.

#### 2: Edge — force-push between PR-merge and tag-push introduces a clippy warning

Jeff merged a PR that passed CI on Monday. On Tuesday, a force-push to `main` (someone else's hotfix that introduced a `clippy::needless_borrow` warning, missed because the per-PR CI had been turned off temporarily) lands on top. Jeff tags `v0.2.0` on Tuesday. The release.yml `clippy` step fails on every target. No build artifact is produced. Jeff investigates, fixes, retags.

#### 3: Error — flaky test surfaces in release.yml

A flaky integration test passes 95% of the time. On the release run, it fails on `macos-13`. The build job fails. `publish-github-release` does not run. Jeff re-runs the failing job (GitHub Actions UI, "Re-run failed jobs"); on the second run the test passes; the release proceeds. (Re-running is acceptable; ignoring is not.)

### UAT Scenarios (BDD)

#### Scenario: Build job runs CI parity gates first

Given the validate-tag job passed
When any build matrix cell starts
Then it runs "cargo fmt --all -- --check" and fails the job on any diff
And it runs "cargo clippy --workspace --all-targets -- -D warnings" and fails on any warning
And it runs "cargo test --workspace --locked" and fails on any test failure
And only after all three gates pass does the cargo build --release step run

#### Scenario: Toolchain matches ci.yml

Given release.yml is on disk
When the build job sets up Rust
Then it uses dtolnay/rust-toolchain@stable
And no `--toolchain nightly` or pinned MSRV is used unless ci.yml does the same

#### Scenario: A clippy warning halts the release

Given a clippy warning exists in the tagged code
When the build job runs `cargo clippy --workspace --all-targets -- -D warnings`
Then the job fails
And no archive is built
And no GitHub Release is published

### Acceptance Criteria

- [ ] Build job's first three steps are `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`
- [ ] Toolchain action: `dtolnay/rust-toolchain@stable` (matches ci.yml exactly)
- [ ] All three gates use the same flags as `ci.yml`
- [ ] Failure of any gate fails the build job (no `continue-on-error: true`)
- [ ] `cargo build --release --locked --target <T>` runs ONLY after all gates pass

### Outcome KPIs

- **Who**: Jeff Bailey (maintainer)
- **Does what**: Avoids shipping a release that ci.yml would have rejected (K-PIPE guardrail)
- **By how much**: 100% of release builds gated by the same checks as the per-PR CI
- **Measured by**: Workflow file diff against `ci.yml`; periodic audit
- **Baseline**: N/A

### Technical Notes

- `Swatinem/rust-cache@v2` SHOULD be used to keep gate-step durations bounded (matches ci.yml).
- If ci.yml ever pins a specific stable version (e.g., `1.75`), release.yml MUST pin the same.

### Dependencies

- US-02 (validate-tag) gates this; build job has `needs: validate-tag`.

---

## US-04: Single-target build produces a stripped archive and sha256

### Problem

Jeff Bailey needs the release pipeline to produce, for each target, a redistributable archive that Homebrew can download and that end users can extract and run. Walking-skeleton scope is one target (`x86_64-unknown-linux-gnu`) — proving the archive shape and naming convention before scaling to a matrix.

### Who

- Jeff Bailey, single maintainer, intermediate Homebrew formula author.

### Solution

After CI parity gates pass, the build job runs `cargo build --release --locked --target ${target} --package modeltap-app`, strips the resulting binary, packages it as `modeltap-${version}-${target}.tar.gz` (containing one file: `modeltap`), computes its sha256 to a sidecar `.sha256` file, and uploads both as a GitHub Actions artifact for downstream jobs to consume.

### Domain Examples

#### 1: Happy path — Linux x86_64 archive

Jeff's `v0.0.1-rc1` walking-skeleton build runs on `ubuntu-22.04`. After the gates pass, `cargo build --release --locked --target x86_64-unknown-linux-gnu --package modeltap-app` produces `target/x86_64-unknown-linux-gnu/release/modeltap` (12.3 MB). `strip` reduces it to ~11.8 MB. The archive `modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz` (~3.5 MB compressed) is created. `sha256sum` writes `e5f6789012abcdef...` to `modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz.sha256`. Both files are uploaded as the `release-x86_64-unknown-linux-gnu` artifact.

#### 2: Edge — version contains a pre-release suffix (`-rc1`)

The walking-skeleton tag is `v0.0.1-rc1`. The version in the archive name is `0.0.1-rc1` (preserves the suffix). Tar packaging works the same; sha256 is computed identically.

#### 3: Error — `cargo build` fails (e.g., disk full on runner)

`cargo build --release` fails mid-link. The job fails. No archive is uploaded. The downstream `publish-github-release` job does not run (US-08 atomic guard). Jeff retries the workflow.

### UAT Scenarios (BDD)

#### Scenario: Single-target archive is produced and named correctly

Given the gates passed for tag v0.2.0 on x86_64-unknown-linux-gnu
When the build job runs cargo build --release --locked --target x86_64-unknown-linux-gnu --package modeltap-app
Then the binary at target/x86_64-unknown-linux-gnu/release/modeltap is stripped
And the archive modeltap-0.2.0-x86_64-unknown-linux-gnu.tar.gz is created containing one file named modeltap
And modeltap-0.2.0-x86_64-unknown-linux-gnu.tar.gz.sha256 contains the sha256 of the archive
And both files are uploaded as a workflow artifact

#### Scenario: Archive naming preserves pre-release suffix

Given the tag is v0.0.1-rc1 and the workspace version is 0.0.1-rc1
When the build job produces the archive
Then the archive filename is modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz

#### Scenario: Build failure prevents archive upload

Given cargo build --release fails on the runner
When the build job runs
Then the job conclusion is "failure"
And no archive is uploaded
And no .sha256 file is uploaded

### Acceptance Criteria

- [ ] Build runs `cargo build --release --locked --target <T> --package modeltap-app`
- [ ] Resulting binary is stripped before packaging
- [ ] Archive filename is exactly `modeltap-${version}-${target}.tar.gz`
- [ ] Archive contains a single file named `modeltap` (no nested directories)
- [ ] Sidecar `.sha256` file is written with `sha256sum < archive > archive.sha256`
- [ ] Both files uploaded as workflow artifact named `release-${target}`

### Outcome KPIs

- **Who**: Jeff Bailey (maintainer); Devon Park (end user, indirectly)
- **Does what**: Produces a downloadable archive a Homebrew formula can reference (K-T2T)
- **By how much**: One archive per target per release, named predictably
- **Measured by**: Workflow artifact existence per target per release
- **Baseline**: N/A

### Technical Notes

- `--locked` ensures Cargo.lock is honored (no version drift mid-build).
- `--package modeltap-app` selects the binary crate; the workspace contains library crates too.
- `strip` is a standard system tool; on Linux runners it's pre-installed.
- For walking-skeleton (US-04), only `x86_64-unknown-linux-gnu` is built. Multi-arch matrix is US-07.

### Dependencies

- US-03 (CI parity gates) gates this; build steps run after the gates.

---

## US-05: `publish-github-release` job creates the GitHub Release with archives, sha256s, and changelog notes

### Problem

After the build matrix produces archives, Jeff Bailey needs them collected into one user-facing GitHub Release page with: the release title (`v0.2.0`), the release notes (the `[0.2.0]` section of `CHANGELOG.md`), and all archive + `.sha256` files attached. Without this, end users have no place to download the binaries from, and the Homebrew formula has no URLs to point at.

### Who

- Jeff Bailey, single maintainer; Devon Park (end user, indirectly via the Release page).

### Solution

A `publish-github-release` job runs after the build matrix, downloads all build artifacts, extracts the `[X.Y.Z]` section from `CHANGELOG.md` into `RELEASE_NOTES.md`, and runs `gh release create v${version} ./modeltap-*.tar.gz ./*.sha256 --title "v${version}" --notes-file ./RELEASE_NOTES.md`.

### Domain Examples

#### 1: Happy path — single-target walking skeleton

Jeff's `v0.0.1-rc1` build matrix produced one archive (`x86_64-unknown-linux-gnu`) + its `.sha256`. The `publish-github-release` job downloads both, extracts the `[0.0.1-rc1]` section from `CHANGELOG.md` (a single line: `Walking-skeleton release-candidate.`), and runs `gh release create v0.0.1-rc1 ./modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz ./modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz.sha256 --title "v0.0.1-rc1" --notes-file ./RELEASE_NOTES.md`. The release page exists at `https://github.com/jeffabailey/modeltap/releases/tag/v0.0.1-rc1`.

#### 2: Edge — release notes contain markdown formatting

The `[0.2.0]` section of `CHANGELOG.md` includes `### Features`, `### Fixes`, code blocks, and links. The extracted `RELEASE_NOTES.md` preserves all formatting; the GitHub Release page renders it correctly.

#### 3: Error — CHANGELOG.md missing the version section

Jeff somehow tagged `v0.2.0` but `CHANGELOG.md` has no `[0.2.0]` heading. The job's extraction step fails with `error: CHANGELOG.md has no [0.2.0] section`. No GitHub Release is created.

### UAT Scenarios (BDD)

#### Scenario: GitHub Release is created with archives, sha256s, and notes

Given the build matrix succeeded with archives and sha256 files
And CHANGELOG.md contains a "## [0.2.0]" section
When the publish-github-release job runs
Then a GitHub Release titled "v0.2.0" is created
And all archive files are attached
And all .sha256 files are attached
And the release notes equal the contents of the [0.2.0] section

#### Scenario: Missing changelog section fails the publish job

Given the build matrix succeeded
And CHANGELOG.md has no [0.2.0] section
When the publish-github-release job runs
Then the job fails with "CHANGELOG.md has no [0.2.0] section"
And no GitHub Release is created

#### Scenario: Pre-release tag is published as a pre-release

Given the tag is v0.0.1-rc1 (contains a hyphen, indicating pre-release per semver)
When the publish-github-release job runs
Then the GitHub Release is marked as a pre-release ("This release is marked as pre-release")

### Acceptance Criteria

- [ ] `publish-github-release` job runs with `needs: [validate-tag, build]`
- [ ] All build artifacts (archives + .sha256s) are downloaded into the workspace
- [ ] `CHANGELOG.md` `[X.Y.Z]` section is extracted into `RELEASE_NOTES.md`
- [ ] Missing `[X.Y.Z]` section fails the job with a clear error
- [ ] `gh release create` is invoked with the tag, all archives, all sha256s, the title, and `--notes-file`
- [ ] Pre-release tags (containing `-`) are marked as pre-release via `--prerelease`

### Outcome KPIs

- **Who**: Jeff Bailey; Devon Park (end user)
- **Does what**: Creates a single release page with all binaries and notes (K-T2T)
- **By how much**: 100% of releases have archives, sha256s, and notes attached
- **Measured by**: Per-release verification: `gh release view v${version} --json assets,body`
- **Baseline**: N/A

### Technical Notes

- `gh release create` is from the GitHub CLI, available pre-installed on GitHub-hosted runners.
- The workflow needs `permissions: contents: write` to create releases.
- `CHANGELOG.md` extraction can be done with `awk '/^## \['"${version}"'\]/,/^## \[/' CHANGELOG.md | head -n -1`.

### Dependencies

- US-04 (build) produces the archives this consumes.
- US-08 (atomic-publish guard) reuses this job's `needs:` declaration.

---

## US-06: `bump-tap-formula` job opens a PR against the tap repo with the rendered formula

### Problem

After the GitHub Release exists, Jeff Bailey needs the Homebrew formula in `jeffabailey/homebrew-modeltap` to be updated to point at the new release URLs and sha256s. Doing this by hand for every release is tedious and error-prone (he might typo a sha256 or forget a platform block). He wants an automated PR opened against the tap repo with the new formula content, ready for `brew test-bot` to validate.

### Who

- Jeff Bailey, single maintainer who owns both the source repo and the tap repo.

### Solution

A `bump-tap-formula` job runs after `publish-github-release`. It checks out `jeffabailey/homebrew-modeltap` using `${tap-bump-token}` (a fine-grained PAT stored as `GH_TAP_TOKEN`), renders `Formula/modeltap.rb` from a template using `${version}`, the four `${release-url}` values, and the four `${sha256}` values (read from the `.sha256` artifact files), commits the change to a branch `bump/v${version}`, and runs `gh pr create --title "modeltap ${version}" --base main --head bump/v${version}` against the tap repo.

### Domain Examples

#### 1: Happy path — tap PR opens for v0.0.1-rc1 (single platform in WS)

Jeff's `v0.0.1-rc1` walking-skeleton has published a GitHub Release with one archive. The `bump-tap-formula` job checks out `jeffabailey/homebrew-modeltap`, renders a one-platform `Formula/modeltap.rb` (only the `on_linux` block populated; `on_macos` blocks are placeholders or omitted in WS), commits to branch `bump/v0.0.1-rc1`, opens PR titled `modeltap 0.0.1-rc1` against the tap's `main` branch.

#### 2: Edge — tap repo has uncommitted state on its main (extremely unlikely but possible if a hand-edit landed)

The job's `git pull --ff-only` inside the tap repo checkout fails. The job fails with `error: tap repo main is not fast-forwardable`. Jeff investigates the tap repo manually.

#### 3: Error — `GH_TAP_TOKEN` expired

The `git push` of branch `bump/v0.2.0` to the tap repo fails with HTTP 401. The job fails with the GitHub CLI error message. The GitHub Release remains intact (it was published in the previous job). Jeff rotates the token and re-runs only the `bump-tap-formula` job.

### UAT Scenarios (BDD)

#### Scenario: Bump-tap-formula opens a PR against the tap repo

Given the GitHub Release for v0.0.1-rc1 has published with a single archive
When the bump-tap-formula job runs
Then jeffabailey/homebrew-modeltap is checked out using GH_TAP_TOKEN
And Formula/modeltap.rb is rendered with version 0.0.1-rc1, release URL, and sha256
And a branch bump/v0.0.1-rc1 is pushed
And a PR titled "modeltap 0.0.1-rc1" is opened against the tap repo's main branch

#### Scenario: Tap-bump runs only after publish-github-release succeeds

Given any failure in publish-github-release
When the workflow continues
Then bump-tap-formula does not run
Because needs: publish-github-release gates it

#### Scenario: Tap-bump-token failure is surfaced clearly

Given GH_TAP_TOKEN has expired
When the bump-tap-formula job tries to push to the tap repo
Then the job fails with the HTTP 401 error visible in the job log
And the job does not silently continue

### Acceptance Criteria

- [ ] `bump-tap-formula` job runs with `needs: publish-github-release`
- [ ] Checks out `jeffabailey/homebrew-modeltap` using `${{ secrets.GH_TAP_TOKEN }}`
- [ ] Renders `Formula/modeltap.rb` from a template using `${version}`, `${release-url}` per target, `${sha256}` per target
- [ ] sha256 values are read from the `.sha256` artifact files (NOT recomputed)
- [ ] Commits to branch `bump/v${version}` and pushes
- [ ] Opens PR titled `modeltap ${version}` against the tap's `main` branch
- [ ] Token failures fail the job visibly (no silent continuation)

### Outcome KPIs

- **Who**: Jeff Bailey (maintainer)
- **Does what**: Eliminates manual formula editing per release (K-TOIL)
- **By how much**: From hand-editing 4 url+sha256 pairs to zero manual steps
- **Measured by**: Per-release audit; tap PR opened automatically for every release
- **Baseline**: N/A

### Technical Notes

- The formula template lives at `release/templates/modeltap.rb.tera` (or similar) in the source repo.
- Template rendering can use `tera` (Rust), `envsubst`, or a small `xtask`. Choice belongs to DESIGN.
- Walking-skeleton renders a one-platform formula; multi-platform is US-10.
- Auto-merge is a separate concern — US-11 enables it; US-06 only opens the PR.

### Dependencies

- US-05 (publish-github-release) produces the GitHub Release this references.
- DESIGN must close D7 (PAT vs GitHub App) before implementation.

---

## US-07: Build matrix expands to all 4 supported targets

### Problem

The walking skeleton built one target. Jeff Bailey needs the real release to cover all 4 supported platforms: macOS Apple Silicon, macOS Intel, Linux x86_64, Linux aarch64. Without the matrix, mac users and aarch64-linux users cannot install via `brew install` — they get `formula not available for this platform`.

### Who

- Jeff Bailey (maintainer); Devon Park (end user on any of 4 platforms).

### Solution

The `build` job uses `strategy.matrix.target` with four entries: `aarch64-apple-darwin` (on `macos-14`), `x86_64-apple-darwin` (on `macos-13`), `x86_64-unknown-linux-gnu` (on `ubuntu-22.04`), `aarch64-unknown-linux-gnu` (on `ubuntu-22.04` cross-compile via `cross` or rustup target). All 4 run in parallel; each produces an archive + sha256.

### Domain Examples

#### 1: Happy path — all 4 targets succeed

Jeff pushes `v0.1.0`. The build matrix runs 4 jobs concurrently. macos-14 builds aarch64-apple-darwin (~3.5 min). macos-13 builds x86_64-apple-darwin (~3.5 min). ubuntu-22.04 builds x86_64-unknown-linux-gnu natively (~2.5 min). ubuntu-22.04 builds aarch64-unknown-linux-gnu via cross-compile (~4 min). All produce archives + sha256s.

#### 2: Edge — cross-compile aarch64-linux requires `cross` tool

The aarch64-linux build cell uses `cross` (or rustup's `aarch64-unknown-linux-gnu` target with the appropriate linker). DESIGN to choose; both work.

#### 3: Error — only the aarch64-linux cell fails

A new dependency adds C-FFI without aarch64 support. The aarch64-linux build fails. Per US-08 atomic-publish guard, the entire release is held; `publish-github-release` does not run. Jeff investigates the dependency, pins or replaces it, retags.

### UAT Scenarios (BDD)

#### Scenario: All 4 targets build in parallel

Given the validate-tag job passed
When the build matrix runs
Then 4 parallel jobs run: aarch64-apple-darwin (macos-14), x86_64-apple-darwin (macos-13), x86_64-unknown-linux-gnu (ubuntu-22.04), aarch64-unknown-linux-gnu (ubuntu-22.04)
And each job produces modeltap-${version}-${target}.tar.gz + .sha256
And all 4 archives are uploaded as separate workflow artifacts

#### Scenario: Cross-compile aarch64-linux uses cross or rustup target

Given the aarch64-unknown-linux-gnu cell is on ubuntu-22.04
When the cargo build runs
Then it uses either `cross build` OR `cargo build` with a configured linker for aarch64-unknown-linux-gnu
And produces a working aarch64 binary verifiable via `file`

#### Scenario: Single-target failure halts the entire release

Given 3 of 4 build cells succeed and 1 fails
When the matrix completes
Then the matrix job's overall status is "failure"
And publish-github-release does not run (US-08 enforces)

### Acceptance Criteria

- [ ] Build job uses `strategy.matrix.target` with 4 entries listed above
- [ ] Each cell is mapped to the correct runner (`macos-14`, `macos-13`, `ubuntu-22.04`, `ubuntu-22.04`)
- [ ] aarch64-linux cell successfully cross-compiles or natively builds
- [ ] All 4 archives + sha256s uploaded as artifacts
- [ ] Matrix uses `fail-fast: false` so a single failure does not cancel still-running cells (helpful for debugging)

### Outcome KPIs

- **Who**: Devon Park (end user across 4 platforms)
- **Does what**: Installs modeltap on their platform via `brew install` (K-COVER)
- **By how much**: 100% platform coverage on every release
- **Measured by**: Per-release artifact list; `brew test-bot` runs install on each platform
- **Baseline**: N/A

### Technical Notes

- `cross` is a third-party tool maintained by the cross-rs project; widely used in Rust CI.
- Alternative: `rustup target add aarch64-unknown-linux-gnu` + `gcc-aarch64-linux-gnu` + `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc`. DESIGN choice.
- `Swatinem/rust-cache@v2` keys per target.

### Dependencies

- US-04 (single-target build) is the base; US-07 expands to a matrix.

---

## US-08: Atomic-publish guard ensures all-or-nothing releases

### Problem

If 3 of 4 build cells succeed and 1 fails, naively running `publish-github-release` would create a GitHub Release with only 3 archives — a half-published release that would 404 for users on the missing platform. Jeff Bailey needs a hard guarantee: all archives publish together, or none do.

### Who

- Jeff Bailey (maintainer); Devon Park (end user, who must never see a half-released version).

### Solution

`publish-github-release` declares `needs: [validate-tag, build]`. GitHub Actions only runs a `needs` job if ALL listed jobs succeeded; if any matrix cell failed, `publish-github-release` is skipped. Same for `bump-tap-formula` (`needs: publish-github-release`). This is the GitHub Actions-native way to express atomicity.

### Domain Examples

#### 1: Happy path — all 4 cells succeed → publish runs

`v0.1.0` build matrix: 4 of 4 cells succeed. `publish-github-release` runs, attaches all 4 archives + 4 sha256s. `bump-tap-formula` runs, opens tap PR with all 4 platform blocks.

#### 2: Error — aarch64-linux cell fails → publish skipped

`v0.1.0` build matrix: 3 of 4 cells succeed; aarch64-linux fails. `publish-github-release` is skipped (status: "skipped" in the workflow UI). No GitHub Release is created. `bump-tap-formula` is also skipped. Jeff sees the workflow failure in `gh run watch` and investigates.

#### 3: Edge — workflow re-run after fixing the failing cell

Jeff fixes the aarch64-linux issue, deletes the v0.1.0 tag, and retags. The whole workflow runs again from `validate-tag` through `bump-tap-formula`. (No partial re-runs of just the failing cell — the validate-tag-to-publish chain is atomic per tag push.)

### UAT Scenarios (BDD)

#### Scenario: All 4 cells succeed → publish runs

Given the build matrix runs and all 4 cells succeed
When the publish-github-release job is evaluated
Then publish-github-release runs
And bump-tap-formula runs

#### Scenario: One cell fails → publish skipped

Given the build matrix runs and 1 of 4 cells fails
When the publish-github-release job is evaluated
Then publish-github-release is skipped (status: "skipped")
And bump-tap-formula is skipped
And no GitHub Release is created
And no tap PR is opened

#### Scenario: Workflow re-run after retag is fully atomic

Given a previous tag v0.1.0 was deleted after a build failure
When Jeff re-pushes the tag v0.1.0 (or pushes v0.1.1)
Then the entire workflow runs again from validate-tag
And atomicity is preserved per tag push (no partial publish from the previous attempt persists)

### Acceptance Criteria

- [ ] `publish-github-release` declares `needs: [validate-tag, build]`
- [ ] `bump-tap-formula` declares `needs: publish-github-release`
- [ ] No `if: always()` or `if: failure()` overrides bypass the guard for `publish` or `bump`
- [ ] Workflow run UI clearly shows "skipped" status for downstream jobs when a build cell fails

### Outcome KPIs

- **Who**: Devon Park (end user)
- **Does what**: Never encounters a half-published release (K-PIPE guardrail; K-COVER guardrail)
- **By how much**: 100% of GitHub Releases have all expected archives
- **Measured by**: Per-release verification: `gh release view --json assets` count == 4 archives + 4 sha256s
- **Baseline**: N/A

### Technical Notes

- This is purely a workflow-graph property (the `needs:` DAG). No imperative code.
- `fail-fast: false` on the matrix is independent — it controls whether running cells are cancelled when one fails. Atomicity is enforced by `needs:` regardless.

### Dependencies

- US-05 (publish-github-release exists), US-06 (bump-tap-formula exists), US-07 (matrix exists).

---

## US-09: SLSA build provenance attestation per archive

### Problem

End users who care about supply chain (and increasingly, employers' security teams) need a way to verify that the binary they downloaded was actually built by the official GitHub Actions workflow — not swapped out post-upload. Jeff Bailey wants every archive to carry a signed attestation that's verifiable with one CLI command.

### Who

- Devon Park (end user concerned about supply chain); Jeff Bailey (maintainer who wants the trust signal).

### Solution

After `tar.gz` packaging in the build job, run `actions/attest-build-provenance@v2` against each archive. The action signs an attestation with GitHub's OIDC provider (SLSA Level 3). Attestations are uploaded with the GitHub Release. End users verify with `gh attestation verify <archive> --owner jeffabailey`.

### Domain Examples

#### 1: Happy path — every archive has attestation

`v0.1.0` build matrix: 4 archives produced, 4 attestations signed and uploaded. End user runs `gh attestation verify modeltap-0.1.0-aarch64-apple-darwin.tar.gz --owner jeffabailey` and sees `verified`.

#### 2: Edge — attestation step fails on one cell (rare)

The `actions/attest-build-provenance@v2` action fails on macos-13 due to a transient GitHub OIDC outage. The build cell fails. Per US-08, the entire release is held. Jeff retries the workflow.

#### 3: End-user verifies attestation

Devon downloads `modeltap-0.1.0-aarch64-apple-darwin.tar.gz`. Before extracting, he runs `gh attestation verify modeltap-0.1.0-aarch64-apple-darwin.tar.gz --owner jeffabailey`. Output: `Verification succeeded! ... built by jeffabailey/modeltap workflow release.yml at commit <sha>`.

### UAT Scenarios (BDD)

#### Scenario: Every archive carries a verifiable attestation

Given the build matrix completed successfully
When the publish-github-release job uploads archives
Then each archive's actions/attest-build-provenance@v2 attestation is attached
And `gh attestation verify <archive> --owner jeffabailey` returns success for each

#### Scenario: Attestation failure halts the build

Given actions/attest-build-provenance@v2 fails on a build cell
When the build job runs
Then the build cell fails
And no archive from that cell is uploaded
And US-08 atomic-publish prevents the release

### Acceptance Criteria

- [ ] `actions/attest-build-provenance@v2` runs in the build job after the tar.gz step
- [ ] One attestation per archive
- [ ] Workflow has `permissions: id-token: write` and `attestations: write` (required by the action)
- [ ] `gh attestation verify <archive> --owner jeffabailey` returns success for every published archive
- [ ] README troubleshooting section documents the verify command

### Outcome KPIs

- **Who**: Devon (security-conscious end user)
- **Does what**: Verifies every archive was built by the official workflow (K-PROV)
- **By how much**: 100% of archives carry a verifiable attestation
- **Measured by**: Spot-check `gh attestation verify` per release; full audit quarterly
- **Baseline**: N/A

### Technical Notes

- `actions/attest-build-provenance@v2` is GitHub-maintained and produces SLSA Level 3 attestations.
- Adds ~30s per build cell.
- Verification is free and requires no credentials on the user's side — just `gh` CLI.

### Dependencies

- US-04 (archive exists) is the input; US-08 (atomic-publish) gates downstream.

---

## US-10: Formula renders all 4 platform blocks with sha256s read from artifact files

### Problem

Walking-skeleton's `Formula/modeltap.rb` had one platform block (Linux x86_64). The real release needs 4 platform blocks (`on_macos { on_arm { ... } on_intel { ... } } on_linux { on_arm { ... } on_intel { ... } }`). Each block needs the correct `url` and `sha256`. Jeff Bailey needs the bump job to render all 4 blocks correctly, with sha256s read from the build artifacts (NOT recomputed by the bump job — that would defeat the integrity check).

### Who

- Jeff Bailey (maintainer); Devon Park (end user across 4 platforms).

### Solution

`bump-tap-formula` downloads all 4 build artifacts (each containing an archive + a `.sha256` file). It reads each `.sha256` file's contents (the hex digest) and inserts each into the corresponding platform block of the formula template. The rendered formula has 4 fully-populated platform blocks.

### Domain Examples

#### 1: Happy path — 4-block formula renders correctly

`v0.1.0` build matrix produced 4 archives + 4 sha256 files. `bump-tap-formula` reads `modeltap-0.1.0-aarch64-apple-darwin.tar.gz.sha256` (contents: `e5f6...7890`), inserts into the `on_macos { on_arm { sha256 "e5f6...7890" } }` block. Repeats for the other 3 targets. The rendered `Formula/modeltap.rb` is committed.

#### 2: Edge — sha256 file is missing for one target (should not happen given US-08 guard, but defensive)

The bump job fails with `error: expected modeltap-0.1.0-aarch64-apple-darwin.tar.gz.sha256 in artifact set, not found`.

#### 3: Edge — formula audit catches a missing platform block

Jeff accidentally removes the `on_linux` block from the template. `brew test-bot audit` fails with `formula must support all advertised platforms`. Auto-merge withholds (US-11 covers this).

### UAT Scenarios (BDD)

#### Scenario: Formula renders 4 platform blocks with correct sha256s

Given the build matrix produced 4 archives + 4 .sha256 files
When the bump-tap-formula job renders Formula/modeltap.rb
Then the formula has 4 platform blocks: on_macos.on_arm, on_macos.on_intel, on_linux.on_arm, on_linux.on_intel
And each block's sha256 field equals the contents of the corresponding .sha256 artifact file
And no sha256 is computed by reading or rehashing the archive in the bump job

#### Scenario: Missing artifact fails the bump job

Given an expected .sha256 artifact is missing
When the bump-tap-formula job runs
Then the job fails with a clear "expected <file>, not found" message

#### Scenario: Brew audit verifies the rendered formula

Given the rendered Formula/modeltap.rb is committed to the bump branch
When brew test-bot audit runs on the PR
Then the audit passes
And the install + test stanzas run on macos-14, macos-13, ubuntu-22.04

### Acceptance Criteria

- [ ] Rendered formula has 4 platform blocks (`on_macos.on_arm`, `on_macos.on_intel`, `on_linux.on_arm`, `on_linux.on_intel`)
- [ ] Each block's `url` and `sha256` are derived from `${release-url}` and the contents of the `.sha256` artifact file
- [ ] sha256 values are read from `.sha256` files; never recomputed by the bump job
- [ ] Missing `.sha256` artifact fails the job with clear error
- [ ] `brew test-bot audit` passes on the resulting formula

### Outcome KPIs

- **Who**: Devon Park (end user)
- **Does what**: Installs modeltap on their platform without sha256 mismatch (K-COVER guardrail)
- **By how much**: 0 sha256 mismatches across releases
- **Measured by**: `brew test-bot install` step in the tap PR
- **Baseline**: N/A

### Technical Notes

- Template uses `tera` placeholders or simple `envsubst`; DESIGN choice.
- `Formula/modeltap.rb` Ruby DSL: `url`, `sha256` per `on_arm`/`on_intel` block under `on_macos`/`on_linux`.

### Dependencies

- US-06 (bump job exists), US-07 (matrix produces 4 artifacts).

---

## US-11: Auto-merge tap-bump PR when `brew test-bot` is green

### Problem

After `bump-tap-formula` opens a PR against the tap repo, Jeff Bailey doesn't want to manually click "merge" on every release. He wants auto-merge to fire when (and only when) `brew test-bot` is green. If `brew test-bot` fails, the PR stays open for him to investigate.

### Who

- Jeff Bailey (maintainer).

### Solution

The `bump-tap-formula` job runs `gh pr merge --auto --squash` against the tap PR. GitHub holds the merge until all required status checks pass; when `brew test-bot` reports success, the PR auto-merges. If `brew test-bot` fails, auto-merge does NOT fire and the PR stays open.

### Domain Examples

#### 1: Happy path — test-bot green, PR auto-merges

`v0.1.0` tap PR opens. `brew test-bot` runs (audit + install on macos-14, macos-13, ubuntu-22.04 + test). All green. Within 5 minutes of opening, the PR auto-merges. Total tag-to-tap latency: ~14 minutes.

#### 2: Error — test-bot install fails on macos-14

The install step on macos-14 fails because the binary requires a glibc version not present (or some macOS framework issue). Auto-merge does NOT fire. The PR stays open. Jeff is notified via GitHub email; he investigates the failure, fixes, retags.

#### 3: Edge — auto-merge requires branch protection rules in tap repo

The tap repo's `main` branch must have a branch protection rule requiring `brew test-bot` to pass. Jeff sets this up once during initial tap-repo setup; it's a precondition for US-11 to work.

### UAT Scenarios (BDD)

#### Scenario: Auto-merge fires on green test-bot

Given the bump-tap-formula PR is open
When brew test-bot's audit, install, and test stanzas all pass
Then the PR auto-merges within 5 minutes of opening

#### Scenario: Auto-merge withholds on failed test-bot

Given the bump-tap-formula PR is open
When brew test-bot's install step fails on macos-14
Then auto-merge does not fire
And the PR stays open
And the maintainer is notified via the PR thread (GitHub email)

#### Scenario: Manual merge is still possible

Given the bump-tap-formula PR is open
And brew test-bot has not yet run (or is hanging)
When Jeff manually clicks merge after waiting
Then the PR merges
And subsequent re-runs of bump-tap-formula are idempotent (US-12)

### Acceptance Criteria

- [ ] `bump-tap-formula` job runs `gh pr merge --auto --squash` on the tap PR
- [ ] Tap repo `main` branch has protection rule requiring `brew test-bot` status check
- [ ] Auto-merge fires only when test-bot is green
- [ ] Failed test-bot leaves the PR open (no force-merge)
- [ ] Documented in `RELEASING.md`: branch protection setup is a one-time precondition

### Outcome KPIs

- **Who**: Jeff Bailey (maintainer)
- **Does what**: Walks away after the tag push; tap PR merges automatically (K-TOIL)
- **By how much**: From 1 manual merge per release to 0
- **Measured by**: Per-release audit; `gh run watch` shows hands-off completion
- **Baseline**: N/A

### Technical Notes

- `gh pr merge --auto` requires the user (or the token's identity) to have write access to the tap repo.
- Branch protection setup is a tap-repo configuration, not a workflow concern.

### Dependencies

- US-06 (PR opens), US-10 (formula renders correctly so test-bot can pass).

---

## US-12: `bump-tap-formula` is idempotent on retry

### Problem

If `bump-tap-formula` fails partway (e.g., `GH_TAP_TOKEN` expired after the PR was opened but before merge), Jeff Bailey wants to re-run only that job. The re-run must not open a duplicate PR; it must update the existing PR (push to the same `bump/v${version}` branch) so auto-merge can re-evaluate.

### Who

- Jeff Bailey (maintainer).

### Solution

The `bump-tap-formula` job checks for an existing branch `bump/v${version}` in the tap repo. If found, it force-pushes the new commit to that branch (the existing PR auto-updates). If not found, it creates the branch and opens a new PR. Either way, the end state is the same: one PR per release version.

### Domain Examples

#### 1: Happy path — first run opens the PR

`v0.1.0` first run: `bump/v0.1.0` branch does not exist; job creates it and opens PR #14.

#### 2: Edge — re-run after token rotation updates existing PR

`bump-tap-formula` failed at the `gh pr merge --auto` step due to expired token. Jeff rotates the token. He re-runs the job. The job sees `bump/v0.1.0` exists, force-pushes (no new commits since the diff is the same), re-runs `gh pr merge --auto`. The existing PR #14 is now armed for auto-merge. No PR #15 is created.

#### 3: Edge — re-run after a manual fix

`brew test-bot` failed because of a typo in the rendered formula. Jeff manually fixes the formula (pushes to `bump/v0.1.0` from his local clone). He re-runs `bump-tap-formula`. The job sees the branch exists, force-pushes its template render again — but Jeff's manual fix gets clobbered. (This is a known trade-off: the bump job is the source of truth for the formula; manual fixes belong in the template, not the rendered file.)

### UAT Scenarios (BDD)

#### Scenario: First run opens a new PR

Given no bump/v0.1.0 branch exists in the tap repo
When the bump-tap-formula job runs
Then a new bump/v0.1.0 branch is created
And a new PR is opened

#### Scenario: Re-run updates existing PR

Given a bump/v0.1.0 branch exists in the tap repo
And a PR #14 references that branch
When the bump-tap-formula job is re-run
Then the same branch is force-pushed (with the same or updated formula content)
And no new PR is created
And PR #14 is the only PR for this version

#### Scenario: Manual edits to bump branch are clobbered on re-run (documented trade-off)

Given the maintainer has manually edited Formula/modeltap.rb on bump/v0.1.0
When the bump-tap-formula job is re-run
Then the manual edits are overwritten by the template render
And RELEASING.md documents this: "fix the template, not the rendered file"

### Acceptance Criteria

- [ ] Job checks for existing `bump/v${version}` branch before creating
- [ ] Force-pushes to existing branch (no duplicate branches)
- [ ] One PR per release version (no duplicates)
- [ ] `RELEASING.md` documents the manual-edit trade-off

### Outcome KPIs

- **Who**: Jeff Bailey (maintainer)
- **Does what**: Recovers from transient failures without manual cleanup (K-PIPE)
- **By how much**: 100% of retry attempts produce the correct end state (one PR, correct content)
- **Measured by**: Tap repo audit: at most one open `bump/v${version}` branch + one PR per version
- **Baseline**: N/A

### Technical Notes

- `git push --force-with-lease` is safer than `git push --force`; use the former.
- `gh pr list --head bump/v${version}` to check existing PR.

### Dependencies

- US-06 (bump job exists).

---

## US-13: `RELEASING.md` runbook exists, ≤10 numbered steps

### Problem

Jeff Bailey needs a one-page runbook he can reference when cutting a release at 10 PM on a Friday. Riley Chen needs the same runbook to understand the release process when she contributes for the first time. A 50-page wiki page is useless; an 8-step printable checklist is gold.

### Who

- Jeff Bailey (maintainer); Riley Chen (open-source contributor).

### Solution

`RELEASING.md` at repo root, ≤10 numbered steps, ≤1 printed page (~50 lines of markdown). Each step is one line. Includes: "Branch from main", "Run `cargo xtask release-prep --version X.Y.Z`", "Open prep PR", "Merge prep PR", "Tag with `git tag -a vX.Y.Z`", "Push tag", "Watch `gh run watch`", "Verify install on a clean machine". Plus a release-log table (one row per release) at the bottom.

### Domain Examples

#### 1: Happy path — Jeff cuts v0.2.0 from the runbook

Jeff opens `RELEASING.md`, follows steps 1-7, the workflow runs, step 8 (`gh run watch`) shows green, step 9 verifies install on a clean Linux box. Total time: 18 minutes (3 minutes of Jeff's attention, 15 minutes of automation).

#### 2: Edge — Riley reads the runbook to understand releases

Riley opens `RELEASING.md`. She reads steps 1-9. She opens `release.yml` (US-14 makes it ≤250 lines). She can explain the release process to a colleague in 5 minutes.

#### 3: Edge — release log table

The bottom of `RELEASING.md` has a markdown table: `| version | tag pushed | release published | tap merged | T2T | platforms verified | provenance verified | notes |`. Jeff appends a row after each release; this is the data source for K-T2T.

### UAT Scenarios (BDD)

#### Scenario: Runbook exists and is short

Given the repo is checked out
When the maintainer opens RELEASING.md
Then the file exists at repo root
And contains ≤10 numbered steps
And is ≤1 page printed (≤80 lines including headers/spacing)

#### Scenario: Riley can explain the release process from the runbook in 5 minutes

Given Riley reads RELEASING.md once
When Riley is asked "how does a modeltap release work?"
Then Riley can explain the 7-step lifecycle in 5 minutes
And cite the workflow file location for deeper detail

#### Scenario: Release log table is the data source for K-T2T

Given multiple releases have shipped
When the maintainer reviews release performance
Then RELEASING.md contains a row per release with timestamps
And K-T2T can be computed from these rows

### Acceptance Criteria

- [ ] `RELEASING.md` exists at repo root
- [ ] Contains ≤10 numbered steps
- [ ] Contains a release-log table (per-release rows)
- [ ] Documents `GH_TAP_TOKEN` rotation procedure
- [ ] Documents the manual-edit-to-bump-branch clobber trade-off (cross-ref US-12)
- [ ] Documents the macOS Gatekeeper `xattr` workaround (cross-ref D6)
- [ ] Total file size ≤80 lines

### Outcome KPIs

- **Who**: Jeff Bailey (maintainer); Riley Chen (contributor)
- **Does what**: Cuts a release without needing to consult any other doc; explains the process in 5 minutes (K-CONTRIB)
- **By how much**: 0 reference docs needed beyond `RELEASING.md` to cut a release
- **Measured by**: Self-test (Jeff cuts a release by following only the runbook); contributor interviews
- **Baseline**: N/A

### Technical Notes

- Plain markdown; no fancy templating.
- Linked from README ("How releases work").

### Dependencies

- All other stories in this feature (the runbook describes the whole flow).

---

## US-14: `release.yml` is ≤250 lines, every job has a purpose comment

### Problem

A release workflow that grows to 800 lines with no comments is opaque even to its author. Jeff Bailey wants to keep the file under 250 lines, with every job having a one-sentence purpose comment, so he and any contributor can read it top-to-bottom in 3 minutes.

### Who

- Jeff Bailey (maintainer); Riley Chen (open-source contributor).

### Solution

A code-review constraint enforced by lint (a small `xtask` or shell script that fails if `release.yml` exceeds 250 lines or any job declaration lacks a `# Purpose:` comment immediately above it). The lint runs in `ci.yml` to catch drift.

### Domain Examples

#### 1: Happy path — initial release.yml is 180 lines

The first version of `release.yml` has 4 jobs (validate-tag, build, publish-github-release, bump-tap-formula). Each has a 1-line purpose comment. Total: 180 lines. Lint passes.

#### 2: Edge — a future contributor adds a 5th job (e.g., notarization) and the file grows to 240 lines

The notarization job has a purpose comment; the file is at 240 lines (still under 250). Lint passes.

#### 3: Error — a contributor adds a 6th job pushing the file to 270 lines

Lint fails with `release.yml exceeds 250 lines (270); split or refactor`. The PR cannot merge until the file is refactored (e.g., extract reusable composite actions).

### UAT Scenarios (BDD)

#### Scenario: release.yml is under the line limit

Given .github/workflows/release.yml exists
When the file is line-counted
Then the count is ≤250 lines (including comments and blank lines)

#### Scenario: Every job has a purpose comment

Given .github/workflows/release.yml exists
When each job declaration is examined
Then a comment line starting with "# Purpose:" appears immediately above each job

#### Scenario: ci.yml lint catches drift

Given a contributor opens a PR that grows release.yml to 260 lines
When ci.yml runs the workflow-lint xtask
Then the lint fails with a clear "exceeds 250 lines" message

### Acceptance Criteria

- [ ] `release.yml` is ≤250 lines
- [ ] Every job declaration has a `# Purpose: <one sentence>` comment immediately above
- [ ] A `cargo xtask lint-workflows` (or shell equivalent) enforces both constraints
- [ ] Lint runs in `ci.yml`

### Outcome KPIs

- **Who**: Riley Chen (contributor); Jeff Bailey (maintainer)
- **Does what**: Reads and understands `release.yml` end-to-end (K-CONTRIB)
- **By how much**: ≤3 minutes to skim; ≤10 minutes to deep-read
- **Measured by**: Contributor interviews; per-release "could a contributor explain this?" check
- **Baseline**: N/A

### Technical Notes

- The lint is small: ~30 lines of Rust or shell.
- Composite actions (`./.github/actions/<name>/action.yml`) can be used to extract repeated step sequences if 250 lines becomes binding.

### Dependencies

- All other release.yml stories must produce content that fits.

---

## US-15: Devon installs modeltap with `brew install` and verifies the version

### Problem

After all the maintainer-side work, the only thing that matters is whether Devon Park (end user) can install and run modeltap on his clean machine. The whole pipeline exists to deliver this single user-facing experience: one `brew install` line, one `modeltap --version` check, and the TUI launches.

### Who

- Devon Park, multi-tool local-AI power user, has Homebrew installed, has never installed modeltap before.

### Solution

After the tap-bump PR merges, Devon runs `brew install jeffabailey/modeltap/modeltap`. Brew downloads the correct per-platform archive, extracts the binary to `/opt/homebrew/Cellar/modeltap/${version}/bin/modeltap`, and links it into PATH. Devon runs `modeltap --version` and sees `modeltap ${version}`. He runs `modeltap` and the TUI launches (per the modeltap-tui feature).

### Domain Examples

#### 1: Happy path — clean macOS Apple Silicon (Devon's MacBook Pro)

Devon, on a clean macOS Sonoma M2 MacBook with Homebrew installed, runs `brew install jeffabailey/modeltap/modeltap`. Brew taps the formula (if not already tapped), downloads `modeltap-0.2.0-aarch64-apple-darwin.tar.gz` (3.5 MB), pours into the cellar, and links to PATH. Devon runs `modeltap --version` and sees `modeltap 0.2.0`. He runs `modeltap` and the two-pane TUI launches.

#### 2: Happy path — clean Linux x86_64 (Devon's home Ubuntu box)

Devon, on a clean Ubuntu 22.04 x86_64 box with Homebrew on Linux installed, runs `brew install jeffabailey/modeltap/modeltap`. Brew downloads `modeltap-0.2.0-x86_64-unknown-linux-gnu.tar.gz`. `modeltap --version` prints `modeltap 0.2.0`.

#### 3: Edge — Devon installs during the 1-2 minute tap-update window

The GitHub Release for `v0.2.0` published 30 seconds ago. The tap-bump PR is still open (waiting for `brew test-bot`). Devon runs `brew install jeffabailey/modeltap/modeltap`. Brew installs the previously-released `v0.1.0` (the formula has not yet been bumped). Devon notices and runs `brew upgrade modeltap` 2 minutes later; `v0.2.0` installs.

### UAT Scenarios (BDD)

#### Scenario: Clean macOS Apple Silicon installs modeltap

Given a clean macOS Sonoma (Apple Silicon) machine with Homebrew installed
And the v0.2.0 tap-bump PR has merged
When Devon runs "brew install jeffabailey/modeltap/modeltap"
Then brew downloads modeltap-0.2.0-aarch64-apple-darwin.tar.gz
And the install completes within 30 seconds (excluding network)
And "modeltap --version" prints exactly "modeltap 0.2.0"
And "modeltap" launches the TUI

#### Scenario: Clean Linux x86_64 installs modeltap

Given a clean Ubuntu 22.04 x86_64 machine with Homebrew on Linux installed
And the v0.2.0 tap-bump PR has merged
When Devon runs "brew install jeffabailey/modeltap/modeltap"
Then brew downloads modeltap-0.2.0-x86_64-unknown-linux-gnu.tar.gz
And "modeltap --version" prints exactly "modeltap 0.2.0"

#### Scenario: Tag-to-install latency is under 15 minutes (median)

Given Cargo.toml workspace version on main is 0.2.0
When Jeff pushes the tag "v0.2.0" at time T
Then the tap-bump PR has merged by time T plus 14 minutes (median)
And a clean machine running "brew install jeffabailey/modeltap/modeltap" by time T plus 15 minutes succeeds

#### Scenario: Install during tap-update window installs the previous version informatively

Given the GitHub Release for v0.2.0 has published
And the tap-bump PR has not yet merged
When Devon runs "brew install jeffabailey/modeltap/modeltap"
Then brew installs the previously-released v0.1.0
And Devon may run "brew upgrade modeltap" after the tap-bump PR merges to get v0.2.0

### Acceptance Criteria

- [ ] `brew install jeffabailey/modeltap/modeltap` succeeds on macOS Sonoma (Apple Silicon)
- [ ] `brew install jeffabailey/modeltap/modeltap` succeeds on macOS Ventura (Intel)
- [ ] `brew install jeffabailey/modeltap/modeltap` succeeds on Ubuntu 22.04 (x86_64)
- [ ] `brew install jeffabailey/modeltap/modeltap` succeeds on Ubuntu 22.04 (aarch64)
- [ ] `modeltap --version` prints exactly `modeltap ${version}` matching the installed version
- [ ] `modeltap` launches the TUI (delegates to modeltap-tui feature for TUI behavior)
- [ ] Median tag-to-install latency ≤ 15 minutes

### Outcome KPIs

- **Who**: Devon Park (end user)
- **Does what**: Installs modeltap and verifies the version (K-T2T, K-COVER)
- **By how much**: 100% install success across 4 platforms; ≤15 min median tag-to-install
- **Measured by**: Per-release manual reference-machine test on at least one platform; `brew test-bot` covers the others
- **Baseline**: N/A (greenfield)

### Technical Notes

- `modeltap --version` uses `clap` `#[command(version)]` reading `CARGO_PKG_VERSION` at compile time.
- The TUI launching behavior is owned by `modeltap-tui`; this story only verifies the install + `--version`, not the TUI.

### Dependencies

- All other stories in this feature (this story is the end-to-end verification).
- `modeltap-tui` US-01 (TUI launches) for the `modeltap` post-install behavior.
