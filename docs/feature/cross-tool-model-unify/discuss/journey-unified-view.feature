Feature: See what is already unified
  As Devon, a developer who wants to audit or confirm cross-tool unification,
  I want a single navigation step that shows me every model currently sharing
  one inode across multiple tools, with size and tool count,
  so I can verify the tool is doing what it claims and quantify the savings.

  Background:
    Given Devon's modeltap session has completed background hashing
    And the following models are unified (share one inode across N tools):
      | model                  | size    | tools                          |
      | llama-3.1-8b-Q4_K_M    | 4.7 GB  | ollama, lm-studio, hf-cache    |
      | phi-3-mini             | 2.3 GB  | ollama, hf-cache               |
      | nomic-embed-v1.5       | 274 MB  | ollama, lm-studio, hf, atomic  |
      | qwen2-7b-instruct      | 4.4 GB  | ollama, lm-studio              |
      | mistral-7b-v0.3        | 4.1 GB  | ollama, hf-cache, lm-studio    |

  Scenario: Left pane shows [All Unified] with correct count
    When Devon launches modeltap and hashing completes
    Then the left pane includes a slot labeled "[All Unified]" with badge "(5)"
    And the slot is positioned below the four tool slots

  Scenario: Selecting [All Unified] populates right pane with unified models
    When Devon navigates to [All Unified] in the left pane
    Then the right pane shows 5 rows
    And each row shows: model name, size, tool count, bytes saved
    And the right-pane footer shows "Unified: 5 models | Total reclaimed by unification: 25.1 GB"

  Scenario: Row data is internally consistent
    Given the [All Unified] view is shown
    When Devon reads the llama-3.1-8b-Q4_K_M row
    Then it shows "4.7 GB"
    And it shows "3 tools"
    And it shows "saves 9.4 GB" (which equals (3-1) * 4.7 GB)

  Scenario: Counts agree across UI surfaces
    Given the [All Unified] view is shown
    Then the left-pane badge "(5)" matches the row count "5" in the right pane
    And the summary bar shows "Unified: 5 models"

  Scenario: Empty state when nothing is unified
    Given Devon has a fresh install with no unified models
    When Devon navigates to [All Unified]
    Then the right pane shows guidance text:
      """
      No models are unified yet.

      Navigate to a tool, find a row marked "=", and press [u] to unify it.

      Models marked "=" can save you disk.
      """
    And the left-pane badge shows "(0)"

  Scenario: Detail screen proves the inode is shared
    Given the [All Unified] view shows the llama-3.1-8b-Q4_K_M row
    When Devon presses Enter on that row
    Then a detail screen opens
    And it shows the same inode number for all 3 paths
    And it lists all 3 paths grouped under that inode
    And it shows "Saves vs. separate copies: 9.4 GB"

  Scenario: Same model count consistent across views
    Given llama-3.1-8b-Q4_K_M is unified across ollama, lm-studio, hf-cache
    When Devon views the model under the [ollama] left-pane slot
    Then the row shows the "#" glyph
    And the row's metadata indicates it is shared with 3 tools (when expanded/inspected)
    When Devon views the same model under [All Unified]
    Then the row's "N tools" column shows "3"
    And both views report the same tool count
