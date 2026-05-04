# =============================================================================
# release-process-homebrew-github — Multi-Arch Release Slice
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-03
#
# Stories covered: US-07, US-08, US-09, US-10
# Slice: Release 1 — "Multi-arch real release"
# =============================================================================

Feature: Multi-Architecture Release with Atomic Publish and SLSA Provenance
  As Jeff Bailey, the modeltap maintainer
  I want every release to ship signed archives for all four supported platforms
  So that Devon Park can install modeltap on macOS or Linux, ARM or Intel,
  and verify the binary was built by the official workflow

  Background:
    Given a clean tempdir workspace for the scenario
    And a fake modeltap repository seeded at "${TMPDIR}/modeltap-fake"
    And a fake Homebrew tap repository seeded at "${TMPDIR}/tap-fake"

  # ===========================================================================
  # US-07 — 4-target build matrix
  # ===========================================================================

  @release-1 @us-07
  Scenario: Build matrix declares all four supported targets with correct runners
    Given the release workflow file at ".github/workflows/release.yml"
    When the workflow definition is parsed
    Then the build job declares a matrix of exactly 4 targets
    And the targets include "aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"
    And "aarch64-apple-darwin" runs on "macos-14"
    And "x86_64-apple-darwin" runs on "macos-13"
    And both linux targets run on "ubuntu-22.04"
    And the matrix uses "fail-fast: false"

  @release-1 @us-07 @requires_docker
  Scenario: aarch64-linux cell cross-compiles successfully via cross
    Given Docker is available in the test environment
    And the build orchestration has been configured for target "aarch64-unknown-linux-gnu"
    When the build cell runs the cross-compile step
    Then a binary at "target/aarch64-unknown-linux-gnu/release/modeltap" is produced
    And the binary's reported architecture is aarch64

  @release-1 @us-07
  Scenario: Each build cell uploads a workflow artifact named by target
    Given the build matrix has run for all 4 targets
    When the upload-artifact steps complete
    Then 4 workflow artifacts named "release-aarch64-apple-darwin", "release-x86_64-apple-darwin", "release-x86_64-unknown-linux-gnu", "release-aarch64-unknown-linux-gnu" exist
    And each artifact contains an archive plus a sha256 sidecar

  # ===========================================================================
  # US-08 — Atomic-publish guard
  # ===========================================================================

  @release-1 @us-08
  Scenario: Publish job declares dependency on validate-tag and build matrix
    Given the release workflow file at ".github/workflows/release.yml"
    When the workflow definition is parsed
    Then the publish-github-release job has needs equal to "validate-tag" and "build"
    And the bump-tap-formula job has needs equal to "publish-github-release"
    And no job uses "if: always()" or "if: failure()" to bypass the guard

  @release-1 @us-08
  Scenario: Single failing build cell prevents publish and tap-bump from running
    Given the build matrix has run with 3 cells succeeding and 1 cell failing
    When the workflow continues
    Then the publish-github-release job is skipped
    And the bump-tap-formula job is skipped
    And no GitHub Release for the tag is created
    And no PR is opened against the tap repository

  @release-1 @us-08 @property
  Scenario: Publish atomicity holds for any combination of build cell outcomes
    Given any combination of pass/fail outcomes across the 4 build cells
    When the workflow evaluates the publish-github-release job
    Then the publish-github-release job runs if and only if every build cell succeeded
    And the bump-tap-formula job runs if and only if publish-github-release ran successfully

  # ===========================================================================
  # US-09 — SLSA build provenance attestation
  # ===========================================================================

  @release-1 @us-09
  Scenario: Build job declares the OIDC permissions required for attestation
    Given the release workflow file at ".github/workflows/release.yml"
    When the workflow definition is parsed
    Then the build job permissions include "id-token: write" and "attestations: write"

  @release-1 @us-09
  Scenario: Each build cell invokes the attest-build-provenance action against its archive
    Given the workflow definition declares the attest-build-provenance step
    When the build cell completes packaging the archive
    Then the attest-build-provenance step is invoked with the archive as the subject
    And the action version is pinned to "@v2"

  @release-1 @us-09 @requires_external
  Scenario: Devon verifies a published archive's attestation with one command
    Given a published archive "modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz"
    When Devon runs "gh attestation verify modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz --owner jeffabailey"
    Then the verification succeeds
    And the output identifies the build was performed by the modeltap workflow

  # ===========================================================================
  # US-10 — Formula renders all 4 platform blocks
  # ===========================================================================

  @release-1 @us-10 @real-io @adapter-integration
  Scenario: Formula renders all 4 platform blocks with sha256s read from artifact files
    Given a fixture artifact directory containing 4 sha256 sidecar files for the 4 supported targets
    And the formula template at "release/templates/modeltap.rb.tera"
    When the maintainer runs "cargo xtask render-formula --version 0.2.0 --template release/templates/modeltap.rb.tera --output ${TMPDIR}/Formula/modeltap.rb --sha256-dir ${TMPDIR}/artifacts --release-base-url https://github.com/jeffabailey/modeltap/releases/download/v0.2.0"
    Then the rendered formula contains an "on_macos.on_arm" block with sha256 from the aarch64-apple-darwin sidecar
    And the rendered formula contains an "on_macos.on_intel" block with sha256 from the x86_64-apple-darwin sidecar
    And the rendered formula contains an "on_linux.on_arm" block with sha256 from the aarch64-unknown-linux-gnu sidecar
    And the rendered formula contains an "on_linux.on_intel" block with sha256 from the x86_64-unknown-linux-gnu sidecar
    And the version field equals "0.2.0"

  @release-1 @us-10 @infrastructure-failure
  Scenario: Render-formula fails when an expected sha256 sidecar is missing
    Given a fixture artifact directory missing the sidecar for "aarch64-apple-darwin"
    When the maintainer runs "cargo xtask render-formula --version 0.2.0 --template release/templates/modeltap.rb.tera --output ${TMPDIR}/Formula/modeltap.rb --sha256-dir ${TMPDIR}/artifacts --release-base-url https://github.com/jeffabailey/modeltap/releases/download/v0.2.0"
    Then the script exits non-zero
    And the message identifies the missing sidecar by filename
    And no formula file is written

  @release-1 @us-10 @infrastructure-failure
  Scenario: Render-formula rejects a sha256 sidecar with malformed content
    Given a fixture artifact directory where one sidecar contains "not-a-valid-hex-digest"
    When the maintainer runs "cargo xtask render-formula --version 0.2.0 --template release/templates/modeltap.rb.tera --output ${TMPDIR}/Formula/modeltap.rb --sha256-dir ${TMPDIR}/artifacts --release-base-url https://github.com/jeffabailey/modeltap/releases/download/v0.2.0"
    Then the script exits non-zero
    And the message identifies the sidecar with the invalid sha256

  @release-1 @us-10 @property
  Scenario: Render-formula round-trip preserves every sha256 verbatim
    Given any valid combination of 4 sha256 sidecars
    When the formula is rendered
    Then each sha256 in the rendered formula equals the verbatim content of its sidecar file
    And no sha256 is computed by reading or rehashing the archive

  @release-1 @us-10 @requires_docker
  Scenario: Brew test-bot audit passes on the rendered formula
    Given Docker is available in the test environment
    And the rendered "Formula/modeltap.rb" is committed to the bump branch
    When "brew test-bot --tap jeffabailey/homebrew-modeltap" runs the audit step
    Then the audit step exits zero
    And no formula DSL violations are reported
