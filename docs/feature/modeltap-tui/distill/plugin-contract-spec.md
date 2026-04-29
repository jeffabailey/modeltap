# Plugin Contract Test Specification — modeltap-tui

**Wave:** DISTILL (5 of 6)
**Author:** Quinn (nw-acceptance-designer)
**Purpose:** specify the contract test suite that EVERY plugin (`ollama`, `llama-cli`, `hf`, `lm-studio`, and any future 5th plugin) must pass. This is the public-API contract for plugin authors (Riley persona, US-18) and the wiring-correctness gate for maintainers.

## 1. Why a Contract Test Suite

The `Tool` trait (defined in `modeltap-core::domain::tool`) is the public API for plugin authors per ADR-001 / US-18. Per `architecture-design.md` § 9, there is no external network integration in v1, so the plugin contract test serves the role that Pact/CDC tests would serve in a microservices design: **it pins the trait's behavioral contract so that breaking changes are caught at PR time, not at runtime.**

Per `architecture-design.md` § 8.4: "Plugin contract tests in `modeltap-core/tests/plugin_contract.rs` parameterized over `T: Tool`. Each plugin crate runs the contract test against itself with fixture directories under `plugins/<name>/tests/fixtures/`."

This document specifies what that parameterized test must assert.

## 2. Test Surface

The contract test is parameterized over a generic plugin instance + a fixture directory:

```rust
// crates/modeltap-core/tests/plugin_contract/mod.rs
//
// Public re-export so each plugin crate can run the same suite.

pub async fn run_full_contract_suite<T: Tool>(plugin: T, fixture_root: &Path) {
    test_discover_returns_expected_models(&plugin, fixture_root).await;
    test_discover_is_idempotent(&plugin, fixture_root).await;
    test_link_produces_same_inode(&plugin, fixture_root).await;
    test_link_idempotent_on_already_unified(&plugin, fixture_root).await;
    test_delete_one_removes_only_target(&plugin, fixture_root).await;
    test_delete_all_removes_only_this_tools_files(&plugin, fixture_root).await;
    test_accepted_formats_is_non_empty_and_stable(&plugin, fixture_root).await;
    test_panic_in_any_method_caught_at_boundary(&plugin, fixture_root).await;
    test_discovery_after_mutation_reflects_change(&plugin, fixture_root).await;
}
```

Each plugin crate has a thin invocation:

```rust
// plugins/ollama/tests/contract.rs
use modeltap_core::tests::plugin_contract::run_full_contract_suite;
use modeltap_plugin_ollama::OllamaPlugin;

#[tokio::test]
async fn ollama_satisfies_tool_contract() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("standard-ollama");
    let plugin = OllamaPlugin::new_for_test(&fixture_root);
    run_full_contract_suite(plugin, &fixture_root).await;
}
```

Same shape for the other 3 plugins.

## 3. Required Tests (the contract)

### 3.1 `test_discover_returns_expected_models`

**Setup:** the fixture directory contains a known model layout (per-plugin fixture spec in §4).

**Assertion:**

- `plugin.discover(&ctx).await?` returns `Ok(Vec<DiscoveredModel>)`.
- The returned vec's length equals the fixture's known model count.
- Each entry has `id_in_tool`, `on_disk_path`, `size_bytes`, `format`, `display_label`, `status` populated.
- `id_in_tool` matches the fixture's expected ids.
- `size_bytes` for each entry equals the fixture file's actual size on disk.
- `status == ModelStatus::Healthy` for all healthy fixture files.
- For broken fixture entries (corrupt GGUF, broken symlink), `status` is `Corrupt {...}` or `BrokenSymlink {...}`, not `Healthy`.
- `dedup_key` is `DedupKey::Tentative(_)` (NOT `Content(_)`) — discovery does not compute SHA256 (lazy, per ADR-002).

### 3.2 `test_discover_is_idempotent`

**Setup:** same fixture as 3.1.

**Assertion:**

- Running `plugin.discover(&ctx).await?` twice in a row returns vectors with the same set of `(id_in_tool, on_disk_path, size_bytes)` triples.
- This proves discovery has no side-effects (does not write to the tool dir, does not modify fixtures).

### 3.3 `test_link_produces_same_inode`

**Setup:** fixture has two copies of the same content at known paths `A` (chosen as canonical) and `B` (target). Both same filesystem.

**Assertion:**

- Pre-link: `stat(A).st_ino != stat(B).st_ino` (independent files).
- Run `plugin.link(canonical_src=&A, model=&B_meta, ctx=&link_ctx).await?`.
- Returns `Ok(LinkOutcome { result: LinkResult::HardLinked { canonical, target, inode }, .. })`.
- Post-link: `stat(A).st_ino == stat(B).st_ino` (same inode).
- File contents at `B` match `A` (SHA256-equal — but we already knew this; this is defense in depth).
- The plugin's tool-specific registration (Ollama manifest, HF symlink, etc.) still resolves to `B`'s path — verified by re-running discover and checking the model is still listed at the original `id_in_tool`.

### 3.4 `test_link_idempotent_on_already_unified`

**Setup:** fixture with `A` and `B` already hardlinked (same inode).

**Assertion:**

- `plugin.link(canonical_src=&A, model=&B_meta, ctx=&link_ctx).await?` returns `Ok(LinkOutcome { result: LinkResult::Skipped { reason: "already unified" }, .. })`.
- Filesystem state unchanged.

### 3.5 `test_delete_one_removes_only_target`

**Setup:** fixture with 2 distinct models `M1` and `M2` registered with this tool. `M1` is also registered with another tool's fixture (cross-tool data set up by the contract test harness, not by this plugin).

**Assertion:**

- Run `plugin.delete_one(&M1, &ctx).await?`.
- Returns `Ok(DeleteOutcome { tool, model_id_in_tool: "<M1 id>", file_deleted: <bool>, registration_removed: true, bytes_freed: <N> })`.
- `M1`'s file at the plugin's path is removed (if `file_deleted`).
- `M2`'s file at the plugin's path is unchanged.
- `M1`'s file at the OTHER tool's path is unchanged (cross-tool isolation — `delete_one` MUST NOT cascade).
- The plugin's manifest/registry no longer references `M1` (verified by re-running discover; M1 absent, M2 present).

### 3.6 `test_delete_all_removes_only_this_tools_files`

**Setup:** fixture with this plugin's tool holding 3 models, AND another tool's fixture holding 2 models (one shared with this tool).

**Assertion:**

- Run `plugin.delete_all(&ctx).await?`.
- Returns `Ok(Vec<DeleteOutcome>)` with length 3.
- All 3 files at this plugin's tool dir are either deleted (unique) or have their registration removed (shared).
- The OTHER tool's fixture is completely unchanged. `delete_all` MUST NOT cascade across tools — this is the safety invariant.
- Re-running discover returns an empty vec (or `ToolStatus::NotInstalled` if the plugin removed its top-level dir).

### 3.7 `test_accepted_formats_is_non_empty_and_stable`

**Setup:** none.

**Assertion:**

- `plugin.accepted_formats()` returns a `&'static [Format]` of length >= 1.
- Calling `plugin.accepted_formats()` twice returns identical slices (same pointer; `&'static` guarantees this).
- The returned formats are valid `Format` enum variants (not `Format::Other` — that signals "unknown format on a model", not "this plugin accepts unknown formats").

### 3.8 `test_panic_in_any_method_caught_at_boundary`

**Setup:** a special "panic-on-call" wrapper plugin that delegates to the real plugin for setup but intentionally panics in one method per test invocation.

**Assertion:**

- Wrap `plugin.discover(&ctx)` in `tokio::task::spawn(...)`. If the wrapped call panics, the `JoinHandle` returns `JoinError` (not a process abort).
- The contract test harness verifies that calling the wrapper through `spawn` produces a `JoinError`, NOT a process abort.
- This proves the boundary catches plugin panics (US-18 AC-4).

This test is parameterized over each method (`discover`, `link`, `delete_one`, `delete_all`, `accepted_formats`).

### 3.9 `test_discovery_after_mutation_reflects_change`

**Setup:** fixture with a known model `M`.

**Assertion (stateless invariant per ADR-003 / Q7):**

- Run `plugin.discover(&ctx).await?` — observe `M` is present.
- Run `plugin.delete_one(&M, &ctx).await?`.
- Run `plugin.discover(&ctx).await?` again — observe `M` is absent.
- This proves the plugin does NOT cache discovery results across calls; it always re-reads the on-disk state. (The app may cache; the plugin must not.)

### 3.10 `test_link_cross_filesystem_returns_exdev`

**Setup:** canonical at `/tmp/canon` (tmpfs), target at `/var/tmp/target` (different mount; if same mount on the test system, skip with `cargo:rustc-cfg=skip_cross_fs_test` or similar).

**Assertion:**

- `plugin.link(canonical_src=&canon, model=&target_meta, ctx=&link_ctx).await` returns `Err(LinkError::CrossFilesystem { canonical, target })`.
- No partial mutation: target file unchanged.

This is the only contract test that requires real cross-fs; CI runs it on Linux only with `sudo mount -t tmpfs` setup. macOS CI skips it. Per `acceptance-test-plan.md` § 3 cross-fs fixture discussion.

## 4. Per-Plugin Fixture Specifications

Each plugin's `tests/fixtures/standard-<plugin>/` follows a per-plugin layout:

### 4.1 ollama fixture

```
plugins/ollama/tests/fixtures/standard-ollama/
└── home/
    └── .ollama/
        └── models/
            ├── manifests/
            │   └── registry.ollama.ai/
            │       └── library/
            │           ├── mistral/
            │           │   └── 7b-instruct-q4_K_M       # JSON pointing at blob
            │           └── llama3/
            │               └── 8b-instruct-q4_K_M       # JSON pointing at blob
            └── blobs/
                ├── sha256-<mistral-hash>                # 4.4 GB sparse
                ├── sha256-<llama3-hash>                 # 4.6 GB sparse
                └── sha256-<shared-blob>                 # referenced by 2 manifests
```

Expected discover() output: 3 models (because 3 manifest entries, even though 2 share a blob — each manifest is a distinct logical model in Ollama's view; size accounting deduplicates blobs at the inventory level, not the model level).

### 4.2 llama-cli fixture

```
plugins/llama-cli/tests/fixtures/standard-llama-cli/
└── home/
    ├── llms/
    │   ├── mistral-7b-q4.gguf                          # 4.4 GB sparse with valid GGUF header
    │   ├── llama-3-8b.gguf                             # 4.6 GB sparse
    │   └── corrupt.gguf                                # 100 bytes, invalid magic
    └── models/
        └── extra.gguf                                  # 3.0 GB sparse
```

Expected discover() output: 4 models, with `corrupt.gguf` having `status: Corrupt`.

### 4.3 hf fixture

```
plugins/hf/tests/fixtures/standard-hf/
└── home/
    └── .cache/
        └── huggingface/
            └── hub/
                ├── models--mistralai--Mistral-7B-v0.3/
                │   ├── snapshots/
                │   │   └── abc123/
                │   │       └── model.gguf -> ../../blobs/<sha>
                │   └── blobs/
                │       └── <sha>                       # 4.4 GB sparse
                ├── models--meta-llama--Llama-3-8B/
                │   ├── snapshots/...
                │   └── blobs/...
                └── models--TheBloke--something-AWQ/
                    ├── snapshots/
                    │   └── def456/
                    │       └── model.awq -> /nonexistent  # broken symlink
                    └── blobs/
```

Expected discover() output: 3 models, one with `status: BrokenSymlink`.

### 4.4 lm-studio fixture

```
plugins/lm-studio/tests/fixtures/standard-lm-studio/
└── home/
    └── .cache/
        └── lm-studio/
            └── models/
                ├── mistralai/
                │   └── Mistral-7B-v0.3/
                │       └── model.gguf                  # 4.4 GB sparse
                └── lmstudio-community/
                    └── llama-3-8b/
                        └── model.gguf                  # 4.6 GB sparse
```

Expected discover() output: 2 models.

## 5. Cross-Tool Test Harness

For tests 3.5 and 3.6, the contract harness needs a "second tool" without depending on a sibling plugin (per architecture rule R2: plugins do not depend on each other). Solution:

The contract harness in `modeltap-core::tests::plugin_contract` provides a minimal `MockOtherToolPlugin` that satisfies `Tool` and accepts setup like "this fixture file is ALSO registered with me". This plugin lives in `modeltap-core::tests::mocks` and is only available under `#[cfg(test)]`. It does NOT violate R2 because it lives in `modeltap-core`, not in `plugins/`.

```rust
// crates/modeltap-core/src/tests/mocks.rs (cfg(test) only)
pub struct MockOtherToolPlugin {
    fixture_root: PathBuf,
    expected_models: Vec<DiscoveredModel>,
}
```

## 6. CI Wiring

Per `ci-pipeline.md` § 2 the `test` job already runs `cargo test --workspace --locked --test plugin_contract`. This invokes EACH plugin crate's `tests/contract.rs`, which in turn calls `run_full_contract_suite()`.

Adding a new plugin = adding `plugins/<new>/tests/contract.rs` with the 6-line invocation shown in §2. CI picks it up automatically.

## 7. Failure Modes (what "the contract is broken" looks like)

| Failure | Likely cause |
|---|---|
| `test_discover_returns_expected_models` fails — count off by 1 | Plugin missed a manifest entry, or counted a directory as a model |
| `test_link_produces_same_inode` fails — different inodes | Plugin used `copy` instead of `hard_link`, or target path resolution is wrong |
| `test_delete_one_removes_only_target` fails — M2 also gone | Plugin's "delete one" actually deletes the parent directory or matches by prefix |
| `test_delete_all_removes_only_this_tools_files` fails — other tool's files affected | Plugin walked outside its own dir during delete (cross-tool contamination — a SAFETY bug per K5) |
| `test_panic_in_any_method_caught_at_boundary` fails — process aborts | The orchestrator did not wrap the call in `tokio::task::spawn` — US-18 AC-4 violated |
| `test_discovery_after_mutation_reflects_change` fails — M still appears | Plugin caches discovery results across calls (violates ADR-003 stateless invariant) |
| `test_link_cross_filesystem_returns_exdev` fails — silent copy | Plugin caught EXDEV and silently fell back to copy — should propagate to caller per US-19 |

## 8. Notes for Plugin Authors (Riley Persona)

If you are adding a 5th plugin (e.g., Atomic Chat):

1. Create `plugins/atomic-chat/Cargo.toml` and `src/lib.rs`.
2. Implement the `Tool` trait. Pay close attention to:
   - `discover()` must NOT have side-effects (3.2 invariant).
   - `link()` must produce same-inode hardlinks for same-fs targets, AND propagate `EXDEV` for cross-fs (3.3, 3.10).
   - `delete_one()` and `delete_all()` MUST NOT touch any path outside this plugin's tool dir (3.5, 3.6 — safety invariant).
   - `accepted_formats()` must return a non-empty `&'static [Format]` (3.7).
   - You do not need to catch panics yourself; the orchestrator does (3.8). But avoid panicking when an `Err(...)` would do.
   - Do NOT cache discovery results across calls (3.9 — ADR-003 invariant).
3. Build a fixture directory at `plugins/atomic-chat/tests/fixtures/standard-atomic-chat/` matching your tool's on-disk layout.
4. Add `plugins/atomic-chat/tests/contract.rs` with the 6-line invocation shown in §2.
5. Run `cargo test --package modeltap-plugin-atomic-chat --test contract` locally — it must pass.
6. Open a PR. CI runs the contract test. If green, merge.

The contract test is the contract. If it passes, your plugin works in modeltap.
