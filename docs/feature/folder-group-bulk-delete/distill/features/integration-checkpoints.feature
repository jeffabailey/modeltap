# =============================================================================
# folder-group-bulk-delete — Integration Checkpoints
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
#
# Cross-cutting invariants that span Milestones 1-6. These are the same kind
# of invariants the parent modeltap-tui captured as INT-1..INT-7 — here we
# capture US-05c's INT-FGD-1..INT-FGD-8 plus reclaim-math properties.
#
# These scenarios assert PROPERTIES, not specific user journeys. They are
# tagged @property where DELIVER may implement them as proptest invariants
# over generated FolderGroup / FolderClassification values, and tagged
# @destructive where they require a real folder-delete to have run.
# =============================================================================

Feature: Folder-Group Delete — Cross-Cutting Integration Invariants

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"

  # ---------------------------------------------------------------------------
  # Reclaim-math invariants
  # ---------------------------------------------------------------------------

  @us-05c @int-fgd-2 @property
  Scenario: For any folder, file_count equals models plus sidecars
    Given any FolderGroup built from any HF fixture
    When the folder header row renders
    Then "folder_group.file_count" equals "len(folder_group.models) + len(folder_group.sidecars)"

  @us-05c @int-fgd-3 @property
  Scenario: For any folder, total_bytes equals reclaim plus retain
    Given any FolderGroup built from any HF fixture
    And the per-file classification has run for that folder
    When the folder-delete dialog renders
    Then "folder_group.bytes_to_reclaim + folder_group.bytes_to_retain" equals "folder_group.total_bytes" within rounding tolerance of 1 byte

  @skip @us-05c @int-fgd-6 @destructive
  Scenario: After a successful folder-delete, total disk_usage decreases by exactly bytes_reclaimed
    Given Devon's pre-delete "total.disk_usage" was recorded as X
    And Devon's "last_action.bytes_reclaimed" after the folder-delete was recorded as Y
    When the summary bar refreshes within 500 milliseconds
    Then the new "total.disk_usage" equals "X - Y" within rounding tolerance of 1 byte

  @skip @us-05c @int-fgd-1 @destructive
  Scenario: After a successful folder-delete, summary bar total equals sum of tool disk_usage
    Given Devon has completed a folder-delete against any HF fixture
    When the summary bar refreshes
    Then "total.disk_usage" equals the sum of "tool.disk_usage" for every installed tool within rounding tolerance of 1 byte

  # ---------------------------------------------------------------------------
  # Cross-tool hardlink preservation
  # ---------------------------------------------------------------------------

  # @skip removed in DELIVER step 03-02.
  @us-05c @int-fgd-4 @destructive @real-io
  Scenario: For every shared file, the other tool's hardlink survives the folder-delete
    Given Devon has completed a folder-delete against any HF fixture that contained shared files
    When the post-action discovery rebuild completes
    Then for every previously-shared file, the other tool's path still stats to a live inode
    And the inode of the other tool's path matches the inode it had before the folder-delete

  # ---------------------------------------------------------------------------
  # Inventory consistency after the action
  # ---------------------------------------------------------------------------

  @skip @us-05c @int-fgd-5 @destructive
  Scenario: After a successful folder-delete, the folder is gone from list_models and list_folder_groups
    Given Devon has completed a folder-delete for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    When the next inventory rebuild runs
    Then the HF plugin's "list_models" output contains no entry whose id_in_tool starts with "bartowski/Llama-3.2-1B-Instruct-GGUF/"
    And the HF plugin's "list_folder_groups" output contains no entry with path "bartowski/Llama-3.2-1B-Instruct-GGUF"

  # ---------------------------------------------------------------------------
  # Typed-confirmation provenance (no hardcoded literal)
  # ---------------------------------------------------------------------------

  @skip @us-05c @int-fgd-7 @property
  Scenario: The typed-confirmation comparator reads folder_group.path, not a hardcoded literal
    Given any folder-delete dialog opening for any HF folder with path P
    When the typed input is compared to the expected confirmation string
    Then the comparator reads "folder_group.path" from the dialog's bound state
    And no literal repo path appears inline in the dispatch code

  # ---------------------------------------------------------------------------
  # Pre-flight refusal (cache writeable + folder still exists)
  # ---------------------------------------------------------------------------

  @us-05c @ac-15 @infrastructure-failure
  Scenario: Read-only HF cache refuses before the dialog opens
    Given Devon has fixture "devon-hf-readonly" with the HF cache directory at mode 0555
    And Devon has navigated the cursor to a folder header in the HF right pane
    When Devon presses Shift+F
    Then no folder-delete dialog opens
    And the right pane shows "Hugging Face cache is read-only -- cannot delete folder"
    And the Hugging Face fixture directory is unchanged

  @us-05c @ac-20 @infrastructure-failure
  Scenario: Folder deleted out-of-band between launch and Shift+F triggers re-discovery
    Given Devon has fixture "devon-hf-allunique" with the repo "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And Devon has launched modeltap and the folder header is visible
    And an out-of-band process has removed the on-disk "models--bartowski--Llama-3.2-1B-Instruct-GGUF/" directory tree
    When Devon presses Shift+F on the now-stale folder header
    Then no folder-delete dialog opens
    And the right pane shows "folder no longer exists -- inventory will refresh"
    And the next inventory rebuild runs
    And the folder header no longer appears in the right pane

  # ---------------------------------------------------------------------------
  # Regression gate: parent journey scenarios still pass
  # ---------------------------------------------------------------------------

  @skip @us-05c @int-fgd-8
  Scenario: Parent feature scenarios continue to pass after folder-delete is introduced
    Given the folder-group-bulk-delete feature is merged into modeltap
    When the parent acceptance suite runs against fixture "devon-multi-tool"
    Then every scenario in modeltap-tui/distill/features/master-acceptance.feature tagged @walking-skeleton still passes
    And no parent scenario produces a new failure attributable to the folder-delete code paths
