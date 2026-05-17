# =============================================================================
# tool-model-info-sqlite-cache — Model detail screen with tool-native metadata (US-22)
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-05-17
#
# Tag glossary (subset relevant to this file):
#   @us-22                   -- story trace
#   @ac-22-N                 -- AC trace
#   @adr-016                 -- ADR-016 (Tool trait inspect extension)
#   @release-1               -- target release per prioritization.md
#   @real-io @adapter-integration -- exercises real plugin adapter
#   @perf @k-info-1-warm-100ms -- detail-screen-open <= 100 ms cached
#
# Scenario count: 5. Error/edge: 2 (40% of this file).
# =============================================================================

Feature: Model detail screen — drill into per-model tool-native metadata

  As Devon Park, a multi-tool local-AI power user,
  I want to drill into any model row and see the GGUF header, Ollama manifest, or HF config.json metadata for that model,
  So that I can confirm a model's quantisation, architecture, and context length before running it for real work, without leaving the TUI.

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/modeltap-test-${SCENARIO_ID}"
    And the cache file path is "${TMPDIR}/modeltap-test-${SCENARIO_ID}/xdg-data/modeltap/cache.sqlite"

  # ===========================================================================
  # Happy path — GGUF header metadata surfaces in model detail
  # ===========================================================================

  @us-22 @ac-22-1 @ac-22-3 @ac-22-4 @ac-22-5 @ac-22-10 @adr-016 @release-1 @real-io @adapter-integration @perf @k-info-1-warm-100ms
  Scenario: Model detail surfaces GGUF header metadata for a Mistral GGUF file
    Given Devon has fixture "devon-mistral-gguf"
    And "mistral:7b-instruct-q4_K_M" is registered in Ollama, llama-cli, and Hugging Face
    And the file format is GGUF v3
    When Devon presses Enter on the Mistral row
    Then the model detail screen opens within 100 ms
    And the Metadata section shows "general.architecture : llama"
    And the Metadata section shows "general.quantization_version : 2"
    And the Metadata section shows "llama.context_length : 32768"
    And the Metadata section shows "llama.embedding_length : 4096"
    And the Metadata section provenance reads "introspected just now"
    And the Format field reads "GGUF v3"
    And the Registered with section lists all 3 tool paths
    And the bottom bar on the detail screen shows "[Esc] back", "[u] unify", "[d] delete-one", "[r] re-introspect", "[?] help"

  # ===========================================================================
  # Happy path — Ollama manifest fields for an Ollama-only model
  # ===========================================================================

  @us-22 @ac-22-3 @ac-22-4 @ac-22-5 @adr-016 @release-1 @real-io @adapter-integration
  Scenario: Model detail surfaces Ollama manifest fields for an Ollama-only model
    Given Devon has fixture "devon-ollama-manifest"
    And "llama3:8b-instruct-q4_K_M" is registered in Ollama only
    When Devon opens its model detail
    Then the Metadata section shows excerpts from the Ollama manifest JSON
    And the Metadata section shows aligned key-value pairs starting with "config.architecture"
    And the Metadata section shows aligned key-value pairs starting with "parameters"
    And the Metadata section shows aligned key-value pairs starting with "template"
    And the Format field reads "Ollama manifest v2"

  # ===========================================================================
  # Happy path — HF config.json fields for HF-only model
  # ===========================================================================

  @us-22 @ac-22-3 @ac-22-4 @ac-22-5 @adr-016 @release-1 @real-io @adapter-integration
  Scenario: Model detail surfaces HF config.json fields for an HF-only model
    Given Devon has fixture "devon-hf-with-config-json"
    And "meta-llama/Llama-3-8B" is in Hugging Face only (16.0 GB)
    When Devon opens its model detail
    Then the Metadata section shows aligned key-value pairs starting with "model_type : llama"
    And the Metadata section shows "architectures : [\"LlamaForCausalLM\"]"
    And the Metadata section shows "hidden_size : 4096"
    And the Metadata section shows "num_attention_heads : 32"
    And the Metadata section shows "num_hidden_layers : 32"
    And the Format field reads "safetensors v2"

  # ===========================================================================
  # Edge — re-introspect updates provenance
  # ===========================================================================

  @us-22 @ac-22-2 @ac-22-8 @adr-016 @release-1 @real-io @adapter-integration @perf @cache-introspection
  Scenario: Re-introspect updates the metadata provenance and refreshes the cache
    Given Devon has fixture "devon-mistral-gguf"
    And Devon is on the Mistral detail screen
    And the metadata was introspected 2 hours ago
    When Devon presses 'r'
    Then "Tool::inspect_model()" re-runs against the current file
    Within 1000 ms the Metadata section updates with new values if any
    And the provenance reads "introspected just now"
    And the cache.models.metadata_introspected_at column updates

  # ===========================================================================
  # Error — un-introspectable file degrades gracefully (NO panic, NO crash)
  # ===========================================================================

  @us-22 @ac-22-7 @adr-016 @release-1 @real-io @adapter-integration @infrastructure-failure
  Scenario: Model detail for an un-introspectable file shows partial info gracefully
    Given Devon has fixture "devon-mistral-corrupt-gguf"
    And a model file's format cannot be parsed
    When Devon opens its model detail
    Then the Format field reads "GGUF v3 (header partially readable)"
    And the Metadata section shows "(introspection failed -- see diagnostics.log)"
    And the screen does not crash
    And the other panels (Registered with, Size on disk, Dedup key) still render
    And "~/.modeltap/diagnostics.log" gains a line tagged "inspect_failed tool=llama-cli reason=format_unreadable"
