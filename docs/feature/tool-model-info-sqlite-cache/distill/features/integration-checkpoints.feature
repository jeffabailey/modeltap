# =============================================================================
# tool-model-info-sqlite-cache — Cross-Cutting Integration Checkpoints
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-17
#
# Cross-cutting invariants for INT-INFO-1..9 from acceptance-criteria.md.
# These are the same kind of invariants the parent and sibling captured in
# their integration-checkpoints files.
#
# These scenarios assert PROPERTIES or CROSS-FEATURE coordination, not specific
# user journeys. They are tagged @property where DELIVER may implement them as
# proptest invariants and @destructive where they require a real mutation.
#
# Scenario count: 7. Error/edge: 6 (86% of this file). Cross-feature integration
# is intrinsically error-path-heavy because it captures invariants like
# "the revalidator MUST be called before every destructive action" — the
# error-path version of "and what happens when the cache is stale?".
# =============================================================================

Feature: tool-model-info-sqlite-cache — Cross-Cutting Integration Invariants

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And the cache file path is "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"

  # ===========================================================================
  # INT-INFO-1 — K3 redefined as K3a (warm) + K3b (cold); both pass
  # ===========================================================================

  @int-info-1 @us-23 @us-25 @release-2 @real-io @perf @k3a-warm-paint @k3b-cold-start
  Scenario: Parent's K3 (first paint < 1 s) is satisfied via K3a OR K3b on every launch
    Given Devon has fixture "devon-cache-warm"
    When Devon runs "modeltap"
    Then the JSONL log "launch.warm_paint_ms" event value is at most 150
    Given Devon has fixture "devon-cache-empty"
    When Devon runs "modeltap"
    Then the JSONL log "launch.first_paint_ms" event value is at most 150
    And the JSONL log "launch.full_inventory_paint_ms" event value is at most 1150

  # ===========================================================================
  # INT-INFO-3 — total.disk_usage == sum(tool.disk_usage) during reconcile
  # ===========================================================================

  @int-info-3 @us-26 @release-2 @real-io @property
  Scenario: total.disk_usage equals the sum of per-tool disk_usage during and after reconcile
    Given Devon has fixture "devon-cache-warm"
    When Devon runs "modeltap" and the background reconcile is mid-flight
    Then "total.disk_usage" equals the sum of "tool.disk_usage" for every installed tool within rounding tolerance of 1 byte
    And the summary bar shows ", reconciling..." while transiently inconsistent
    When the background reconcile completes
    Then "total.disk_usage" equals the sum of "tool.disk_usage" for every installed tool within rounding tolerance of 1 byte
    And the summary bar shows "as of just now"

  # ===========================================================================
  # INT-INFO-4 — every destructive action runs the revalidator
  # ===========================================================================

  @int-info-4 @us-26 @ac-26-5 @release-2 @real-io @destructive
  Scenario Outline: The pre-mutate revalidator is invoked before every destructive action
    Given Devon has fixture "devon-cache-warm"
    And the cache file matches the filesystem (no drift)
    When Devon performs the <action> action on a model
    Then the JSONL log shows a "revalidate.invoked" event with source = "<action>" before the action's outcome event
    And the action proceeds with ValidationResult Match

    Examples:
      | action        |
      | unify         |
      | zap           |
      | delete_one    |
      | folder_delete |

  # ===========================================================================
  # INT-INFO-5 — --no-cache is a true bypass
  # ===========================================================================

  @int-info-5 @us-23 @ac-23-8 @us-25 @ac-25-7 @release-2 @real-io
  Scenario: --no-cache produces zero cache writes for the entire launch
    Given Devon has fixture "devon-cache-warm"
    And a valid cache file exists at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"
    And the pre-launch DirManifest of "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/" is recorded
    When Devon runs "modeltap --no-cache" and performs one of {refresh, unify, zap, delete_one}
    Then the post-launch DirManifest of "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/" equals the pre-launch DirManifest
    And no cache.sqlite-wal or cache.sqlite-shm files exist that were not present pre-launch
    And the launch follows the stateless rediscovery path from ADR-003

  # ===========================================================================
  # INT-INFO-6 — modeltap --version succeeds even with a corrupted cache
  # ===========================================================================

  @int-info-6 @us-23 @release-2 @real-io @infrastructure-failure
  Scenario: modeltap --version succeeds when the cache is unreadable
    Given Devon has fixture "devon-cache-corrupt"
    And the cache file "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite" exists but returns SQLITE_CORRUPT on open
    When Devon runs "modeltap --version"
    Then the process exits with exit code 0
    And stdout contains a version string
    And the cache file is not touched

  # ===========================================================================
  # INT-INFO-7 — folder-group-bulk-delete [F] also runs the revalidator
  # ===========================================================================

  @int-info-7 @us-26 @us-05c @ac-26-5 @release-2 @real-io @destructive
  Scenario: Folder-group [F] runs the pre-mutate revalidator before deleting files
    Given Devon has fixture "devon-hf-allunique"
    And the cache contains the folder's 5 files with current stat quads
    When Devon successfully folder-deletes "bartowski/Llama-3.2-1B-Instruct-GGUF"
    Then the JSONL log shows one "revalidate.invoked" event per file in the folder before any "delete.outcome" events
    And the cache.models rows for the deleted files are removed atomically with the filesystem unlink

  # ===========================================================================
  # INT-INFO-8 — plugin panic in inspect_* is caught at the orchestrator boundary
  # ===========================================================================

  @int-info-8 @us-21 @ac-21-9 @us-22 @ac-22-7 @adr-016 @release-1 @real-io @plugin-trait @infrastructure-failure
  Scenario: Plugin panic during inspect_tool or inspect_model is caught at the orchestrator boundary
    Given Devon has fixture "devon-multi-tool"
    And the Ollama plugin's inspect_tool implementation will panic when called
    When Devon opens Ollama's detail screen
    Then the detail screen shows "(inspection failed -- see diagnostics.log)"
    And the other detail-screen fields render with what discover() provided
    And "~/.modeltap/diagnostics.log" gains a line tagged "inspect_panic tool=ollama"
    And the TUI does not crash
    And the process is still alive after the panic
