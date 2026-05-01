Feature: Unify a model across all tools
  As Devon, a small-team developer with multiple local AI tools installed,
  I want to take a model that exists as separate copies across Ollama, LM Studio,
  HF cache, and Atomic Chat and have them all share one inode,
  so I can reclaim disk space without losing models from any tool.

  Background:
    Given Devon has Ollama, LM Studio, HF cache, and Atomic Chat installed
    And the model "llama-3.1-8b-Q4_K_M" exists as 3 separate 4.7 GB copies
      | tool       | path                                          |
      | ollama     | ~/.ollama/models/blobs/sha256-e5c19af2         |
      | lm-studio  | ~/.cache/lm-studio/models/.../Q4_K_M.gguf     |
      | hf-cache   | ~/.cache/huggingface/hub/.../Q4_K_M.gguf      |
    And all three copies are byte-identical (same SHA256: e5c1...9af2)

  Scenario: First paint shows hashing in progress (no hardcoded 0)
    When Devon launches modeltap
    Then within 1 second, all 19 model rows are visible
    And each row shows dedup glyph "?" while hashes are pending
    And the summary bar shows "Hashing 0/19... | Dedup-able: computing..."
    And the summary bar does NOT show "Dedup-able: 0 B" as a final value

  Scenario: Background hashing updates row glyphs and summary bar
    Given Devon has just launched modeltap
    When the background hash worker completes the SHA256 for llama-3.1-8b-Q4_K_M across all 3 copies
    Then the llama-3.1-8b-Q4_K_M row glyph flips from "?" to "="
    And the summary bar "Dedup-able" value increases by 9.4 GB (the two redundant copies)
    And the summary bar "Hashing N/M" progress advances by 3

  Scenario: A model already sharing one inode shows # not =
    Given the model "phi-3-mini" exists in ollama and hf-cache as a hardlink (one inode, 2.3 GB)
    When the hash worker classifies phi-3-mini
    Then the phi-3-mini row glyph is "#" (already-unified)
    And the summary bar "Unified: N models" count includes phi-3-mini
    And the [All Unified] left-pane slot count includes phi-3-mini

  Scenario: User selects a row marked = (dedup-able)
    Given hashing is complete and the llama-3.1-8b-Q4_K_M row shows "="
    When Devon presses j until that row is highlighted
    Then the status line shows "llama-3.1-8b-Q4_K_M | 4.7 GB | in: ollama, lm-studio, hf-cache"

  Scenario: u from main view opens dialog with mates pre-populated
    Given the llama-3.1-8b-Q4_K_M row is highlighted and shows "="
    When Devon presses "u"
    Then the unify dialog opens
    And the dialog shows canonical = ollama (4.7 GB)
    And the dialog lists lm-studio and hf-cache as targets, both checked
    And the dialog shows "Total reclaim: 9.4 GB"
    And the dialog shows "[Enter] Apply  [space] Toggle  [Esc] Cancel"

  Scenario: u on a unique row shows status hint
    Given the nomic-embed row is highlighted and shows "-" (unique, no mates)
    When Devon presses "u"
    Then the unify dialog does NOT open
    And the status line shows "nomic-embed is unique — no copies in other tools to unify with."

  Scenario: u on a row whose hash is still computing shows status hint
    Given the qwen2-7b row is highlighted and shows "?" or "~"
    When Devon presses "u"
    Then the unify dialog does NOT open
    And the status line shows "Cannot unify qwen2-7b — hash still computing. Try again in a moment."

  Scenario: Confirming the dialog applies the plan and shows reclaim
    Given the unify dialog for llama-3.1-8b-Q4_K_M is open with both targets checked
    When Devon presses Enter
    Then progress lines appear for each target ("Linking lm-studio... OK", "Linking hf-cache... OK")
    And a success toast shows "Unified. Reclaimed 9.4 GB."
    And after dismissing the toast, the llama-3.1-8b-Q4_K_M row glyph is now "#"
    And the summary bar "Dedup-able" value has decreased by 9.4 GB
    And the summary bar "Unified: N models" count has incremented by 1

  Scenario: After unify, row glyph and counts persist
    Given Devon has just unified llama-3.1-8b-Q4_K_M
    When Devon navigates away and back to the row
    Then the row still shows "#"
    And the [All Unified] left-pane count reflects the new total

  Scenario: Cross-filesystem fallback shows s/c/x dialog (ADR-008)
    Given lm-studio's models live on a different filesystem from ollama
    And the unify dialog for llama-3.1-8b-Q4_K_M is open
    When Devon presses Enter
    Then the cross-filesystem fallback dialog appears
    And it offers [s]kip lm-studio, [c]opy instead of link, or [x]cancel
    When Devon presses "s"
    Then lm-studio is skipped
    And hf-cache is still hardlinked
    And the toast shows "Unified into 1 of 2 tools. Reclaimed 4.7 GB. Skipped: lm-studio (cross-fs)."

  Scenario: Tool-in-use detection blocks unify until user retries
    Given Devon launched ollama which holds llama-3.1-8b-Q4_K_M open
    And the unify dialog for that model is open
    When Devon presses Enter
    Then a "Tool in use" dialog appears for ollama
    And it offers [r]etry, [s]kip ollama, or [x]cancel
    When Devon stops ollama and presses "r"
    Then the unify proceeds and completes successfully

  Scenario: Partial success reports per-target outcomes
    Given the unify dialog for llama-3.1-8b-Q4_K_M is open
    And hf-cache's directory is read-only
    When Devon presses Enter
    Then lm-studio is hardlinked successfully
    And hf-cache fails with "Permission denied"
    And the toast shows "Unified into 1 of 2 tools. Reclaimed 4.7 GB. Failed: hf-cache (Permission denied)."
    And the row glyph remains "=" because at least one tool still holds a separate copy
    And the launch.log records both outcomes as JSONL events
