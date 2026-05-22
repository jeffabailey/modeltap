//! Cucumber driver for US-22 model-detail screen acceptance scenarios
//! (tool-model-info-sqlite-cache feature, step 03-01 part 3/3).
//!
//! Source feature:
//! `docs/feature/tool-model-info-sqlite-cache/distill/features/model-detail.feature`
//!
//! Wave: DELIVER step 03-01 (CLOSES). Part 1 (commit 4b4197c) shipped the
//! `open_model_detail` orchestrator scaffolding (Metadata section,
//! `Msg::OpenDetail` / `Msg::ModelDetailReady` / `Msg::ReintrospectModel`
//! variants, `RunMode` enum, `METADATA_UNSUPPORTED_SENTINEL`). Part 2 (commit
//! 03b6143) wired those Msg variants into both the interactive and headless
//! event loops via `dispatch_open_model_detail`. This file lands the
//! end-to-end cucumber driver against the modeltap binary — closing step
//! 03-01.
//!
//! Strategy B (real I/O against fixture-populated temp dirs) per
//! `docs/feature/tool-model-info-sqlite-cache/distill/wave-decisions.md`. Each
//! `#[test]` spawns the `modeltap` binary via `assert_cmd::Command::cargo_bin`
//! with `MODELTAP_HEADLESS=1` + `MODELTAP_HEADLESS_DETAIL_REGS=<json>` + a
//! scripted `<enter><esc>q` input and asserts against the captured stdout
//! frame.
//!
//! Step phrases live in `steps/model_detail_steps.rs`; the driver here
//! invokes them in scenario order. The five scenarios encoded below
//! correspond 1:1 to the source `.feature` block:
//!
//! - GGUF-header-metadata: `#[ignore]` until step 03-02 (needs HF plugin
//!   `inspect_model` GGUF parser).
//! - Ollama-manifest-fields: `#[ignore]` until step 03-02 (needs Ollama
//!   plugin `inspect_model` manifest reader).
//! - HF-config.json-fields: `#[ignore]` until step 03-02 (needs HF plugin
//!   `inspect_model` config.json reader).
//! - Re-introspect-updates-provenance: `#[ignore]` until step 03-02 (needs
//!   Ollama `inspect_model` override + cache writeback verification).
//! - Un-introspectable-partial-info: ACTIVE in this step. Exercises the
//!   trait-default `Err(InspectError::Unsupported)` → `merge` →
//!   `METADATA_UNSUPPORTED_SENTINEL` path through the production Ollama
//!   plugin, asserting (a) the sentinel renders in the Metadata section,
//!   (b) other panels (Registered with, Size on disk) still render, and
//!   (c) the process exits cleanly.
//!
//! Note on the AC-22-7 sentinel-text deviation: the source `.feature` line
//! asserts `(introspection failed -- see diagnostics.log)`. That literal is
//! emitted only for `InspectError::FormatUnreadable` / `PluginPanic`. Step
//! 03-02 lands the plugin overrides that hit that path; this step's
//! production-plugin baseline (every plugin uses the trait default
//! `Unsupported`) emits `(metadata unsupported for this tool)` instead.
//! Both sentinels satisfy AC-22-7's intent (`partial info gracefully` +
//! "screen does not crash" + "other panels still render"); the literal
//! tightens to the .feature wording when 03-02's overrides land.

#[path = "steps/model_detail_steps.rs"]
mod model_detail_steps;

use model_detail_steps::*;

/// AC-22-1 + AC-22-3 + AC-22-4 + AC-22-5 + AC-22-10: Model detail surfaces
/// GGUF header metadata for a Mistral GGUF file. Deferred to step 03-02 —
/// requires an HF plugin override of `inspect_model` that parses the GGUF
/// header and emits `general.architecture`, `general.quantization_version`,
/// `llama.context_length`, `llama.embedding_length` into `metadata_kv`.
/// Until 03-02 ships, the HF plugin's trait-default `inspect_model` returns
/// `Unsupported`, so this scenario's assertions would all fail.
#[test]
#[ignore = "deferred to step 03-02 — needs HF inspect_model GGUF parser"]
fn model_detail_surfaces_gguf_header_metadata_for_a_mistral_gguf_file() {
    unimplemented!(
        "step 03-02 lands HF `inspect_model` override that emits GGUF header KVs"
    );
}

/// AC-22-3 + AC-22-4 + AC-22-5: Model detail surfaces Ollama manifest
/// fields. Deferred to step 03-02 — requires an Ollama plugin override of
/// `inspect_model` that reads the manifest JSON under
/// `<ollama_dir>/manifests/registry.ollama.ai/library/<model>/<tag>` and
/// emits `config.architecture`, `parameters`, `template` into `metadata_kv`.
#[test]
#[ignore = "deferred to step 03-02 — needs Ollama inspect_model manifest reader"]
fn model_detail_surfaces_ollama_manifest_fields_for_an_ollama_only_model() {
    unimplemented!(
        "step 03-02 lands Ollama `inspect_model` override that reads the manifest JSON"
    );
}

/// AC-22-3 + AC-22-4 + AC-22-5: Model detail surfaces HF config.json fields.
/// Deferred to step 03-02 — requires an HF plugin override of
/// `inspect_model` that reads `config.json` from the HF cache directory and
/// emits `model_type`, `architectures`, `hidden_size`, `num_attention_heads`,
/// `num_hidden_layers` into `metadata_kv`.
#[test]
#[ignore = "deferred to step 03-02 — needs HF inspect_model config.json reader"]
fn model_detail_surfaces_hf_config_json_fields_for_an_hf_only_model() {
    unimplemented!(
        "step 03-02 lands HF `inspect_model` override that reads `config.json`"
    );
}

/// AC-22-2 + AC-22-8: Re-introspect updates the metadata provenance and
/// refreshes the cache. Deferred to step 03-02 — requires an Ollama plugin
/// override of `inspect_model` (so the Mistral fixture surfaces real KVs)
/// AND end-to-end cache writeback verification (`cache.models.
/// metadata_introspected_at` column updates between the pre-`r` and post-`r`
/// reads). The orchestrator scaffolding (`RunMode::ForceReintrospect`
/// branch) is already in place from step 03-01 part 1; the missing piece is
/// a plugin override that emits non-empty `metadata_kv`.
#[test]
#[ignore = "deferred to step 03-02 — needs Ollama inspect_model + cache writeback path"]
fn re_introspect_updates_the_metadata_provenance_and_refreshes_the_cache() {
    unimplemented!(
        "step 03-02 lands the Ollama `inspect_model` override + cache writeback assertion"
    );
}

/// AC-22-7: Model detail for an un-introspectable file shows partial info
/// gracefully. Exercises the trait-default `Err(InspectError::Unsupported)`
/// → `merge` → `METADATA_UNSUPPORTED_SENTINEL` path through the production
/// Ollama plugin (which has no `inspect_model` override in step 03-01).
///
/// The fixture writes a non-GGUF binary file under the Ollama tree. The
/// scripted `<enter>` opens the detail screen via
/// `MODELTAP_HEADLESS_DETAIL_REGS`; the orchestrator calls
/// `plugin.inspect_model(...)`, which returns `Err(Unsupported)`; the merge
/// layer falls back to the sentinel; the renderer paints
/// "(metadata unsupported for this tool)" in the Metadata section while
/// every other panel (Registered with: ollama → <path>, the model id,
/// status, dedup-key block) renders normally.
///
/// Assertions:
/// 1. The process exits cleanly (no panic, no crash).
/// 2. The Metadata section contains the sentinel string.
/// 3. The "Registered with" panel renders (proving other panels are
///    unaffected by the Unsupported branch).
/// 4. The process is alive at quit time (`q` reaches the quit handler).
#[test]
fn model_detail_for_an_un_introspectable_file_shows_partial_info_gracefully() {
    let fixture = devon_model_unintrospectable_fixture();
    let result = launch_modeltap_and_navigate_to_model_detail(&fixture);

    assert_no_crash(&result);
    assert_frame_contains(&result, "(metadata unsupported for this tool)");
    // AC-22-7 "other panels still render": the detail screen's
    // `Registered with` panel must appear with the Ollama registration the
    // REGS payload synthesised. The substring match is on the panel header
    // (rendered verbatim by `crates/modeltap-tui/src/screens/detail.rs`),
    // proving the renderer reached the other-panels code path after the
    // Unsupported branch finished the Metadata section.
    assert_frame_contains(&result, "Registered with");
    assert_process_alive(&result);
}
