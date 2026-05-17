# Feature file — tool-model-info-sqlite-cache
# Companion to journey-info-and-cache.yaml; consumed by acceptance-designer (DISTILL) to extend
# docs/feature/modeltap-tui/distill/features/master-acceptance.feature with new US-21..US-27 scenarios.
#
# Persona is Devon Park (parent feature persona). Scenarios use real model names, real paths,
# real timestamps, and real sizes per the no-generic-data rule.

@feature_tool_model_info_sqlite_cache
Feature: Tool & model inspection with SQLite-backed cache
  As Devon Park, a multi-tool local-AI power user,
  I want to drill into per-tool and per-model details inside the TUI
  and have modeltap remember my inventory across launches so warm starts are instant,
  so that I can verify what I have before acting and stop waiting through cold discovery every time I check disk.

  Background:
    Given Devon has Ollama 0.6.4 installed with 12 models including `llama3:8b-instruct-q4_K_M` (4.9 GB) and `mistral:7b-instruct-q4_K_M` (4.4 GB)
    And Devon has llama-cli with 6 GGUF files under `~/llms/`
    And Devon has Hugging Face cache with 31 model directories under `~/.cache/huggingface/hub/`
    And Devon has LM Studio with 9 models under `~/.cache/lm-studio/models/`
    And the modeltap cache file lives at `~/.local/share/modeltap/cache.sqlite` (or `$XDG_DATA_HOME/modeltap/cache.sqlite` if set)

  # ─── Step 1: Launch (warm / cold / recovery) ──────────────────────────────

  @us_23 @us_25
  Scenario: Warm start paints cached inventory within 100 ms
    Given the cache file exists and is valid
    And the cache contains inventory data written at the previous launch 14 minutes ago
    When Devon runs `modeltap`
    Then the TUI paints the cached inventory within 100 ms of process start
    And the summary bar reads "Total: 138.4 GB | 58 models | as of 14 min ago, refreshing..."
    And the bottom bar shows `[r] refresh tool [Shift+R] refresh all` among its shortcuts

  @us_23
  Scenario: Cold start falls back to the ADR-003 skeleton paint when no cache exists
    Given the cache file does not exist
    When Devon runs `modeltap`
    Then the TUI paints the skeleton "discovering..." placeholders within 150 ms
    And full inventory paints within 1.15 seconds (matching parent K3)
    And the summary bar reads "as of just now"

  @us_23
  Scenario: Cache corruption is detected on open and recovered automatically
    Given the cache file `~/.local/share/modeltap/cache.sqlite` exists but returns SQLITE_CORRUPT on open
    When Devon runs `modeltap`
    Then the cache file is renamed to `~/.local/share/modeltap/cache.sqlite.corrupt-<timestamp>` matching `cache\.sqlite\.corrupt-\d{4}-\d{2}-\d{2}T\d{6}`
    And a recovery banner appears reading "Previous cache reset (corrupted or schema mismatch). Renamed to <path>. Cold-start discovery in progress. See ~/.modeltap/diagnostics.log."
    And `~/.modeltap/diagnostics.log` gains a line tagged `cache_recovery reason=corrupted`
    And cold-start discovery proceeds without crashing modeltap

  @us_23
  Scenario: Schema migration runs forward when binary expects a newer schema
    Given the cache PRAGMA user_version is 1
    And the binary's expected_schema_version is 3
    When Devon runs `modeltap`
    Then the migrator runs migrations `1_to_2.sql` and `2_to_3.sql` in order
    And the cache PRAGMA user_version becomes 3
    And `~/.modeltap/diagnostics.log` records `cache_migration from=1 to=3 status=ok`
    And the launch proceeds normally with warm-start paint

  @us_23
  Scenario: Downgrade detected — cache was written by a newer binary
    Given the cache PRAGMA user_version is 5
    And the current binary's expected_schema_version is 3
    When Devon runs `modeltap`
    Then the cache file is renamed to `~/.local/share/modeltap/cache.sqlite.future-version-5`
    And the recovery banner explains the downgrade and the rename target
    And cold-start discovery proceeds

  @us_23
  Scenario: --no-cache bypasses the cache for one launch (ADR-003 behaviour)
    Given a valid cache file exists
    When Devon runs `modeltap --no-cache`
    Then the cache file is neither opened nor written
    And the launch follows the stateless rediscovery path from ADR-003
    And the summary bar reads "as of just now"

  # ─── Step 2: Background reconcile ──────────────────────────────────────────

  @us_25 @us_26
  Scenario: Background reconcile updates the cache after warm-start paint
    Given the warm-start paint has completed from cached data
    When the background reconcile finishes for Ollama at 09:14:22
    Then the `cache.tools` row for Ollama updates with `last_scan_at=2026-05-16T09:14:22`
    And the right-pane re-renders if the inventory changed
    And the summary bar updates to "as of just now"

  @us_26
  Scenario: Per-tool TTL forces cold paint for stale entries
    Given the cache contains Ollama inventory with `last_scan_at` 25 hours ago
    And `cache.tool_ttl_seconds` is 86400 (24 hours)
    When Devon runs `modeltap`
    Then Ollama is NOT painted from cache on warm-start
    And Ollama's left-pane row shows the cold-start spinner
    And cold-start discovery for Ollama proceeds while other tools paint from cache

  @us_26
  Scenario: Failed reconcile keeps the stale cache visible
    Given Ollama's directory becomes unreadable between launches (chmod 000)
    When Devon runs `modeltap` and the Ollama reconcile fails
    Then the cached Ollama inventory remains painted
    And Ollama's left-pane row shows "(error)" alongside the cached model count
    And `~/.modeltap/diagnostics.log` gains a line tagged `reconcile_failed tool=ollama reason=permission_denied`
    And the cache.tools row for Ollama is NOT overwritten (preserves last-known-good)

  @us_25
  Scenario: Inventory change since last reconcile shows the silent ack indicator
    Given the cache shows Ollama with 12 models
    And the user ran `ollama pull qwen2.5:32b-q4_K_M` in another terminal since the last reconcile
    When Devon runs `modeltap` and the background reconcile completes for Ollama
    Then the Ollama left-pane row updates to 13 models
    And a tiny blue `*` appears next to the Ollama row name for 3 seconds
    And no modal or dialog is shown

  # ─── Step 3: Manual refresh ────────────────────────────────────────────────

  @us_24
  Scenario: [r] refreshes the selected tool
    Given Devon has Ollama selected in the left pane
    And no dialog is open
    When Devon presses `r`
    Then a spinner appears next to the Ollama row
    And the summary bar reads "refreshing Ollama..."
    And within 1 second the spinner clears and the summary bar reads "as of just now (Ollama refreshed)"
    And the `cache.tools` row for Ollama updates with the new `last_scan_at`

  @us_24
  Scenario: [Shift+R] refreshes all tools in parallel
    Given no dialog is open
    When Devon presses Shift+R
    Then all four tool rows show the per-tool spinner
    And the summary bar reads "refreshing all tools..."
    And within 2 seconds all spinners clear
    And the `cache.tools` rows for every tool are updated

  @us_24
  Scenario: [r] is a no-op when a dialog is open
    Given the unify dialog is open
    When Devon presses `r`
    Then no refresh is triggered
    And the `[r] refresh tool` shortcut in the bottom bar is dimmed
    And the unify dialog state is preserved

  # ─── Step 4: Tool detail screen (US-21) ────────────────────────────────────

  @us_21
  Scenario: Pressing Enter on a left-pane row opens the tool detail screen
    Given Devon has Ollama selected in the left pane
    When Devon presses Enter
    Then the tool detail screen opens
    And it shows Ollama's discovery root `~/.ollama/models/`
    And it shows the configured search paths under that root
    And it shows model count `12`, disk usage `47.3 GB`, last scan `2026-05-16 09:14:22 (N min ago)`, and plugin version `modeltap-plugin-ollama 0.2.6`
    And it shows the largest model: `llama3:70b-instruct-q4_K_M (39.8 GB)`

  @us_21
  Scenario: Tool detail screen shows undetectable version gracefully
    Given a plugin's `inspect_tool()` returns no version (e.g., llama-cli static binary cannot self-introspect)
    When Devon opens that tool's detail screen
    Then the Version field reads "(not detectable)"
    And no false or stale version is shown
    And the rest of the detail screen renders normally

  @us_21
  Scenario: Last error surfaces in tool detail when discovery failed
    Given Ollama's discovery failed at last scan with `permission denied` reading `~/.ollama/models/manifests/`
    When Devon opens Ollama's detail screen
    Then the Last error field shows "permission denied reading ~/.ollama/models/manifests/ (errno 13)" with the timestamp
    And the bottom bar offers `[r] refresh this tool` to retry after fixing permissions

  @us_21
  Scenario: User-configured search paths are labelled
    Given Devon has added `search_paths = ["/data/models"]` to `~/.modeltap/config.toml` under `[plugins.llama-cli]`
    When Devon opens the llama-cli detail screen
    Then the Search paths section lists `~/llms/ (default)`, `~/models/ (default)`, and `/data/models/ (user config)`

  # ─── Step 5: Model detail screen with tool-native metadata (US-22) ─────────

  @us_22
  Scenario: Model detail surfaces GGUF header metadata
    Given `mistral:7b-instruct-q4_K_M` is registered in Ollama, llama-cli, and Hugging Face
    And the file is a GGUF v3
    When Devon presses Enter on the Mistral row
    Then the model detail screen opens
    And the Metadata section shows `general.architecture : llama`, `general.quantization_version : 2`, `llama.context_length : 32768`, `llama.embedding_length : 4096`
    And the Metadata section provenance reads "introspected <N> ago"
    And the Registered with section lists all 3 tool paths

  @us_22
  Scenario: Model detail surfaces Ollama manifest fields for Ollama models
    Given `llama3:8b-instruct-q4_K_M` is registered in Ollama only
    When Devon opens its model detail
    Then the Metadata section shows excerpts from the Ollama manifest JSON: `config.architecture`, `parameters`, and `template`
    And the Format field reads `Ollama manifest v2` (or the actual manifest format version)

  @us_22
  Scenario: Model detail surfaces HF config.json fields for HF-only models
    Given `meta-llama/Llama-3-8B` is in Hugging Face only (16.0 GB)
    When Devon opens its model detail
    Then the Metadata section shows excerpts from `config.json`: `model_type`, `architectures`, `hidden_size`, `num_attention_heads`, `num_hidden_layers`
    And the Format field reads the detected file format (e.g., `safetensors v2` or `GGUF v3` depending on which file is the primary)

  @us_22
  Scenario: Re-introspect updates metadata provenance
    Given Devon is on the Mistral detail screen
    And the metadata was introspected 2 hours ago
    When Devon presses `r`
    Then `Tool::inspect_model()` re-runs against the current file
    And the Metadata section updates with new values if any
    And the provenance reads "introspected just now"
    And the `cache.models.metadata_introspected_at` column updates

  @us_22
  Scenario: Model detail for an un-introspectable file shows partial info gracefully
    Given a model file's format cannot be parsed (corrupt GGUF, unknown format)
    When Devon opens its model detail
    Then the Format field reads what could be detected (e.g., "GGUF v3 (header partially readable)")
    And the Metadata section shows "(introspection failed — see diagnostics.log)"
    And the screen does not crash
    And the other panels (Registered with, Size on disk, Dedup key) still render

  # ─── Step 6: Pre-mutate revalidation (cache safety rule) ───────────────────

  @us_25 @us_26
  Scenario: Pre-unify validation passes when cache matches filesystem
    Given Mistral-7B is registered in 3 tools per the cache
    And all 3 files match the cached `(mtime, size, inode_dev)` tuple
    When Devon presses `u` on the Mistral row and confirms the dialog
    Then the unify proceeds normally
    And post-action the cache is updated with the new hardlink state (same inode for all 3 paths)

  @us_25 @us_26
  Scenario: Pre-unify validation re-introspects when a file has drifted
    Given Mistral-7B is registered in 3 tools per the cache
    And the llama-cli copy's mtime has changed since the last cache write
    When Devon presses `u`
    Then the validator detects the drift before opening the confirmation dialog
    And the dialog displays "Re-introspecting before proceeding..." with a brief progress indicator
    And the dedup-key / size for the drifted file is recomputed
    And Devon is shown the (possibly updated) reclaim estimate
    And Devon must re-confirm if the reclaim amount changed by more than rounding

  @us_25 @us_26
  Scenario: Pre-mutate validation aborts when a file no longer exists
    Given Mistral-7B is registered in 2 tools per the cache
    And one file has been deleted out-of-band between launch and Devon's action
    When Devon attempts to unify
    Then the pre-flight check refuses with "file no longer exists; refreshing inventory"
    And no destructive action occurs
    And an automatic per-tool refresh is triggered for the affected tool
    And the right pane updates to reflect the missing file

  @us_25 @us_26
  Scenario: Pre-delete-one validation aborts when file has changed since last seen
    Given a model row in the right pane comes from the cache
    And the file's size has changed since the cache write
    When Devon opens the detail screen and presses `d` to delete
    Then the cache safety rule re-stats the file
    And the delete dialog includes a "WARNING: file has changed since last seen" line
    And the dialog requires explicit re-confirmation before proceeding

  # ─── Step 7: SHA256 persistence (US-27 — Release 2 candidate) ──────────────

  @us_27
  Scenario: SHA256 hash persists across launches when the file is unchanged
    Given Devon computed SHA256 for `~/llms/mistral-7b-instruct-q4_K_M.gguf` in a previous session
    And the file's `(mtime, size, inode_dev)` matches the cached entry
    When Devon launches modeltap again and opens the Mistral detail screen
    Then the dedup key displays without recomputing the SHA256
    And the provenance reads "dedup key computed <N> days ago"

  @us_27
  Scenario: SHA256 hash invalidates when mtime, size, or inode_dev differs
    Given Devon computed SHA256 for a file in a previous session
    And the file's mtime has changed since
    When the SHA256 is needed again (detail-screen open or pre-unify)
    Then the cached hash is invalidated
    And a fresh SHA256 computation is queued via the background hash pool (ADR-013)
    And the dedup key shows "(computing...)" until the new hash completes

  @us_27
  Scenario: modeltap cache verify rehashes everything and reports drift
    When Devon runs `modeltap cache verify`
    Then every cached SHA256 entry is recomputed
    And entries where the recomputed hash differs from the cached value are listed in stdout
    And the cache is updated with the recomputed values
    And `~/.modeltap/diagnostics.log` records `cache_verify drift_count=<n>`

  # ─── Concurrency (US-23 / J7) ──────────────────────────────────────────────

  @us_23
  Scenario: Two modeltap processes can read the cache concurrently (SQLite WAL)
    Given a first modeltap process is reading the cache (PRAGMA journal_mode=WAL)
    When a second modeltap process opens the same cache file
    Then both processes coexist without `SQLITE_BUSY` errors during reads
    And both processes display consistent inventory data (each from its own snapshot)

  @us_23
  Scenario: Concurrent cache writes serialise via busy_timeout
    Given two modeltap processes are running with cache writes enabled
    And process A is mid-transaction writing a `cache.tools` row update
    When process B attempts to write a `cache.tools` row update
    Then process B waits up to 5 seconds (`PRAGMA busy_timeout=5000`)
    And process B's write succeeds after process A commits
    And neither process crashes or returns an error to the user

  # ─── Cross-tool integration invariants (extends parent registry) ───────────

  @us_21 @us_22 @us_25
  Scenario: tool.model_count matches across left pane, detail screen, and cache
    Given Ollama has 12 models per the cache
    When Devon opens the Ollama detail screen
    Then the detail screen reads "Model count : 12"
    And the left-pane row reads "Ollama 12"
    And the cache.tools row for Ollama has `model_count=12`
    And all three are equal at every redraw

  @us_22 @us_25
  Scenario: model.size matches across right-pane row, detail screen, and cache
    Given `mistral:7b-instruct-q4_K_M` shows `4.4 GB` in the Ollama right pane
    When Devon opens the Mistral detail screen
    Then "Size on disk" reads "4.4 GB (per-tool)"
    And the cache.models row size matches
    And drift between any two of these is detected by the shared-artifacts validator
