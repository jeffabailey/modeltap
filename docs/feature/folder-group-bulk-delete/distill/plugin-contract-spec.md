# Plugin Contract Spec — folder-group-bulk-delete

**Wave:** DISTILL (5 of 6) — brownfield extension of `modeltap-tui`
**Author:** Quinn (nw-acceptance-designer)
**Purpose:** specify the contract test suite that EVERY plugin must satisfy for the new `Tool::delete_folder` capability introduced by ADR-010.

This document **extends** the parent's `plugin-contract-spec.md`. The parent's 10 contract tests (3.1–3.10) cover `discover`, `link`, `delete_one`, `delete_all`, `accepted_formats`, panic catching, idempotence, and cross-fs. This document specifies the NEW contract test 3.11 for `delete_folder`.

---

## 1. Why a `delete_folder` contract test

Per ADR-010, the `Tool` trait grows a seventh method with a default body returning `Err(DeleteError::Unsupported)`. The HF plugin overrides; the other three plugins inherit the default. Two failure modes the contract test guards against:

1. **A new plugin author** (Riley persona, US-18) implements `Tool` but forgets to override `delete_folder` even though their tool has folder semantics. Silent fall-through to `Unsupported` looks correct in unit tests but breaks the user contract.
2. **The HF plugin's override** drifts from the contract — for example, fails to preserve cross-tool hardlinks, or rolls back partial successes. Acceptance tests catch this for one specific fixture; the contract test catches it for the entire space of contract-compliant inputs.

The contract test is the third pillar of the trait extension story (the first two being the default body and the architecture lint).

---

## 2. Test Surface

Same parameterization pattern as the parent contract suite. One new free function added to the parent's `plugin_contract` module:

```rust
// crates/modeltap-core/tests/plugin_contract/mod.rs
//
// Added alongside the existing run_full_contract_suite.

pub async fn run_folder_delete_contract<T: Tool>(
    plugin: T,
    fixture_root: &Path,
    capability: FolderDeleteCapability,
) {
    match capability {
        FolderDeleteCapability::Unsupported => {
            test_delete_folder_returns_unsupported(&plugin, fixture_root).await;
        }
        FolderDeleteCapability::Supported => {
            test_delete_folder_all_unique(&plugin, fixture_root).await;
            test_delete_folder_mixed_shared_and_unique(&plugin, fixture_root).await;
            test_delete_folder_with_sidecars(&plugin, fixture_root).await;
            test_delete_folder_partial_failure(&plugin, fixture_root).await;
            test_delete_folder_idempotent_retry(&plugin, fixture_root).await;
            test_delete_folder_preserves_cross_tool_hardlinks(&plugin, fixture_root).await;
            test_delete_folder_removes_empty_tree(&plugin, fixture_root).await;
            test_delete_folder_only_sidecars(&plugin, fixture_root).await;
        }
    }
}

pub enum FolderDeleteCapability {
    /// The plugin's `delete_folder` is expected to return
    /// `Err(DeleteError::Unsupported)` for any input. Used by Ollama,
    /// llama-cli, lm-studio in v1.
    Unsupported,
    /// The plugin overrides `delete_folder` and must honor the full
    /// behavioral contract below. Used by HF in v1.
    Supported,
}
```

### Plugin invocations

Each plugin crate adds one (or extends one) test file:

```rust
// plugins/hf/tests/folder_delete_contract.rs
use modeltap_core::tests::plugin_contract::{
    run_folder_delete_contract, FolderDeleteCapability,
};
use modeltap_plugin_hf::HfPlugin;

#[tokio::test]
async fn hf_satisfies_folder_delete_contract() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("standard-hf-folder");
    let plugin = HfPlugin::new_for_test(&fixture_root);
    run_folder_delete_contract(plugin, &fixture_root, FolderDeleteCapability::Supported).await;
}
```

```rust
// plugins/ollama/tests/folder_delete_contract.rs
use modeltap_core::tests::plugin_contract::{
    run_folder_delete_contract, FolderDeleteCapability,
};
use modeltap_plugin_ollama::OllamaPlugin;

#[tokio::test]
async fn ollama_returns_unsupported_for_delete_folder() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("standard-ollama");
    let plugin = OllamaPlugin::new_for_test(&fixture_root);
    run_folder_delete_contract(plugin, &fixture_root, FolderDeleteCapability::Unsupported).await;
}
```

Identical shape for `plugins/llama-cli/tests/folder_delete_contract.rs` and `plugins/lm-studio/tests/folder_delete_contract.rs`.

---

## 3. Required Tests — `FolderDeleteCapability::Unsupported`

### 3.11.U.1 `test_delete_folder_returns_unsupported`

**Setup:** any minimal `FolderDeletePlan` — DELIVER constructs one with a single non-existent path. The plan's contents are irrelevant because the default body must short-circuit before touching the filesystem.

**Assertion:**

- `plugin.delete_folder(&plan).await` returns `Err(DeleteError::Unsupported { tool })` where `tool == plugin.name()`.
- The fixture's filesystem state is unchanged (manifest equality).
- The error message contains the plugin's name (i.e., the `tool` field, which is what the UI surfaces).

That is the entirety of the `Unsupported` path. One assertion, one test. The default body's behavior is the contract; anything more is gold-plating.

---

## 4. Required Tests — `FolderDeleteCapability::Supported`

These tests run against the HF plugin's override in v1. The fixture root for each is a freshly-built tempdir tree mimicking `<HF_HOME>/hub/models--<author>--<repo>/`.

### 3.11.S.1 `test_delete_folder_all_unique`

**Setup:** fixture with one HF repo containing 2 unique model files (sparse, 1 GB each) and 0 sidecars. No other tool has any of these files.

**Assertion:**

- `plugin.delete_folder(&plan).await?` returns `Ok(Vec<DeleteOutcome>)` of length 2.
- Each entry has `registration_removed: true`, `file_deleted: true`, `bytes_freed: <file_size>`.
- Both files no longer exist on disk.
- The `models--<author>--<repo>/` directory tree is fully removed.
- The sum of `bytes_freed` equals the plan's `bytes_to_reclaim`.

### 3.11.S.2 `test_delete_folder_mixed_shared_and_unique`

**Setup:** fixture with one HF repo containing 3 model files. File 1 (1 GB) is unique. Files 2 and 3 (1 GB each) are hardlinked into a sibling Ollama fixture tree (set up by the contract harness via the parent's `MockOtherToolPlugin` pattern). 0 sidecars.

**Assertion:**

- `plugin.delete_folder(&plan).await?` returns `Ok(Vec<DeleteOutcome>)` of length 3.
- File 1's entry: `registration_removed: true`, `file_deleted: true`, `bytes_freed: 1_073_741_824`.
- File 2's entry: `registration_removed: true`, `file_deleted: false`, `bytes_freed: 0`.
- File 3's entry: `registration_removed: true`, `file_deleted: false`, `bytes_freed: 0`.
- Post-delete: the HF-side paths for all 3 files no longer exist.
- Post-delete: the Ollama-side paths for files 2 and 3 still exist and stat to the same inode they had pre-delete.
- The `models--<author>--<repo>/` directory tree is fully removed (all HF-side references are gone; the blobs are no longer needed since the snapshot symlinks are gone, AND the Ollama inodes are independent).

### 3.11.S.3 `test_delete_folder_with_sidecars`

**Setup:** fixture with one HF repo containing 2 unique model files + 3 sidecars (README.md, .imatrix, .gguf.urls).

**Assertion:**

- `plugin.delete_folder(&plan).await?` returns `Ok(Vec<DeleteOutcome>)` of length 5.
- 2 entries (model files): `registration_removed: true`, `file_deleted: true`, `bytes_freed: <size>`.
- 3 entries (sidecars): `registration_removed: true`, `file_deleted: true`, `bytes_freed: <size>`. The `model_id_in_tool` field carries the sidecar's filename for diagnostic logging.
- All 5 files no longer exist on disk.
- The `models--<author>--<repo>/` directory tree is fully removed.
- The `bytes_freed` sum equals the plan's `bytes_to_reclaim`.

### 3.11.S.4 `test_delete_folder_partial_failure`

**Setup:** fixture with one HF repo containing 5 unique model files. The harness configures one file (let's call it `file3.gguf`) to fail unlink — either via `MODELTAP_TEST_EBUSY_PATHS=<path>` or by placing `file3.gguf` in a directory with mode 0555. **Both mechanisms must be tested** (DELIVER picks one as the canonical for this test; the other appears in the E2E `@infrastructure-failure` scenario).

**Assertion:**

- `plugin.delete_folder(&plan).await?` returns `Ok(Vec<DeleteOutcome>)` of length 5.
- 4 entries (the unaffected files): `registration_removed: true`, `file_deleted: true`.
- 1 entry (`file3.gguf`): `registration_removed: false`, `file_deleted: false`, `bytes_freed: 0`.
- Post-delete: 4 files are gone; `file3.gguf` remains on disk.
- The `models--<author>--<repo>/` directory tree is NOT fully removed (the subdirectory containing `file3.gguf` remains).
- The `models--<author>--<repo>/` root directory itself remains (it contains the orphan subtree).
- The function does NOT panic and does NOT abort early — every file is attempted.

### 3.11.S.5 `test_delete_folder_idempotent_retry`

**Setup:** the post-state of test 3.11.S.4 — 1 file remains on disk in `models--<author>--<repo>/`. The harness now removes the EBUSY simulation (or chmod's the directory back to 0755).

**Assertion:**

- Building a new `FolderDeletePlan` against the current state of the folder yields a plan with 1 file.
- `plugin.delete_folder(&plan).await?` returns `Ok(Vec<DeleteOutcome>)` of length 1.
- That entry: `registration_removed: true`, `file_deleted: true`, `bytes_freed: <size>`.
- Post-delete: the file is gone AND the `models--<author>--<repo>/` directory tree is fully removed.
- Calling `plugin.delete_folder(&plan).await` AGAIN on the now-empty folder is allowed to either (a) return `Ok(Vec::new())` (the folder is already gone) or (b) return `Err(DeleteError::NotFound(...))` (no such folder). DELIVER picks one; the contract permits both as long as no panic and no destructive side effect occurs.

### 3.11.S.6 `test_delete_folder_preserves_cross_tool_hardlinks`

**Setup:** same as 3.11.S.2 but the assertion focuses on the hardlink survival post-condition explicitly.

**Assertion:**

- Pre-delete: `stat(hf_path_for_file2).st_ino == stat(ollama_path_for_file2).st_ino` (same inode).
- After `plugin.delete_folder(&plan).await?`:
  - `hf_path_for_file2` does NOT exist.
  - `ollama_path_for_file2` DOES exist.
  - `stat(ollama_path_for_file2).st_ino` equals the inode recorded pre-delete.
  - The file's SHA256 matches the pre-delete SHA256 (defense in depth — proves the inode is the same file, not a coincidentally-equal new inode number).

This is INT-FGD-4 hoisted into the contract test. The same invariant is asserted at Layer A (the M3 scenario) for the integration story; the contract test asserts it for the plugin in isolation.

### 3.11.S.7 `test_delete_folder_removes_empty_tree`

**Setup:** same as 3.11.S.1 (all-unique, 2 files, 0 sidecars).

**Assertion:**

- After `delete_folder` completes, the entire `<HF_HOME>/hub/models--<author>--<repo>/` directory tree is gone.
- The parent `<HF_HOME>/hub/` directory still exists (only this repo's subtree is removed).
- Sibling repos under `<HF_HOME>/hub/` (if any in the fixture) are untouched.

### 3.11.S.8 `test_delete_folder_only_sidecars`

**Setup:** fixture with one HF repo containing 0 model files and 1 sidecar (`README.md`) — the leftover-after-manual-delete case from the journey draft.

**Assertion:**

- `plugin.delete_folder(&plan).await?` returns `Ok(Vec<DeleteOutcome>)` of length 1.
- That entry: `registration_removed: true`, `file_deleted: true`, `bytes_freed: <readme_size>`.
- The `models--<author>--<repo>/` directory tree is fully removed.

This is the journey's "Pressing [F] on a folder with only sidecars" edge case promoted to a contract test.

---

## 5. Per-Plugin Fixture Specifications

### 5.1 HF plugin fixture — `standard-hf-folder`

```
plugins/hf/tests/fixtures/standard-hf-folder/
└── home/
    └── .cache/
        └── huggingface/
            └── hub/
                ├── models--bartowski--Test-Repo-Unique/
                │   ├── snapshots/
                │   │   └── abc123/
                │   │       ├── model-q4.gguf  -> ../../blobs/<sha-q4>
                │   │       └── model-q8.gguf  -> ../../blobs/<sha-q8>
                │   └── blobs/
                │       ├── <sha-q4>           # 1 GB sparse
                │       └── <sha-q8>           # 1 GB sparse
                ├── models--bartowski--Test-Repo-Mixed/
                │   ├── snapshots/
                │   │   └── def456/
                │   │       ├── model-1.gguf   -> ../../blobs/<sha-unique>
                │   │       ├── model-2.gguf   -> ../../blobs/<sha-shared-1>
                │   │       └── model-3.gguf   -> ../../blobs/<sha-shared-2>
                │   └── blobs/
                │       ├── <sha-unique>       # 1 GB sparse
                │       ├── <sha-shared-1>     # 1 GB sparse; hardlinked into Ollama tree
                │       └── <sha-shared-2>     # 1 GB sparse; hardlinked into Ollama tree
                ├── models--bartowski--Test-Repo-Sidecars/
                │   ├── README.md              # 24 KB
                │   ├── snapshots/
                │   │   └── ghi789/
                │   │       ├── model-q4.gguf -> ../../blobs/<sha-q4-s>
                │   │       ├── model-q8.gguf -> ../../blobs/<sha-q8-s>
                │   │       ├── model.imatrix  # 1.3 MB
                │   │       └── model.gguf.urls # 8 KB
                │   └── blobs/
                │       ├── <sha-q4-s>         # 1 GB sparse
                │       └── <sha-q8-s>         # 1 GB sparse
                ├── models--bartowski--Test-Repo-Partial/
                │   └── ... (5 model files, one in a 0555 subdir OR EBUSY-marked)
                └── models--bartowski--Test-Repo-Sidecar-Only/
                    └── README.md              # 24 KB; no snapshots/, no blobs/
```

Plus the sibling Ollama tree under `<fixture>/home/.ollama/models/blobs/` containing hardlinks to `<sha-shared-1>` and `<sha-shared-2>`.

### 5.2 Non-HF plugin fixtures

The Ollama / llama-cli / lm-studio plugins reuse their existing parent contract fixtures (`standard-ollama`, `standard-llama-cli`, `standard-lm-studio`). The `Unsupported` test does not need any folder-specific fixture content — it short-circuits on the default body.

---

## 6. Cross-Tool Test Harness (extension)

For test 3.11.S.6 the contract harness needs to set up "this fixture file is ALSO registered with Ollama (via hardlink)" without depending on the actual Ollama plugin crate (per architecture rule R2: plugins do not depend on each other).

Solution: extend the parent's `MockOtherToolPlugin` with a helper:

```rust
// crates/modeltap-core/src/tests/mocks.rs (cfg(test) only)
impl MockOtherToolPlugin {
    /// Sets up a hardlink between `hf_blob_path` and `mock_other_tool_path`
    /// in the fixture. Asserts post-condition that both stat() to the same
    /// inode. Used by folder-delete contract test 3.11.S.6.
    pub fn hardlink_shared_file(
        &self,
        hf_blob_path: &Path,
        mock_other_tool_path: &Path,
    ) -> std::io::Result<()> { /* ... */ }
}
```

This lives in `modeltap-core` (not in any plugin crate), so it does not violate R2.

---

## 7. CI Wiring

The new contract tests live in:

- `plugins/hf/tests/folder_delete_contract.rs` — runs `FolderDeleteCapability::Supported`.
- `plugins/ollama/tests/folder_delete_contract.rs` — runs `FolderDeleteCapability::Unsupported`.
- `plugins/llama-cli/tests/folder_delete_contract.rs` — runs `FolderDeleteCapability::Unsupported`.
- `plugins/lm-studio/tests/folder_delete_contract.rs` — runs `FolderDeleteCapability::Unsupported`.

The existing CI invocation `cargo test --workspace --locked` picks them up automatically.

Adding a future 5th plugin (Atomic Chat, per US-18) follows the parent's pattern: the new plugin gets its own `folder_delete_contract.rs` declaring `Unsupported` (or `Supported` if it has folder semantics — at which point it adds the override AND switches the capability flag).

---

## 8. Failure Modes (what "the folder-delete contract is broken" looks like)

| Failure | Likely cause |
|---|---|
| 3.11.U.1 fails — non-HF plugin returns Ok or wrong error variant | The plugin accidentally implemented `delete_folder` instead of inheriting the default. |
| 3.11.S.1 fails — `bytes_freed` sum doesn't match plan | The plugin used the wrong size source (e.g., re-stat'd a file already unlinked, getting 0). |
| 3.11.S.2 fails — shared file fully deleted, Ollama inode dead | The HF plugin's `delete_one_at` ref-counting is broken — likely deleted the blob without checking nlink. |
| 3.11.S.3 fails — sidecar count off | The HF plugin's `enumerate_sidecars` missed a file type or hardcoded the suffix list incorrectly. |
| 3.11.S.4 fails — function aborts on first error | The plugin used `?` propagation instead of per-file outcome capture; rewrite to collect outcomes. |
| 3.11.S.5 fails — second call panics or destructively erases something | Idempotence violated — the plugin assumed first-call semantics. |
| 3.11.S.6 fails — Ollama inode different post-delete | The HF plugin unlinked the blob OR called something that resolves and removes hardlinks (e.g., `realpath`-based deletion). |
| 3.11.S.7 fails — empty directory tree left behind | The plugin's `remove_empty_repo_tree` step is missing or buggy. |
| 3.11.S.8 fails — function returns Err for sidecar-only folder | The plugin special-cases "no model files" incorrectly; sidecar-only folders are legitimate. |

---

## 9. Relationship to Acceptance Tests

The contract tests (Layer B) and the E2E acceptance scenarios (Layer A) cover overlapping invariants. The distinction:

- **Layer A** (acceptance scenarios in `features/folder-group-delete.feature`) drives the real `modeltap` binary through user-observable flows. Asserts on TUI frames, JSONL logs, exit codes. Slower (~1 s per scenario); validates the full stack.
- **Layer B** (contract tests in this spec) drives the plugin trait method directly with synthetic plans. Asserts on returned `Vec<DeleteOutcome>` and filesystem state. Faster (~100 ms per test); validates the plugin in isolation.

For the M5 walking-skeleton-adjacent scenario ("non-HF plugin returns Unsupported"), the E2E version asserts user-observable behavior ("right pane shows `<plugin> does not support folder-delete`"). The contract test asserts the trait-level behavior (`Err(DeleteError::Unsupported)`). Both are required: the E2E proves the orchestrator surfaces the error; the contract test proves the plugin returns the right thing in the first place.
