# =============================================================================
# modeltap-tui — Master Acceptance Feature File
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-04-28
#
# Tag glossary:
#   @walking-skeleton  -- WS exit gate; DELIVER ships WS when these pass
#   @release-1         -- "Make duplication visible"
#   @release-2         -- "Reclaim disk safely"
#   @release-3         -- "Built to grow"
#   @us-NN             -- traceability to user-stories.md story ID
#   @destructive       -- modifies the fixture tree on disk
#   @cross-platform    -- exercises per-OS code paths
#   @cross-fs          -- exercises EXDEV / cross-filesystem fallback
#   @k3-latency        -- asserts K3 timing budget
#   @kpi-instrumentation -- asserts JSONL log output (~/.modeltap/launch.log)
#   @plugin-trait      -- exercises Tool trait extensibility (US-18)
#   @property          -- universal invariant (DELIVER may implement as proptest)
#   @real-io           -- uses real filesystem; required for at least one per adapter
#   @adapter-integration -- proves a single driven adapter against real I/O
#   @interactive       -- requires real PTY (expectrl); not headless-mode
#   @infrastructure-failure -- driven-adapter failure scenario
#   @in-memory         -- uses in-memory test double (forbidden in walking skeletons)
#
# Walking-skeleton exit gate (16 scenarios; DELIVER ships when green):
#   @walking-skeleton+@us-01 (5)
#   @walking-skeleton+@us-02 (5)
#   @walking-skeleton+@us-03 (4)
#   @walking-skeleton+@us-05 (5)
#   @walking-skeleton+@us-06 (4)
#   = 22 walking-skeleton scenarios. NOT all 22 must be green for WS exit;
#   the WS subset is: 1 scenario per WS story = 5 minimum:
#     - "Devon launches modeltap and sees Ollama models" (us-01 + us-02 + us-03)
#     - "Devon zaps llama-cli successfully" (us-05)
#     - "Devon sees reclaim message after zap" (us-06)
#   The other 17 are focused/error scenarios graduating to release-1.
# =============================================================================

Feature: Cleanup and Unify Local AI Models — Acceptance Test Suite
  As Devon Park, a local-AI power user running multiple inference tools on macOS/Linux
  I want to discover, deduplicate, and zap locally-downloaded models in one TUI
  So that I can reclaim disk space and use one canonical model copy across tools

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And the bottom bar always displays the available keyboard shortcuts

  # =============================================================================
  # US-01 — TUI launches and quits cleanly  (Walking Skeleton)
  # =============================================================================

  @walking-skeleton @us-01 @real-io
  Scenario: Devon launches modeltap and sees the two-pane layout
    Given Devon's terminal is 100 columns wide
    And Devon has only Ollama installed in fixture "devon-only-ollama"
    When Devon runs "modeltap" in headless mode
    Then within 1 second the TUI renders the two-pane layout
    And the left pane lists "Ollama" with its model count
    And the bottom bar shows "[<-/->] tools  [up/down] models  [u] unify  [z] zap tool  [?] help  [q] quit"
    And modeltap exits cleanly with code 0

  @walking-skeleton @us-01
  Scenario: Devon quits with q
    Given Devon has launched modeltap in headless mode against fixture "devon-only-ollama"
    When Devon presses "q"
    Then modeltap exits with code 0
    And the terminal is restored to normal cursor and color state

  @walking-skeleton @us-01
  Scenario: Devon quits with Ctrl+C
    Given Devon has launched modeltap in headless mode against fixture "devon-only-ollama"
    When Devon presses Ctrl+C
    Then modeltap exits with code 130
    And the terminal is restored to normal cursor and color state

  @walking-skeleton @us-01
  Scenario: Terminal too narrow refuses to start
    Given Devon's terminal is 60 columns wide
    When Devon runs "modeltap" in headless mode
    Then modeltap prints "Terminal too narrow: need at least 80 columns, found 60" to stderr
    And modeltap exits with code 2
    And no partial TUI is rendered

  @us-01 @infrastructure-failure
  Scenario: Modeltap log directory is unwritable
    Given Devon's "${TMPDIR}/modeltap-test-${SCENARIO_ID}" directory exists with mode 0500
    When Devon runs "modeltap" in headless mode against fixture "devon-only-ollama"
    Then modeltap renders the two-pane layout
    And modeltap prints "warning: cannot write launch log to ${LOG_DIR}" to stderr
    And modeltap exits cleanly with code 0

  # =============================================================================
  # US-02 — Discover Ollama models  (Walking Skeleton)
  # =============================================================================

  @walking-skeleton @us-02 @real-io @adapter-integration
  Scenario: Devon sees all 12 Ollama models with sizes
    Given Devon has Ollama installed in fixture "devon-multi-tool" containing 12 models totaling 47.3 GB
    When Devon runs "modeltap" in headless mode
    And selects "Ollama" in the left pane
    Then the right pane lists 12 models with their tags
    And each row shows the model size in GB
    And the right-pane header reads "Models in Ollama (12, 47.3 GB)"

  @walking-skeleton @us-02
  Scenario: Devon has only Ollama installed
    Given Devon has only Ollama installed in fixture "devon-only-ollama"
    When Devon runs "modeltap" in headless mode
    Then the left pane shows "Ollama" with model count
    And the left pane shows "llama-cli" with "0" and "(not installed)"
    And the left pane shows "Hugging Face" with "0" and "(not installed)"
    And the left pane shows "LM Studio" with "0" and "(not installed)"

  @us-02 @infrastructure-failure
  Scenario: Unreadable Ollama directory does not crash modeltap
    Given Devon's Ollama directory in fixture "devon-permission-denied" has mode 0000
    When Devon runs "modeltap" in headless mode
    Then the left pane shows "Ollama" with "(error)"
    And the diagnostics log contains an event with level "ERROR" and target "modeltap_plugin_ollama::discover"
    And the other tools render normally

  @us-02
  Scenario: Ollama deduplicates blob references in size accounting
    Given Devon has Ollama in fixture "devon-multi-tool" with 2 manifest entries pointing at the same blob "sha256-abc123"
    When Devon runs "modeltap" in headless mode
    And selects "Ollama" in the left pane
    Then the right-pane header reports the blob's size exactly once in the total GB

  @us-02
  Scenario: Ollama discovery completes within 2 seconds for 200 models
    Given Devon has Ollama in fixture "k3-bench" with 200 models
    When Devon runs "modeltap" in headless mode
    Then the JSONL log event "launch.timing" records "plugin_timings_ms.ollama" less than 2000

  # =============================================================================
  # US-03 — Two-pane layout  (Walking Skeleton)
  # =============================================================================

  @walking-skeleton @us-03
  Scenario: Default selection is the alphabetically first installed tool
    Given Devon has fixture "devon-multi-tool" with all four tools installed
    When Devon runs "modeltap" in headless mode
    Then the left pane highlights "Hugging Face" (alphabetically first)
    And the right pane shows the Hugging Face model list

  @walking-skeleton @us-03
  Scenario: Right Arrow switches to the next tool
    Given Devon has launched modeltap against fixture "devon-multi-tool"
    And Ollama is highlighted in the left pane
    When Devon presses Right Arrow
    Then the highlight moves to "llama-cli"
    And the right-pane header reads "Models in llama-cli (6, 21.4 GB)"

  @walking-skeleton @us-03
  Scenario: Down Arrow scrolls a long model list
    Given Devon has launched modeltap against fixture "devon-multi-tool"
    And Devon has selected "Hugging Face" with 31 models
    And the visible window holds 28 rows
    When Devon presses Down Arrow 3 times past the last visible row
    Then the bottom-right indicator shows "29/31"

  @walking-skeleton @us-03
  Scenario: Unbound key is silently ignored
    Given Devon has launched modeltap against fixture "devon-multi-tool"
    When Devon presses "x"
    Then no action is taken
    And the bottom bar briefly highlights as a visual reminder
    And modeltap exits cleanly when Devon then presses "q"

  # =============================================================================
  # US-04 — Row metadata (indicator + size + also-in)
  # =============================================================================

  @release-1 @us-04
  Scenario: Multi-tool model shows the * indicator and other tools
    Given fixture "devon-multi-tool" registers Mistral-7B-v0.3 q4_K_M GGUF in Ollama, llama-cli, and Hugging Face with identical SHA256
    When Devon runs "modeltap" in headless mode
    And selects "Ollama" in the left pane
    Then the row for "mistral:7b-instruct-q4_K_M" begins with "*"
    And the row shows "also in: llama-cli, Hugging Face"

  @release-1 @us-04
  Scenario: Single-tool format-compatible model shows o
    Given fixture "devon-multi-tool" registers Llama-3-8B GGUF only in Hugging Face
    And Ollama, llama-cli, and LM Studio all accept GGUF
    When Devon runs "modeltap" in headless mode
    And selects "Hugging Face" in the left pane
    Then the row for "meta-llama/Llama-3-8B" begins with "o"
    And the row shows no "also in:" annotation

  @release-1 @us-04
  Scenario: Unknown format shows ? indicator
    Given fixture "devon-multi-tool" contains a model file with an unrecognized format
    When Devon runs "modeltap" in headless mode
    And selects the tool containing the unknown-format model
    Then the row begins with "?"
    And the format field shows "[format: ?]"

  @release-1 @us-04
  Scenario: NO_COLOR environment variable still preserves the indicator symbol
    Given fixture "devon-multi-tool" contains a "!"-marked AWQ model in Hugging Face
    And the environment variable "NO_COLOR" is set to "1"
    When Devon runs "modeltap" in headless mode
    And selects "Hugging Face" in the left pane
    Then the AWQ row begins with "!"
    And no ANSI color codes appear in the captured frame

  # =============================================================================
  # US-05 — Zap a tool's models with typed confirmation  (Walking Skeleton)
  # =============================================================================

  @walking-skeleton @us-05 @destructive @real-io @adapter-integration
  Scenario: Devon zaps llama-cli successfully
    Given Devon has fixture "devon-multi-tool" with llama-cli holding 6 models (4 shared, 2 unique) totaling 21.4 GB
    When Devon runs "modeltap" in headless mode
    And selects "llama-cli" in the left pane
    And presses "z"
    And types "llama-cli"
    And presses Enter
    Then the 2 unique model files are removed from the llama-cli fixture directory
    And the 4 shared model registrations are removed from llama-cli
    And the 4 shared model files remain in their other tools' directories
    And the right pane shows "Last action: zap llama-cli (success)"
    And the right pane shows "Reclaimed: 14.6 GB (6.8 GB retained — also linked from other tools)"

  @walking-skeleton @us-05 @destructive
  Scenario: Wrong typed name cancels zap
    Given Devon has launched modeltap against fixture "devon-multi-tool"
    And Devon has opened the zap dialog for "llama-cli"
    When Devon types "llamacli" and presses Enter
    Then the dialog closes with no models deleted
    And the llama-cli fixture directory is unchanged

  @walking-skeleton @us-05
  Scenario: Zap on empty tool shows benign message
    Given Devon has launched modeltap against fixture "devon-empty"
    And Devon has selected "Hugging Face" with 0 models
    When Devon presses "z"
    Then the dialog reads "Hugging Face has 0 models. Nothing to zap."
    And only "[Esc] close" is offered
    And no destructive action is performed when Devon presses Esc

  @walking-skeleton @us-05
  Scenario: Esc cancels zap at any point
    Given Devon has opened the zap dialog for "llama-cli" against fixture "devon-multi-tool"
    When Devon presses Esc
    Then the dialog closes
    And no models are deleted

  @walking-skeleton @us-05 @destructive @kpi-instrumentation
  Scenario: Successful zap emits action.zap_all event
    Given Devon has fixture "devon-multi-tool" with llama-cli holding 6 models
    When Devon zaps llama-cli successfully
    Then the JSONL log contains exactly one "action.zap_all" event
    And the event has tool == "llama-cli"
    And the event has models_removed == 6
    And the event has bytes_reclaimed > 0
    And the event has outcome == "success"

  # =============================================================================
  # US-05b — Single-model delete  (NEW — added per BE-7 patch / ADR-009)
  # =============================================================================

  @release-2 @us-05b @destructive @real-io @adapter-integration
  Scenario: Shared single-model delete uses [y/n] confirmation
    Given fixture "devon-multi-tool" registers Mistral-7B in both Ollama and llama-cli with identical SHA256
    And Devon is on the Mistral detail screen viewing the llama-cli registration
    When Devon presses "d"
    And presses "y"
    Then the llama-cli copy of Mistral is removed from disk
    And the Ollama copy of Mistral remains in its directory
    And the right pane shows "Last action: delete-from-one (success)"
    And the right pane shows "Reclaimed: 4.4 GB"

  @release-2 @us-05b @destructive
  Scenario: Unique single-model delete requires typed model id
    Given fixture "devon-multi-tool" contains an AWQ model registered only in Hugging Face
    And Devon is on the detail screen for "TheBloke/something-AWQ"
    When Devon presses "d"
    Then the dialog reads "DELETE TheBloke/something-AWQ (3.2 GB) from Hugging Face. This is the ONLY copy"
    And the dialog requires the user to type "TheBloke/something-AWQ"
    When Devon types "TheBloke/something-AWQ"
    And presses Enter
    Then the AWQ file is removed from the Hugging Face fixture directory
    And the right pane shows "Reclaimed: 3.2 GB"

  @release-2 @us-05b @destructive
  Scenario: Unique single-model delete cancels on wrong typed id
    Given Devon is on the detail screen for a unique-to-llama-cli model "oddball-model"
    When Devon presses "d"
    And types "oddball" (incomplete)
    And presses Enter
    Then the dialog closes with no file deleted
    And the oddball-model file remains in the llama-cli fixture directory

  @release-2 @us-05b
  Scenario: Esc cancels single-model delete at any point
    Given Devon has opened the single-model delete dialog for any model
    When Devon presses Esc
    Then no file is deleted
    And the detail screen returns

  @release-2 @us-05b @destructive @kpi-instrumentation
  Scenario: Successful single-model delete emits action.zap_one event
    Given Devon has fixture "devon-multi-tool"
    When Devon deletes the llama-cli copy of Mistral via the [d] shortcut
    Then the JSONL log contains exactly one "action.zap_one" event
    And the event has tool == "llama-cli"
    And the event has bytes_reclaimed == 4724464026
    And the event has was_shared == true
    And the event has outcome == "success"

  # =============================================================================
  # US-06 — Show last action and reclaimed bytes  (Walking Skeleton)
  # =============================================================================

  @walking-skeleton @us-06
  Scenario: Successful zap shows reclaimed and retained bytes
    Given Devon has just zapped llama-cli reclaiming 14.6 GB and retaining 6.8 GB against fixture "devon-multi-tool"
    When the zap action completes
    Then the right pane shows "Last action: zap llama-cli (success)"
    And the right pane shows "Reclaimed: 14.6 GB (6.8 GB retained — also linked from other tools)"
    And the summary bar shows the updated total disk usage within 500 milliseconds

  @walking-skeleton @us-06
  Scenario: Successful unify shows hardlink count
    Given Devon has just unified Mistral-7B with 3 hardlinks created against fixture "devon-multi-tool"
    When the unify action completes
    Then the right pane shows "Last action: unify mistral:7b (success)"
    And the body reads "Reclaimed: 8.8 GB (1 inode, 3 hardlinks)"

  @walking-skeleton @us-06
  Scenario: Partial unify shows partial-success message
    Given Devon ran unify against fixture "devon-cross-fs" and 2 of 3 targets succeeded
    When the action completes
    Then the right pane shows "Last action: unify mistral:7b (partial: 2 of 3 targets linked)"
    And the failed target's path and reason are shown below

  @walking-skeleton @us-06
  Scenario: Last-action message clears when Devon navigates
    Given Devon sees a "Last action: zap llama-cli (success)" message in the right pane
    When Devon presses Right Arrow to switch tools
    Then the right pane shows the new tool's models
    And the "Last action" line is no longer displayed

  # =============================================================================
  # US-07 — Discover llama-cli models
  # =============================================================================

  @release-1 @us-07 @real-io @adapter-integration
  Scenario: Default search paths are scanned
    Given fixture "devon-multi-tool" contains "~/llms/mistral-7b-q4.gguf" of size 4.4 GB
    When Devon runs "modeltap" in headless mode
    And selects "llama-cli" in the left pane
    Then the model "mistral-7b-q4.gguf" appears with size "4.4 GB"

  @release-1 @us-07
  Scenario: Configured additional search path is honored
    Given fixture "devon-empty" contains "/data/models/extra.gguf"
    And the user has set "[plugins.llama-cli] search_paths = [\"/data/models\"]" in config
    When Devon runs "modeltap" in headless mode
    Then "extra.gguf" appears in the llama-cli model list

  @release-1 @us-07 @infrastructure-failure
  Scenario: Corrupt GGUF flagged but does not crash discovery
    Given fixture "devon-multi-tool" contains a truncated file "~/llms/corrupt.gguf"
    When Devon runs "modeltap" in headless mode
    And selects "llama-cli" in the left pane
    Then the row for "corrupt.gguf" shows "[format: corrupt]"
    And modeltap continues to render the other 5 llama-cli models

  @release-1 @us-07
  Scenario: GGUF header parsing extracts quantization label
    Given fixture "devon-multi-tool" contains "~/llms/llama-3-8b-q4_K_M.gguf" with a valid GGUF header
    When Devon runs "modeltap" in headless mode
    And selects "llama-cli" in the left pane
    Then the row's display label includes "q4_K_M"

  # =============================================================================
  # US-08 — Bottom bar with shortcuts always visible
  # =============================================================================

  @release-2 @us-08
  Scenario: Unavailable shortcuts are dimmed
    Given Devon has launched modeltap against fixture "devon-multi-tool"
    And no model is selected in the right pane
    When the bottom bar renders
    Then "[u] unify" is shown but dimmed
    And "[z] zap tool" is shown brightly

  @release-2 @us-08
  Scenario: Detail screen shortcuts replace the main bar
    Given Devon has launched modeltap against fixture "devon-multi-tool"
    When Devon presses Enter on a model row to open the detail screen
    Then the bottom bar shows "[Esc] back   [u] unify   [d] delete-from-one   [?] help"
    And the main shortcuts are not displayed

  @release-2 @us-08
  Scenario: Help overlay shows all shortcuts
    Given Devon has launched modeltap against fixture "devon-multi-tool"
    When Devon presses "?"
    Then a help overlay opens listing all shortcuts grouped by context
    When Devon presses Esc
    Then the help overlay closes

  # =============================================================================
  # US-09 — Compatibility indicator engine
  # =============================================================================

  @release-1 @us-09
  Scenario: Multi-tool model gets *
    Given fixture "devon-multi-tool" has a model registered with 2+ tools matching by SHA256
    When Devon runs "modeltap" in headless mode
    Then the model's row indicator is "*" in every tool's pane

  @release-1 @us-09
  Scenario: Format-compatible single-tool model gets o
    Given fixture "devon-multi-tool" has Llama-3-8B GGUF in only Hugging Face
    And Ollama and llama-cli both declare GGUF in accepted_formats
    When Devon runs "modeltap" in headless mode
    Then the Llama-3-8B row indicator is "o"

  @release-1 @us-09
  Scenario: Format-locked model gets !
    Given fixture "devon-multi-tool" has TheBloke/something-AWQ in only Hugging Face
    And no other supported tool declares AWQ in accepted_formats
    When Devon runs "modeltap" in headless mode
    Then the AWQ row indicator is "!"

  @release-1 @us-09 @property
  Scenario: For any inventory, every row indicator is one of {o, *, !, ?}
    Given any populated inventory built from fixture "devon-multi-tool"
    When indicators are computed for every model
    Then every row begins with one of "o", "*", "!", "?"

  # =============================================================================
  # US-10 — Unify a model across tools using hardlinks
  # =============================================================================

  @release-2 @us-10 @destructive @real-io @adapter-integration
  Scenario: Unify creates hardlinks and reclaims disk
    Given fixture "devon-multi-tool" has Mistral-7B with 3 separate copies of 4.4 GB across 3 tools on the same filesystem
    When Devon runs "modeltap" in headless mode
    And opens the Mistral detail screen
    And presses "u"
    And presses Enter to confirm
    Then one of the existing tool-owned Mistral paths is chosen as canonical
    And the other 2 paths stat to the same inode as the canonical
    And no file is created under "${LOG_DIR}/store"
    And the right pane shows "Reclaimed: 8.8 GB"

  @release-2 @us-10
  Scenario: Already-unified model shows benign message
    Given fixture "devon-multi-tool" has a model whose 3 registered paths all stat to the same inode
    When Devon opens the model's detail screen
    And presses "u"
    Then the dialog reads "Already unified — all 3 registrations point to the same file."
    And no [Enter] proceed action is offered

  @release-2 @us-10 @destructive
  Scenario: Each tool's registration remains valid after unify
    Given fixture "devon-multi-tool" has Mistral registered in Ollama as a manifest pointing at a blob
    When Devon unifies Mistral with the llama-cli copy as canonical
    Then the Ollama blob path "${OLLAMA_DIR}/blobs/sha256-<mistral-hash>" stats to the same inode as the llama-cli "mistral-7b-q4.gguf"
    And the Ollama manifest at "${OLLAMA_DIR}/manifests/registry.ollama.ai/library/mistral/7b-instruct-q4_K_M" still references the blob hash

  @release-2 @us-10 @destructive @kpi-instrumentation
  Scenario: Successful unify emits action.unify event
    Given Devon has just unified Mistral against fixture "devon-multi-tool"
    Then the JSONL log contains exactly one "action.unify" event
    And the event has bytes_reclaimed == 9663676416
    And the event has outcome == "success"
    And the event has tools_unified including "ollama" and "llama-cli"

  @release-2 @us-10
  Scenario: Unify is refused for a single-tool model
    Given Devon opens the detail screen for an AWQ model registered only in Hugging Face
    When Devon presses "u"
    Then the [u] shortcut is dimmed
    And the screen shows "single tool — unify not applicable"

  @release-2 @us-10 @cross-fs
  Scenario: Unify with one cross-fs target offers per-target choice
    Given fixture "devon-cross-fs" has Mistral in 3 tools where llama-cli is on a different filesystem
    When Devon presses "u" on Mistral
    And presses Enter to proceed
    Then the dialog reads "1 of 3 targets on different filesystem"
    And the dialog offers "[s] skip cross-fs / [c] copy / [x] cancel"

  # =============================================================================
  # US-11 — Updated totals after action
  # =============================================================================

  @release-2 @us-11 @destructive
  Scenario: Totals update after zap within 500ms
    Given Devon's pre-zap total was 138.4 GB shown in the summary bar against fixture "devon-multi-tool"
    When Devon zaps llama-cli reclaiming 14.6 GB
    Then within 500 milliseconds the summary bar shows "123.8 GB"

  @release-2 @us-11 @destructive
  Scenario: Totals update after unify (disk down, model count steady)
    Given Devon's pre-unify summary bar shows "138.4 GB" and "58 models" against fixture "devon-multi-tool"
    When Devon unifies Mistral reclaiming 8.8 GB
    Then the summary bar shows "129.6 GB"
    And the summary bar shows "58 models" (unchanged)

  @release-2 @us-11 @infrastructure-failure
  Scenario: Refresh failure shows degraded indicator
    Given Devon has just completed a zap action against fixture "devon-multi-tool"
    And the post-action discovery rebuild fails because the tool directory was removed
    When the summary bar tries to refresh
    Then it shows the previous values with "(refresh failed)" indicator
    And "[r] retry" is offered in the bottom bar

  # =============================================================================
  # US-12 — Discover Hugging Face cache models
  # =============================================================================

  @release-1 @us-12 @real-io @adapter-integration
  Scenario: Default HF cache is discovered
    Given fixture "devon-multi-tool" contains 31 model directories under "${HF_HOME}/hub/"
    When Devon runs "modeltap" in headless mode
    And selects "Hugging Face" in the left pane
    Then 31 models are listed with their org/repo ids and sizes

  @release-1 @us-12
  Scenario: HF_HOME override is honored
    Given the environment variable "HF_HOME" is set to "/data/hf-cache"
    And "/data/hf-cache/hub/" contains 5 model directories from fixture "devon-multi-tool"
    When Devon runs "modeltap" in headless mode
    Then 5 models are listed under "Hugging Face"

  @release-1 @us-12 @infrastructure-failure
  Scenario: Broken symlinks are flagged
    Given fixture "devon-multi-tool" contains a model directory with a broken snapshot symlink
    When Devon runs "modeltap" in headless mode
    And selects "Hugging Face" in the left pane
    Then the affected model row shows "[broken: missing blob]"
    And its size does not contribute to the Hugging Face disk usage shown in the header

  @release-1 @us-12
  Scenario: Hugging Face model id is org/repo path-style
    Given fixture "devon-multi-tool" contains "${HF_HOME}/hub/models--meta-llama--Llama-3-8B/"
    When Devon runs "modeltap" in headless mode
    And selects "Hugging Face" in the left pane
    Then the row's id reads "meta-llama/Llama-3-8B"

  # =============================================================================
  # US-13 — Model detail screen
  # =============================================================================

  @release-1 @us-13
  Scenario: Detail screen shows duplicate paths and reclaim estimate
    Given fixture "devon-multi-tool" has Mistral with 3 separate file copies of 4.4 GB across 3 tools
    When Devon selects the Mistral row and presses Enter
    Then the detail screen lists all 3 paths
    And the status reads "NOT UNIFIED — 3 separate copies (13.2 GB total)"
    And the reclaim estimate reads "If unified: would reclaim 8.8 GB"

  @release-1 @us-13
  Scenario: Single-tool model detail dims [u]
    Given fixture "devon-multi-tool" has an AWQ model only in Hugging Face
    When Devon opens its detail screen
    Then the screen shows 1 path
    And the [u] shortcut is dimmed in the bottom bar
    And the screen shows "single tool — unify not applicable"

  @release-1 @us-13
  Scenario: Already-unified model detail shows hardlink count
    Given fixture "devon-multi-tool" has a model whose 3 registered paths all stat to the same inode
    When Devon opens its detail screen
    Then the status reads "UNIFIED — 1 inode, 3 hardlinks"
    And the screen shows "Reclaimed: 8.8 GB"

  @release-1 @us-13
  Scenario: Esc returns from detail to main view
    Given Devon has opened any detail screen against fixture "devon-multi-tool"
    When Devon presses Esc
    Then the main two-pane view is shown
    And the previously-selected row remains highlighted

  # =============================================================================
  # US-14 — Dry-run preview before unify
  # =============================================================================

  @release-2 @us-14
  Scenario: Dry-run shows the plan without touching disk
    Given Devon has opened the unify dialog for Mistral against fixture "devon-multi-tool"
    When Devon presses "n" for dry-run
    Then the dialog shows "(dry-run) Would create canonical at ${SOME_TOOL_PATH}"
    And the dialog shows "Would create hardlinks at" listing 2 target paths
    And the dialog shows "Reclaim: 8.8 GB"
    And no inode in the fixture tree changes

  @release-2 @us-14 @cross-fs
  Scenario: Dry-run reveals cross-filesystem issue
    Given Devon has opened the unify dialog for Mistral against fixture "devon-cross-fs"
    When Devon presses "n" for dry-run
    Then the dry-run output includes "WARNING: target ${LLAMACLI_PATH} on different filesystem — would fall back to copy"
    And no inode in the fixture tree changes

  @release-2 @us-14 @kpi-instrumentation
  Scenario: Dry-run emits action.unify_dry_run event
    Given Devon has just dry-run unify on Mistral
    Then the JSONL log contains exactly one "action.unify_dry_run" event
    And the event has bytes_would_reclaim == 9663676416
    And the JSONL log contains zero "action.unify" events for this session

  # =============================================================================
  # US-15 — Discover LM Studio models
  # =============================================================================

  @release-1 @us-15 @real-io @adapter-integration
  Scenario: LM Studio cache is discovered
    Given fixture "devon-multi-tool" contains 9 models under "~/.cache/lm-studio/models/"
    When Devon runs "modeltap" in headless mode
    And selects "LM Studio" in the left pane
    Then 9 models are listed with their ids and sizes

  @release-1 @us-15
  Scenario: Older LM Studio path is honored
    Given fixture "devon-multi-tool" contains models under "~/.lmstudio/models/" (older convention)
    And no "~/.cache/lm-studio/" directory exists
    When Devon runs "modeltap" in headless mode
    And selects "LM Studio" in the left pane
    Then the older-path models are listed

  @release-1 @us-15
  Scenario: LM Studio not installed shows benign message
    Given neither "~/.cache/lm-studio/" nor "~/.lmstudio/" exist in fixture "devon-empty"
    When Devon runs "modeltap" in headless mode
    Then the left pane shows "LM Studio" with "0" and "(not installed)"

  # =============================================================================
  # US-16 — Format-locked indicator (red !)
  # =============================================================================

  @release-1 @us-16
  Scenario: AWQ model gets red !
    Given fixture "devon-multi-tool" has TheBloke/something-AWQ only in Hugging Face
    And no other tool's accepted_formats lists AWQ
    When Devon runs "modeltap" in headless mode
    And selects "Hugging Face" in the left pane
    Then the AWQ row's first character is "!"
    And the captured frame's cell at the AWQ row indicator position has foreground color "Red"

  @release-1 @us-16
  Scenario: Format-locked model in detail screen
    Given Devon is on the AWQ model's detail screen
    Then the [u] shortcut is dimmed
    And the screen shows "single tool — unify not applicable"

  @release-1 @us-16
  Scenario: Missing capability metadata produces ? not !
    Given a registered plugin "broken-plugin" returns an empty slice from accepted_formats()
    When Devon runs "modeltap" in headless mode
    And selects "broken-plugin" in the left pane
    Then every row begins with "?"
    And the diagnostics log contains a warning "plugin broken-plugin returned empty accepted_formats()"

  # =============================================================================
  # US-17 — Detect running tools and prompt-then-retry
  # =============================================================================
  # Per intake Q5 (post-edit): detect-and-prompt-then-retry, NOT silent override.

  @release-2 @us-17
  Scenario: Running tool surfaces close-and-retry prompt
    Given the FsProbe adapter is configured with fake-lsof script "lsof-running-ollama" reporting "ollama PID 4421 holds ${OLLAMA_DIR}/blobs/sha256-<mistral>"
    When Devon presses "u" on a model registered in Ollama against fixture "devon-multi-tool"
    Then the dialog reads "Ollama is running and has this file open. Close Ollama and retry."
    And the dialog offers "[r] retry" and "[Esc] cancel"
    And no filesystem mutation occurs while the dialog is open

  @release-2 @us-17
  Scenario: After user closes the tool, retry succeeds
    Given the running-tool dialog is showing for Ollama
    When the FsProbe adapter is reconfigured with fake-lsof script "lsof-empty"
    And Devon presses "r"
    Then the unify proceeds normally
    And the right pane shows "Reclaimed: 8.8 GB"

  @release-2 @us-17
  Scenario: No running tools, no warning
    Given the FsProbe adapter reports no holding processes
    When Devon presses "u" on a Mistral model against fixture "devon-multi-tool"
    Then the dialog has no running-tool warning section
    And the dialog proceeds directly to the unify plan

  @release-2 @us-17
  Scenario: lsof unavailable surfaces explicit message
    Given the FsProbe adapter returns LsofResult::Unavailable
    When Devon opens the unify dialog
    Then the dialog includes "Running-tool detection unavailable on this system"
    And Devon can still proceed at his own risk

  # =============================================================================
  # US-18 — Plugin trait — adding a 5th tool requires no core changes
  # =============================================================================

  @release-3 @us-18 @plugin-trait @real-io
  Scenario: A new plugin appears in the left pane on launch
    Given the workspace contains a 5th plugin "atomic-chat" implementing the Tool trait per fixture "riley-fifth-plugin"
    And no source file under "crates/modeltap-core/src/" was modified
    When Devon runs "modeltap" in headless mode
    Then the left pane includes "Atomic Chat" alongside the original four tools

  @release-3 @us-18 @plugin-trait @infrastructure-failure
  Scenario: A plugin panic does not crash modeltap
    Given a registered plugin "broken-plugin" panics inside its discover() method
    When Devon runs "modeltap" in headless mode
    Then the left pane shows "broken-plugin" with "(error)"
    And the other plugins render normally
    And the diagnostics log contains an event with level "ERROR" and field "panic_message"

  @release-3 @us-18 @plugin-trait
  Scenario: Architecture rule — modeltap-core has no plugin dependency
    Given the workspace has been built
    When the architecture-lint test runs
    Then no plugin crate appears in modeltap-core's direct dependencies
    And no plugin crate depends on another plugin crate
    And no concrete plugin crate appears in modeltap-tui's direct dependencies

  @release-3 @us-18 @plugin-trait @kpi-instrumentation
  Scenario: launch.inventory event lists all registered plugins
    Given Devon has fixture "riley-fifth-plugin" with 5 registered plugins
    When Devon runs "modeltap" in headless mode
    Then the JSONL log "launch.inventory" event has tools_registered listing all 5 plugin names

  # =============================================================================
  # US-19 — Hardlink fallback when cross-filesystem
  # =============================================================================

  @release-2 @us-19 @cross-fs @destructive
  Scenario: All-same-filesystem unify proceeds normally
    Given fixture "devon-multi-tool" places all Mistral copies on the same filesystem
    When Devon proceeds with unify
    Then hardlinks are created for all targets
    And no fallback prompt appears

  @release-2 @us-19 @cross-fs @destructive
  Scenario: Skip option leaves cross-fs target untouched
    Given fixture "devon-cross-fs" has 1 of 3 Mistral copies on a different filesystem
    When Devon presses "u" then Enter, then "s" to skip cross-fs targets
    Then the 2 same-fs targets become hardlinks to the canonical
    And the cross-fs target remains an independent file at its original path
    And the right pane shows "Reclaimed: 4.4 GB" and "Skipped: 1 cross-fs target"

  @release-2 @us-19 @cross-fs @destructive
  Scenario: Copy option duplicates bytes to cross-fs target
    Given fixture "devon-cross-fs" has 1 of 3 Mistral copies on a different filesystem
    When Devon presses "u" then Enter, then "c" to copy to cross-fs targets
    Then the 2 same-fs targets become hardlinks to the canonical
    And the cross-fs target's file contents match the canonical (SHA256-equal) but with a different inode
    And the right pane shows "Reclaimed: 4.4 GB" and "Copied: 1 cross-fs target (no reclaim)"

  @release-2 @us-19 @cross-fs
  Scenario: All-cross-fs unify is refused
    Given fixture "devon-cross-fs" places all Mistral copies on mutually different filesystems
    When Devon presses "u"
    Then the dialog reads "all targets on different filesystems — unify cannot proceed"
    And no action is performed

  # =============================================================================
  # US-20 — Cross-platform path discovery (macOS + Linux + WSL)
  # =============================================================================

  @release-3 @us-20 @cross-platform
  Scenario Outline: Discovery uses per-OS default paths
    Given the environment variable "MODELTAP_FORCE_PLATFORM" is set to "<platform>"
    And fixture "devon-multi-tool" contains all four tools at their <platform> default paths
    When Devon runs "modeltap" in headless mode
    Then all four tools are discovered with non-zero model counts
    And the JSONL log "launch.started" event has platform == "<platform>"

    Examples:
      | platform     |
      | macos-aarch64|
      | linux-x86_64 |
      | linux-aarch64|

  @release-3 @us-20 @cross-platform
  Scenario: WSL is treated as Linux
    Given the environment variable "MODELTAP_FORCE_PLATFORM" is set to "linux-x86_64"
    And the user is running modeltap inside WSL2 against fixture "devon-multi-tool"
    When Devon runs "modeltap" in headless mode
    Then discovery succeeds with the same paths as native Linux
    And the JSONL log platform field reads "linux-x86_64"

  @release-3 @us-20 @cross-platform
  Scenario: Native Windows binary refuses to run with clear message
    Given the modeltap binary is executed on native Windows (not WSL)
    When the binary starts
    Then it prints "Windows is supported only via WSL — see https://learn.microsoft.com/windows/wsl/install" to stderr
    And it exits with code 64

  # =============================================================================
  # K3 latency benchmark (cross-cutting; see k3-benchmark-spec.md)
  # =============================================================================

  @k3-latency @kpi-instrumentation @real-io
  Scenario: First paint latency under 1 second on K3 fixture
    Given fixture "k3-bench" with 200 models across 4 plugins
    When Devon runs "modeltap" in headless mode with --emit-timing
    Then the JSONL log "launch.timing" event has process_start_to_first_paint_ms < 1000
    And the JSONL log "launch.timing" event has full_inventory_ms < 5000

  @k3-latency @kpi-instrumentation
  Scenario: Full inventory latency under 5 seconds on K3 fixture
    Given fixture "k3-bench" with 200 models across 4 plugins
    When Devon runs "modeltap" in headless mode with --emit-timing
    Then the timing JSON printed to stdout has full_inventory_ms < 5000

  # =============================================================================
  # KPI instrumentation invariants (cross-cutting)
  # =============================================================================

  @kpi-instrumentation
  Scenario: Every session emits launch.started as the first event
    Given Devon runs "modeltap" in headless mode against fixture "devon-multi-tool"
    Then the JSONL log's first event has event == "launch.started"
    And the event has session_id matching ULID format
    And the event has modeltap_version matching semver

  @kpi-instrumentation
  Scenario: launch.inventory is privacy-preserving
    Given Devon has fixture "devon-multi-tool"
    When Devon runs "modeltap" in headless mode
    Then the JSONL log "launch.inventory" event contains no model names
    And contains no file paths
    And contains no SHA256 hashes
    And contains only counts and tool names

  @kpi-instrumentation
  Scenario: launch.ended emitted on clean quit
    Given Devon runs "modeltap" in headless mode
    When Devon presses "q"
    Then the JSONL log's last event has event == "launch.ended"
    And the event has session_duration_ms > 0

  @kpi-instrumentation
  Scenario: launch.ended NOT emitted on Ctrl+C
    Given Devon runs "modeltap" in headless mode
    When Devon presses Ctrl+C
    Then the JSONL log's last event is not "launch.ended"
