Feature: Delete a Hugging Face Folder Group
  As Devon Park, a local-AI power user who audits many quant variants of the same logical model in the Hugging Face cache
  I want to delete every file in an HF repo folder (<author>/<repo>/) in one keystroke + typed confirmation
  So that I can reclaim disk space without typing one [d]-then-confirm per file
  And without stranding sidecar files (README.md, .imatrix, .gguf.urls)
  And without breaking another tool that hardlinks one of the .gguf files

  Background:
    Given Devon has installed modeltap on macOS or Linux
    And Devon has the Hugging Face cache populated at ~/.cache/huggingface/hub/
    And the bottom bar always displays the available keyboard shortcuts including "[F] folder-delete"

  # ---------------------------------------------------------------
  # Step 1 — Recognise the folder group
  # ---------------------------------------------------------------

  Scenario: Hugging Face right pane groups files under repo folder headers
    Given Devon's HF cache contains the repo "bartowski/Llama-3.2-1B-Instruct-GGUF" with 20 .gguf files (657.3 MB ... 2.5 GB) and 3 sidecars (README.md 24 KB, .imatrix 1.3 MB, .gguf.urls 8 KB)
    When Devon launches modeltap and selects Hugging Face in the left pane
    Then the right pane shows a folder header "[+] bartowski/Llama-3.2-1B-Instruct-GGUF"
    And the header line reads "21 files, 14.7 GB (20 unique, 1 shared)"
    And the header is cursor-targetable with up/down arrows

  Scenario: Expanding a folder header shows model and sidecar children
    Given Devon's cursor is on the folder header "bartowski/Llama-3.2-1B-Instruct-GGUF"
    When Devon presses Enter
    Then the header changes to "[-] bartowski/Llama-3.2-1B-Instruct-GGUF"
    And 20 indented model rows appear with their *, o, !, or ? indicators
    And 3 indented sidecar rows appear prefixed with "." (dim)
    And the sidecar rows are not cursor-targetable

  Scenario: Folder aggregates roll up into the existing tool disk usage
    Given the Hugging Face cache contains 3 folder groups totaling 92.4 GB
    When the inventory builds
    Then the Hugging Face row in the left pane shows total disk usage 92.4 GB
    And this equals the sum of folder_group.total_bytes for all HF folder groups
    And the summary bar's total.disk_usage equals the sum of all tool.disk_usage values

  # ---------------------------------------------------------------
  # Step 2 — Open the folder-delete dialog with [F]
  # ---------------------------------------------------------------

  Scenario: Pressing [F] on a folder header opens the typed-confirmation dialog
    Given Devon's cursor is on the folder header "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And the folder contains 20 .gguf files (19 unique, 1 shared with Ollama) and 3 sidecars
    When Devon presses Shift+F
    Then a modal dialog opens titled "Delete folder group: bartowski/Llama-3.2-1B-Instruct-GGUF"
    And the dialog shows "THIS WILL DELETE 21 FILES (14.7 GB) FROM Hugging Face."
    And the dialog itemises: 19 unique + 1 shared + 3 sidecars
    And the dialog shows "Reclaim: 14.0 GB" and "Retained: 0.7 GB"
    And the dialog asks Devon to type "bartowski/Llama-3.2-1B-Instruct-GGUF" to confirm

  Scenario: Pressing [F] on a non-folder row is a no-op
    Given Devon's cursor is on a single model row (not a folder header)
    When Devon presses Shift+F
    Then no dialog opens
    And no destructive action occurs
    And the bottom bar briefly highlights to indicate "[F]" applies only to folder headers

  Scenario: Pressing [F] on a folder with only sidecars still opens the dialog
    Given a folder group contains 0 model files but 1 sidecar (a leftover README.md from a manual delete)
    When Devon presses Shift+F on the folder header
    Then the dialog opens
    And the body reads "0 model files, 1 sidecar file. Confirm to sweep sidecars only."
    And typed confirmation is still required

  Scenario: Folder-delete dialog shows running-tool warning when a file is open
    Given ollama is running with PID 4421 and has one .gguf in this folder open
    When Devon presses Shift+F on the folder header
    Then the dialog includes "Running tools detected: ollama (PID 4421) -- file open: 1 of 21"
    And Devon can still proceed with confirmation (per intake Q5 — detect-and-prompt-then-retry)

  # ---------------------------------------------------------------
  # Step 3 — Confirm and execute
  # ---------------------------------------------------------------

  Scenario: Typed confirmation executes the folder delete
    Given the folder-delete dialog is open for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    And no tool is holding any file open
    When Devon types "bartowski/Llama-3.2-1B-Instruct-GGUF" exactly and presses Enter
    Then 19 unique .gguf files are unlinked from the HF cache
    And 1 shared .gguf file has only its HF path unlinked (the Ollama-side hardlink keeps the inode alive)
    And 3 sidecar files (README.md, .imatrix, .gguf.urls) are unlinked
    And the now-empty models--bartowski--Llama-3.2-1B-Instruct-GGUF/ directory tree is removed
    And modeltap reports "Reclaimed 14.0 GB, Retained 0.7 GB"

  Scenario: Wrong typed path cancels the folder delete
    Given the folder-delete dialog is open for "bartowski/Llama-3.2-1B-Instruct-GGUF"
    When Devon types "Llama-3.2-1B-Instruct-GGUF" (missing the author prefix) and presses Enter
    Then the dialog closes with no changes
    And no files are deleted
    And the inventory is unchanged

  Scenario: Esc cancels the folder delete at any point
    Given the folder-delete dialog is open
    When Devon presses Esc
    Then the dialog closes with no changes
    And no files are deleted

  Scenario: Shared file preservation — Ollama-side hardlink survives
    Given Llama-3.2-1B-Instruct-Q4_K_M.gguf is hardlinked into both HF and Ollama
    And both paths stat to the same inode pre-delete
    When Devon successfully folder-deletes the HF repo containing this file
    Then the HF path no longer exists
    And the Ollama path still exists and still stats to the original inode
    And running "ollama run llama3.2:1b" still succeeds (model file intact)

  # ---------------------------------------------------------------
  # Step 4 — Partial failure handling
  # ---------------------------------------------------------------

  Scenario: Partial failure — Ollama holds 2 files open during execution
    Given Devon has confirmed the folder-delete
    And ollama is running and holds 2 of the 21 files open
    When modeltap attempts to unlink each file
    Then 19 files are successfully unlinked (1 of those being shared, which leaves the inode alive in Ollama)
    And 2 files fail with reason "file open by ollama" and remain on disk
    And the post-action summary reads "partial: 19 of 21 files removed"
    And the summary lists the 2 failed files with their reasons
    And modeltap does NOT roll back the 19 successful deletions
    And the folder-delete operation can be re-run after closing Ollama to finish

  Scenario: Permission failure on individual file
    Given one .gguf file in the folder has restrictive permissions (read-only directory)
    When Devon proceeds with folder-delete
    Then the unaffected files are unlinked
    And the permission-restricted file remains on disk with reason "permission denied"
    And the partial-success summary is shown

  Scenario: HF cache read-only refuses before opening the dialog
    Given the entire HF cache directory is read-only
    When Devon presses Shift+F on a folder header
    Then a pre-flight check refuses with "Hugging Face cache is read-only -- cannot delete folder"
    And the typed-confirm dialog does NOT open
    And no destructive action occurs

  # ---------------------------------------------------------------
  # Step 4 — Post-action summary
  # ---------------------------------------------------------------

  Scenario: Post-action summary shows reclaim and retain bytes
    Given the folder delete just completed successfully (21 of 21 files removed)
    When the TUI returns to the main view
    Then the right pane shows "Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF (success)"
    And the right pane shows "21 of 21 files removed."
    And the right pane shows "Reclaimed: 14.0 GB"
    And the right pane shows "Retained: 0.7 GB (1 file also linked in Ollama)"
    And the summary bar's total.disk_usage decreases by 14.0 GB within 500ms
    And the folder header no longer appears in the right pane

  Scenario: Summary bar totals refresh consistently after folder delete
    Given pre-delete total.disk_usage was 92.4 GB and Hugging Face tool.disk_usage was 92.4 GB
    When Devon successfully folder-deletes a 14.0 GB repo
    Then within 500ms the summary bar shows total.disk_usage of 78.4 GB
    And Hugging Face's tool.disk_usage shows 78.4 GB
    And the sum of all tool.disk_usage values equals total.disk_usage (per parent integration invariant)

  # ---------------------------------------------------------------
  # Cross-feature integration
  # ---------------------------------------------------------------

  Scenario: Folder-delete coexists with single-model delete (US-05b)
    Given Devon has expanded a folder header
    And his cursor is on an individual model row within the folder
    When Devon presses [d]
    Then the existing US-05b single-model delete dialog opens (not the folder-delete dialog)
    And the single .gguf is deleted per US-05b semantics

  Scenario: Folder-delete coexists with whole-tool zap (US-05)
    Given Devon's cursor is on Hugging Face in the LEFT pane
    When Devon presses [z]
    Then the existing US-05 whole-tool zap dialog opens (not the folder-delete dialog)
    And the [F] key would have applied only if the cursor were on a folder header in the right pane

  Scenario: Folder-delete only applies to the Hugging Face plugin in v1
    Given Devon's cursor is on a model row in the Ollama right pane
    When Devon presses Shift+F
    Then no dialog opens
    And the [F] indicator in the bottom bar is dimmed when the active tool is not Hugging Face
