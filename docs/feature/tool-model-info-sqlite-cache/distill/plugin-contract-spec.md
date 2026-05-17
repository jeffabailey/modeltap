# Plugin Contract Spec — tool-model-info-sqlite-cache

**Wave:** DISTILL (5 of 6) — brownfield extension of `modeltap-tui`
**Author:** Quinn (nw-acceptance-designer)
**Purpose:** specify the contract test suite that EVERY plugin must satisfy for the new `Tool::inspect_tool` and `Tool::inspect_model` capabilities introduced by ADR-016.

This document **extends** the parent's `plugin-contract-spec.md` and the sibling's `folder-group-bulk-delete/distill/plugin-contract-spec.md`. The parent's 10 contract tests (3.1–3.10) cover `discover`, `link`, `delete_one`, `delete_all`, `accepted_formats`, panic catching, idempotence, and cross-fs. The sibling's 3.11 covers `delete_folder`. This document specifies the NEW contract tests **3.12** (`inspect_tool`) and **3.13** (`inspect_model`).

---

## 1. Why a `inspect_*` contract test

Per ADR-016, the `Tool` trait grows two methods (positions #8 and #9 after `delete_folder`), each with a default body returning `Err(InspectError::Unsupported { tool: self.name() })`. Ollama, HF, LM Studio override; llama-cli overrides `inspect_model` only; atomic-chat and gpt4all inherit defaults for both. Three failure modes the contract test guards against:

1. **A new plugin author** (Riley persona, US-18) implements `Tool` and forgets to override `inspect_*` even though their tool has introspectable artifacts. Silent fall-through to `Unsupported` looks correct in unit tests but presents the user with permanent "(not detectable)" / "(introspection failed)" in the TUI.
2. **An overriding plugin's implementation drifts from the contract** — for example, returns a `ModelDetail` with an empty `metadata_kv` for a file that has valid metadata, OR panics on a corrupt file instead of returning `Err(InspectError::FormatUnreadable)`.
3. **A panic in `inspect_*` reaches the orchestrator** instead of being caught at the spawn boundary (regresses parent US-18 panic-isolation invariant; new INT-INFO-8).

The contract tests are the third pillar of the trait extension story (the other two being the default body and the architecture lint that already covers `delete_folder` semantics).

---

## 2. Test Surface

Same parameterization pattern as the parent and sibling contract suites. Two new free functions added to the parent's `plugin_contract` module:

```rust
// crates/modeltap-core/tests/plugin_contract/mod.rs
//
// Added alongside the existing run_full_contract_suite and run_folder_delete_contract.

pub async fn run_inspect_tool_contract<T: Tool>(
    plugin: T,
    fixture_root: &Path,
    capability: InspectCapability,
) {
    match capability {
        InspectCapability::Unsupported => {
            test_inspect_tool_returns_unsupported(&plugin, fixture_root).await;
        }
        InspectCapability::Supported => {
            test_inspect_tool_happy_path(&plugin, fixture_root).await;
            test_inspect_tool_deterministic(&plugin, fixture_root).await;
            test_inspect_tool_panic_isolation(&plugin, fixture_root).await;
        }
    }
}

pub async fn run_inspect_model_contract<T: Tool>(
    plugin: T,
    fixture_root: &Path,
    capability: InspectCapability,
) {
    match capability {
        InspectCapability::Unsupported => {
            test_inspect_model_returns_unsupported(&plugin, fixture_root).await;
        }
        InspectCapability::Supported => {
            test_inspect_model_happy_path(&plugin, fixture_root).await;
            test_inspect_model_unknown_id_returns_not_found(&plugin, fixture_root).await;
            test_inspect_model_corrupt_returns_format_unreadable(&plugin, fixture_root).await;
            test_inspect_model_deterministic(&plugin, fixture_root).await;
            test_inspect_model_field_schema(&plugin, fixture_root).await;
            test_inspect_model_panic_isolation(&plugin, fixture_root).await;
        }
    }
}

pub enum InspectCapability {
    /// The plugin's `inspect_*` is expected to return
    /// `Err(InspectError::Unsupported)` for any input. Used by
    /// atomic-chat and gpt4all (both methods), and by llama-cli
    /// for `inspect_tool` only.
    Unsupported,
    /// The plugin overrides `inspect_*` and must honor the full
    /// behavioral contract below. Used by Ollama, HF, LM Studio
    /// (both methods), and by llama-cli for `inspect_model` only.
    Supported,
}
```

### Plugin invocations

Each plugin crate adds two test files (one per method):

```rust
// plugins/ollama/tests/inspect_tool_contract.rs
use modeltap_core::tests::plugin_contract::{run_inspect_tool_contract, InspectCapability};
use modeltap_plugin_ollama::OllamaPlugin;

#[tokio::test]
async fn ollama_satisfies_inspect_tool_contract() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests").join("fixtures").join("standard-ollama");
    // The plugin's inspect_tool calls http://localhost:11434/api/version by
    // default; test sets MODELTAP_OLLAMA_VERSION=0.6.4 to short-circuit (D12).
    std::env::set_var("MODELTAP_OLLAMA_VERSION", "0.6.4");
    let plugin = OllamaPlugin::new_for_test(&fixture_root);
    run_inspect_tool_contract(plugin, &fixture_root, InspectCapability::Supported).await;
}
```

```rust
// plugins/atomic-chat/tests/inspect_tool_contract.rs
#[tokio::test]
async fn atomic_chat_returns_unsupported_for_inspect_tool() {
    let fixture_root = ...;
    let plugin = AtomicChatPlugin::new_for_test(&fixture_root);
    run_inspect_tool_contract(plugin, &fixture_root, InspectCapability::Unsupported).await;
}
```

Capability matrix per plugin (mirrors the matrix in ADR-016 §"Plugin overrides"):

| Plugin | inspect_tool | inspect_model |
|---|:---:|:---:|
| ollama | Supported (HTTP `/api/version`) | Supported (manifest JSON parse) |
| hf | Supported (HF CLI version detection) | Supported (config.json parse) |
| lm-studio | Unsupported (best-effort or none) | Supported (GGUF header / model.json parse) |
| llama-cli | Unsupported (static binary, no canonical source) | Supported (GGUF header parse) |
| atomic-chat | Unsupported | Unsupported |
| gpt4all | Unsupported | Unsupported |

---

## 3. Required Tests — `InspectCapability::Unsupported`

### 3.12.U.1 `test_inspect_tool_returns_unsupported`

**Setup:** any plugin instance pointed at any fixture root. The fixture's contents are irrelevant because the default body short-circuits before touching anything.

**Assertion:**
- `plugin.inspect_tool().await` returns `Err(InspectError::Unsupported { tool })` where `tool == plugin.name()`.
- No filesystem read occurred (verified by snapshot of fixture mtime pre/post).
- No HTTP request was made (Ollama plugin: assert no inbound connection on the stub HTTP server during the call; for non-Ollama, trivially satisfied).
- The error message contains the plugin's `name()` value.

### 3.13.U.1 `test_inspect_model_returns_unsupported`

**Setup:** plugin instance + a `ModelId` known to the fixture.

**Assertion:**
- `plugin.inspect_model(&id).await` returns `Err(InspectError::Unsupported { tool })`.
- No filesystem read occurred.
- The error message contains the plugin's `name()`.

That is the entirety of the `Unsupported` path. Two assertions, two tests. The default body's behavior is the contract.

---

## 4. Required Tests — `InspectCapability::Supported` for `inspect_tool`

These tests run against Ollama (and HF) in v1.

### 3.12.S.1 `test_inspect_tool_happy_path`

**Setup:** fixture with the plugin's expected tool-home layout (`standard-ollama` for Ollama, `standard-hf` for HF). For Ollama, set `MODELTAP_OLLAMA_VERSION=0.6.4` (the env-var short-circuit per D12) OR run a stub HTTP server at `MODELTAP_OLLAMA_API_URL=http://127.0.0.1:<port>` returning `{"version": "0.6.4"}`.

**Assertion:**
- `plugin.inspect_tool().await?` returns `Ok(ToolDetail { ... })`.
- `tool_detail.tool_id == plugin.name()`.
- `tool_detail.install_path` is the absolute path of the plugin's discovery root within the fixture.
- `tool_detail.plugin_version` is a non-empty `String` (the plugin's own crate version, e.g., `"modeltap-plugin-ollama 0.2.6"`).
- For Ollama: `tool_detail.detected_version == Some("0.6.4")` (matches the env-var short-circuit or stub response).
- `tool_detail.search_paths` is non-empty and contains at least one entry with `source == SearchPathSource::Default`.
- `tool_detail.model_count >= 0` (could be zero for empty fixture).
- `tool_detail.disk_usage_bytes >= 0`.

### 3.12.S.2 `test_inspect_tool_deterministic`

**Setup:** same as 3.12.S.1.

**Assertion:**
- Call `inspect_tool()` twice in succession (within 100 ms apart).
- Both calls return `Ok`.
- The two `ToolDetail` values are equal in every field EXCEPT `last_scan_at` and `last_scan_duration_ms` (which may differ by milliseconds).

This catches plugins that read mutable global state or rely on non-deterministic ordering.

### 3.12.S.3 `test_inspect_tool_panic_isolation`

**Setup:** the contract harness wraps the plugin's `inspect_tool` call site with a `tokio::spawn` boundary mimicking the orchestrator's `execute_inspect_*`. The fixture is configured to cause the plugin's implementation to panic — for Ollama, set `MODELTAP_OLLAMA_API_URL=http://invalid-host-that-will-panic-the-parser:99999` AND inject a malformed response via a stub server that returns binary garbage instead of JSON. For HF, point the search-path at a directory containing a corrupt file the implementation does not handle.

**Assertion:**
- The wrapping `tokio::spawn`'s `JoinHandle` returns either `Ok(Err(InspectError::FormatUnreadable))` (graceful) OR `Err(JoinError::Panic)` (panic caught at boundary).
- In the panic case, the harness converts the panic into `InspectError::PluginPanic { tool: plugin.name(), message: <stringified> }` per ADR-016 §"New error variant".
- The fixture filesystem is unchanged.
- This test PASSES if either the graceful or the caught-panic path is taken. It FAILS if the panic propagates above the spawn boundary.

This is the INT-INFO-8 invariant proved at the contract level.

---

## 5. Required Tests — `InspectCapability::Supported` for `inspect_model`

These tests run against Ollama, HF, LM Studio, and llama-cli in v1.

### 3.13.S.1 `test_inspect_model_happy_path`

**Setup:** fixture with a known model file. For each plugin, the contract test uses:

| Plugin | Fixture model |
|---|---|
| ollama | `~/.ollama/models/manifests/registry.ollama.ai/library/llama3/8b-instruct-q4_K_M` (real manifest JSON) |
| hf | `models--meta-llama--Llama-3-8B/snapshots/abc/config.json` with `model_type=llama` |
| lm-studio | `Llama-3-8B-Q4_K_M.gguf` (real GGUF v3 minimal header) |
| llama-cli | Same GGUF as lm-studio |

**Assertion:**
- `plugin.inspect_model(&known_model_id).await?` returns `Ok(ModelDetail { ... })`.
- `model_detail.model_id == known_model_id`.
- `model_detail.metadata_kv` is non-empty.
- For GGUF-parsing plugins (lm-studio, llama-cli): `metadata_kv` contains at least the key `general.architecture` AND `metadata_kv["general.architecture"] == "llama"` for the Llama-3 fixture.
- For Ollama: `metadata_kv` contains at least one of `config.architecture`, `parameters`, `template`.
- For HF: `metadata_kv` contains at least `model_type` AND `model_type == "llama"`.
- `model_detail.format` is `Some(_)` and non-empty.
- `model_detail.introspected_at` is `Some(<recent SystemTime>)`.

### 3.13.S.2 `test_inspect_model_unknown_id_returns_not_found`

**Setup:** plugin instance + a `ModelId` constructed to point at a file that does NOT exist in the fixture.

**Assertion:**
- `plugin.inspect_model(&unknown_id).await` returns `Err(InspectError::FileReadable { path, source })` where `source.kind() == ErrorKind::NotFound` OR `Err(InspectError::FormatUnreadable { path, detail })` where `detail.contains("not found")` (DELIVER picks ONE; both are acceptable in the contract).
- Does NOT return `Ok` with empty fields.
- Does NOT panic.

This catches the "future bug" where a plugin returns `Ok(ModelDetail { format: None, metadata_kv: BTreeMap::new(), .. })` for a missing file.

### 3.13.S.3 `test_inspect_model_corrupt_returns_format_unreadable`

**Setup:** fixture has a file at the expected model path BUT the file content is corrupt:

| Plugin | Corrupt fixture |
|---|---|
| ollama | manifest JSON file containing `not valid JSON {{{` |
| hf | `config.json` containing the same |
| lm-studio | GGUF file: 100 bytes of magic-followed-by-garbage |
| llama-cli | Same as lm-studio |

**Assertion:**
- `plugin.inspect_model(&model_id).await` returns `Err(InspectError::FormatUnreadable { path, detail })`.
- `detail` is a non-empty string explaining what failed (e.g., "JSON parse error at byte 4" or "GGUF header truncated").
- The plugin's implementation does NOT panic.

This is the AC-22-7 invariant proved at the contract level.

### 3.13.S.4 `test_inspect_model_deterministic`

**Setup:** same as 3.13.S.1.

**Assertion:**
- Call `inspect_model(&id)` twice in succession.
- Both return `Ok` with equal `metadata_kv` and `format`.
- `introspected_at` may differ by milliseconds.

### 3.13.S.5 `test_inspect_model_field_schema`

**Setup:** same as 3.13.S.1.

**Assertion (per `data-models.md`):**
- `model_detail.model_id` is non-empty.
- `model_detail.metadata_kv` is a `BTreeMap<String, String>` (compile-time-checked by Rust types; runtime assertion: all keys and values are valid UTF-8).
- `model_detail.metadata_kv` is non-empty (for a supported file format).
- `model_detail.format` is `Some(_)`.
- `model_detail.introspected_at` is `Some(_)`.
- `model_detail.parameters_billions` is `None` OR a positive finite f64.
- `model_detail.context_length` is `None` OR a positive u32.

This catches schema-shape drift (e.g., a plugin returning `Some(0.0)` for parameters when it should be `None`).

### 3.13.S.6 `test_inspect_model_panic_isolation`

Same shape as 3.12.S.3 but for `inspect_model`. The harness injects a fixture that causes the implementation to panic (e.g., for GGUF, a file with magic bytes claiming KV count = `u64::MAX`).

**Assertion:** panic is caught at the orchestrator's `tokio::spawn` boundary; surfaced as `InspectError::PluginPanic` OR `InspectError::FormatUnreadable`. Either is acceptable; what is NOT acceptable is panic propagation above the spawn boundary.

---

## 6. Per-Plugin Fixture Specifications

### 6.1 Ollama plugin fixture — `standard-ollama-inspect`

```
plugins/ollama/tests/fixtures/standard-ollama-inspect/
└── home/
    └── .ollama/
        └── models/
            ├── manifests/
            │   └── registry.ollama.ai/
            │       └── library/
            │           └── llama3/
            │               └── 8b-instruct-q4_K_M       # real Ollama manifest JSON
            └── blobs/
                └── sha256-<hash>                          # sparse 4.9 GB
```

Manifest content (for 3.13.S.1):
```json
{
  "schemaVersion": 2,
  "config": {
    "architecture": "llama",
    "format": "gguf"
  },
  "parameters": "8.0B",
  "template": "{{ if .System }}<|start_header_id|>system<|end_header_id|>..."
}
```

### 6.2 HF plugin fixture — `standard-hf-inspect`

```
plugins/hf/tests/fixtures/standard-hf-inspect/
└── home/
    └── .cache/
        └── huggingface/
            └── hub/
                └── models--meta-llama--Llama-3-8B/
                    ├── snapshots/
                    │   └── abc123/
                    │       ├── config.json              # real HF config
                    │       └── model.safetensors        # sparse 16 GB
                    └── blobs/
                        └── <sha-of-safetensors>
```

`config.json` content:
```json
{
  "model_type": "llama",
  "architectures": ["LlamaForCausalLM"],
  "hidden_size": 4096,
  "num_attention_heads": 32,
  "num_hidden_layers": 32,
  "max_position_embeddings": 8192
}
```

### 6.3 LM Studio plugin fixture — `standard-lm-studio-inspect`

```
plugins/lm-studio/tests/fixtures/standard-lm-studio-inspect/
└── home/
    └── .cache/
        └── lm-studio/
            └── models/
                └── lmstudio-community/
                    └── Llama-3-8B/
                        └── Llama-3-8B-Q4_K_M.gguf       # real GGUF v3 minimal header
```

### 6.4 llama-cli plugin fixture — `standard-llama-cli-inspect`

```
plugins/llama-cli/tests/fixtures/standard-llama-cli-inspect/
└── home/
    └── llms/
        └── Llama-3-8B-Q4_K_M.gguf                       # same as 6.3
```

### 6.5 Non-inspect-capable plugin fixtures

Atomic-chat and gpt4all reuse their parent contract fixtures (`standard-atomic-chat`, `standard-gpt4all`). The `Unsupported` test does not need any inspect-specific fixture content — it short-circuits on the default body.

### 6.6 Corrupt-file fixtures (per 3.13.S.3)

Each `Supported` plugin's fixture directory contains a sibling subdirectory `corrupt/` with one corrupt-equivalent file:

| Plugin | Corrupt file path |
|---|---|
| ollama | `corrupt/manifests/library/test/corrupt-tag` |
| hf | `corrupt/models--test--Corrupt/snapshots/abc/config.json` |
| lm-studio | `corrupt/lmstudio-community/Test/corrupt.gguf` |
| llama-cli | `corrupt/corrupt.gguf` |

Each file contains the format-class-specific corruption described in §5.3.

---

## 7. Cross-Tool Test Harness (extension)

The inspect contract tests do NOT require cross-tool fixture coordination (unlike the sibling's `delete_folder` test 3.11.S.6). Each plugin's contract test is self-contained.

The orchestrator-level panic-isolation harness (3.12.S.3, 3.13.S.6) wraps `tokio::spawn` in the same way `modeltap-app::orchestration::execute_inspect_*` does in production. DELIVER implements this wrapping once in `crates/modeltap-core/src/tests/inspect_panic_harness.rs` (cfg(test) only); each plugin's contract test reuses it.

---

## 8. CI Wiring

The new contract tests live in:

- `plugins/ollama/tests/inspect_tool_contract.rs` — `Supported`.
- `plugins/ollama/tests/inspect_model_contract.rs` — `Supported`.
- `plugins/hf/tests/inspect_tool_contract.rs` — `Supported`.
- `plugins/hf/tests/inspect_model_contract.rs` — `Supported`.
- `plugins/lm-studio/tests/inspect_tool_contract.rs` — `Unsupported`.
- `plugins/lm-studio/tests/inspect_model_contract.rs` — `Supported`.
- `plugins/llama-cli/tests/inspect_tool_contract.rs` — `Unsupported`.
- `plugins/llama-cli/tests/inspect_model_contract.rs` — `Supported`.
- `plugins/atomic-chat/tests/inspect_tool_contract.rs` — `Unsupported`.
- `plugins/atomic-chat/tests/inspect_model_contract.rs` — `Unsupported`.
- `plugins/gpt4all/tests/inspect_tool_contract.rs` — `Unsupported`.
- `plugins/gpt4all/tests/inspect_model_contract.rs` — `Unsupported`.

The existing CI invocation `cargo test --workspace --locked` picks them up automatically.

Adding a future 7th plugin follows the parent's pattern: the new plugin gets its own pair of `inspect_*_contract.rs` files declaring `Unsupported` (or `Supported` if it has introspectable artifacts — at which point it adds the override AND switches the capability flag for that method).

---

## 9. Failure Modes (what "the inspect contract is broken" looks like)

| Failure | Likely cause |
|---|---|
| 3.12.U.1 / 3.13.U.1 fails — non-Supported plugin returns Ok | The plugin accidentally implemented `inspect_*` instead of inheriting the default. |
| 3.12.S.1 fails — `detected_version` is `Some("")` or wrong | The HTTP parser returns empty on parse error instead of `None`; or the env-var short-circuit is misconfigured. |
| 3.12.S.2 / 3.13.S.4 fails — two calls differ | The plugin reads mutable global state; or HTTP client returns different responses (re-stub the response). |
| 3.12.S.3 / 3.13.S.6 fails — panic propagates | The plugin's implementation panics outside the spawn boundary; the orchestrator's `execute_inspect_*` is missing the panic-catch wrapping. |
| 3.13.S.1 fails — `metadata_kv` is empty | The plugin's KV-selection is broken (e.g., reads the wrong fields). |
| 3.13.S.2 fails — unknown id returns Ok with empty fields | The plugin returns a default `ModelDetail` for missing files; should return `Err(NotFound)` or `FormatUnreadable`. |
| 3.13.S.3 fails — corrupt file panics | The plugin's parser does not handle malformed input; convert panics to `FormatUnreadable`. |
| 3.13.S.5 fails — `parameters_billions == Some(0.0)` for a valid file | The plugin emits sentinel "zero" values instead of `None`. |

---

## 10. Relationship to Acceptance Tests

The contract tests (Layer B) and the E2E acceptance scenarios (Layer A) cover overlapping invariants. The distinction:

- **Layer A** (acceptance scenarios in `features/tool-detail.feature` + `features/model-detail.feature` + `features/integration-checkpoints.feature`) drives the real `modeltap` binary through user-observable flows. Asserts on TUI frames, JSONL logs, exit codes. Slower (~1 s per scenario); validates the full stack.
- **Layer B** (contract tests in this spec) drives the plugin trait method directly with synthetic plans. Asserts on returned `ToolDetail` / `ModelDetail` and on error variants. Faster (~100 ms per test); validates the plugin in isolation.

For US-21's "(not detectable)" UAT scenario, the E2E version asserts user-observable behavior (`Then the Version field reads "(not detectable)"`). The contract test 3.12.S.1 asserts the trait-level behavior (`detected_version: None`). Both are required: the E2E proves the TUI renders "(not detectable)" when the trait returns `None`; the contract test proves the trait actually returns `None` for the failure case.

For US-22's "(introspection failed)" UAT scenario, the E2E version asserts the screen shows "(introspection failed — see diagnostics.log)". The contract test 3.13.S.3 asserts the trait returns `Err(InspectError::FormatUnreadable)`. Both layers required.

For INT-INFO-8 (panic isolation), the contract test 3.12.S.3 / 3.13.S.6 catches panics at the trait boundary. The E2E scenario in `integration-checkpoints.feature` asserts the TUI does NOT crash when a plugin panics. Defense in depth.
