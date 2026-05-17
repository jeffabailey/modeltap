# =============================================================================
# tool-model-info-sqlite-cache — SHA256 persistence across launches (US-27)
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-17
#
# Tag glossary (subset relevant to this file):
#   @us-27           -- story trace
#   @ac-27-N         -- AC trace
#   @adr-015 @adr-018 -- ADR trace
#   @release-3       -- DEFERRED per prioritization.md
#   @skip            -- DELIVER does NOT enable these scenarios in Release 2
#   @property        -- universal invariant (DELIVER may proptest)
#   @real-io @adapter-integration -- exercises real ADR-013 hash pool + cache
#
# Scenario count: 3. Error/edge: 1 (33%). DEFERRED to Release 3 per
# ADR-018 §"Implementation guidance (for DELIVER -- Release 2 only)":
#   "Release 3 (US-27) is out of scope for this DELIVER. The cache_sha256 table,
#    the opt-in flag, the modeltap cache verify subcommand, and R10 enforcement
#    all land in a future DELIVER."
#
# Every scenario in this file is tagged @release-3 AND @skip. DELIVER removes
# @skip ONE AT A TIME after Release 2 dogfooding completes and US-27 is
# unblocked. The @release-3 tag stays as a release-slice marker.
# =============================================================================

Feature: SHA256 hash persistence across launches (DEFERRED to Release 3)

  As Devon Park, a power user with 50+ GB of model files,
  I want modeltap to remember the SHA256 hashes it computed last week,
  So that opening modeltap today does not re-pay the 30-60 second hashing cost for files that have not changed since.

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And the cache file path is "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"
    And cache.persist_sha256 = true in the config

  # ===========================================================================
  # Happy path — SHA256 persists when file unchanged
  # ===========================================================================

  @us-27 @ac-27-1 @ac-27-2 @ac-27-7 @adr-015 @adr-018 @release-3 @skip @real-io @adapter-integration
  Scenario: SHA256 hash persists across launches when the file is unchanged
    Given Devon has fixture "devon-cache-warm"
    And Devon computed SHA256 for "~/llms/mistral-7b-instruct-q4_K_M.gguf" in a previous session
    And the file's (mtime, size, inode, dev) matches the cached entry
    When Devon launches modeltap again and opens the Mistral detail screen
    Then the dedup key displays without recomputing the SHA256
    And the provenance reads "dedup key computed <N> days ago"
    And no hash.computed event appears in launch.log for this file

  # ===========================================================================
  # Edge — SHA256 invalidates on mtime/size/inode/dev drift
  # ===========================================================================

  @us-27 @ac-27-2 @ac-27-3 @ac-27-4 @adr-015 @adr-018 @release-3 @skip @real-io @adapter-integration @property
  Scenario: SHA256 hash invalidates when (mtime, size, inode, dev) differs from cached entry
    Given Devon has fixture "devon-cache-warm"
    And Devon computed SHA256 for a file in a previous session
    And the file's mtime has changed since
    When the SHA256 is needed again
    Then the cached hash is invalidated
    And a fresh SHA256 computation is queued via the background hash pool
    And the dedup key shows "(computing...)" until the new hash completes

  # ===========================================================================
  # Error — modeltap cache verify rehashes everything and reports drift
  # ===========================================================================

  @us-27 @ac-27-5 @adr-015 @adr-018 @release-3 @skip @real-io @adapter-integration
  Scenario: modeltap cache verify rehashes everything and reports drift
    Given Devon has fixture "devon-cache-warm"
    And Devon has 58 models in his library with cached SHA256 values
    And 2 of those files have been replaced manually with files of the same mtime and size but different content
    When Devon runs "modeltap cache verify"
    Then every cached SHA256 entry is recomputed
    And entries where the recomputed hash differs from the cached value are listed in stdout
    And the cache is updated with the recomputed values
    And "~/.modeltap/diagnostics.log" records "cache_verify drift_count=2"
