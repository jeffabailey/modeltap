# =============================================================================
# release-process-homebrew-github — Hands-Off Automation Slice
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-03
#
# Stories covered: US-11 (auto-merge), US-12 (idempotent retry),
#                  US-13 (RELEASING.md runbook), US-14 (workflow line limit)
# Slice: Release 2 — "Hands-off automation"
# =============================================================================

Feature: Hands-Off Release Automation
  As Jeff Bailey, the modeltap maintainer
  I want to push a tag and walk away while the tap PR auto-merges and a contributor can read the workflow end-to-end
  So that release toil drops to zero and Riley Chen can understand the pipeline in 5 minutes

  Background:
    Given a clean tempdir workspace for the scenario
    And a fake Homebrew tap repository seeded at "${TMPDIR}/tap-fake"

  # ===========================================================================
  # US-11 — Auto-merge tap-bump PR when brew test-bot is green
  # ===========================================================================

  @release-2 @us-11
  Scenario: Bump-tap-formula step invokes auto-merge with squash strategy
    Given the bump-tap-formula step has opened a PR against the tap repository
    When the bump-tap-formula step continues
    Then the step invokes the gh command to enable auto-merge with squash strategy
    And the auto-merge invocation targets the bump branch for the current version

  @release-2 @us-11 @requires_external @cross-repo
  Scenario: Auto-merge fires within 5 minutes when brew test-bot is green
    Given a tap-bump PR is open against the live tap repository
    And the tap repository main branch protection requires brew test-bot to pass
    When brew test-bot reports success on the PR
    Then the PR is auto-merged within 5 minutes
    And the maintainer was not required to click merge

  @release-2 @us-11 @requires_external @cross-repo
  Scenario: Auto-merge withholds when brew test-bot fails on any platform
    Given a tap-bump PR is open against the live tap repository
    When brew test-bot reports a failure on the macOS Apple Silicon install step
    Then the PR is not auto-merged
    And the PR remains open awaiting maintainer review
    And the maintainer is notified via the PR comment thread

  # ===========================================================================
  # US-12 — Bump-tap-formula is idempotent on retry
  # ===========================================================================

  @release-2 @us-12 @real-io @adapter-integration @cross-repo
  Scenario: First-run creates the bump branch and opens a new PR
    Given no "bump/v0.2.0" branch exists in the tap repository
    When the bump-tap-formula step runs for tag "v0.2.0"
    Then a "bump/v0.2.0" branch is created in the tap repository
    And exactly one PR titled "modeltap 0.2.0" is open against the tap repository

  @release-2 @us-12 @real-io @adapter-integration @cross-repo
  Scenario: Re-run after token rotation force-pushes to the existing branch
    Given a "bump/v0.2.0" branch already exists in the tap repository with a previous commit
    And exactly one PR for the bump branch is already open
    When the bump-tap-formula step is re-run for tag "v0.2.0"
    Then the existing branch is force-pushed with the latest rendered formula
    And no second PR for the same version is created
    And the existing PR remains the only PR for the version

  @release-2 @us-12 @property
  Scenario: One PR per version invariant holds across any number of retries
    Given the bump-tap-formula step has been re-run any number of times for the same version
    When the tap repository's PR list is queried
    Then exactly one PR exists for the version
    And exactly one bump branch exists for the version

  @release-2 @us-12
  Scenario: Manual edits to the bump branch are clobbered by the next render
    Given the maintainer has manually edited "Formula/modeltap.rb" on the "bump/v0.2.0" branch
    When the bump-tap-formula step is re-run for tag "v0.2.0"
    Then the manual edits are overwritten by the rendered formula
    And the runbook documents this trade-off

  # ===========================================================================
  # US-13 — RELEASING.md runbook
  # ===========================================================================

  @release-2 @us-13
  Scenario: Runbook exists at repo root within the line budget
    Given the source repository
    When "RELEASING.md" is opened
    Then the file exists at the repository root
    And the file has at most 80 lines
    And the file contains at most 10 numbered steps

  @release-2 @us-13
  Scenario: Runbook contains the per-release log table
    Given "RELEASING.md" exists
    When the file is parsed
    Then a markdown table with columns for version, tag-pushed-at, release-published-at, tap-merged-at, time-to-tap, platforms-verified, provenance-verified, and notes is present

  @release-2 @us-13
  Scenario: Runbook documents the operational safety notes
    Given "RELEASING.md" exists
    When the file is read
    Then the file documents the GH_TAP_TOKEN rotation procedure
    And the file documents the manual-edit-clobber trade-off for bump branches
    And the file documents the macOS Gatekeeper xattr workaround

  # ===========================================================================
  # US-14 — Workflow file ≤250 lines, every job has a purpose comment
  # ===========================================================================

  @release-2 @us-14 @real-io @adapter-integration
  Scenario: Lint-workflows accepts a release.yml within the line budget
    Given a workflow file at "${TMPDIR}/release.yml" with 180 lines
    And every job in the workflow has a "# Purpose:" comment immediately above its declaration
    When the maintainer runs "cargo xtask lint-workflows --workflow ${TMPDIR}/release.yml --max-lines 250"
    Then the script exits zero
    And no diagnostic message is printed

  @release-2 @us-14 @real-io @adapter-integration
  Scenario: Lint-workflows rejects a release.yml exceeding the line budget
    Given a workflow file at "${TMPDIR}/release.yml" with 270 lines
    When the maintainer runs "cargo xtask lint-workflows --workflow ${TMPDIR}/release.yml --max-lines 250"
    Then the script exits non-zero
    And the message says the workflow exceeds the 250-line limit
    And the message reports the actual line count

  @release-2 @us-14 @real-io @adapter-integration
  Scenario: Lint-workflows rejects a job missing the purpose comment
    Given a workflow file at "${TMPDIR}/release.yml" with 200 lines
    And the "build" job does not have a "# Purpose:" comment immediately above its declaration
    When the maintainer runs "cargo xtask lint-workflows --workflow ${TMPDIR}/release.yml --max-lines 250"
    Then the script exits non-zero
    And the message identifies "build" as the job missing its purpose comment

  @release-2 @us-14 @property
  Scenario: Lint-workflows accepts every workflow that satisfies both constraints
    Given any workflow file with at most 250 lines
    And every job in the workflow has a "# Purpose:" comment immediately above its declaration
    When the lint runs
    Then the lint exits zero
