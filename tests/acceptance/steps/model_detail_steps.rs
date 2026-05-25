//! Step-definition helpers for the US-22 model-detail acceptance scenarios.
//!
//! Driven by `tests/acceptance/model_detail.rs`. Each helper is named after
//! the Gherkin phrase it implements (per `step-definitions-skeleton.md`
//! conventions) so the driver reads scenario-order with no glue.
//!
//! The acceptance crate does NOT use cucumber-rs; every existing scenario
//! is a plain `#[test]` function that drives the `modeltap` binary through
//! `assert_cmd::Command::cargo_bin`. This module mirrors that pattern (see
//! `tool_detail_steps.rs` for the US-21 parallel and
//! `integration_checkpoints_steps.rs` for the INT-INFO-8 parallel).
//!
//! Wave: DELIVER step 03-01 part 3/3 — closes the cucumber driver layer of
//! the model-detail acceptance scaffolding. Part 1 (commit 4b4197c) landed
//! the open_model_detail orchestrator and the Msg::OpenDetail /
//! ReintrospectModel variants; part 2 (commit 03b6143) wired those Msg
//! variants into both the interactive and headless event loops via
//! `dispatch_open_model_detail`. This part lands the end-to-end cucumber
//! driver against the modeltap binary.
//!
//! Step 03-01 scope: ONE active scenario (AC-22-7 partial-info-graceful via
//! the Unsupported render path) + four `#[ignore]` scenarios deferred to
//! step 03-02 (the GGUF / Ollama-manifest / HF-config.json / re-introspect
//! scenarios all require plugin overrides of `inspect_model` that no
//! production plugin ships in this step).
//!
//! Plugin route: AC-22-7 is driven through the Ollama plugin's
//! trait-default `inspect_model` (returns `Err(InspectError::Unsupported)`),
//! which the orchestrator's merge maps to `METADATA_UNSUPPORTED_SENTINEL`
//! ("(metadata unsupported for this tool)"). The `MODELTAP_HEADLESS_DETAIL_REGS`
//! JSON payload's `tool` field uses `"ollama"` because the headless
//! synthesizer's whitelist accepts `{ollama, hf, lm-studio}` only (see
//! `synthesize_detail_from_env` in `crates/modeltap-app/src/headless.rs`).
//! No new env-var seam is added — the Unsupported default body already
//! exercises the partial-info-graceful render that AC-22-7 specifies.

#![allow(dead_code)] // The four #[ignore]'d scenarios in the driver leave
                     // some helpers unused until step 03-02 picks them up.

use std::process::Output;
use std::time::Duration;

use assert_cmd::Command;
pub use modeltap_acceptance::fixtures::inspect_fixtures::{
    devon_hf_config_json_path, devon_hf_with_config_json_fixture, devon_mistral_gguf_fixture,
    devon_model_unintrospectable_fixture, devon_ollama_manifest_fixture,
    devon_ollama_manifest_path, devon_unintrospectable_model_path, InspectFixture,
    LmStudioGgufFixture, HF_CONFIG_JSON_FIXTURE_ID, LM_STUDIO_GGUF_FIXTURE_ID,
    OLLAMA_MANIFEST_FIXTURE_ID,
};

/// Captured outcome of one `modeltap` headless launch. Mirrors
/// `integration_checkpoints_steps::LaunchResult` shape exactly so the
/// Then-step helpers below can be re-used across the two suites without
/// type-shimming.
pub struct LaunchResult {
    pub output: Output,
    pub stdout: String,
    pub stderr: String,
}

impl LaunchResult {
    fn from_output(output: Output) -> Self {
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Self {
            output,
            stdout,
            stderr,
        }
    }
}

/// `When Devon opens the model detail screen for the un-introspectable file`
///
/// Spawns one modeltap process headless and scripts an `<enter><esc>q`
/// keystroke sequence so the orchestrator's `Msg::OpenDetail(_)` path runs
/// end to end. The `MODELTAP_HEADLESS_DETAIL_REGS` JSON payload synthesises
/// a `DetailScreenState` registered with the Ollama plugin against the
/// fixture's un-introspectable model path; the production Ollama plugin's
/// trait-default `inspect_model` returns `Err(InspectError::Unsupported)`,
/// which the orchestrator's merge maps to the `METADATA_UNSUPPORTED_SENTINEL`
/// rendered in the Metadata section.
///
/// The lift `MODELTAP_HEADLESS_DETAIL_REGS` env-var documented in
/// `headless.rs::synthesize_detail_from_env` is the only seam that triggers
/// `Msg::OpenDetail` from the headless event loop's `<enter>` script-token
/// (the production keymap routes Enter on Main into
/// `Msg::ToggleFolderExpansion`; the headless harness has a parallel "lift"
/// that, when REGS is set, rewrites that Msg into `Msg::OpenDetail(detail)`
/// — same lift pattern as `MODELTAP_HEADLESS_TOOL_DETAIL=1` for US-21).
///
/// Returns a `LaunchResult` carrying the captured stdout (the painted
/// frames), stderr, and `ExitStatus`. The Then-step helpers below substring-
/// match the captured stdout.
pub fn launch_modeltap_and_navigate_to_model_detail(fixture: &InspectFixture) -> LaunchResult {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");

    let model_path = devon_unintrospectable_model_path(fixture);
    // The synthesizer accepts only {ollama, hf, lm-studio} for the `tool`
    // field. We route through the Ollama plugin because its trait-default
    // `inspect_model` returns Unsupported — the exact merge-branch AC-22-7's
    // partial-info-graceful render requires.
    let regs_payload = serde_json::json!({
        "id": "unintrospectable-model",
        "regs": [
            {
                "tool": "ollama",
                "path": model_path.to_string_lossy(),
            }
        ]
    })
    .to_string();

    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "120")
        // Register the in-process TestTool so the modeltap composition root
        // boots with a non-empty plugin registry (used by the other
        // scenarios in this suite when 03-02 lands their plugin overrides).
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", fixture.test_tool_root())
        // Point the production Ollama plugin at the fixture's ollama-root.
        // The plugin's discover() surfaces NotInstalled (the manifests/
        // directory is empty), so production discovery contributes nothing
        // to the left pane — every model in the detail-screen render comes
        // from the synthesised REGS payload. inspect_model on the Ollama
        // plugin falls through to the trait-default Unsupported.
        .env(
            "MODELTAP_OLLAMA_DIR",
            fixture.ollama_dir.to_string_lossy().into_owned(),
        )
        // The MODELTAP_HEADLESS_DETAIL_REGS seam (US-10 / US-19 parallel)
        // synthesises a DetailScreenState the headless loop pushes into the
        // app after the scripted <enter>. Combined with the dispatch wiring
        // landed in step 03-01 part 2, this drives Msg::OpenDetail →
        // dispatch_open_model_detail → open_model_detail::run end to end.
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs_payload)
        // Best-effort diagnostics dir override so any future panic-path
        // additions to this suite write to the tempdir rather than
        // `~/.modeltap`. Not strictly required by AC-22-7 (the Unsupported
        // path doesn't touch diagnostics.log), but mirrors the production
        // wiring and isolates the test from the dev's home directory.
        .env("MODELTAP_DIAGNOSTICS_DIR", fixture.diagnostics_dir())
        .env("MODELTAP_CACHE_PATH", fixture.cache_path())
        .env("MODELTAP_LOG_DIR", fixture.log_dir())
        // Per-plugin isolation — keep every other real plugin at a
        // nonexistent path so they discover-NotInstalled. Mirrors the
        // tool_detail / integration_checkpoints suites.
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env(
            "MODELTAP_CONFIG_PATH",
            fixture.config_path.to_string_lossy().into_owned(),
        )
        .env("HF_HOME", "/nonexistent/no-such-hf-cache")
        // Short-circuit Ollama's `inspect_tool` HTTP probe (ADR-016 §D12 /
        // R5). Irrelevant to AC-22-7's assertion surface (Metadata section
        // sentinel + non-crash) but keeps the inspect path deterministic
        // across CI and dev hardware.
        .env("MODELTAP_OLLAMA_VERSION", "0.6.4")
        .env("MODELTAP_HEADLESS_INPUT", "<enter><esc>q")
        .timeout(Duration::from_secs(30));

    let output = cmd.output().expect("spawn modeltap process");
    LaunchResult::from_output(output)
}

// ---------------------------------------------------------------------------
// Then-step helpers — shape-identical to integration_checkpoints_steps so
// the AC-22-7 scenario reads symmetrically with INT-INFO-8. Re-defined
// rather than re-exported because that module's `#[allow(dead_code)]` keeps
// its helpers crate-private; cross-test-binary visibility would require
// promoting them into the lib half, which is more churn than this step
// warrants.
// ---------------------------------------------------------------------------

/// `Then the screen does not crash`
///
/// AC-22-7 invariant: the orchestrator's `inspect_model` merge handles the
/// `Err(InspectError::Unsupported)` branch cleanly. The process must
/// therefore exit with status 0 (the headless harness drives the scripted
/// `q` quit naturally) — a non-zero exit would indicate the Unsupported
/// branch escaped the merge layer and unwound the modeltap process itself.
///
/// We assert success here rather than later because a non-zero exit
/// invalidates every other assertion: if the process aborted mid-render
/// the captured stdout frame may be truncated.
pub fn assert_no_crash(result: &LaunchResult) {
    assert!(
        result.output.status.success(),
        "modeltap process must exit 0 (Unsupported branch handled cleanly); \
         got status={:?}, stderr={}",
        result.output.status,
        result.stderr,
    );
}

/// `Then the rendered frame contains "<substring>"`
///
/// Substring-greps the captured stdout (the headless harness prints every
/// painted frame). After the `<enter>` the detail screen renders; its
/// Metadata section contains `METADATA_UNSUPPORTED_SENTINEL`. The frame
/// painted before `<esc>` is the one we assert on, but ANY frame containing
/// the substring satisfies the assertion (the sentinel is unique and only
/// appears when the orchestrator's `merge()` path took the Unsupported
/// branch).
pub fn assert_frame_contains(result: &LaunchResult, needle: &str) {
    assert!(
        result.stdout.contains(needle),
        "captured frame must contain '{needle}'; got stdout:\n{}\nstderr:\n{}",
        result.stdout,
        result.stderr,
    );
}

/// `Then the process is still alive after the merge`
///
/// At the integration-test boundary "still alive" reduces to "exited
/// cleanly under its own keystroke control" — the scripted `q` token quits
/// the event loop naturally, so `output.status.success()` confirms the
/// process reached the quit handler rather than being torn down by an
/// unwinding panic. (A panic that escaped the orchestrator and unwound the
/// main thread would either abort the process with a non-zero status or
/// hang past the 30-second `assert_cmd` timeout — both surface as
/// `output.status.success() == false` here.)
///
/// This is intentionally a stronger statement than `assert_no_crash`:
/// `assert_no_crash` says "the process did not crash mid-render"; this
/// helper says "the process ran past the merge, painted the post-merge
/// frame, accepted the `<esc>` to dismiss the detail screen, and accepted
/// the `q` to quit". Same pattern as `integration_checkpoints_steps::
/// assert_process_alive`.
pub fn assert_process_alive(result: &LaunchResult) {
    assert!(
        result.output.status.success(),
        "process must have completed its scripted input run; non-zero exit \
         indicates the merge unwound past the orchestrator boundary. \
         status={:?}, stderr={}",
        result.output.status,
        result.stderr,
    );
}

// ---------------------------------------------------------------------------
// AC-22-3 + AC-22-4 + AC-22-5 (step 03-02 part 1/N): Ollama manifest fields
// surface in the Metadata section for an Ollama-only model.
//
// Drives the production Ollama plugin's `inspect_model` override (landed in
// step 03-02 part 1) end to end: the fixture seeds a synthetic manifest at
// `<ollama_dir>/manifests/registry.ollama.ai/library/llama3/8b-instruct-q4_K_M`;
// the `MODELTAP_HEADLESS_DETAIL_REGS` payload's `id` field is
// `OLLAMA_MANIFEST_FIXTURE_ID` so the orchestrator passes that exact id to
// `plugin.inspect_model(&id)`; the plugin's locator walks the manifests dir,
// finds the matching file, parses the JSON, projects the KV subset
// (`config.architecture`, `parameters`, `template`, `system`), and returns
// `Ok(ModelDetail)`. The detail-screen renderer paints each KV pair as an
// aligned `key : value` line.
//
// The keystroke script differs by scenario:
// - AC-22-3 (warm-path open):       `<enter><esc>q`
// - AC-22-8 (re-introspect refresh): `<enter>r<esc>q`
// ---------------------------------------------------------------------------

/// Spawn modeltap headless against the AC-22-3 / AC-22-8 manifest fixture and
/// script the requested keystroke sequence. Returns the captured frame so the
/// Then-step helpers can substring-match the metadata KVs.
///
/// `headless_input` controls the scenario shape — `<enter><esc>q` exercises
/// the cold-open path (AC-22-3) and `<enter>r<esc>q` exercises the re-
/// introspect refresh path (AC-22-8). Both routes flow through the same
/// `dispatch_open_model_detail` orchestrator, just with `RunMode::WarmIfCached`
/// vs `RunMode::ForceReintrospect`.
pub fn launch_modeltap_ollama_manifest(
    fixture: &InspectFixture,
    headless_input: &str,
) -> LaunchResult {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");

    let manifest_path = devon_ollama_manifest_path(fixture);
    // The headless synthesizer routes the `tool` field through the production
    // Ollama plugin. `path` is the on-disk model artefact's location; the
    // synthesizer uses it to derive inode + size, the renderer prints it
    // verbatim in the `Registered with` panel. The plugin's `inspect_model`
    // does NOT read this path — the inspector reads the manifest file under
    // `<ollama_dir>/manifests/...`. Using the manifest path here gives the
    // renderer a real on-disk file to stat (so the panel paints non-zero
    // values) without inventing a phantom blob path.
    let regs_payload = serde_json::json!({
        "id": OLLAMA_MANIFEST_FIXTURE_ID,
        "regs": [
            {
                "tool": "ollama",
                "path": manifest_path.to_string_lossy(),
            }
        ]
    })
    .to_string();

    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "160")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", fixture.test_tool_root())
        .env(
            "MODELTAP_OLLAMA_DIR",
            fixture.ollama_dir.to_string_lossy().into_owned(),
        )
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs_payload)
        .env("MODELTAP_DIAGNOSTICS_DIR", fixture.diagnostics_dir())
        .env("MODELTAP_CACHE_PATH", fixture.cache_path())
        .env("MODELTAP_LOG_DIR", fixture.log_dir())
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env(
            "MODELTAP_CONFIG_PATH",
            fixture.config_path.to_string_lossy().into_owned(),
        )
        .env("HF_HOME", "/nonexistent/no-such-hf-cache")
        // Short-circuit the inspect_tool HTTP probe (irrelevant to the
        // model-detail surface but keeps the launch deterministic).
        .env("MODELTAP_OLLAMA_VERSION", "0.6.4")
        .env("MODELTAP_HEADLESS_INPUT", headless_input)
        .timeout(Duration::from_secs(30));

    let output = cmd.output().expect("spawn modeltap process");
    LaunchResult::from_output(output)
}

// ---------------------------------------------------------------------------
// AC-22-3 + AC-22-4 + AC-22-5 (step 03-02 part 2/N): HF config.json fields
// surface in the Metadata section for an HF-only model.
//
// Drives the production HF plugin's `inspect_model` override (landed in step
// 03-02 part 2) end to end: the fixture seeds a synthetic config.json at
// `<HF_HOME>/hub/models--mistralai--Mistral-7B-v0.1/snapshots/<rev>/config.json`;
// the `MODELTAP_HEADLESS_DETAIL_REGS` payload's `id` field is
// `HF_CONFIG_JSON_FIXTURE_ID` so the orchestrator passes that exact id to
// `plugin.inspect_model(&id)`; the plugin's locator parses out
// `<org>/<repo>`, joins the snapshot dir via `refs/main`, reads the JSON, and
// projects the KV subset (`model_type`, `architectures`, `hidden_size`,
// `num_attention_heads`, `num_hidden_layers`, `max_position_embeddings`) into
// `ModelDetail.metadata_kv`. The detail-screen renderer paints each KV pair
// as an aligned `key : value` line.
// ---------------------------------------------------------------------------

/// Spawn modeltap headless against the AC-22-3 / AC-22-4 / AC-22-5 HF fixture
/// and script the requested keystroke sequence. Returns the captured frame so
/// the Then-step helpers can substring-match the metadata KVs.
pub fn launch_modeltap_hf_config_json(
    fixture: &InspectFixture,
    headless_input: &str,
) -> LaunchResult {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");

    let config_path = devon_hf_config_json_path(fixture);
    // The headless synthesizer routes the `tool` field through the production
    // HF plugin. `path` is the on-disk model artefact's location; the
    // synthesizer uses it to derive inode + size, the renderer prints it
    // verbatim in the `Registered with` panel. The plugin's `inspect_model`
    // does NOT read this path — the inspector reads `config.json` under
    // `<HF_HOME>/hub/...`. Using the config.json path here gives the
    // renderer a real on-disk file to stat (so the panel paints non-zero
    // values) without inventing a phantom blob path.
    let regs_payload = serde_json::json!({
        "id": HF_CONFIG_JSON_FIXTURE_ID,
        "regs": [
            {
                "tool": "hf",
                "path": config_path.to_string_lossy(),
            }
        ]
    })
    .to_string();

    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "160")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", fixture.test_tool_root())
        // Point the HF plugin at the fixture's hf-cache root. The plugin
        // reads `<HF_HOME>/hub/` per `resolve_hub_root` (see
        // `plugins/hf/src/discover.rs`).
        .env("HF_HOME", fixture.hf_home.to_string_lossy().into_owned())
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs_payload)
        .env("MODELTAP_DIAGNOSTICS_DIR", fixture.diagnostics_dir())
        .env("MODELTAP_CACHE_PATH", fixture.cache_path())
        .env("MODELTAP_LOG_DIR", fixture.log_dir())
        // Park every OTHER plugin at a nonexistent root so they
        // discover-NotInstalled and contribute nothing to the inventory —
        // only the HF path is under test here.
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama-root")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env(
            "MODELTAP_CONFIG_PATH",
            fixture.config_path.to_string_lossy().into_owned(),
        )
        // Short-circuit Ollama's inspect_tool HTTP probe (irrelevant to the
        // HF model-detail surface, but keeps the launch deterministic across
        // CI and dev hardware).
        .env("MODELTAP_OLLAMA_VERSION", "0.6.4")
        .env("MODELTAP_HEADLESS_INPUT", headless_input)
        .timeout(Duration::from_secs(30));

    let output = cmd.output().expect("spawn modeltap process");
    LaunchResult::from_output(output)
}

// ---------------------------------------------------------------------------
// AC-22-3 + AC-22-4 + AC-22-5 (step 03-02 part 3/N): GGUF header KVs surface
// in the Metadata section for an LM-Studio-only model.
//
// Drives the production LM Studio plugin's `inspect_model` override (landed
// in step 03-02 part 3) end to end: the fixture seeds a synthetic GGUF v3
// file at `<root>/mistralai/Mistral-7B-Instruct-v0.2-GGUF/mistral.Q4_K_M.gguf`;
// the `MODELTAP_HEADLESS_DETAIL_REGS` payload's `id` field is
// `LM_STUDIO_GGUF_FIXTURE_ID` so the orchestrator passes that exact id to
// `plugin.inspect_model(&id)`; the plugin's locator walks each configured
// search path, finds the matching file under `MODELTAP_LMSTUDIO_DIRS`,
// invokes `modeltap_core::domain::gguf::parse_header` to read the metadata
// KV table, projects the standard subset (`general.architecture`,
// `general.quantization_version`, `llama.context_length`,
// `llama.embedding_length`, `tokenizer.ggml.model`), and returns
// `Ok(ModelDetail)`. The detail-screen renderer paints each KV pair as an
// aligned `key : value` line.
// ---------------------------------------------------------------------------

/// Spawn modeltap headless against the AC-22-1 LM Studio GGUF fixture and
/// script the requested keystroke sequence. Returns the captured frame so the
/// Then-step helpers can substring-match the metadata KVs.
pub fn launch_modeltap_lm_studio_gguf(
    fixture: &LmStudioGgufFixture,
    headless_input: &str,
) -> LaunchResult {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");

    let gguf_path = fixture.gguf_path();
    // The headless synthesizer routes the `tool` field through the production
    // LM Studio plugin. `path` is the on-disk GGUF artefact's location; the
    // synthesizer uses it to derive inode + size, and the plugin's
    // `inspect_model` reads the same file under `MODELTAP_LMSTUDIO_DIRS`.
    let regs_payload = serde_json::json!({
        "id": LM_STUDIO_GGUF_FIXTURE_ID,
        "regs": [
            {
                "tool": "lm-studio",
                "path": gguf_path.to_string_lossy(),
            }
        ]
    })
    .to_string();

    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "160")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", fixture.inner.test_tool_root())
        // Point the LM Studio plugin at the fixture's lm-studio-models root.
        // The plugin's `config::load_config` reads `MODELTAP_LMSTUDIO_DIRS`
        // (colon-separated) and threads it into `LmStudioPlugin.search_paths`,
        // which `inspect_model_impl` walks.
        .env(
            "MODELTAP_LMSTUDIO_DIRS",
            fixture.lm_studio_root.to_string_lossy().into_owned(),
        )
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs_payload)
        .env("MODELTAP_DIAGNOSTICS_DIR", fixture.inner.diagnostics_dir())
        .env("MODELTAP_CACHE_PATH", fixture.inner.cache_path())
        .env("MODELTAP_LOG_DIR", fixture.inner.log_dir())
        // Park every OTHER plugin at a nonexistent root so they
        // discover-NotInstalled and contribute nothing to the inventory —
        // only the LM Studio path is under test here.
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama-root")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env(
            "MODELTAP_CONFIG_PATH",
            fixture.inner.config_path.to_string_lossy().into_owned(),
        )
        .env("HF_HOME", "/nonexistent/no-such-hf-cache")
        // Short-circuit Ollama's inspect_tool HTTP probe (irrelevant to the
        // LM Studio model-detail surface, but keeps the launch deterministic
        // across CI and dev hardware).
        .env("MODELTAP_OLLAMA_VERSION", "0.6.4")
        .env("MODELTAP_HEADLESS_INPUT", headless_input)
        .timeout(Duration::from_secs(30));

    let output = cmd.output().expect("spawn modeltap process");
    LaunchResult::from_output(output)
}
