//! Plugin-contract tests for `LmStudioPlugin::inspect_model` (US-22 step 03-03).
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.13 — LM Studio overrides `inspect_model` (GGUF v3 header parser, step
//! 03-02 part 3) and must satisfy the `InspectCapability::Supported` six-test
//! suite (happy-path, unknown-id => FileReadable, corrupt => FormatUnreadable,
//! determinism, metadata_kv ≤ 10 keys per AC-22-6, panic isolation).
//!
//! NOTE — the §3.12 inspect_tool contract test for LM Studio (in the sibling
//! `inspect_tool_contract.rs`) uses `InspectCapability::Unsupported` because
//! the plugin does NOT override `inspect_tool`. The two capabilities are
//! independent: a plugin can opt into `inspect_model` without opting into
//! `inspect_tool` and vice-versa.

use std::path::PathBuf;

use modeltap_core::domain::inspect::ModelId;
use modeltap_core::tests::inspect::{run_inspect_model_contract, InspectCapability};
use modeltap_core::ToolId;
use modeltap_plugin_lm_studio::LmStudioPlugin;

/// §3.13 Supported contract: invoke the cross-plugin harness against the LM
/// Studio plugin after seeding a model tree with one happy-path GGUF v3 file
/// and one corrupt file (bad magic bytes).
///
/// Fixture layout (under `tempdir/`):
/// ```
///   mistralai/Mistral-7B-Instruct-v0.2-GGUF/mistral.Q4_K_M.gguf  ← happy GGUF v3
///   corrupt/Model/bad.gguf                                       ← corrupt magic
/// ```
///
/// The unknown-id branch uses `unknown/model/missing.gguf` — no on-disk
/// match in the configured search paths; locator returns `Err(FileReadable)`.
#[tokio::test]
async fn lm_studio_satisfies_inspect_model_contract() {
    let fixture =
        tempfile::tempdir().expect("tempdir for lm-studio inspect_model contract fixture");
    let root = fixture.path().to_path_buf();

    // Happy-path GGUF v3 file — five standard KV header entries.
    let happy_dir = root.join("mistralai").join("Mistral-7B-Instruct-v0.2-GGUF");
    std::fs::create_dir_all(&happy_dir).expect("create happy gguf dir");
    let happy_gguf = happy_dir.join("mistral.Q4_K_M.gguf");
    std::fs::write(&happy_gguf, build_synthetic_gguf_v3()).expect("write happy gguf");

    // Corrupt GGUF — wrong magic bytes. Parser returns BadMagic which the
    // plugin maps to InspectError::FormatUnreadable.
    let corrupt_dir = root.join("corrupt").join("Model");
    std::fs::create_dir_all(&corrupt_dir).expect("create corrupt gguf dir");
    std::fs::write(corrupt_dir.join("bad.gguf"), b"NOTAGGUFHEADER").expect("write corrupt gguf");

    let plugin = LmStudioPlugin::new_with_search_paths(vec![root]);
    run_inspect_model_contract(
        &plugin,
        ToolId("lm-studio"),
        InspectCapability::Supported,
        ModelId::from("mistralai/Mistral-7B-Instruct-v0.2-GGUF/mistral.Q4_K_M.gguf"),
        ModelId::from("unknown/model/missing.gguf"),
        Some(ModelId::from("corrupt/Model/bad.gguf")),
    )
    .await;
}

/// Build a minimal synthetic GGUF v3 header byte sequence carrying five KVs.
///
/// Local copy of the canonical builder in `tests/src/fixtures/inspect_fixtures.rs`
/// (`write_gguf_v3_header` + `GgufKv::*`). Re-implemented here so the plugin's
/// integration test stays decoupled from the acceptance crate (plugins must
/// not depend on acceptance-only fixtures per architecture rule R2).
///
/// Layout (LE everywhere):
///   magic "GGUF" | version 3u32 | tensor_count 0u64 | kv_count u64 |
///   { key_len u64, key_bytes, value_type u32, value_bytes }*
fn build_synthetic_gguf_v3() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"GGUF");
    out.extend_from_slice(&3u32.to_le_bytes()); // version
    out.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
    let kvs: &[(&str, GgufValue)] = &[
        ("general.architecture", GgufValue::Str("llama")),
        ("general.quantization_version", GgufValue::Str("Q4_K_M")),
        ("llama.context_length", GgufValue::U32(4096)),
        ("llama.embedding_length", GgufValue::U32(4096)),
        ("tokenizer.ggml.model", GgufValue::Str("llama")),
    ];
    out.extend_from_slice(&(kvs.len() as u64).to_le_bytes());
    for (key, value) in kvs {
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        match value {
            GgufValue::Str(s) => {
                out.extend_from_slice(&8u32.to_le_bytes()); // TYPE_STRING
                out.extend_from_slice(&(s.len() as u64).to_le_bytes());
                out.extend_from_slice(s.as_bytes());
            }
            GgufValue::U32(n) => {
                out.extend_from_slice(&4u32.to_le_bytes()); // TYPE_UINT32
                out.extend_from_slice(&n.to_le_bytes());
            }
        }
    }
    out
}

enum GgufValue {
    Str(&'static str),
    U32(u32),
}

// Suppress unused-warning if PathBuf is not referenced after the join chain
// (defensive: keeps the inspect_tool_contract.rs shape symmetric).
const _: fn() = || {
    let _: PathBuf = PathBuf::new();
};
