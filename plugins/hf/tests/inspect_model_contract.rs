//! Plugin-contract tests for `HfPlugin::inspect_model` (US-22 step 03-03).
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.13 — HF overrides `inspect_model` (config.json reader, step 03-02
//! part 2) and must satisfy the `InspectCapability::Supported` six-test
//! suite (happy-path, unknown-id => FileReadable, corrupt => FormatUnreadable,
//! determinism, metadata_kv ≤ 10 keys per AC-22-6, panic isolation).
//!
//! The parametrized harness in `modeltap_core::tests::inspect` is the single
//! source of truth for the contract — this file is a thin shim that wires
//! the HF plugin instance into it after seeding a synthetic hub tree under
//! a tempdir.

use modeltap_core::domain::inspect::ModelId;
use modeltap_core::tests::inspect::{run_inspect_model_contract, InspectCapability};
use modeltap_core::ToolId;
use modeltap_plugin_hf::HfPlugin;

/// Synthetic `config.json` body — carries every field the projector reads.
const SYNTHETIC_CONFIG_JSON: &str = r#"{
  "model_type": "mistral",
  "architectures": ["MistralForCausalLM"],
  "hidden_size": 4096,
  "num_attention_heads": 32,
  "num_hidden_layers": 32,
  "max_position_embeddings": 32768,
  "vocab_size": 32000
}"#;

/// Corrupt `config.json` body — invalid JSON. Plugin must surface
/// `Err(InspectError::FormatUnreadable)`.
const CORRUPT_CONFIG_JSON: &str = "{ not json";

const SNAPSHOT_REV: &str = "abc1234567890";
const CORRUPT_SNAPSHOT_REV: &str = "def4567890abc";

/// §3.13 Supported contract: invoke the cross-plugin harness against the HF
/// plugin after seeding a hub tree with one happy-path model and one
/// corrupt-config model.
///
/// Fixture layout (under `tempdir/`):
/// ```
///   models--mistralai--Mistral-7B-v0.1/refs/main
///   models--mistralai--Mistral-7B-v0.1/snapshots/abc.../config.json   ← happy
///   models--corrupt--Model/refs/main
///   models--corrupt--Model/snapshots/def.../config.json               ← corrupt
/// ```
///
/// The unknown-id branch uses `unknown/missing` — the model directory does
/// not exist; `locate_config_json_for_id` returns `Err(FileReadable)`.
#[tokio::test]
async fn hf_satisfies_inspect_model_contract() {
    let _env = ScopedEnv::set("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");

    let fixture = tempfile::tempdir().expect("tempdir for hf inspect_model contract fixture");
    let hub_root = fixture.path().to_path_buf();

    // Happy-path model tree.
    let happy_model = hub_root.join("models--mistralai--Mistral-7B-v0.1");
    let happy_snapshot = happy_model.join("snapshots").join(SNAPSHOT_REV);
    std::fs::create_dir_all(&happy_snapshot).expect("create happy snapshot dir");
    std::fs::write(happy_snapshot.join("config.json"), SYNTHETIC_CONFIG_JSON)
        .expect("write happy config.json");
    let happy_refs = happy_model.join("refs");
    std::fs::create_dir_all(&happy_refs).expect("create happy refs dir");
    std::fs::write(happy_refs.join("main"), SNAPSHOT_REV).expect("write happy refs/main");

    // Corrupt-config model tree.
    let corrupt_model = hub_root.join("models--corrupt--Model");
    let corrupt_snapshot = corrupt_model.join("snapshots").join(CORRUPT_SNAPSHOT_REV);
    std::fs::create_dir_all(&corrupt_snapshot).expect("create corrupt snapshot dir");
    std::fs::write(corrupt_snapshot.join("config.json"), CORRUPT_CONFIG_JSON)
        .expect("write corrupt config.json");
    let corrupt_refs = corrupt_model.join("refs");
    std::fs::create_dir_all(&corrupt_refs).expect("create corrupt refs dir");
    std::fs::write(corrupt_refs.join("main"), CORRUPT_SNAPSHOT_REV)
        .expect("write corrupt refs/main");

    let plugin = HfPlugin::new_with_hub_root(hub_root);
    run_inspect_model_contract(
        &plugin,
        ToolId("hf"),
        InspectCapability::Supported,
        ModelId::from("mistralai/Mistral-7B-v0.1"),
        ModelId::from("unknown/missing"),
        Some(ModelId::from("corrupt/Model")),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Test helpers — process-env scoping. The HF plugin reads
// `MODELTAP_CONFIG_PATH` so it can locate `~/.modeltap/config.toml` for the
// `[plugins.hf] search_paths` entries (irrelevant to inspect_model, but the
// plugin reads it regardless). We pin it to a nonexistent path so the
// suite is hermetic. Mirrors the inspect_tool_contract.rs helper.
// ---------------------------------------------------------------------------

use std::sync::Mutex;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

struct ScopedEnv {
    key: String,
    prior: Option<String>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl ScopedEnv {
    fn set(key: &str, value: &str) -> Self {
        let guard = ENV_MUTEX.lock().unwrap_or_else(|p| p.into_inner());
        let prior = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key: key.to_string(),
            prior,
            _guard: guard,
        }
    }
}

impl Drop for ScopedEnv {
    fn drop(&mut self) {
        match &self.prior {
            Some(v) => std::env::set_var(&self.key, v),
            None => std::env::remove_var(&self.key),
        }
    }
}
