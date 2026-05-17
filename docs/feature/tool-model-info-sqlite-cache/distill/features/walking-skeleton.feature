# =============================================================================
# tool-model-info-sqlite-cache — Walking Skeleton
#
# Wave: DISTILL (5 of 6) — brownfield extension of modeltap-tui
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-17
#
# Tag glossary (inherits parent's master-acceptance.feature glossary + sibling's
# folder-group-delete.feature glossary):
#   @walking-skeleton  -- WS exit gate; DELIVER ships when this passes
#   @us-21-cache       -- this feature's walking skeleton (composite cross-story
#                         seam; touches US-23 cache lifecycle + US-25 warm-read)
#   @us-23             -- traceability: cache file lifecycle exercised
#   @us-25             -- traceability: warm-start read exercised
#   @adr-015           -- traceability: ADR-015 (state model: SQLite cache)
#   @adr-016           -- traceability: ADR-016 (Tool trait inspect extension)
#   @adr-017           -- traceability: ADR-017 (rusqlite_migration)
#   @release-2         -- target release per prioritization.md
#   @real-io           -- uses real filesystem + real on-disk SQLite
#   @adapter-integration -- proves driven adapters against real I/O
#   @cache-introspection -- step assertions read cache.sqlite directly (test-only seam)
#   @k-info-1-warm-100ms -- KPI K-INFO-1 warm-start latency budget
#   @k3a-warm-paint    -- INT-INFO-1 redefined K3a
#
# Strategy declaration: Strategy B (real I/O against fixture-populated temp
# dirs) per wave-decisions.md §D5. Inherits parent + sibling Strategy B.
#
# The walking-skeleton is the ONLY scenario in this file. Its purpose is to
# prove the cache wiring end-to-end:
#   - The in-process TestTool plugin discovers a model.
#   - Process A persists it to a real on-disk cache.sqlite at MODELTAP_CACHE_PATH.
#   - The SQLite file is created with PRAGMA journal_mode=WAL and PRAGMA
#     user_version=1 (proves migrate-v0-to-v1).
#   - Process A exits cleanly.
#   - Process B launches against the same cache file.
#   - Process B's warm-start paint reads the persisted row in <= 150 ms.
#   - The model appears in process B's right pane.
#
# This proves: cache file lifecycle, migration v0->v1, WAL init, warm-read path,
# dependency wiring of modeltap-store into modeltap-app, in-process Tool trait
# extension. Everything else builds on it.
#
# Litmus test for WS user-centricity (critique Dim 5):
#   Title: "Devon's second launch shows yesterday's inventory instantly from cache"
#   — describes USER GOAL (instant warm-start), not technical flow.
#   Then steps: "the right pane shows the model" + "the summary bar reads 'as of
#   <N> seconds ago'" — describe USER OBSERVATIONS.
#   Demo-able to Devon: "yes, that's what I need — modeltap should remember my
#   inventory between launches."
# =============================================================================

@walking-skeleton @us-21-cache @us-23 @us-25 @adr-015 @adr-016 @adr-017 @release-2 @real-io @adapter-integration @cache-introspection @k-info-1-warm-100ms @k3a-warm-paint
Feature: Walking skeleton — modeltap remembers Devon's inventory across launches

  As Devon Park, a multi-tool local-AI power user who opens modeltap many times per day,
  I want modeltap to remember what models I have from the previous launch
  So that the inventory paints instantly when I re-open the TUI, instead of paying the full discovery cost every time.

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And the cache file path is "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"
    And the in-process TestTool plugin is registered
    And the TestTool will discover one model "Test-Model-7B-Q4_K_M" at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/test-tool/models/Test-Model-7B-Q4_K_M.gguf"

  # ===========================================================================
  # The walking skeleton — the ONE scenario that proves the cache wiring works.
  # Removed from @skip when DELIVER step 01 (cache infrastructure) is complete.
  # ===========================================================================

  Scenario: Devon's second launch shows yesterday's inventory instantly from cache
    Given the cache file does not exist
    When Devon runs "modeltap" in headless mode and quits after first paint
    Then the cache file at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite" exists with PRAGMA user_version = 1
    And cache_models contains exactly 1 row for tool_id "test-tool"
    And cache_tools contains a row for tool_id "test-tool" with model_count = 1
    When a second modeltap process launches against the same cache file
    Then the second process's TUI shows "Test-Model-7B-Q4_K_M" in the right pane
    And the second process's warm-paint time is at most 150 ms
    And the second process's summary bar shows "as of just now" or "as of <N> seconds ago"
