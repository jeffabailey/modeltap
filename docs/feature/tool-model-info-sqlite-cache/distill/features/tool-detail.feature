# =============================================================================
# tool-model-info-sqlite-cache — Tool detail screen (US-21)
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-17
#
# Tag glossary (subset relevant to this file):
#   @us-21                   -- story trace
#   @ac-21-N                 -- AC trace
#   @adr-016                 -- ADR-016 (Tool trait inspect extension)
#   @release-1               -- target release per prioritization.md
#   @real-io @adapter-integration -- exercises real plugin adapter
#   @perf @k-info-1-warm-100ms -- detail-screen-open <= 100 ms budget
#
# Scenario count: 5. Error/edge: 3 (60% of this file).
# =============================================================================

Feature: Tool detail screen — drill into per-tool metadata without leaving the TUI

  As Devon Park, a multi-tool local-AI power user,
  I want to drill into any tool's row in the left pane and see its install path, version, model count, disk usage, last scan time, last error, and configured search paths,
  So that I can diagnose "(error)" annotations and audit tool inventory without alt-tabbing to a second terminal.

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And the cache file path is "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"

  # ===========================================================================
  # Happy path — Devon checks Ollama's tool detail
  # ===========================================================================

  @us-21 @ac-21-1 @ac-21-2 @ac-21-8 @adr-016 @release-1 @real-io @adapter-integration @perf @k-info-1-warm-100ms
  Scenario: Pressing Enter on a left-pane row opens the tool detail screen within 100 ms
    Given Devon has fixture "devon-multi-tool"
    And MODELTAP_OLLAMA_VERSION is set to "0.6.4"
    And Devon has Ollama selected in the left pane
    When Devon presses Enter
    Then the tool detail screen opens within 100 ms
    And it shows Ollama's discovery root "~/.ollama/models/"
    And it shows the configured search paths under that root
    And it shows model count 12, disk usage 47.3 GB, last scan "2026-05-16 09:14:22 (N min ago)", and plugin version "modeltap-plugin-ollama 0.2.6"
    And it shows the largest model: "llama3:70b-instruct-q4_K_M (39.8 GB)"
    And the bottom bar on the detail screen shows "[Esc] back", "[r] refresh this tool", "[?] help"

  # ===========================================================================
  # Edge — undetectable version renders as "(not detectable)"
  # ===========================================================================

  @us-21 @ac-21-3 @adr-016 @release-1 @real-io @adapter-integration
  Scenario: Undetectable version is shown as "(not detectable)"
    Given Devon has fixture "devon-multi-tool"
    And a plugin's inspect_tool() returns no version for llama-cli
    When Devon opens llama-cli's detail screen
    Then the Version field reads "(not detectable)"
    And no false or stale version is shown
    And the rest of the detail screen renders normally

  # ===========================================================================
  # Error — last error surfaces with timestamp
  # ===========================================================================

  @us-21 @ac-21-4 @ac-21-6 @adr-016 @release-1 @real-io @adapter-integration @infrastructure-failure
  Scenario: Last error surfaces in tool detail when discovery failed
    Given Devon has fixture "devon-tool-error-ollama"
    And Ollama's discovery failed at last scan with "permission denied reading ~/.ollama/models/manifests/ (errno 13)"
    When Devon opens Ollama's detail screen
    Then the Last error field shows "permission denied reading ~/.ollama/models/manifests/ (errno 13)" with the timestamp
    And the bottom bar offers "[r] refresh this tool" to retry after fixing permissions

  # ===========================================================================
  # Edge — user-configured search paths labelled separately from defaults
  # ===========================================================================

  @us-21 @ac-21-5 @adr-016 @release-1 @real-io @adapter-integration
  Scenario: User-configured search paths are labelled
    Given Devon has fixture "devon-llamacli-userconfig"
    And Devon has added 'search_paths = ["/data/models"]' to ~/.modeltap/config.toml under [plugins.llama-cli]
    When Devon opens llama-cli's detail screen
    Then the Search paths section lists "~/llms/", "~/models/", and "/data/models/"
    And "~/llms/" is labelled "(default)"
    And "~/models/" is labelled "(default)"
    And "/data/models/" is labelled "(user config)"

  # ===========================================================================
  # Edge — Esc returns and preserves cursor position
  # ===========================================================================

  @us-21 @ac-21-7 @ac-21-8 @adr-016 @release-1 @real-io
  Scenario: Esc from the tool detail screen returns to main view preserving left-pane cursor
    Given Devon has fixture "devon-multi-tool"
    And Devon has the cursor on Ollama in the left pane
    When Devon presses Enter
    And Devon presses Esc
    Then the main view returns
    And the cursor is still on Ollama in the left pane
