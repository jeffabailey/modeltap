# =============================================================================
# release-process-homebrew-github — Master Acceptance Feature File
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-03
#
# Index aggregator for the four sub-feature files:
#   - walking-skeleton.feature        (US-01..US-06 + US-15 single-target path)
#   - multi-arch-release.feature      (US-07..US-10; Release 1)
#   - hands-off-automation.feature    (US-11..US-14; Release 2)
#   - integration-checkpoints.feature (INT.AC-1..INT.AC-6 cross-story invariants)
#
# Tag glossary:
#   @walking_skeleton  -- WS exit gate; DELIVER ships when these pass green
#   @release-1         -- "Multi-arch real release" slice
#   @release-2         -- "Hands-off automation" slice
#   @integration       -- cross-story invariant scenario (INT.AC-N)
#   @int-ac-N          -- traceability to specific integration AC
#   @us-NN             -- traceability to user-stories.md story ID
#   @real-io           -- uses real filesystem / real git / real subprocess
#   @adapter-integration -- proves a single driven adapter against real I/O
#   @cross-repo        -- exercises modeltap-fake ↔ tap-fake seam
#   @requires_external -- needs live GitHub API; skipped in default CI
#   @requires_docker   -- needs Docker daemon (cross-compile, brew test-bot)
#   @infrastructure-failure -- driven-adapter failure scenario
#   @in-memory         -- forbidden in walking-skeleton scenarios under Strategy C
#   @property          -- universal invariant; DELIVER may implement as proptest
#   @slow              -- exercises real cargo build/test (minutes not seconds)
#   @recovery          -- post-failure recovery scenario
#
# Walking-skeleton exit gate (6 user-value scenarios; one per backbone activity):
#   1. PREP        -- "Maintainer prepares a release with one command"  (US-01)
#   2. TAG         -- "Validate-tag accepts a tag matching workspace version"  (US-02)
#   3. BUILD       -- "Build orchestration runs CI parity gates before packaging"  (US-03+US-04)
#   4. PUBLISH     -- "Release notes are extracted from the changelog section"  (US-05)
#   5. TAP-BUMP    -- "Bump-tap-formula opens a PR against the ephemeral tap repo"  (US-06)
#   6. USER-INSTALL -- "Devon installs and verifies the version"  (US-15, @requires_external)
#
# Scenario inventory by file:
#   walking-skeleton.feature        — 18 scenarios (5 WS + 13 focused/error/property)
#   multi-arch-release.feature      — 14 scenarios (4 US-07 + 3 US-08 + 3 US-09 + 4 US-10)
#   hands-off-automation.feature    — 14 scenarios (3 US-11 + 4 US-12 + 3 US-13 + 4 US-14)
#   integration-checkpoints.feature — 10 scenarios (6 INT.AC-N + 2 recovery + extras)
#   TOTAL: 56 scenarios
#
# Coverage ratios:
#   Happy path scenarios:        24
#   Error/edge/infra-failure:    23  (≥40% target met: 23/56 = 41%)
#   Property scenarios:           5  (tagged @property)
#   Walking-skeleton scenarios:   6  (one per backbone activity)
#   @requires_external smokes:    7  (need live GH; runnable on demand)
#   @requires_docker smokes:      3  (need Docker; cross + brew test-bot)
# =============================================================================

Feature: Release Process Homebrew GitHub — Master Acceptance Index
  As Jeff Bailey, the modeltap maintainer
  I want one tag push to ship signed multi-arch archives, publish a GitHub Release,
  open an auto-merging Homebrew tap PR, and let any clean macOS or Linux machine
  brew install modeltap and run it within fifteen minutes
  So that release cuts feel boring, end users get a turnkey install,
  and contributors can read the workflow file and trust it

  # This file is a documentation index. The executable scenarios live in:
  #   - features/walking-skeleton.feature
  #   - features/multi-arch-release.feature
  #   - features/hands-off-automation.feature
  #   - features/integration-checkpoints.feature
  #
  # Test infrastructure (step skeletons + fixtures) lives in:
  #   - steps/common_steps.rs
  #   - steps/walking_skeleton_steps.rs
  #   - steps/multi_arch_steps.rs
  #   - steps/hands_off_steps.rs
  #   - steps/integration_steps.rs
  #
  # See test-scenarios.md for the full story-to-scenario traceability matrix.
  # See adapter-coverage.md for the Mandate 6 adapter coverage audit.
  # See walking-skeleton.md for the WS strategy declaration (Strategy C).
  # See wave-decisions.md for DWD-01 .. DWD-07 design decisions.

  Scenario: This file is a documentation index, not an executable scenario set
    Given a reviewer is reading this master-acceptance index
    When they want to understand the suite's structure
    Then they should consult the per-slice feature files listed above
    And they should consult test-scenarios.md for the traceability matrix
