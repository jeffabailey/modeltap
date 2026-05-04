# =============================================================================
# release-process-homebrew-github — Walking Skeleton Feature
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-03
# WS Strategy: C — Real local resources (DWD-01)
#
# Tag glossary (specific to this feature):
#   @walking_skeleton  -- WS exit gate; DELIVER ships when these pass green
#   @us-NN             -- traceability to user-stories.md story ID (US-01..US-15)
#   @real-io           -- uses real filesystem / real git / real subprocess
#   @adapter-integration -- proves a single driven adapter against real I/O
#   @cross-repo        -- exercises modeltap-fake ↔ tap-fake seam
#   @requires_external -- needs live GitHub API; skipped in default CI
#   @requires_docker   -- needs Docker daemon (cross-compile, brew test-bot)
#   @infrastructure-failure -- driven-adapter failure scenario
#   @in-memory         -- uses in-memory test double (forbidden in WS scenarios)
#   @property          -- universal invariant; DELIVER may implement as proptest
#   @slow              -- exercises real cargo build/test; minutes not seconds
#
# Walking-skeleton exit gate (6 user-value scenarios, one per backbone activity):
#   1. PREP        -- "Maintainer prepares a release with one command"
#   2. TAG         -- "Maintainer pushes matching tag and validation accepts it"
#   3. BUILD       -- "Build job runs CI parity gates before any artifact"
#   4. PUBLISH     -- "Release notes are extracted from the changelog section"
#   5. TAP-BUMP    -- "Tap-bump opens a PR against the ephemeral tap repo"
#   6. USER-INSTALL -- "Devon installs and verifies the version" (@requires_external smoke)
# =============================================================================

Feature: Tag-to-Brew-Install Walking Skeleton
  As Jeff Bailey, the modeltap maintainer
  I want a single tag push to produce a downloadable release that lands in Homebrew
  So that Devon Park can `brew install` modeltap and run it on a clean machine

  Background:
    Given a clean tempdir workspace for the scenario
    And a fake modeltap repository seeded at "${TMPDIR}/modeltap-fake" with conventional commit history
    And a fake Homebrew tap repository seeded at "${TMPDIR}/tap-fake" reachable via "file://${TMPDIR}/tap-fake"

  # ===========================================================================
  # 1. PREP — Maintainer prepares the release with one command  (US-01)
  # ===========================================================================

  @walking_skeleton @us-01 @real-io @adapter-integration
  Scenario: Maintainer prepares a release with one command
    Given the workspace version in Cargo.toml is "0.1.0"
    And there are 17 conventional commits since the v0.1.0 tag
    When the maintainer runs "cargo xtask release-prep --version 0.0.1-rc1"
    Then Cargo.toml workspace.package.version becomes "0.0.1-rc1"
    And CHANGELOG.md gains a new "## [0.0.1-rc1]" section grouping commits by type
    And the script exits zero with instructions to commit, push, and open a PR

  @us-01 @real-io @adapter-integration
  Scenario: Release-prep refuses on a dirty working tree
    Given the workspace has an uncommitted local change
    When the maintainer runs "cargo xtask release-prep --version 0.0.1-rc1"
    Then the script exits non-zero
    And the message says "working tree is dirty: commit or stash first"
    And no files in the workspace are modified

  @us-01
  Scenario: Release-prep refuses a non-monotonic version bump
    Given the workspace version in Cargo.toml is "0.2.0"
    When the maintainer runs "cargo xtask release-prep --version 0.1.5"
    Then the script exits non-zero
    And the message says "proposed version 0.1.5 is not greater than current 0.2.0"

  @us-01 @real-io @adapter-integration @slow
  Scenario: Release-prep runs CI parity gates locally and exits zero on success
    Given the workspace version in Cargo.toml is "0.1.0"
    And the workspace passes formatting, linting, and tests
    When the maintainer runs "cargo xtask release-prep --version 0.2.0"
    Then formatting, linting, and tests all pass before the script exits
    And the script exits zero

  @us-01 @infrastructure-failure
  Scenario: Release-prep halts when a CI parity gate fails
    Given the workspace version in Cargo.toml is "0.1.0"
    And the workspace contains a linting warning
    When the maintainer runs "cargo xtask release-prep --version 0.2.0"
    Then the script exits non-zero after the linting step
    And no version bump is committed
    And the message identifies which gate failed

  # ===========================================================================
  # 2. TAG — Validate-tag accepts matching tag, rejects mismatched  (US-02)
  # ===========================================================================

  @walking_skeleton @us-02 @real-io @adapter-integration
  Scenario: Validate-tag accepts a tag that matches the workspace version
    Given the workspace version in Cargo.toml is "0.0.1-rc1"
    When the maintainer runs "cargo xtask validate-tag --tag v0.0.1-rc1"
    Then the script exits zero
    And no error message is printed

  @us-02 @real-io @adapter-integration
  Scenario: Validate-tag rejects a tag that does not match the workspace version
    Given the workspace version in Cargo.toml is "0.1.0"
    When the maintainer runs "cargo xtask validate-tag --tag v0.2.0"
    Then the script exits non-zero
    And the message says "tag v0.2.0 does not match workspace version 0.1.0"

  @us-02
  Scenario: Validate-tag rejects a tag missing the leading v prefix
    Given the workspace version in Cargo.toml is "0.1.0"
    When the maintainer runs "cargo xtask validate-tag --tag 0.1.0"
    Then the script exits non-zero
    And the message identifies that the tag is missing the "v" prefix

  @us-02 @property
  Scenario Outline: Validate-tag enforces tag-equals-v-plus-version invariant
    Given the workspace version in Cargo.toml is "<version>"
    When the maintainer runs "cargo xtask validate-tag --tag <tag>"
    Then the script exit code is "<result>"

    Examples:
      | version    | tag         | result   |
      | 0.1.0      | v0.1.0      | zero     |
      | 0.1.0      | v0.1.1      | non-zero |
      | 0.0.1-rc1  | v0.0.1-rc1  | zero     |
      | 0.0.1-rc1  | v0.0.1      | non-zero |
      | 1.0.0      | v1.0.0      | zero     |

  # ===========================================================================
  # 3. BUILD — CI parity gates run before any artifact  (US-03, US-04)
  # ===========================================================================

  @walking_skeleton @us-03 @us-04 @real-io
  Scenario: Build orchestration runs formatting, linting, and tests before packaging
    Given the workspace version in Cargo.toml is "0.0.1-rc1"
    And the validate-tag step has passed for tag "v0.0.1-rc1"
    When the build orchestration runs for target "x86_64-unknown-linux-gnu"
    Then formatting, linting, and tests all pass before any release artifact is built
    And only after all gates pass is the release binary produced

  @us-04 @real-io @adapter-integration
  Scenario: Single-target archive is produced and named correctly
    Given the workspace version in Cargo.toml is "0.0.1-rc1"
    And the build orchestration has produced a stripped binary for target "x86_64-unknown-linux-gnu"
    When the packaging step runs
    Then an archive named "modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz" is created
    And the archive contains exactly one file named "modeltap"
    And a sidecar file "modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz.sha256" contains the bare-hex sha256 of the archive

  @us-04 @property
  Scenario: Archive sha256 sidecar always contains a valid 64-character lowercase hex digest
    Given any release archive produced by the build packaging step
    When the sidecar file is read
    Then its content is exactly 64 lowercase hex characters
    And the content matches the actual sha256 of the archive

  # ===========================================================================
  # 4. PUBLISH — Release notes from CHANGELOG section  (US-05)
  # ===========================================================================

  @walking_skeleton @us-05 @real-io @adapter-integration
  Scenario: Release notes are extracted from the matching changelog section
    Given a CHANGELOG.md file containing sections "## [0.1.0]" and "## [0.0.1-rc1]"
    And the "## [0.0.1-rc1]" section says "Walking-skeleton release-candidate."
    When the maintainer runs "cargo xtask extract-changelog --version 0.0.1-rc1 --input CHANGELOG.md --output RELEASE_NOTES.md"
    Then RELEASE_NOTES.md exists
    And its content equals the body of the "## [0.0.1-rc1]" section
    And no content from other sections leaks in

  @us-05 @infrastructure-failure
  Scenario: Missing changelog section fails the publish step with a clear message
    Given a CHANGELOG.md file containing only a "## [0.1.0]" section
    When the maintainer runs "cargo xtask extract-changelog --version 0.2.0 --input CHANGELOG.md --output RELEASE_NOTES.md"
    Then the script exits non-zero
    And the message says "CHANGELOG.md has no [0.2.0] section"
    And no RELEASE_NOTES.md file is written

  @us-05 @requires_external @cross-repo
  Scenario: Publish step shells out to gh release create with all archives, sha256s, and notes
    Given a fixture artifact directory containing one archive, one sha256 sidecar, and a RELEASE_NOTES.md
    And a live GitHub repository the maintainer has write access to
    When the publish step is invoked for tag "v0.0.1-rc1"
    Then a GitHub Release titled "v0.0.1-rc1" exists
    And the archive and the sha256 sidecar are attached to the release
    And the release body equals the RELEASE_NOTES.md content
    And the release is marked as a pre-release because the tag contains a hyphen

  # ===========================================================================
  # 5. TAP-BUMP — render-formula + open PR against ephemeral tap  (US-06)
  # ===========================================================================

  @walking_skeleton @us-06 @real-io @adapter-integration @cross-repo
  Scenario: Render-formula produces a single-platform formula for the walking skeleton
    Given the formula template at "release/templates/modeltap.rb.tera"
    And a sha256 sidecar file for target "x86_64-unknown-linux-gnu" with content "e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    When the maintainer runs "cargo xtask render-formula --version 0.0.1-rc1 --template release/templates/modeltap.rb.tera --output ${TMPDIR}/Formula/modeltap.rb --sha256-dir ${TMPDIR}/artifacts --release-base-url https://github.com/jeffabailey/modeltap/releases/download/v0.0.1-rc1"
    Then "${TMPDIR}/Formula/modeltap.rb" is created
    And the formula contains a "version" field equal to "0.0.1-rc1"
    And the formula contains the on_linux on_intel block with url ending in "modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz"
    And the formula contains the sha256 "e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    And no other platform blocks are populated

  @walking_skeleton @us-06 @real-io @adapter-integration @cross-repo
  Scenario: Bump-tap-formula opens a PR against the ephemeral tap repository
    Given the GitHub Release for "v0.0.1-rc1" exists with one archive
    And the rendered formula has been written into the tap-repo working tree
    When the bump-tap-formula step commits and pushes the bump branch to "file://${TMPDIR}/tap-fake"
    Then a branch "bump/v0.0.1-rc1" exists in the tap repository
    And the branch's HEAD commit contains the rendered "Formula/modeltap.rb"
    And the commit message is "modeltap 0.0.1-rc1"

  @us-06 @infrastructure-failure
  Scenario: Tap-bump step surfaces token failure visibly
    Given the bump-tap-formula step has been configured with an invalid tap-bump-token
    When the bump-tap-formula step attempts to push to the tap repository
    Then the step exits non-zero
    And the error output identifies an authentication failure
    And the step does not silently succeed

  @us-06 @requires_external @cross-repo
  Scenario: Bump-tap-formula opens a real PR titled correctly against the live tap repo
    Given the live tap repository at "jeffabailey/homebrew-modeltap" is reachable
    And a valid GH_TAP_TOKEN is configured
    When the bump-tap-formula step runs for tag "v0.0.1-rc1"
    Then a pull request titled "modeltap 0.0.1-rc1" is open against the tap repository's main branch

  # ===========================================================================
  # 6. USER-INSTALL — Devon installs and verifies version  (US-15)
  # ===========================================================================

  @walking_skeleton @us-15 @requires_external
  Scenario: Devon installs modeltap on a clean Linux machine and verifies the version
    Given a clean Ubuntu 22.04 x86_64 environment with Homebrew on Linux installed
    And the v0.0.1-rc1 tap-bump PR has merged into the tap repository
    When Devon runs "brew install jeffabailey/modeltap/modeltap"
    Then the install completes successfully
    And running "modeltap --version" prints exactly "modeltap 0.0.1-rc1"
