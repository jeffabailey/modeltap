# =============================================================================
# release-process-homebrew-github — Integration Checkpoints
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-03
#
# Covers cross-story invariants INT.AC-1..INT.AC-6 from acceptance-criteria.md.
# These scenarios test horizontal integration: the same value appearing in
# multiple consumers must agree.
# =============================================================================

Feature: Cross-Story Integration Invariants
  As Jeff Bailey, the modeltap maintainer
  I want every shared artifact (version, sha256, release URL, atomic guarantees)
  to remain consistent across every producer and consumer in the pipeline
  So that no release ever lies about its identity, no archive ever fails its checksum,
  and no half-published state ever reaches a user

  Background:
    Given a clean tempdir workspace for the scenario
    And a fake modeltap repository seeded at "${TMPDIR}/modeltap-fake"
    And a fake Homebrew tap repository seeded at "${TMPDIR}/tap-fake"

  # ===========================================================================
  # INT.AC-1 — Version is the same string in every consumer
  # ===========================================================================

  @integration @int-ac-1 @real-io
  Scenario: Version string agrees across Cargo.toml, tag, archive name, release title, and binary output
    Given the workspace version in Cargo.toml is "0.2.0"
    And the maintainer pushes the tag "v0.2.0"
    And the build matrix produces archives named "modeltap-0.2.0-<target>.tar.gz" for the 4 targets
    And the GitHub Release titled "v0.2.0" is published with those archives
    And the formula is rendered with version "0.2.0"
    When the binary at "modeltap-0.2.0-x86_64-unknown-linux-gnu.tar.gz" is extracted and run with "--version"
    Then every consumer reads or produces the version string "0.2.0"
    And no consumer reads a different version string

  @integration @int-ac-1 @property
  Scenario: Version-string consistency holds for any valid semver release
    Given any valid semver version "X.Y.Z" or pre-release "X.Y.Z-suffix" recorded in Cargo.toml
    When the entire pipeline runs through to a published binary
    Then the maintainer's tag, the archive name, the release title, the formula version field, and the binary --version output all reduce to the same version string

  # ===========================================================================
  # INT.AC-2 — sha256 in formula equals sha256 in artifact for every target
  # ===========================================================================

  @integration @int-ac-2 @real-io
  Scenario: Each target's formula sha256 equals the artifact sidecar content
    Given the build matrix has produced archives plus sha256 sidecars for the 4 targets
    When the formula is rendered using those sidecars
    Then for each target, the sha256 field in the formula equals the bare-hex content of the sidecar file
    And no formula sha256 was computed by rehashing the archive

  # ===========================================================================
  # INT.AC-3 — Release URL in formula equals GitHub Release URL for every target
  # ===========================================================================

  @integration @int-ac-3 @real-io
  Scenario: Each target's formula URL equals the GitHub Release archive URL
    Given the GitHub Release for tag "v0.2.0" has been published with archives for the 4 targets
    When the formula is rendered with release-base-url "https://github.com/jeffabailey/modeltap/releases/download/v0.2.0"
    Then for each target, the url field in the formula starts with the release-base-url
    And for each target, the url field ends with the archive name "modeltap-0.2.0-<target>.tar.gz"

  # ===========================================================================
  # INT.AC-4 — End-to-end SLA: tag-to-install ≤ 15 minutes (median)
  # ===========================================================================

  @integration @int-ac-4 @requires_external
  Scenario: From tag push to clean machine install in 15 minutes (median)
    Given Cargo.toml workspace version on main is "0.2.0"
    And a clean Ubuntu 22.04 x86_64 machine with Homebrew on Linux installed
    When the maintainer pushes the tag "v0.2.0" at time T
    Then the GitHub Release exists by time T plus 12 minutes (median)
    And the tap-bump PR has merged by time T plus 14 minutes (median)
    And running "brew install jeffabailey/modeltap/modeltap" on the clean machine succeeds by time T plus 15 minutes

  # ===========================================================================
  # INT.AC-5 — Atomic publish: all-or-nothing visible effects
  # ===========================================================================

  @integration @int-ac-5
  Scenario: All four build cells succeeding produces all visible effects
    Given the build matrix has run for tag "v0.2.0" with all 4 cells succeeding
    When the workflow continues
    Then a GitHub Release titled "v0.2.0" is created
    And 4 archives are attached to the release
    And a tap-bump PR is opened against the tap repository

  @integration @int-ac-5
  Scenario: Any build cell failing produces no visible effects
    Given the build matrix has run for tag "v0.2.0" with at least one cell failing
    When the workflow continues
    Then no GitHub Release titled "v0.2.0" is created
    And no tap-bump PR is opened against the tap repository
    And the maintainer is notified via the workflow run conclusion "failure"

  # ===========================================================================
  # INT.AC-6 — release.yml runs the same CI parity gates as ci.yml
  # ===========================================================================

  @integration @int-ac-6
  Scenario: release.yml CI parity gates use the exact same flags as ci.yml
    Given the existing CI workflow at ".github/workflows/ci.yml"
    And the release workflow at ".github/workflows/release.yml"
    When both files are parsed
    Then both files use the action "dtolnay/rust-toolchain@stable"
    And both files invoke "cargo fmt --all -- --check"
    And both files invoke "cargo clippy --workspace --all-targets -- -D warnings"
    And both files invoke "cargo test --workspace --locked"
    And no flag differs between the two files for the three parity gates

  @integration @int-ac-6
  Scenario: release.yml runs CI parity gates before any cargo build release step
    Given the release workflow at ".github/workflows/release.yml"
    When the build job's step ordering is examined
    Then "cargo fmt --all -- --check" appears before any "cargo build --release" step
    And "cargo clippy --workspace --all-targets -- -D warnings" appears before any "cargo build --release" step
    And "cargo test --workspace --locked" appears before any "cargo build --release" step

  # ===========================================================================
  # Recovery scenarios (cross-cutting)
  # ===========================================================================

  @integration @recovery
  Scenario: GitHub Release succeeds but tap-bump fails leaves an intact release
    Given the GitHub Release for "v0.2.0" has been published with all 4 archives
    When the bump-tap-formula step fails because GH_TAP_TOKEN has expired
    Then the GitHub Release for "v0.2.0" remains intact
    And the maintainer can re-run only the bump-tap-formula step after rotating the token
    And the re-run produces the same end state with no duplicate PR

  @integration @recovery
  Scenario: Maintainer yanks a release after a critical defect is found
    Given v0.2.0 has shipped end-to-end and is installed by users
    And a critical defect has been discovered post-release
    When the maintainer deletes the GitHub Release and the tag
    And the maintainer reverts the tap-bump PR in the tap repository
    Then "brew install jeffabailey/modeltap/modeltap" resolves to the previously released version
    And the CHANGELOG.md "## [0.2.0]" section is annotated "(yanked)"
    And the next release "v0.2.1" follows the standard process

  # ===========================================================================
  # Follow-up workflows (DEVOPS handoff items #1 and #3)
  # Added 2026-05-03 to close roadmap reviewer blocker D1.
  # ===========================================================================

  @integration @us-14 @follow-up-workflow
  Scenario: Release-pipeline-alert opens issue on workflow failure
    Given the release-pipeline-alert workflow at ".github/workflows/release-pipeline-alert.yml"
    And it is configured to trigger on workflow_run completion of "Release"
    When the Release workflow concludes with conclusion "failure"
    Then release-pipeline-alert opens a GitHub issue
    And the issue title contains "release-pipeline-failure"
    And the issue body contains a link to the failed workflow run
    And the issue is labeled "release-pipeline-failure"

  @integration @us-14 @follow-up-workflow
  Scenario: Release-pipeline-alert stays silent on workflow success
    Given the release-pipeline-alert workflow is configured per the prior scenario
    When the Release workflow concludes with conclusion "success"
    Then no new "release-pipeline-failure" issue is opened

  @integration @us-14 @follow-up-workflow
  Scenario: Token-expiry-warning opens issue when GH_TAP_TOKEN is expired
    Given the token-expiry-warning workflow at ".github/workflows/token-expiry-warning.yml"
    And it is configured to run weekly on schedule and on workflow_dispatch
    When the workflow probes the tap repo via "gh api /repos/jeffabailey/homebrew-modeltap"
    And the probe returns HTTP 401 indicating an expired or revoked token
    Then token-expiry-warning opens a GitHub issue titled "GH_TAP_TOKEN expired or invalid"
    And the issue body links to the rotation procedure in RELEASING.md

  @integration @us-14 @follow-up-workflow
  Scenario: Token-expiry-warning stays silent when token is healthy
    Given the token-expiry-warning workflow is configured per the prior scenario
    When the workflow probes the tap repo and the probe returns HTTP 200
    Then no new "GH_TAP_TOKEN expired" issue is opened

  @integration @us-14 @follow-up-workflow
  Scenario: Both follow-up workflows pass xtask lint-workflows
    Given the follow-up workflow files exist at the paths in the prior scenarios
    When "cargo xtask lint-workflows" is run against ".github/workflows/"
    Then lint-workflows reports "OK" for release-pipeline-alert.yml
    And lint-workflows reports "OK" for token-expiry-warning.yml
    And every job in both workflows has a "# Purpose:" comment
    And both files are within the per-workflow line budget
