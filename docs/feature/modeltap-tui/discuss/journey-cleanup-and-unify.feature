Feature: Cleanup and Unify Local AI Models
  As Devon Park, a local-AI power user running multiple inference tools on macOS/Linux
  I want to see, deduplicate, and zap locally-downloaded models in one TUI
  So that I can reclaim disk space and use one canonical model copy across tools

  Background:
    Given Devon has installed modeltap on macOS or Linux
    And Devon has at least one of {Ollama, llama-cli, Hugging Face cache, LM Studio} installed
    And the bottom bar always displays the available keyboard shortcuts

  # ---------------------------------------------------------------
  # Step 1 — Launch
  # ---------------------------------------------------------------

  Scenario: TUI launches and shows inventory within one second
    Given Devon has Ollama (12 models), llama-cli (6), Hugging Face (31), LM Studio (9) installed
    When Devon runs "modeltap" from a terminal
    Then within 1 second the TUI is rendered
    And the left pane lists all four tools with their model counts
    And the right pane shows the models registered with the first tool
    And the bottom bar lists "[u] unify  [z] zap tool  [?] help  [q] quit"

  Scenario: TUI launches with one tool failing discovery
    Given the LM Studio config directory is unreadable
    When Devon runs "modeltap"
    Then the TUI launches successfully
    And LM Studio is shown in the left pane with "(error)" beside it
    And the other three tools list their models normally

  # ---------------------------------------------------------------
  # Step 2 — Browse and recognize compatibility
  # ---------------------------------------------------------------

  Scenario: Multi-tool model is marked with `*`
    Given Mistral-7B-v0.3 q4_K_M GGUF exists in Ollama, llama-cli, and Hugging Face
    When Devon selects Hugging Face in the left pane
    Then the Mistral row is marked with `*`
    And the row shows "also in: Ollama, llama-cli"

  Scenario: Format-locked model is marked with `!` (red)
    Given TheBloke/something-AWQ exists only in the Hugging Face cache
    And no other supported tool accepts AWQ
    When Devon selects Hugging Face in the left pane
    Then the AWQ model row is marked with `!`
    And the row metadata reads "only Hugging Face accepts this"

  Scenario: Unique-but-compatible model is marked with `o`
    Given meta-llama/Llama-3-8B-Instruct GGUF exists only in Hugging Face
    And both Ollama and llama-cli accept GGUF
    When Devon selects Hugging Face in the left pane
    Then the Llama-3 row is marked with `o`
    And the row does not list other tools

  # ---------------------------------------------------------------
  # Step 3 — Inspect duplicates
  # ---------------------------------------------------------------

  Scenario: Detail screen shows duplicate copies and reclaim estimate
    Given Mistral-7B-v0.3 q4_K_M GGUF has 3 separate file copies of 4.4 GB across 3 tools
    When Devon selects the Mistral row and presses Enter
    Then the detail screen shows all 3 file paths
    And the status reads "NOT UNIFIED — 3 separate copies exist (13.2 GB total)"
    And reclaim estimate reads "8.8 GB"

  Scenario: Detail screen refuses unify when sizes don't match the dedup key
    Given two models share a dedup key but have different file sizes (4.4 GB and 4.5 GB)
    When Devon opens the detail screen
    Then the screen shows a warning "size mismatch — refusing to offer unify"
    And the [u] shortcut is disabled on this screen

  # ---------------------------------------------------------------
  # Step 4a — Unify
  # ---------------------------------------------------------------

  Scenario: Dry-run unify shows plan without changing disk
    Given Mistral-7B-v0.3 has 3 separate copies across 3 tools
    When Devon presses "u" and then "n" for dry-run
    Then the dialog shows which existing tool-owned copy will be chosen as canonical (per BE-9; modeltap does not own a central store)
    And the dialog lists the 2 hardlink target paths to be replaced
    And no filesystem changes are made
    And no file is created under ~/.modeltap/

  Scenario: Unify creates hardlinks and reclaims disk
    Given Mistral-7B-v0.3 has 3 separate copies of 4.4 GB across 3 tools
    When Devon presses "u" and then Enter to proceed
    Then one existing tool-owned copy (e.g., the Ollama blob path) is chosen as canonical
    And the other 2 tools' paths are replaced with hardlinks to the canonical (same inode)
    And no file is created under ~/.modeltap/
    And modeltap reports "Reclaimed 8.8 GB"

  Scenario: Unify prompts user to close a tool that is currently running
    Given the Ollama process is running with the model file open
    When Devon presses "u" on a model registered in Ollama
    Then the unify dialog shows "Ollama is running and has this file open. Close Ollama and retry." (per intake Q5)
    And Devon's options are [r] retry / [Esc] cancel
    And no partial mutation occurs

  Scenario: Unify falls back gracefully when hardlink is impossible
    Given the canonical store path and a tool's path are on different filesystems
    When Devon proceeds with unify
    Then modeltap reports "Cannot hardlink across filesystems"
    And modeltap offers a fallback ("copy" or "skip this target")
    And no partial state is left behind

  # ---------------------------------------------------------------
  # Step 4b — Zap
  # ---------------------------------------------------------------

  Scenario: Zap requires typed confirmation matching tool name
    Given Devon has selected "llama-cli" in the left pane
    When Devon presses "z"
    Then the zap dialog appears showing model count, total bytes, and unique-vs-shared breakdown
    And Devon must type "llama-cli" exactly to proceed

  Scenario: Zap proceeds when typed name matches
    Given Devon has selected "llama-cli" with 6 models (4 shared, 2 unique) totaling 21.4 GB
    When Devon presses "z", types "llama-cli", presses Enter
    Then all 6 llama-cli registrations are removed
    And the 2 unique model files are deleted from disk
    And the 4 shared models remain available in their other tools

  Scenario: Zap cancels when typed name does not match
    Given Devon has opened the zap dialog for "llama-cli"
    When Devon types "llamacli" (missing hyphen) and presses Enter
    Then no models are deleted
    And the dialog closes returning to the main view

  Scenario: Zap escapes cleanly with Esc
    Given Devon has opened the zap dialog
    When Devon presses Esc
    Then the dialog closes with no changes

  # ---------------------------------------------------------------
  # Step 5 — Verify outcome
  # ---------------------------------------------------------------

  Scenario: Successful zap is summarized in the main view
    Given Devon's pre-zap total disk usage was 138.4 GB
    And Devon successfully zapped llama-cli reclaiming 14.6 GB
    When the action completes
    Then the right pane shows "Last action: zap llama-cli (success)"
    And the right pane shows "Reclaimed: 14.6 GB"
    And the summary bar shows total disk usage 123.8 GB (within rounding)

  Scenario: Successful unify is summarized in the main view
    Given Devon's pre-unify total disk usage was 138.4 GB
    And Devon successfully unified Mistral-7B-v0.3 reclaiming 8.8 GB
    When the action completes
    Then the right pane shows "Reclaimed: 8.8 GB"
    And the model row is now marked with `*` and "(unified — 1 inode, 3 hardlinks)"

  # ---------------------------------------------------------------
  # Plugin extensibility — non-functional, requirements-level
  # ---------------------------------------------------------------

  Scenario: A contributor adds a fifth tool by implementing one trait
    Given a new tool "Atomic Chat" is to be supported
    When a contributor implements the Tool trait providing discover, list, link, delete, and capability metadata
    And registers the plugin via the documented registration mechanism
    Then modeltap shows Jan in the left pane on next launch
    And no other source files in modeltap need to be modified

  Scenario: Cross-platform path discovery
    Given Devon is on Linux (Ubuntu)
    When Devon runs "modeltap"
    Then the same four tools are discoverable using their Linux default paths
    And unify uses Linux hardlink semantics
