# =============================================================================
# folder-group-bulk-delete — Folder-Group Delete Feature File
#
# Wave: DISTILL (5 of 6) — brownfield extension of modeltap-tui
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-11
# User story: US-05c (single story; 20 ACs + 8 cross-story integration ACs)
#
# Tag glossary (inherits parent's master-acceptance.feature glossary):
#   @walking-skeleton  -- WS exit gate; DELIVER ships when this passes
#   @us-05c            -- traceability to user-stories.md story ID
#   @ac-NN             -- traceability to a specific US-05c.AC-NN criterion
#   @int-fgd-NN        -- traceability to INT-FGD-NN integration AC
#   @release-3-folder  -- "Folder-group delete" sub-release within parent release-2 ("Reclaim disk safely")
#   @milestone-N       -- DELIVER ordering hint (1..6 in handoff order)
#   @destructive       -- modifies the fixture tree on disk
#   @real-io           -- uses real filesystem; required for at least one per adapter
#   @adapter-integration -- proves a single driven adapter against real I/O
#   @kpi-instrumentation -- asserts JSONL log output (~/.modeltap/launch.log)
#   @plugin-trait      -- exercises Tool trait extensibility (US-18 / ADR-010)
#   @property          -- universal invariant (DELIVER may implement as proptest)
#   @infrastructure-failure -- driven-adapter failure scenario
#   @interactive       -- requires real PTY (expectrl); not headless-mode
#   @skip              -- not yet enabled (Quinn one-at-a-time discipline)
#
# Walking-skeleton subset (1 scenario, the all-unique case):
#   @walking-skeleton+@us-05c == "M1 — Devon deletes an all-unique HF folder"
#
# Milestone counts (this file):
#   M1 walking skeleton              : 1 (1 @real-io)
#   M2 confirmation safety           : 4
#   M3 mixed shared/unique           : 4
#   M4 partial failure (concurrency) : 3
#   M5 capability boundary (trait)   : 1 (parameterized, 3 non-HF plugins covered)
#   M6 KPI guardrails                : 2
#   TOTAL: 15 scenarios in this file. Cross-cutting invariants live in
#          integration-checkpoints.feature.
#
# Error-path ratio: 9 of 15 (60%) — well above 40% minimum (critique Dim 1).
#
# Strategy declaration: Strategy B (real I/O against fixture-populated temp
# dirs) — declared in wave-decisions.md. Inherits parent's Strategy B.
#
# Wave-decisions tracked separately in ../wave-decisions.md.
# =============================================================================

Feature: Delete a Hugging Face Folder Group
  As Devon Park, a local-AI power user who audits many quant variants of the same logical model in the Hugging Face cache
  I want to delete every file in an HF repo folder in one keystroke plus a typed confirmation
  So that I can reclaim disk space without typing one [d]-then-confirm per file
  And without stranding sidecar files
  And without breaking another tool that hardlinks one of the model files

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And Devon has installed modeltap on macOS or Linux
    And the bottom bar always displays the available keyboard shortcuts including "[F] folder-delete"

  # ===========================================================================
  # MILESTONE 1 — Walking Skeleton (all-unique folder, happy path E2E)
  # Exit gate: DELIVER ships when this scenario passes against a real HF
  # tempdir fixture using the real HF plugin's delete_folder override.
  # ===========================================================================

  # @skip removed in DELIVER step 01-05 — this is the walking-skeleton exit
  # gate. After M1 goes green, subsequent milestones (M2..M6) un-skip their
  # scenarios one at a time per Quinn's discipline (step-definitions-skeleton.md §I.1).
  @walking-skeleton @us-05c @milestone-1 @ac-4 @ac-8 @ac-10 @ac-11 @ac-16 @destructive @real-io @adapter-integration
  Scenario: Devon deletes an all-unique HF repo folder and reclaims disk
    Given Devon has fixture "devon-hf-allunique" with the HF cache containing the repo "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And the folder contains 2 model files "Llama-3.2-1B-Instruct-Q4_K_M.gguf" (808 MB) and "Llama-3.2-1B-Instruct-Q8_0.gguf" (1.3 GB) unique to Hugging Face
    And the folder contains 3 sidecars "README.md" (24 KB), "Llama-3.2-1B-Instruct.imatrix" (1.3 MB), "Llama-3.2-1B-Instruct-Q4_K_M.gguf.urls" (8 KB)
    When Devon runs "modeltap" in headless mode
    And selects "Hugging Face" in the left pane
    And navigates the cursor to the folder header "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And presses Shift+F
    And types "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And presses Enter
    Then the 2 model files are removed from the Hugging Face fixture directory
    And the 3 sidecar files are removed from the Hugging Face fixture directory
    And the now-empty "models--bartowski--Llama-3.2-1B-Instruct-GGUF/" directory tree is removed
    And the right pane shows "Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF (success)"
    And the right pane shows "5 of 5 files removed"
    And the right pane shows "Reclaimed: 2.1 GB"
    And the right pane shows "Retained: 0.0 GB"
    And the folder header no longer appears in the right pane

  # ===========================================================================
  # MILESTONE 2 — Confirmation safety (typed-confirm guardrails)
  # ===========================================================================

  # @skip removed in DELIVER step 02-01.
  @us-05c @milestone-2 @ac-8 @destructive
  Scenario: Wrong typed path cancels the folder delete with no destructive action
    Given Devon has fixture "devon-hf-allunique" with the repo "bartowski/Llama-3.2-1B-Instruct-GGUF" (5 files, 2.1 GB)
    And Devon has opened the folder-delete dialog for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    When Devon types "Llama-3.2-1B-Instruct-GGUF" and presses Enter
    Then the dialog closes with no changes
    And no files are removed from the Hugging Face fixture directory
    And the folder header still appears in the right pane

  # @skip removed in DELIVER step 02-01.
  @us-05c @milestone-2 @ac-9
  Scenario: Esc cancels the folder delete with no destructive action
    Given Devon has fixture "devon-hf-allunique" with the repo "bartowski/Llama-3.2-1B-Instruct-GGUF" (5 files, 2.1 GB)
    And Devon has opened the folder-delete dialog for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    When Devon presses Esc
    Then the dialog closes with no changes
    And no files are removed from the Hugging Face fixture directory
    And the folder header still appears in the right pane

  # @skip removed in DELIVER step 02-01.
  @us-05c @milestone-2 @ac-8 @destructive
  Scenario: Typed path with trailing slash is treated as mismatch
    Given Devon has fixture "devon-hf-allunique" with the repo "bartowski/Llama-3.2-1B-Instruct-GGUF" (5 files, 2.1 GB)
    And Devon has opened the folder-delete dialog for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    When Devon types "bartowski/Llama-3.2-1B-Instruct-GGUF/" and presses Enter
    Then the dialog closes with no changes
    And no files are removed from the Hugging Face fixture directory
    And the folder header still appears in the right pane

  # @skip removed in DELIVER step 02-02.
  @us-05c @milestone-2 @ac-5
  Scenario: Shift+F is a no-op when the active tool is not Hugging Face
    Given Devon has fixture "devon-multi-tool" with both Ollama and Hugging Face installed
    And Devon has selected "Ollama" in the left pane
    And the cursor is on a model row in the Ollama right pane
    When Devon presses Shift+F
    Then no dialog opens
    And the "[F]" indicator in the bottom bar is dimmed
    And the Ollama fixture directory is unchanged

  # ===========================================================================
  # MILESTONE 3 — Mixed shared/unique within one folder
  # Mirrors US-05b classification rubric for per-file decisions.
  # ===========================================================================

  # @skip removed in DELIVER step 03-01.
  @us-05c @milestone-3 @ac-6 @ac-7
  Scenario: Dialog itemises unique, shared, and sidecar counts for a mixed folder
    Given Devon has fixture "devon-hf-mixed" with the HF cache containing "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And the folder contains 19 model files unique to Hugging Face totaling 13.2 GB
    And the folder contains 1 model file "Llama-3.2-1B-Instruct-Q4_K_M.gguf" (808 MB) hardlinked into Ollama
    And the folder contains 3 sidecar files totaling 1.3 MB
    And Devon navigates the cursor to the folder header "bartowski/Llama-3.2-1B-Instruct-GGUF"
    When Devon presses Shift+F
    Then a modal dialog opens titled "Delete folder group: bartowski/Llama-3.2-1B-Instruct-GGUF"
    And the dialog itemises "19 unique + 1 shared + 3 sidecars"
    And the dialog identifies the shared file as "also linked in Ollama"
    And the dialog shows "Reclaim: 13.2 GB"
    And the dialog shows "Retained: 0.8 GB"

  # @skip removed in DELIVER step 03-02.
  @us-05c @milestone-3 @ac-10 @int-fgd-4 @destructive @real-io
  Scenario: Folder-delete preserves the Ollama-side hardlink for a shared model file
    Given Devon has fixture "devon-hf-mixed" with "Llama-3.2-1B-Instruct-Q4_K_M.gguf" hardlinked into both Hugging Face and Ollama
    And the Hugging Face and Ollama paths stat to the same inode pre-delete
    When Devon successfully folder-deletes "bartowski/Llama-3.2-1B-Instruct-GGUF"
    Then the Hugging Face path "models--bartowski--Llama-3.2-1B-Instruct-GGUF/blobs/<sha>" no longer exists
    And the Ollama path "blobs/sha256-<llama-q4-hash>" still exists
    And the Ollama path stats to a live inode with the original SHA256 content

  # @skip removed in DELIVER step 03-01.
  @us-05c @milestone-3 @ac-7 @ac-16 @destructive
  Scenario: Post-action summary reports bytes reclaimed and retained separately for a mixed folder
    Given Devon has completed a folder-delete against fixture "devon-hf-mixed" for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And the folder had 19 unique files (13.2 GB), 1 shared file (0.8 GB), 3 sidecars (1.3 MB)
    When the post-action summary renders
    Then the right pane shows "Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF (success)"
    And the right pane shows "23 of 23 files removed"
    And the right pane shows "Reclaimed: 13.2 GB"
    And the right pane shows "Retained: 0.8 GB (1 file also linked in Ollama)"

  @us-05c @milestone-3 @ac-13 @property
  Scenario: For any folder, per-file classification matches compute_indicator on every child
    Given any populated HF folder group built from fixture "devon-hf-mixed"
    When the folder-delete dialog opens for that folder
    Then every model file classified as "shared" has compute_indicator returning "Shared" with the same other-tool set
    And every model file classified as "unique" has compute_indicator returning one of "Compatible", "FormatLocked", or "Unknown"
    And no classification path bypasses compute_indicator

  # ===========================================================================
  # MILESTONE 4 — Partial failure (per-file detect-and-prompt-then-retry)
  # Per ADR-010 § Concurrency: no rollback, continue-and-report.
  # ===========================================================================

  # @skip removed in DELIVER step 04-01.
  @us-05c @milestone-4 @ac-12 @ac-16 @destructive @infrastructure-failure
  Scenario: Ollama holds 2 model files open and folder-delete continues for the rest
    Given Devon has fixture "devon-hf-busy" with the repo "bartowski/Llama-3.2-1B-Instruct-GGUF" (21 files, 14.7 GB)
    And the FsProbe adapter is configured with fake-lsof reporting "ollama PID 4421 holds Llama-3.2-1B-Instruct-Q4_K_M.gguf and Llama-3.2-1B-Instruct-Q4_0.gguf open"
    And the fixture's filesystem will return EBUSY for those 2 files only
    When Devon successfully confirms the folder-delete for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    Then 19 of 21 files are removed from the Hugging Face fixture directory
    And the 2 EBUSY model files remain on disk
    And the right pane shows "Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF (partial)"
    And the right pane shows "19 of 21 files removed"
    And the right pane lists "Llama-3.2-1B-Instruct-Q4_K_M.gguf reason: file open by ollama"
    And the right pane lists "Llama-3.2-1B-Instruct-Q4_0.gguf reason: file open by ollama"
    And the right pane hints "Press [F] again after closing ollama to finish"

  # @skip removed in DELIVER step 04-02.
  @us-05c @milestone-4 @ac-12 @destructive
  Scenario: Re-running folder-delete after closing the holding tool removes the remaining files
    Given Devon completed a partial folder-delete leaving 2 EBUSY files in "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And the holding tool has been closed
    And the next inventory rebuild lists the folder header with 2 remaining files
    When Devon presses Shift+F on the folder header
    And types "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And presses Enter
    Then 2 of 2 remaining files are removed
    And the now-empty "models--bartowski--Llama-3.2-1B-Instruct-GGUF/" directory tree is removed
    And the right pane shows "Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF (success)"
    And the folder header no longer appears in the right pane

  # @skip removed in DELIVER step 04-01.
  @us-05c @milestone-4 @ac-12 @destructive @infrastructure-failure
  Scenario: A permission-denied file does not block the rest of the folder
    Given Devon has fixture "devon-hf-perm" with the repo "bartowski/Llama-3.2-1B-Instruct-GGUF" (5 files, 2.1 GB)
    And one model file "Llama-3.2-1B-Instruct-Q8_0.gguf" lives in a directory with mode 0555
    When Devon successfully confirms the folder-delete for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    Then 4 of 5 files are removed from the Hugging Face fixture directory
    And the right pane shows "Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF (partial)"
    And the right pane lists "Llama-3.2-1B-Instruct-Q8_0.gguf reason: permission denied"
    And the right pane shows "Reclaimed: 1.3 GB"

  # ===========================================================================
  # MILESTONE 5 — Capability boundary (plugin contract)
  # One scenario parameterized over the 3 non-HF plugins; HF passes the
  # contract via the dedicated plugin-contract-spec.md suite, not here.
  # ===========================================================================

  # @skip removed in DELIVER step 05-01 — Layer A is implemented in
  # crates/modeltap-app/tests/acceptance/folder_delete_capability_boundary.rs
  # and Layer B in plugins/<name>/tests/folder_delete_contract.rs. The
  # Examples table is updated to match the workspace (no `llama-cli` crate
  # exists; `Atomic Chat` is the third non-HF plugin that inherits the
  # ADR-010 default body).
  @us-05c @milestone-5 @ac-5 @plugin-trait
  Scenario Outline: Non-HF plugins return Unsupported when asked to delete a folder
    Given Devon has fixture "devon-multi-tool" with the <plugin> plugin installed
    And the orchestrator attempts a folder-delete dispatch against the <plugin> plugin
    When the <plugin> plugin's Tool::delete_folder is invoked through the orchestrator
    Then the orchestrator receives DeleteError::Unsupported with tool == "<plugin>"
    And no filesystem mutation occurs in the <plugin> fixture directory
    And the right pane shows "<plugin> does not support folder-delete"

    Examples:
      | plugin      |
      | ollama      |
      | lm-studio   |
      | Atomic Chat |

  # ===========================================================================
  # MILESTONE 6 — KPI guardrails (from outcome-kpis.md)
  # K-FGD-2 keystrokes, K-FGD-3 mis-target rate.
  # ===========================================================================

  # @skip removed in DELIVER step 06-01.
  @us-05c @milestone-6 @kpi-instrumentation @destructive
  Scenario: Keystroke count for a 20-file folder is bounded and independent of file count
    Given Devon has fixture "devon-hf-20files" with "bartowski/Llama-3.2-1B-Instruct-GGUF" containing 20 model files
    When Devon completes a folder-delete via Shift+F, typed path "bartowski/Llama-3.2-1B-Instruct-GGUF", and Enter
    Then the JSONL log "action.folder_delete" event has "keystroke_count" less than or equal to 40
    And the JSONL log "action.folder_delete" event has "keystroke_count" independent of the folder's file_count
    And the JSONL log "action.folder_delete" event has "outcome" == "success"

  # @skip removed in DELIVER step 06-01.
  @us-05c @milestone-6 @ac-8 @property @kpi-instrumentation
  Scenario: Every aborted typed-confirmation results in zero filesystem mutations
    Given any folder-delete dialog opening for any HF folder
    When the user enters any input that is not the byte-exact folder path
    Then the Hugging Face fixture directory is byte-identical pre and post (manifest equal)
    And the JSONL log "action.folder_delete" event has "outcome" == "cancelled_mismatch"
    And no DeleteOutcome is produced for any file in the folder
