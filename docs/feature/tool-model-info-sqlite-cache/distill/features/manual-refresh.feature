# =============================================================================
# tool-model-info-sqlite-cache — Manual refresh + provenance line (US-24)
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-17
#
# Tag glossary (subset relevant to this file):
#   @us-24                    -- story trace
#   @ac-24-N                  -- AC trace
#   @adr-015                  -- ADR-015 (cache state model)
#   @release-2                -- target release per prioritization.md
#   @real-io @adapter-integration -- exercises real plugin + real cache
#   @perf @k-info-2-refresh-1s -- per-tool refresh wall-clock <= 1 s
#
# Scenario count: 4. Error/edge: 1 (25% of this file). Cross-file ratio is met
# by cache-state-model.feature and integration-checkpoints.feature where the
# error-heavy scenarios concentrate.
# =============================================================================

Feature: Manual refresh hotkeys and provenance line

  As Devon Park, a multi-tool local-AI power user,
  I want a one-keystroke way to refresh a tool's inventory after running ollama pull or huggingface-cli delete-cache in another terminal,
  And I want a provenance line that tells me how stale the current view is,
  So that I can trust modeltap's data without relaunching, and act on the indicator with confidence.

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And the cache file path is "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"

  # ===========================================================================
  # Happy path — [r] refreshes the selected tool within 1 s
  # ===========================================================================

  @us-24 @ac-24-3 @ac-24-7 @ac-24-8 @adr-015 @release-2 @real-io @adapter-integration @perf @k-info-2-refresh-1s @cache-introspection
  Scenario: [r] refreshes the selected tool within 1 second
    Given Devon has fixture "devon-cache-warm"
    And Devon has Ollama selected in the left pane
    And no dialog is open
    When Devon presses 'r'
    Then a spinner appears next to the Ollama row
    And the summary bar reads "refreshing Ollama..."
    Within 1000 ms the spinner clears and the summary bar reads "as of just now (Ollama refreshed)"
    And the cache.tools row for Ollama updates with the new last_scan_at

  # ===========================================================================
  # Happy path — [Shift+R] refreshes all four tools in parallel
  # ===========================================================================

  @us-24 @ac-24-4 @ac-24-7 @ac-24-8 @adr-015 @release-2 @real-io @adapter-integration @perf @cache-introspection
  Scenario: [Shift+R] refreshes all four tools in parallel within 2 seconds
    Given Devon has fixture "devon-cache-warm"
    And no dialog is open
    When Devon presses Shift+R
    Then all four tool rows show the per-tool spinner
    And the summary bar reads "refreshing all tools..."
    Within 2000 ms all spinners clear and the summary bar reads "as of just now"
    And the cache.tools rows for every tool are updated

  # ===========================================================================
  # Error / edge — [r] is a no-op when a dialog is open
  # ===========================================================================

  @us-24 @ac-24-5 @adr-015 @release-2 @real-io
  Scenario: [r] is a no-op when a dialog is open
    Given Devon has fixture "devon-cache-warm"
    And the unify dialog is open
    When Devon presses 'r'
    Then no refresh is triggered
    And the "[r] refresh tool" shortcut in the bottom bar is dimmed
    And the unify dialog state is preserved

  # ===========================================================================
  # Happy path — provenance line always shows freshness with human-readable suffix
  # ===========================================================================

  @us-24 @ac-24-1 @ac-24-2 @adr-015 @release-2 @real-io
  Scenario: Provenance line always shows freshness with a human-readable suffix
    Given Devon has fixture "devon-cache-warm"
    And the cache contains inventory data written at the previous launch 14 minutes ago
    When Devon runs "modeltap"
    Then the summary bar shows "as of 14 min ago, reconciling..."
    And the timestamp updates as reconcile progresses
    When the background reconcile completes
    Then the summary bar updates to "as of just now"
