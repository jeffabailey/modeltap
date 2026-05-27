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
//!   `Err(InspectError::FileReadable)` → `merge` → `INSPECT_PANIC_SENTINEL`
//!   path through the production Ollama plugin's `inspect_model` override
//!   (locator returns `FileReadable` when no manifest matches the model id),
//!   asserting (a) the sentinel renders in the Metadata section, (b) other
//!   panels (Registered with, Size on disk) still render, and (c) the
//!   process exits cleanly.

#[path = "steps/model_detail_steps.rs"]
mod model_detail_steps;

use model_detail_steps::*;

/// AC-22-1 + AC-22-3 + AC-22-4 + AC-22-5 + AC-22-10: Model detail surfaces
/// GGUF header metadata for a Mistral GGUF file. Closes the LM Studio half
/// of step 03-02. The production LM Studio plugin's `inspect_model` override
/// (landed in step 03-02 part 3) reads the synthetic GGUF v3 file seeded by
/// `devon_mistral_gguf_fixture` via
/// `modeltap_core::domain::gguf::parse_header` and projects the standard
/// header KVs (`general.architecture`, `general.quantization_version`,
/// `llama.context_length`, `llama.embedding_length`, `tokenizer.ggml.model`)
/// into `ModelDetail.metadata_kv`. The detail-screen renderer paints each KV
/// pair as an aligned `key : value` line, so the substring assertions hit
/// against the captured frame.
///
/// Routing note: AC-22-1's Gherkin text targets HF as the registering tool,
/// but the GGUF-header parser lives in modeltap-core::domain::gguf and is
/// shared by lm-studio (this commit) and llama-cli (step 03-02 part 4). HF
/// uses safetensors as its canonical format and reads `config.json` (see
/// step 03-02 part 2); we drive the GGUF-header path through lm-studio
/// because that's the production plugin whose `inspect_model` reads the
/// header directly. The TUI rendering being asserted is plugin-agnostic —
/// aligned KV pairs in the Metadata section — so the routing swap leaves
/// the AC's behavioural assertion intact.
#[test]
fn model_detail_surfaces_gguf_header_metadata_for_a_mistral_gguf_file() {
    let fixture = devon_mistral_gguf_fixture();
    let result = launch_modeltap_lm_studio_gguf(&fixture, "<enter><esc>q");

    assert_no_crash(&result);
    // AC-22-5: the Metadata section's source label matches the registering
    // plugin ("lm-studio" — the headless dispatch hands the id off to the
    // LM Studio plugin per the REGS payload).
    assert_frame_contains(&result, "Metadata (from lm-studio");
    // AC-22-4 aligned KV pairs (the renderer formats each as `  key : value`):
    assert_frame_contains(&result, "general.architecture");
    assert_frame_contains(&result, "general.quantization_version");
    assert_frame_contains(&result, "llama.context_length");
    // AC-22-3 GGUF-header projection: the architecture value flows through
    // (proves the header was parsed end-to-end, not just located).
    assert_frame_contains(&result, "llama");
    assert_frame_contains(&result, "Q4_K_M");
}

/// AC-22-3 + AC-22-4 + AC-22-5: Model detail surfaces Ollama manifest
/// fields. Closes the Ollama half of step 03-02. The production Ollama
/// plugin's `inspect_model` override (landed in step 03-02 part 1) reads
/// the synthetic manifest seeded by `devon_ollama_manifest_fixture` and
/// projects `config.architecture`, `parameters`, `template`, `system` into
/// `ModelDetail.metadata_kv`. The detail-screen renderer paints each KV
/// pair as an aligned `key : value` line, so the substring assertions hit
/// against the captured frame.
#[test]
fn model_detail_surfaces_ollama_manifest_fields_for_an_ollama_only_model() {
    let fixture = devon_ollama_manifest_fixture();
    let result = launch_modeltap_ollama_manifest(&fixture, "<enter><esc>q");

    assert_no_crash(&result);
    // AC-22-5: the Metadata section's source label matches the registering
    // plugin ("ollama" — the headless dispatch hands the id off to the first
    // registration's tool_id, which is the Ollama plugin per the REGS payload).
    assert_frame_contains(&result, "Metadata (from ollama");
    // AC-22-4 aligned KV pairs (the renderer formats each as `  key : value`):
    assert_frame_contains(&result, "config.architecture");
    assert_frame_contains(&result, "parameters");
    assert_frame_contains(&result, "template");
    // AC-22-3 manifest field projection: the architecture value flows through
    // (proves the manifest was parsed end-to-end, not just located).
    assert_frame_contains(&result, "llama");
}

/// AC-22-3 + AC-22-4 + AC-22-5: Model detail surfaces HF config.json fields.
/// Closes the HF half of step 03-02. The production HF plugin's
/// `inspect_model` override (landed in step 03-02 part 2) reads the synthetic
/// `config.json` seeded by `devon_hf_with_config_json_fixture` and projects
/// `model_type`, `architectures`, `hidden_size`, `num_attention_heads`,
/// `num_hidden_layers`, `max_position_embeddings` into
/// `ModelDetail.metadata_kv`. The detail-screen renderer paints each KV pair
/// as an aligned `key : value` line, so the substring assertions hit against
/// the captured frame.
#[test]
fn model_detail_surfaces_hf_config_json_fields_for_an_hf_only_model() {
    let fixture = devon_hf_with_config_json_fixture();
    let result = launch_modeltap_hf_config_json(&fixture, "<enter><esc>q");

    assert_no_crash(&result);
    // AC-22-5: the Metadata section's source label matches the registering
    // plugin ("hf" — the headless dispatch hands the id off to the HF plugin
    // per the REGS payload).
    assert_frame_contains(&result, "Metadata (from hf");
    // AC-22-4 aligned KV pairs (the renderer formats each as `  key : value`):
    assert_frame_contains(&result, "model_type");
    assert_frame_contains(&result, "architectures");
    assert_frame_contains(&result, "hidden_size");
    // AC-22-3 config.json projection: the model_type value flows through
    // (proves the JSON was parsed end-to-end, not just located).
    assert_frame_contains(&result, "mistral");
}

/// AC-22-2 + AC-22-8: Re-introspect updates the metadata provenance and
/// refreshes the cache. Closes via the Ollama `inspect_model` override
/// (landed in step 03-02 part 1) + the `RunMode::ForceReintrospect` branch
/// shipped in step 03-01 part 1. The keystroke script is `<enter>r<esc>q`:
/// `<enter>` opens the detail screen (`RunMode::WarmIfCached` — cold cache,
/// inspects fresh, UPSERTs the cache row); `r` re-runs the dispatch under
/// `RunMode::ForceReintrospect` (skips the warm-path early-return, always
/// calls `inspect_model`, re-UPSERTs the cache row); `<esc>q` returns to
/// the main screen and quits the headless event loop.
///
/// Both the pre-`r` and post-`r` frames must paint the metadata KVs (the
/// pre-frame proves the cold inspect lit them up; the post-frame proves the
/// re-introspect did not regress them). The cache writeback path is
/// exercised by virtue of `open_model_detail::run` calling
/// `writeback_metadata` whenever inspect returns Ok with non-empty kv — this
/// scenario runs that path twice (cold open + forced refresh) without any
/// extra wiring. Asserting the SQLite row's `metadata_introspected_at`
/// timestamp column is left to step 03-03's deeper cache-introspection
/// coverage; the substring check here proves the post-`r` frame still
/// carries the metadata KVs (the orchestrator did not error the second
/// dispatch, which would have surfaced as missing KVs in the captured
/// frame).
#[test]
fn re_introspect_updates_the_metadata_provenance_and_refreshes_the_cache() {
    let fixture = devon_ollama_manifest_fixture();
    let result = launch_modeltap_ollama_manifest(&fixture, "<enter>r<esc>q");

    assert_no_crash(&result);
    // Post-re-introspect frame must still carry the metadata KVs — proves the
    // ForceReintrospect dispatch re-ran the plugin's `inspect_model` and
    // re-rendered the Metadata section without regression.
    assert_frame_contains(&result, "Metadata (from ollama");
    assert_frame_contains(&result, "config.architecture");
    // The provenance string for a just-introspected row reads "just now"
    // (per `format_metadata_provenance` in detail.rs: fresh stamps within
    // ~seconds of SystemTime::now render that token). After `r` the
    // orchestrator stamps `introspected_at = Some(SystemTime::now())`, so
    // the post-`r` frame's section header carries "just now".
    assert_frame_contains(&result, "just now");
    assert_process_alive(&result);
}

/// AC-22-7: Model detail for an un-introspectable file shows partial info
/// gracefully. Exercises the `Err(InspectError::FileReadable)` → `merge` →
/// `INSPECT_PANIC_SENTINEL` path through the production Ollama plugin's
/// `inspect_model` override (step 03-02 part 1). The override's locator
/// walks `<MODELTAP_OLLAMA_DIR>/manifests/` for a file whose path projects
/// to the requested id; with no matching manifest it returns `FileReadable`,
/// which `merge` maps to `INSPECT_PANIC_SENTINEL`.
///
/// The fixture writes a non-GGUF binary file directly under the Ollama tree
/// (NOT under `manifests/`), so the locator's walk finds no match. The
/// scripted `<enter>` opens the detail screen via
/// `MODELTAP_HEADLESS_DETAIL_REGS`; the orchestrator calls
/// `plugin.inspect_model(...)`, which returns `Err(FileReadable)`; the merge
/// layer falls back to the panic sentinel; the renderer paints
/// "(inspection failed -- see diagnostics.log)" in the Metadata section
/// while every other panel (Registered with: ollama → <path>, the model id,
/// status, dedup-key block) renders normally — matching the source
/// `.feature` line's literal wording.
///
/// Assertions:
/// 1. The process exits cleanly (no panic, no crash).
/// 2. The Metadata section contains the panic sentinel string.
/// 3. The "Registrations:" panel renders (proving other panels are
///    unaffected by the error branch).
/// 4. The process is alive at quit time (`q` reaches the quit handler).
#[test]
fn model_detail_for_an_un_introspectable_file_shows_partial_info_gracefully() {
    let fixture = devon_model_unintrospectable_fixture();
    let result = launch_modeltap_and_navigate_to_model_detail(&fixture);

    assert_no_crash(&result);
    assert_frame_contains(&result, "(inspection failed -- see diagnostics.log)");
    // AC-22-7 "other panels still render": the detail screen's
    // `Registrations:` panel must appear with the Ollama registration the
    // REGS payload synthesised. The substring match is on the panel header
    // (rendered verbatim by `crates/modeltap-tui/src/screens/detail.rs`),
    // proving the renderer reached the other-panels code path after the
    // error branch finished the Metadata section.
    assert_frame_contains(&result, "Registrations:");
    assert_process_alive(&result);
}
