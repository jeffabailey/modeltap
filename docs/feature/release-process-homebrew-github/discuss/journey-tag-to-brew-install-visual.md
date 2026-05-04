# Journey: Tag a Release, Land in Homebrew — Visual

**Feature:** release-process-homebrew-github
**Primary persona:** Jeff Bailey — modeltap maintainer (single-maintainer OSS project). Cuts releases on a personal cadence, runs macOS Sonoma + Linux WSL, comfortable with `git`, GitHub Actions, and Homebrew formula authoring. Wants release cuts to feel routine and boring.
**Secondary persona:** Devon Park — modeltap end user (already met in `modeltap-tui`). On macOS or Linux. Discovers modeltap via README, wants to `brew install` it and start using the TUI within minutes. Has Homebrew installed, has never written a tap formula.
**Tertiary persona:** Riley Chen — open-source contributor. Already validated the plugin trait in `modeltap-tui`. Now wants to understand the release pipeline so she can confidently propose changes.
**Goal:** A single `git push origin v0.x.0` produces signed, attested, multi-arch release binaries on GitHub, updates the Homebrew tap formula automatically, and lets a clean macOS or Linux machine `brew install modeltap/tap/modeltap` and run `modeltap` within fifteen minutes.

## Emotional Arc

This is an infrastructure feature. The dominant emotion is **boring confidence** — the absence of drama is the value.

| Phase | State | What drives it |
|---|---|---|
| Start (maintainer) | Mild dread ("releases always break something") | Memory of past hand-rolled releases that drifted from intent |
| Mid (maintainer) | Watchful patience | CI runs, jobs go green one by one, no manual steps |
| End (maintainer) | Boring satisfaction ("it just shipped") | Tap formula updated automatically, release notes generated, no late-night patching |
| Start (installer Devon) | Mild skepticism ("does this even build on my Mac?") | Burnt before by curl-pipe-bash README install instructions |
| End (installer Devon) | Quiet relief ("oh, it just worked") | One `brew install` line, then `modeltap --version` prints the expected number |
| Contributor Riley | Confident curiosity | Workflow file is short, well-commented, and traceable end-to-end |

Failure-mode emotional rule: **the maintainer must always know what state a release is in, and what to do next.** No silent failures, no half-published releases. Either the release lands fully or it rolls back observably.

## Journey Flow (ASCII)

```
[Maintainer trigger]                                           [End user outcome]
"v0.2.0 is ready to ship.                              `brew install` works on a
 Push the tag, walk away,                              clean machine. Binary runs.
 come back to a finished release."                     Maintainer sleeps fine.
                |                                                  ^
                v                                                  |
+-----+-------+   +------+------+   +------+------+   +------+------+   +------+------+
| Step 1      |   | Step 2      |   | Step 3      |   | Step 4      |   | Step 5      |
| Bump        |-->| Push tag    |-->| Workflow    |-->| Tap formula |-->| User runs   |
| version +   |   | v0.x.0      |   | builds +    |   | bumped to   |   | brew install|
| changelog   |   |             |   | publishes   |   | new sha256  |   | + verifies  |
+-------------+   +-------------+   +--------------+   +-------------+   +-------------+
 Feels:            Feels:            Feels:              Feels:              Feels:
 deliberate        committed          watchful            relieved            quietly
 (reviewed PR)     ("here goes")     (matrix runs        (automated PR        delighted
                                      green)              merged)             (it just worked)
```

## Step-by-Step Detail

### Step 1: Bump version and changelog (maintainer prep)

**Command:** `git checkout -b release/v0.2.0 && cargo set-version 0.2.0 && cargo xtask release-prep`

**Terminal mockup (the maintainer's prep PR diff):**

```
$ cargo xtask release-prep --version 0.2.0
[release-prep] Reading workspace version ............................. 0.1.0
[release-prep] Setting workspace.package.version ..................... 0.2.0
[release-prep] Updating Cargo.lock ................................... ok
[release-prep] Generating CHANGELOG.md from conventional commits ..... ok
[release-prep]   - 17 commits since v0.1.0
[release-prep]   - 4 fix:, 8 feat:, 3 chore:, 2 refactor:
[release-prep] Verifying CI gates locally:
[release-prep]   cargo fmt --all -- --check ......................... ok
[release-prep]   cargo clippy --workspace --all-targets -- -D warns . ok
[release-prep]   cargo test --workspace --locked ..................... ok (332 tests)
[release-prep] Done. Commit, push, open PR titled 'release: v0.2.0'.

$ git add -A && git commit -m "release: v0.2.0" && git push -u origin release/v0.2.0
```

**Shared artifacts referenced:** `${version}=0.2.0` (single source: `Cargo.toml [workspace.package].version`), `${tag}=v0.2.0` (derived: `v` prefix + version), `${changelog}` (single source: `CHANGELOG.md`, generated from conventional commits between previous tag and HEAD).

**Emotional state:** entry "is the tree ready?" → exit "yes, the prep PR shows exactly what will ship." Confidence-builder: every CI gate runs locally before push.

**Integration checkpoint:** The `cargo xtask release-prep` script MUST be the only writer of `${version}` and `${changelog}` in this step. Hand-edits to either file are a smell. The PR review is the human gate before tagging.

---

### Step 2: Push the tag (the only manual production action)

**Command:** `git tag -a v0.2.0 -m "v0.2.0" && git push origin v0.2.0`

**Terminal mockup:**

```
$ git checkout main && git pull --ff-only
Already up to date.

$ git tag -a v0.2.0 -m "v0.2.0"
$ git push origin v0.2.0
To github.com:jeffabailey/modeltap.git
 * [new tag]         v0.2.0 -> v0.2.0

$ gh run watch --exit-status
- release.yml * v0.2.0 in_progress
   build (aarch64-apple-darwin)         queued
   build (x86_64-apple-darwin)          queued
   build (x86_64-unknown-linux-gnu)     queued
   build (aarch64-unknown-linux-gnu)    queued
   publish-github-release               waiting
   bump-tap-formula                     waiting
```

**Shared artifacts:** `${tag}=v0.2.0` (must equal `v` + the workspace version that landed in `main`; CI fails the workflow if they disagree).

**Emotional state:** entry "OK, here goes" → exit "it's running, no path back." Norman's principle of feedback: the user sees the workflow start within seconds.

**Integration checkpoint:** The release workflow MUST validate `${tag} == "v" + workspace.package.version` before any artifact is built. Mismatch = abort with a clear error. Without this guard, a maintainer who forgets to bump `Cargo.toml` ships a v0.2.0 tag pointing at v0.1.0 code.

---

### Step 3: Workflow builds, signs, and publishes the GitHub Release

**Workflow surface (what CI does, no manual steps):**

```
+- .github/workflows/release.yml — triggered on: push tag matching v*.*.* ----+
|                                                                              |
| job: validate-tag                                                            |
|   - assert tag matches v + workspace.package.version                         |
|                                                                              |
| job: build (matrix)                                                          |
|   strategy:                                                                  |
|     - aarch64-apple-darwin       on macos-14 (Apple Silicon)                 |
|     - x86_64-apple-darwin        on macos-13                                 |
|     - x86_64-unknown-linux-gnu   on ubuntu-22.04                             |
|     - aarch64-unknown-linux-gnu  on ubuntu-22.04 (cross-compile)             |
|   steps:                                                                     |
|     - cargo fmt --all -- --check         # CI parity gate                    |
|     - cargo clippy ... -D warnings       # CI parity gate                    |
|     - cargo test --workspace --locked    # CI parity gate                    |
|     - cargo build --release --locked --target ${{ matrix.target }}           |
|     - strip + tar.gz the binary as modeltap-${version}-${target}.tar.gz      |
|     - sha256sum > .sha256                                                    |
|     - actions/attest-build-provenance@v2  # supply-chain attestation         |
|     - upload artifact                                                        |
|                                                                              |
| job: publish-github-release                                                  |
|   needs: [validate-tag, build]                                               |
|   - download all 4 tarballs + sha256s                                        |
|   - extract release notes from CHANGELOG.md (the v0.2.0 section)             |
|   - gh release create v0.2.0 ./modeltap-*.tar.gz ./*.sha256                  |
|     --title "v0.2.0" --notes-file ./RELEASE_NOTES.md                         |
|                                                                              |
| job: bump-tap-formula                                                        |
|   needs: publish-github-release                                              |
|   - checkout jeffabailey/homebrew-modeltap (via deploy key or PAT)           |
|   - render Formula/modeltap.rb from template using:                          |
|       version, mac-arm64-url + sha256, mac-x86-url + sha256,                 |
|       linux-x86-url + sha256, linux-arm64-url + sha256                       |
|   - open PR titled "modeltap 0.2.0" against the tap repo                     |
|   - auto-merge if CI in tap repo green (formula audit)                       |
+------------------------------------------------------------------------------+
```

**Shared artifacts:** `${binary-targets[]}` (single source: `release.yml` matrix), `${binary-archive-name}` = `modeltap-${version}-${target}.tar.gz` (single source: a build-step environment variable, never hand-typed), `${sha256[]}` per archive (single source: computed in the build job, written to `.sha256`, consumed by formula bump), `${release-url}` = `https://github.com/jeffabailey/modeltap/releases/download/${tag}/${archive}`.

**Emotional state:** entry "watch the matrix run" → exit "all green, release page exists." Visibility-of-system-status (Nielsen #1) is provided by `gh run watch` — every job has an explicit step name, no hidden failures.

**Integration checkpoint:** Every published archive's `sha256sum` MUST appear in both the GitHub release attachment AND the bumped Homebrew formula. A mismatch (e.g., one archive uploaded but its sha256 file forgotten) MUST fail the workflow. The `bump-tap-formula` job MUST NOT run if any build matrix cell failed (`needs: publish-github-release` enforces the chain).

---

### Step 4: Tap formula bumped (automated PR + auto-merge)

**Tap repo activity (`jeffabailey/homebrew-modeltap`):**

```
+- jeffabailey/homebrew-modeltap — PR #14 ----------------------------+
|                                                                      |
| modeltap 0.2.0                                          [open]       |
|   opened by github-actions[bot] 35 seconds ago                       |
|                                                                      |
| Files changed:                                                       |
|                                                                      |
|   Formula/modeltap.rb                                                |
|   --- a/Formula/modeltap.rb                                          |
|   +++ b/Formula/modeltap.rb                                          |
|   @@                                                                  |
|   -  version "0.1.0"                                                 |
|   +  version "0.2.0"                                                 |
|                                                                      |
|       on_macos do                                                    |
|         on_arm do                                                    |
|   -      url ".../v0.1.0/modeltap-0.1.0-aarch64-apple-darwin.tar.gz" |
|   -      sha256 "a1b2...c3d4"                                        |
|   +      url ".../v0.2.0/modeltap-0.2.0-aarch64-apple-darwin.tar.gz" |
|   +      sha256 "e5f6...7890"                                        |
|         end                                                          |
|         on_intel do                                                  |
|   -      url ".../v0.1.0/modeltap-0.1.0-x86_64-apple-darwin.tar.gz"  |
|   -      sha256 "1122...3344"                                        |
|   +      url ".../v0.2.0/modeltap-0.2.0-x86_64-apple-darwin.tar.gz"  |
|   +      sha256 "5566...7788"                                        |
|         end                                                          |
|       end                                                            |
|       # ... linux blocks similar                                     |
|                                                                      |
| CI: brew test-bot                                                    |
|   audit          ok                                                  |
|   install        ok (macos-14, macos-13, ubuntu-22.04)               |
|   test           ok ('modeltap --version' prints '0.2.0')            |
|                                                                      |
| Auto-merge enabled. Will merge when checks pass.                     |
+----------------------------------------------------------------------+
```

**Shared artifacts:** `${version}` (matches the application repo's tag exactly), `${release-url}[]` per target (built from `${tag}` + `${archive-name}` per target), `${sha256}[]` per target (read from the application repo's release artifacts).

**Emotional state:** entry "did the formula update?" → exit "PR opened, CI green, merged." Norman's principle of constraints: auto-merge fires only when the brew CI gate passes — never blind-merge a broken formula.

**Integration checkpoint:** The tap repo's `Formula/modeltap.rb` test stanza MUST run `modeltap --version` and assert it prints exactly `${version}` to stdout. This is the cross-repo end-to-end smoke that catches "uploaded the wrong binary" failures. If `brew test-bot` fails, auto-merge is disabled; the maintainer is notified via the PR thread.

---

### Step 5: End user installs and verifies

**End user terminal (Devon's clean Mac):**

```
$ brew tap jeffabailey/modeltap
==> Tapping jeffabailey/modeltap
Cloning into '/usr/local/Homebrew/Library/Taps/jeffabailey/homebrew-modeltap'...
Tapped 1 formula (14 files, 8.9 KB).

$ brew install modeltap
==> Fetching jeffabailey/modeltap/modeltap
==> Downloading https://github.com/jeffabailey/modeltap/releases/download/v0.2.0/modeltap-0.2.0-aarch64-apple-darwin.tar.gz
==> Pouring modeltap-0.2.0-aarch64-apple-darwin.tar.gz
  /opt/homebrew/Cellar/modeltap/0.2.0: 4 files, 12.3MB, built in 2 seconds

$ modeltap --version
modeltap 0.2.0

$ modeltap
# (TUI launches — see modeltap-tui journey)
```

**Shared artifacts:** `${tap-name}` = `jeffabailey/modeltap` (single source: README + tap repo name), `${install-command}` = `brew install jeffabailey/modeltap/modeltap` (single source: README install section, generated from `${tap-name}` + binary name), `${version}` (printed by `modeltap --version`, sourced from `clap` `#[command(version)]` which reads `CARGO_PKG_VERSION`).

**Emotional state:** entry "I hope this works on a fresh machine" → exit "huh, it just worked." Visibility-of-system-status: `modeltap --version` prints the expected version number, confirming the install matched the release page.

**Integration checkpoint:** `modeltap --version` MUST print `${version}` from `Cargo.toml`, NOT a hard-coded string. This is the cross-pipeline verification: if the maintainer's Cargo.toml said 0.2.0, the release tar said 0.2.0, the formula said 0.2.0, then the binary MUST also say 0.2.0. One source of truth, four consumers.

## Error Paths (acknowledged)

| Failure | UX response |
|---|---|
| Maintainer pushes a tag that does not match `Cargo.toml` version | `validate-tag` job fails before any build runs; maintainer must delete the tag, fix `Cargo.toml`, retag |
| One target in the build matrix fails (e.g., aarch64-linux cross-compile breaks) | The whole release is held; `publish-github-release` does not run; maintainer fixes and retags (release process is atomic per tag) |
| GitHub release upload succeeds but tap-bump fails (network, auth) | GitHub release exists with binaries; tap PR fails to open; maintainer is notified; manual retry of just `bump-tap-formula` job is possible (idempotent) |
| Formula `brew test-bot` fails (binary missing dynamic lib, wrong arch) | Auto-merge does not fire; PR sits open; maintainer investigates via PR comments; common fix is to rebuild with the correct toolchain or add a `depends_on` |
| End user runs `brew install` during the 1-2 minute window where the GH release exists but the tap PR has not merged | `brew install` fails with "formula not found at this version"; documented as expected; tap-bump completes within minutes; user retries |
| macOS Gatekeeper blocks the unsigned binary on first launch | Documented in README troubleshooting: `xattr -dr com.apple.quarantine $(which modeltap)`; tracked as a future work item to add notarization |
| Maintainer needs to yank a release | `gh release delete v0.2.0`, revert tap PR, document in CHANGELOG; no automated yank in v1 |

## CLI / Workflow vocabulary (consistency check)

| Concept | Term used | Never call it |
|---|---|---|
| The thing the maintainer pushes that triggers everything | "tag" | "version", "release tag" |
| The published artifact set | "release" (matches `gh release`) | "build", "drop", "package" |
| The tar.gz binary archive | "archive" | "tarball", "bundle", "package" |
| The Homebrew formula PR | "tap-bump PR" | "formula PR", "homebrew PR" |
| The `Cargo.toml` workspace version | "workspace version" | "crate version", "core version" |
| The single source of version truth | "Cargo.toml `[workspace.package].version`" | various |
| The Homebrew tap repo | "the tap" / "the tap repo" | "the brew repo", "the formula repo" |

## Open Decisions (flagged for resolution before DESIGN)

| ID | Decision | Default proposed | Why surface it |
|---|---|---|---|
| D1 | Tap repo location | `jeffabailey/homebrew-modeltap` (personal namespace) for v1; revisit if a `modeltap` GitHub org is created later | A future org migration would change `${tap-name}` (a HIGH-risk shared artifact). Defer until contributor count justifies an org. |
| D2 | Release-cut trigger | Manual `git tag` push for v1 (simplest, least magic). Defer `release-please` / `cargo-release` to a later iteration. | Fully automated PR-driven releases reduce maintainer toil but add tooling-debugging burden up front. Deliberate manual cut for v1 keeps the failure surface small. |
| D3 | macOS code signing / notarization | **Skip for v1.** Document `xattr -dr com.apple.quarantine` workaround in README. Track notarization as a follow-up feature. | Notarization requires an Apple Developer account + secrets in CI; out of scope for the first release-process feature. |
| D4 | Binaries shipped in v1 | Just `modeltap` (the TUI binary from `modeltap-app`). `modeltap-cli` is listed in `CLAUDE.md` as a future crate; ship it through this same pipeline if/when it exists. | Avoids designing a multi-binary formula now for a crate that may not ship for months. |
| D5 | Changelog generation | `git-cliff` driven by conventional-commit prefixes (`feat:`, `fix:`, `chore:`, `refactor:`). Hand-curated CHANGELOG is rejected (drifts from intent). | Requires the repo to adopt conventional commits — which the recent commit history (`fix(ci):`, `chore(docs):`, `refactor(gpt4all):`) already follows. |
| D6 | SLSA / build provenance | Use `actions/attest-build-provenance@v2` for SLSA Level 3 attestation on each archive. Adds ~30s per build, no maintainer-visible work. | Free supply-chain hygiene win; signals seriousness for an OSS tool that asks users to download binaries. |

These will be batched into a single AskUserQuestion to the maintainer if any disagree with the proposed defaults. Otherwise, all defaults are taken and DESIGN proceeds.
