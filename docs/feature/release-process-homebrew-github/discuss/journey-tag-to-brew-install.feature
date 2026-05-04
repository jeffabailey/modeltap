Feature: Tag a Release, Land in Homebrew
  As Jeff Bailey, the modeltap maintainer
  I want a single `git push origin v0.x.0` to produce signed multi-arch release
  archives, publish a GitHub Release with notes, open an auto-merging PR
  against the Homebrew tap with refreshed sha256s, and let any clean macOS
  or Linux machine `brew install jeffabailey/modeltap/modeltap` and run
  `modeltap` within fifteen minutes
  So that release cuts feel boring, end users get a turnkey install,
  and contributors can read the workflow file and trust it

  Background:
    Given the repository is github.com/jeffabailey/modeltap
    And the Homebrew tap repository is github.com/jeffabailey/homebrew-modeltap
    And the workspace is a Rust 2021 workspace with stable rustc as the target toolchain
    And conventional commits are used (feat:, fix:, chore:, refactor:, docs:)
    And the existing CI workflow runs cargo fmt, clippy, test, deny, and the K3 smoke

  # ---------------------------------------------------------------
  # Step 1 — Maintainer prep
  # ---------------------------------------------------------------

  Scenario: Maintainer prepares a release with one command
    Given the workspace version in Cargo.toml is 0.1.0
    And there are 17 conventional commits since the v0.1.0 tag
    When the maintainer runs "cargo xtask release-prep --version 0.2.0"
    Then Cargo.toml [workspace.package].version becomes 0.2.0
    And Cargo.lock is updated to match
    And CHANGELOG.md gains a new "## [0.2.0]" section
    And the section groups commits under Features, Fixes, Refactor, and Chore headings
    And cargo fmt, clippy, and test all pass locally before the script exits
    And the script exits zero with instructions to commit, push, and open a PR

  Scenario: Release-prep refuses to bump if the working tree is dirty
    Given the workspace has uncommitted local changes
    When the maintainer runs "cargo xtask release-prep --version 0.2.0"
    Then the script exits non-zero
    And the message says "working tree is dirty: commit or stash first"
    And no files are modified

  Scenario: Release-prep refuses if the proposed version is not greater than current
    Given the workspace version in Cargo.toml is 0.2.0
    When the maintainer runs "cargo xtask release-prep --version 0.1.5"
    Then the script exits non-zero
    And the message says "proposed version 0.1.5 is not greater than current 0.2.0"

  # ---------------------------------------------------------------
  # Step 2 — Push the tag
  # ---------------------------------------------------------------

  Scenario: Pushing a matching tag triggers the release workflow
    Given the prep PR has merged and Cargo.toml workspace version on main is 0.2.0
    When the maintainer pushes the annotated tag "v0.2.0" to origin
    Then the release workflow starts within 30 seconds
    And the validate-tag job confirms the tag matches "v" plus the workspace version

  Scenario: Pushing a tag that does not match Cargo.toml fails fast
    Given Cargo.toml workspace version on main is 0.1.0
    When the maintainer mistakenly pushes the annotated tag "v0.2.0"
    Then the validate-tag job fails before any build job starts
    And the workflow conclusion is "failure"
    And the failure message says "tag v0.2.0 does not match workspace version 0.1.0"

  Scenario: Pushing a non-version tag does not trigger the release workflow
    Given main is at any state
    When the maintainer pushes a tag "experiment-foo"
    Then the release workflow does not run
    Because the trigger filter is "v*.*.*"

  # ---------------------------------------------------------------
  # Step 3 — Build, sign, publish
  # ---------------------------------------------------------------

  Scenario: All four targets build and publish atomically
    Given the validate-tag job passed for tag v0.2.0
    When the build matrix runs for aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, and aarch64-unknown-linux-gnu
    Then each target produces a stripped release binary
    And each target packages its binary into modeltap-0.2.0-<target>.tar.gz
    And each archive has a corresponding modeltap-0.2.0-<target>.tar.gz.sha256 file
    And each archive carries an actions/attest-build-provenance@v2 attestation
    And the publish-github-release job uploads all 4 archives plus 4 .sha256 files
    And the GitHub Release notes equal the CHANGELOG.md "[0.2.0]" section content

  Scenario: CI parity gates run inside release.yml before any artifact is built
    Given the validate-tag job passed
    When any build matrix cell starts
    Then it runs "cargo fmt --all -- --check" and fails the job on diff
    And it runs "cargo clippy --workspace --all-targets -- -D warnings" and fails on warning
    And it runs "cargo test --workspace --locked" and fails on any test failure
    And only after all three gates pass does the cargo build --release step run

  Scenario: A single failing target halts the release atomically
    Given the validate-tag job passed
    When the aarch64-unknown-linux-gnu build job fails (e.g., cross-compile error)
    Then publish-github-release does not run
    And bump-tap-formula does not run
    And no GitHub Release for v0.2.0 is created
    And no PR is opened against the tap repository
    And the maintainer is notified via the workflow run conclusion "failure"

  Scenario: SLSA build provenance attestations are attached to every archive
    Given the build matrix completed successfully
    When the publish-github-release job uploads archives
    Then each archive's actions/attest-build-provenance@v2 attestation is attached
    And the attestation can be verified with "gh attestation verify <archive>"

  # ---------------------------------------------------------------
  # Step 4 — Tap formula bump
  # ---------------------------------------------------------------

  Scenario: Tap formula updates automatically and passes brew test-bot
    Given the GitHub Release for v0.2.0 published successfully with all 4 archives and sha256s
    When the bump-tap-formula job runs
    Then a PR titled "modeltap 0.2.0" opens against jeffabailey/homebrew-modeltap
    And Formula/modeltap.rb references the four new release URLs
    And Formula/modeltap.rb references the four new sha256 values, read from the .sha256 artifact files
    And the formula's test stanza runs "modeltap --version" and asserts output includes "0.2.0"
    And brew test-bot's audit, install (macos-14, macos-13, ubuntu-22.04), and test stanzas all pass
    And auto-merge merges the PR within 5 minutes of the tag push

  Scenario: Auto-merge withholds when brew test-bot fails
    Given the bump-tap-formula PR was opened
    When brew test-bot's install step fails on macos-14
    Then auto-merge does not fire
    And the PR stays open for maintainer review
    And the maintainer is notified via the PR thread

  Scenario: Sha256 values in the formula are read from artifact files, not recomputed
    Given the build matrix wrote modeltap-0.2.0-<target>.tar.gz.sha256 alongside each archive
    When the bump-tap-formula job constructs Formula/modeltap.rb
    Then each sha256 value in the formula equals the contents of the corresponding .sha256 artifact file
    And no sha256 is computed by reading or rehashing the archive in the bump job

  # ---------------------------------------------------------------
  # Step 5 — End-user install and verify
  # ---------------------------------------------------------------

  Scenario: Clean macOS Apple Silicon machine installs modeltap
    Given a clean macOS Sonoma (Apple Silicon) machine with Homebrew installed
    And the v0.2.0 tap-bump PR has merged
    When Devon runs "brew install jeffabailey/modeltap/modeltap"
    Then brew downloads modeltap-0.2.0-aarch64-apple-darwin.tar.gz
    And the install completes within 30 seconds (excluding network)
    And "modeltap --version" prints exactly "modeltap 0.2.0"

  Scenario: Clean macOS Intel machine installs modeltap
    Given a clean macOS Ventura (Intel) machine with Homebrew installed
    And the v0.2.0 tap-bump PR has merged
    When Devon runs "brew install jeffabailey/modeltap/modeltap"
    Then brew downloads modeltap-0.2.0-x86_64-apple-darwin.tar.gz
    And "modeltap --version" prints exactly "modeltap 0.2.0"

  Scenario: Clean Linux x86_64 machine installs modeltap
    Given a clean Ubuntu 22.04 x86_64 machine with Homebrew on Linux installed
    And the v0.2.0 tap-bump PR has merged
    When Devon runs "brew install jeffabailey/modeltap/modeltap"
    Then brew downloads modeltap-0.2.0-x86_64-unknown-linux-gnu.tar.gz
    And "modeltap --version" prints exactly "modeltap 0.2.0"

  Scenario: Clean Linux aarch64 machine (e.g., Raspberry Pi 5 / AWS Graviton) installs modeltap
    Given a clean Ubuntu 22.04 aarch64 machine with Homebrew on Linux installed
    And the v0.2.0 tap-bump PR has merged
    When Devon runs "brew install jeffabailey/modeltap/modeltap"
    Then brew downloads modeltap-0.2.0-aarch64-unknown-linux-gnu.tar.gz
    And "modeltap --version" prints exactly "modeltap 0.2.0"

  Scenario: Install attempted during the tap-update window is informative
    Given the GitHub Release for v0.2.0 has published
    And the tap-bump PR has not yet merged
    When Devon runs "brew install jeffabailey/modeltap/modeltap"
    Then brew installs the previously-released v0.1.0 (the formula has not yet been bumped)
    And Devon may run "brew upgrade modeltap" after the tap-bump PR merges to get v0.2.0

  # ---------------------------------------------------------------
  # End-to-end SLA scenario
  # ---------------------------------------------------------------

  Scenario: From tag push to user install in under fifteen minutes (median)
    Given Cargo.toml workspace version on main is 0.2.0
    When the maintainer pushes the tag "v0.2.0" at time T
    Then the GitHub Release exists by time T plus 12 minutes (median)
    And the tap-bump PR has merged by time T plus 14 minutes (median)
    And a clean machine running "brew install jeffabailey/modeltap/modeltap" by time T plus 15 minutes succeeds

  # ---------------------------------------------------------------
  # Contributor walkthrough scenario (Riley)
  # ---------------------------------------------------------------

  Scenario: A contributor reads the release workflow and understands it end-to-end
    Given Riley clones the modeltap repository
    When Riley opens .github/workflows/release.yml
    Then the file is under 250 lines including comments
    And every job has a one-sentence comment describing its purpose
    And the four jobs (validate-tag, build, publish-github-release, bump-tap-formula) appear in linear order
    And the maintainer release runbook (RELEASING.md) walks through the full cycle in under 10 numbered steps

  # ---------------------------------------------------------------
  # Failure / recovery scenarios
  # ---------------------------------------------------------------

  Scenario: GitHub release upload succeeds but tap bump fails (network or auth)
    Given publish-github-release succeeded and all 4 archives are visible
    When bump-tap-formula fails (e.g., GH_TAP_TOKEN expired)
    Then the GitHub Release remains intact
    And the maintainer can re-run only the bump-tap-formula job after rotating the token
    And the rerun is idempotent (existing PR is updated, not duplicated)

  Scenario: Maintainer needs to yank a release
    Given v0.2.0 has shipped end-to-end
    And a critical defect is discovered post-release
    When the maintainer runs "gh release delete v0.2.0 --cleanup-tag"
    And the maintainer reverts the tap-bump PR in jeffabailey/homebrew-modeltap
    Then "brew install jeffabailey/modeltap/modeltap" resolves to v0.1.0
    And the CHANGELOG.md "[0.2.0]" section is annotated "(yanked)"
    And the next release (v0.2.1) follows the standard process

  Scenario: Future Apple notarization can be added without redesigning the pipeline
    Given v0.x.0 ships unsigned in v1 of this feature
    When a future feature adds Apple Developer ID notarization
    Then the existing build job gains a "notarize" step before "tar.gz"
    And no other job changes
    And no shared artifact name changes
