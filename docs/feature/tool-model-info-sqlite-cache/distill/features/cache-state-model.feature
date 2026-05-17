# =============================================================================
# tool-model-info-sqlite-cache — Cache state model (US-23, US-25, US-26)
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-17
#
# Tag glossary (subset relevant to this file):
#   @us-23, @us-25, @us-26     -- story trace
#   @ac-23-N, @ac-25-N, @ac-26-N -- AC trace
#   @adr-015, @adr-017         -- ADR trace
#   @release-2                 -- target release per prioritization.md
#   @real-io @adapter-integration -- exercises driven adapter against real I/O
#   @cache-introspection       -- step asserts cache.sqlite contents via read-only Connection
#   @concurrent                -- requires two real modeltap processes
#   @infrastructure-failure    -- adapter failure scenario (corruption, perms, transient I/O)
#   @perf                      -- contains wall-clock latency assertion; runs in --release only
#   @k-info-1-warm-100ms       -- K-INFO-1 warm-start latency budget (<= 150 ms upper bound)
#   @k-info-4-recovery-100     -- K-INFO-4 recovery rate (100%)
#   @k-info-7-overhead-50ms    -- K-INFO-7 cache-open overhead (<= 100 ms upper bound)
#   @k3a-warm-paint @k3b-cold-start -- INT-INFO-1 redefined K3 sub-KPIs
#   @property                  -- universal invariant (DELIVER may proptest)
#
# Scenario count: 11. Error/edge: 8 (73% of this file).
#
# DELIVER ordering: scenarios un-skipped one at a time after the walking
# skeleton (in walking-skeleton.feature) is green. Recommended order:
#   1. Cold-start (no cache exists) — proves the fallback path
#   2. dirs::data_dir() resolution proof — proves the path resolver
#   3. Forward migration v0->v1 — proves the migrator
#   4. Warm-start within 100 ms — proves the warm-read path (the KPI scenario)
#   5. Corruption recovery — proves the rename + cold-start fallback
#   6. Downgrade recovery — proves the future-version rename
#   7. --no-cache true bypass — proves the opt-out
#   8. config cache.enabled=false — proves the config opt-out
#   9. Per-tool TTL stale forces cold paint — proves TTL eligibility
#  10. Mixed warm/cold per-tool — proves the mixed-state warm-paint path
#  11. Concurrent reads + write contention — proves WAL + busy_timeout
#  12. Pre-mutate drift refreshes the dialog — proves the revalidator (drift)
#  13. Pre-mutate file-gone aborts and refreshes — proves the revalidator (gone)
#  14. Failed reconcile keeps the stale cache visible — proves graceful degradation
#  15. Silent ack indicator on inventory change — proves the diff detector
# =============================================================================

Feature: Cache state model — schema, recovery, concurrency, warm-start, revalidation

  As Devon Park, a multi-tool local-AI power user,
  I want modeltap's cache to be resilient (recover from corruption), correct (revalidate before mutation),
  fast (warm-paint <= 150 ms), and safe to run twice (WAL concurrency),
  So that I can trust the cache as an optimization without it ever becoming load-bearing for correctness.

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And the cache file path is "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"

  # ===========================================================================
  # Cold-start: no cache exists
  # ===========================================================================

  @us-23 @ac-23-11 @adr-015 @release-2 @real-io
  Scenario: Cold start falls back to the ADR-003 skeleton paint when no cache exists
    Given Devon has fixture "devon-cache-empty"
    And the cache file does not exist
    When Devon runs "modeltap"
    Then the TUI paints the skeleton "discovering..." placeholders within 150 ms
    And full inventory paints within 1.15 seconds
    And the summary bar reads "as of just now"
    And the cache file at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite" exists with PRAGMA user_version = 1

  # ===========================================================================
  # Path resolution: dirs::data_dir() works on the host platform
  # ===========================================================================

  @us-23 @ac-23-1 @adr-015 @release-2 @real-io
  Scenario: Production cache path resolves via XDG_DATA_HOME on Linux or Library/Application Support on macOS
    Given Devon has fixture "devon-cache-empty"
    And XDG_DATA_HOME is set to "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data"
    And MODELTAP_CACHE_PATH is unset for this scenario
    When Devon runs "modeltap"
    Then the cache layer creates the file at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite" on Linux
    Or the cache layer creates the file at "${HOME}/Library/Application Support/modeltap/cache.sqlite" on macOS
    And the launch proceeds normally

  # ===========================================================================
  # Schema migration: forward v0 to v1
  # ===========================================================================

  @us-23 @ac-23-3 @ac-23-4 @adr-015 @adr-017 @release-2 @real-io @cache-introspection
  Scenario: Schema migration runs forward when binary expects a newer schema
    Given the cache PRAGMA user_version is 0
    And the binary's expected_schema_version is 1
    When Devon runs "modeltap"
    Then the migrator runs migration "0001_initial.sql"
    And the cache PRAGMA user_version becomes 1
    And "~/.modeltap/diagnostics.log" gains a line tagged "cache_migration from=0 to=1 status=ok"
    And the launch proceeds normally with warm-start paint

  # ===========================================================================
  # Warm-start: paint cached inventory within 150 ms (K-INFO-1 / K3a)
  # ===========================================================================

  @us-25 @ac-25-1 @us-23 @ac-23-2 @release-2 @real-io @perf @k-info-1-warm-100ms @k-info-7-overhead-50ms @k3a-warm-paint
  Scenario: Warm start paints cached inventory within 150 ms
    Given Devon has fixture "devon-cache-warm"
    And the cache contains inventory data written at the previous launch 14 minutes ago
    When Devon runs "modeltap"
    Then the TUI paints the cached inventory within 150 ms of process start
    And the cache-open overhead is at most 100 milliseconds
    And the summary bar shows "Total: 138.4 GB | 58 models | as of 14 min ago"
    And the bottom bar shows "[r] refresh tool [Shift+R] refresh all" among its shortcuts

  # ===========================================================================
  # Corruption recovery (K-INFO-4)
  # ===========================================================================

  @us-23 @ac-23-6 @ac-23-7 @ac-23-11 @adr-015 @release-2 @real-io @infrastructure-failure @k-info-4-recovery-100
  Scenario: Cache corruption is detected on open and recovered automatically
    Given Devon has fixture "devon-cache-corrupt"
    And the cache file "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite" exists but returns SQLITE_CORRUPT on open
    When Devon runs "modeltap"
    Then the cache file at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite.corrupt-*" exists
    And a recovery banner appears reading "Previous cache reset (corrupted or schema mismatch). Renamed to ${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite.corrupt-<timestamp>. Cold-start discovery in progress. See ~/.modeltap/diagnostics.log."
    And "~/.modeltap/diagnostics.log" gains a line tagged "cache_recovery reason=corrupted"
    And cold-start discovery proceeds without crashing modeltap

  # ===========================================================================
  # Downgrade recovery
  # ===========================================================================

  @us-23 @ac-23-5 @ac-23-7 @adr-015 @release-2 @real-io @infrastructure-failure @k-info-4-recovery-100
  Scenario: Downgrade detected — cache was written by a newer binary
    Given Devon has fixture "devon-cache-future-v"
    And the cache PRAGMA user_version is 99
    And the binary's expected_schema_version is 1
    When Devon runs "modeltap"
    Then the cache file at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite.future-version-99" exists
    And a recovery banner appears explaining the downgrade and the rename target
    And "~/.modeltap/diagnostics.log" gains a line tagged "cache_recovery reason=downgrade"
    And cold-start discovery proceeds without crashing modeltap

  # ===========================================================================
  # --no-cache true bypass (cross-references @int-info-5)
  # ===========================================================================

  @us-23 @ac-23-8 @int-info-5 @adr-015 @release-2 @real-io
  Scenario: --no-cache bypasses the cache for one launch
    Given Devon has fixture "devon-cache-warm"
    And a valid cache file exists at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"
    When Devon runs "modeltap --no-cache"
    Then no cache.sqlite, cache.sqlite-wal, or cache.sqlite-shm files are modified at "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap"
    And the launch follows the stateless rediscovery path from ADR-003
    And the summary bar reads "as of just now"

  # ===========================================================================
  # Config-file opt-out (and CLI-precedence-over-config check)
  # ===========================================================================

  @us-23 @ac-23-9 @int-info-5 @adr-015 @release-2 @real-io
  Scenario: cache.enabled = false config has the same effect as --no-cache
    Given Devon has fixture "devon-cache-warm"
    And a valid cache file exists
    And cache.enabled=false in the config
    When Devon runs "modeltap"
    Then the cache file is neither opened nor written
    And the launch follows the stateless rediscovery path from ADR-003

  # ===========================================================================
  # Per-tool TTL forces cold paint for stale entries
  # ===========================================================================

  @us-25 @ac-25-2 @ac-25-4 @us-26 @release-2 @real-io
  Scenario: Per-tool TTL forces cold paint for stale tool entries while other tools paint from cache
    Given Devon has fixture "devon-cache-stale-tool"
    And the cache contains Ollama inventory with last_scan_at 25 hours ago
    And the cache contains llama-cli inventory with last_scan_at 2 hours ago
    And cache.tool_ttl_seconds is 86400
    When Devon runs "modeltap"
    Then llama-cli's models paint from cache instantly
    And Ollama's left-pane row shows the cold-start spinner
    And cold-start discovery for Ollama proceeds while other tools paint from cache
    And the summary bar reads "as of varies per-tool, reconciling..."

  # ===========================================================================
  # Concurrent processes — WAL reads + busy_timeout writes
  # ===========================================================================

  @us-23 @ac-23-10 @adr-015 @release-2 @real-io @concurrent
  Scenario: Two modeltap processes can read the cache concurrently via SQLite WAL
    Given Devon has fixture "devon-cache-warm"
    And two modeltap processes share the same cache.sqlite
    When the first modeltap process is reading the cache
    And a second modeltap process opens the same cache file
    Then both processes coexist without SQLITE_BUSY errors during reads
    And both processes display consistent inventory data

  @us-23 @ac-23-2 @ac-23-10 @adr-015 @release-2 @real-io @concurrent @cache-introspection
  Scenario: Concurrent cache writes serialise via busy_timeout
    Given Devon has fixture "devon-cache-warm"
    And two modeltap processes are running with cache writes enabled
    And process A holds an open write transaction for 2 seconds via MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS=2000
    When process B attempts to write a cache_tools row update
    Then process B waits up to 5 seconds for the WAL lock
    And process B's write succeeds after process A commits
    And neither process crashes or returns an error to the user
    And the final cache_tools last_scan_at reflects process B's later write

  # ===========================================================================
  # Pre-mutate revalidation — drift detected, dialog refreshes
  # ===========================================================================

  @us-26 @ac-26-5 @ac-26-6 @adr-015 @release-2 @real-io
  Scenario: Pre-unify validation re-introspects when a file has drifted since the cache write
    Given Devon has fixture "devon-cache-mtime-drift"
    And "mistral:7b-instruct-q4_K_M" is registered in 3 tools per the cache
    And the llama-cli copy's mtime has changed since the last cache write
    When Devon presses 'u' on the Mistral row
    Then the validator detects the drift before opening the confirmation dialog
    And the dialog displays "Re-introspecting before proceeding..." with a brief progress indicator
    And the dedup-key / size for the drifted file is recomputed
    And Devon is shown the (possibly updated) reclaim estimate
    And Devon must re-confirm if the reclaim amount changed by more than rounding

  # ===========================================================================
  # Pre-mutate revalidation — file gone, action aborts
  # ===========================================================================

  @us-26 @ac-26-5 @ac-26-7 @adr-015 @release-2 @real-io
  Scenario: Pre-mutate validation aborts when a file no longer exists
    Given Devon has fixture "devon-cache-file-gone"
    And "mistral:7b-instruct-q4_K_M" is registered in 2 tools per the cache
    And one file has been deleted out-of-band between launch and Devon's action
    When Devon attempts to unify
    Then the pre-flight check refuses with "file no longer exists; refreshing inventory"
    And no destructive action occurs
    And an automatic per-tool refresh is triggered for the affected tool
    And the right pane updates to reflect the missing file

  # ===========================================================================
  # Failed reconcile keeps stale cache visible (last-known-good)
  # ===========================================================================

  @us-26 @ac-26-3 @release-2 @real-io @infrastructure-failure @cache-introspection
  Scenario: Failed reconcile keeps the stale cache visible with an (error) annotation
    Given Devon has fixture "devon-cache-warm"
    And Ollama's directory becomes unreadable between launches due to chmod 000
    When Devon runs "modeltap" and the Ollama reconcile fails
    Then the cached Ollama inventory remains painted
    And Ollama's left-pane row shows "Ollama (error)" alongside the cached model count
    And "~/.modeltap/diagnostics.log" gains a line tagged "reconcile_failed tool=ollama reason=permission_denied"
    And the cache_tools row for Ollama is NOT overwritten

  # ===========================================================================
  # Silent ack indicator when inventory changed between launches
  # ===========================================================================

  @us-26 @ac-26-4 @release-2 @real-io
  Scenario: Inventory change since last reconcile shows the silent ack indicator
    Given Devon has fixture "devon-cache-warm"
    And the cache shows Ollama with 12 models
    And the user ran "ollama pull qwen2.5:32b-q4_K_M" in another terminal since the last reconcile
    When Devon runs "modeltap" and the background reconcile completes for Ollama
    Then the Ollama left-pane row updates to 13 models
    And a tiny blue "*" appears next to the Ollama row name for 3 seconds
    And no modal or dialog is shown
