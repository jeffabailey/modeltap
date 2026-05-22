//! Plugin-contract tests for `OllamaPlugin::inspect_model` (US-22 step 03-03).
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.13 — Ollama overrides `inspect_model` (manifest JSON reader, step
//! 03-02 part 1) and must satisfy the `InspectCapability::Supported` six-test
//! suite (happy-path, unknown-id => FileReadable, corrupt => FormatUnreadable,
//! determinism, metadata_kv ≤ 10 keys per AC-22-6, panic isolation).
//!
//! The parametrized harness in `modeltap_core::tests::inspect` is the single
//! source of truth for the contract — this file is a thin shim that wires
//! the Ollama plugin instance into it after seeding a synthetic manifest tree
//! under a tempdir. Step-02-02 / 03-02 Ollama-specific behaviors stay in
//! `tests/inspect_tool_contract.rs` and the in-crate unit tests respectively.

use std::path::PathBuf;

use modeltap_core::domain::inspect::ModelId;
use modeltap_core::tests::inspect::{run_inspect_model_contract, InspectCapability};
use modeltap_core::ToolId;
use modeltap_plugin_ollama::OllamaPlugin;

/// A synthetic Ollama manifest JSON that the plugin's `inspect_model` parses
/// for the §3.13.S.1 / §3.13.S.4 / §3.13.S.5 happy-path cases. Mirrors the
/// canonical fixture body the Ollama in-crate tests use, trimmed to the
/// fields the projector reads (`config.architecture`, `config.parameter_size`,
/// `config.quantization_level`, `template`, `system`).
const SYNTHETIC_MANIFEST: &str = r#"{
  "schemaVersion": 2,
  "config": {
    "architecture": "llama",
    "parameter_size": "7B",
    "quantization_level": "Q4_K_M"
  },
  "template": "{{ .System }}\nUser: {{ .Prompt }}\n",
  "system": "You are a helpful assistant."
}"#;

/// A corrupt manifest body — invalid JSON. The plugin's `inspect_model`
/// reads + parses the file and must surface
/// `Err(InspectError::FormatUnreadable)` per §3.13.S.3.
const CORRUPT_MANIFEST: &str = "{ this is not valid json";

/// §3.13 Supported contract: invoke the cross-plugin harness against the
/// Ollama plugin after seeding a manifest tree with one happy-path manifest
/// and one corrupt manifest.
///
/// Fixture layout (under `tempdir/models-root/`):
/// ```
///   manifests/
///     registry.ollama.ai/library/llama3/8b-instruct-q4_K_M    ← happy
///     registry.ollama.ai/library/broken/tag                   ← corrupt
/// ```
///
/// The unknown-id branch uses `nonexistent:tag` — the locator walks the
/// tree, finds no match, returns `Err(FileReadable)`.
#[tokio::test]
async fn ollama_satisfies_inspect_model_contract() {
    let fixture = tempfile::tempdir().expect("tempdir for ollama inspect_model contract fixture");
    let models_root = fixture.path().to_path_buf();

    // Happy-path manifest.
    let happy_dir = models_root
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("llama3");
    std::fs::create_dir_all(&happy_dir).expect("create happy manifest dir");
    std::fs::write(happy_dir.join("8b-instruct-q4_K_M"), SYNTHETIC_MANIFEST)
        .expect("write happy manifest");

    // Corrupt manifest — same locator projection, body unparseable.
    let corrupt_dir = models_root
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("broken");
    std::fs::create_dir_all(&corrupt_dir).expect("create corrupt manifest dir");
    std::fs::write(corrupt_dir.join("tag"), CORRUPT_MANIFEST).expect("write corrupt manifest");

    let plugin = OllamaPlugin::new_with_root(models_root);
    run_inspect_model_contract(
        &plugin,
        ToolId("ollama"),
        InspectCapability::Supported,
        ModelId::from("llama3:8b-instruct-q4_K_M"),
        ModelId::from("nonexistent:tag"),
        Some(ModelId::from("broken:tag")),
    )
    .await;

    // Suppress unused-import lint if PathBuf isn't otherwise referenced after
    // the join chain (defensive: matches the sibling inspect_tool_contract).
    let _: PathBuf = std::path::PathBuf::new();
}
