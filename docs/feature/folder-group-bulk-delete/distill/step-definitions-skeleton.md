# Step Definitions Skeleton — folder-group-bulk-delete

**Wave:** DISTILL (5 of 6) — brownfield extension of `modeltap-tui`
**Author:** Quinn (nw-acceptance-designer)
**Purpose:** specify every NEW Given/When/Then phrase introduced by `features/folder-group-delete.feature` and `features/integration-checkpoints.feature`, what each step asserts, and which seam it tests at. DELIVER's software-crafter writes the actual Rust step code.

**Inheritance:** the parent's `step-definitions-skeleton.md` defines all common phrases (launch, navigate, press key, type, snapshot assertions, JSONL assertions). This document specifies only DELTAS. Where a phrase appears in the parent skeleton, this document does not duplicate it — it references the parent section.

---

## Conventions (inherited)

- **Seam labels** (same as parent): `BIN`, `BIN-HEADLESS`, `APP`, `CORE`, `FS`, `JSONL`, `SNAPSHOT`.
- **`World` type** (inherited from parent's `acceptance/world.rs`): carries `temp_dir`, `fixture_name`, `env: HashMap<String,String>`, `cmd`, `output`, `frames`, `script_path`, `log_dir`.
- **New World fields this feature adds** (DELIVER's `World` extension):
  - `hf_cache_root: PathBuf` — absolute path to the per-scenario HF cache root (`${temp_dir}/hf-cache/`). Used by FS assertions and by `HF_HOME` env-var injection.
  - `ebusy_paths: Vec<PathBuf>` — list of paths the test seam should synthesize EBUSY for. Materialized at the start of each scenario via `MODELTAP_TEST_EBUSY_PATHS`.
  - `pre_action_inventory: Option<DirManifest>` — snapshot of the HF cache file tree before a destructive `When` step. Used for "no destructive action" assertions.
  - `pre_action_disk_usage: Option<u64>` — pre-action sum of file sizes; used for "total decreases by bytes_reclaimed" assertions.

- **Step file organization** (DELIVER will add): one new file under the parent's `tests/acceptance/steps/` directory:
  - `folder_delete.rs` — Sections A–F below.
  - Plus minor extensions to the parent's `discovery.rs` (folder header rendering steps) and `kpi.rs` (the new `action.folder_delete` JSONL event).

---

## A. Folder grouping and discovery (deltas to parent `discovery.rs`)

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given Devon has fixture "<name>" with the HF cache containing the repo "<author>/<repo>"` | FS | Builds `${TMPDIR}/hf-cache-<scenario>/hub/models--<author>--<repo>/` with the fixture's named contents. Sets `HF_HOME=${TMPDIR}/hf-cache-<scenario>`. Records `world.hf_cache_root`. |
| `Given the folder contains <N> model files "<file1>" (<size1>) and "<file2>" (<size2>) unique to Hugging Face` | FS | Writes sparse files at `<hub>/models--<author>--<repo>/blobs/<sha>` with the named sizes; creates snapshot-tree symlinks per the HF layout. Marks files as not-shared with any other tool. |
| `Given the folder contains <N> sidecars "<file1>" (<size1>), "<file2>" (<size2>), "<file3>" (<size3>)` | FS | Writes the named files at `<hub>/models--<author>--<repo>/` (README.md at top) or `<hub>/models--<author>--<repo>/snapshots/<rev>/` (`.imatrix`, `.gguf.urls`) per the HF convention. |
| `Given the folder contains <N> model files unique to Hugging Face totaling <S> GB` | FS | Builds N sparse files whose sum equals S GB. |
| `Given the folder contains <N> model file "<file>" (<size>) hardlinked into Ollama` | FS | Builds the file ONCE; hardlinks it into the HF blob directory AND into a parallel Ollama fixture tree at `${TMPDIR}/ollama/models/blobs/`. Sets `MODELTAP_OLLAMA_DIR=${TMPDIR}/ollama/models`. Asserts `stat(hf_path).st_ino == stat(ollama_path).st_ino`. |
| `Given the folder contains <N> sidecar files totaling <S>` | FS | As above, summed. |
| `Given the Hugging Face and Ollama paths stat to the same inode pre-delete` | FS | Asserts `stat(hf_path).st_ino == stat(ollama_path).st_ino`. Records the inode in `world` for the post-delete assertion. |
| `Given Devon has fixture "devon-hf-busy" with the repo "<author>/<repo>" (<N> files, <S> GB)` | FS | Combined: builds 21-file HF folder. |
| `Given the FsProbe adapter is configured with fake-lsof reporting "<text>"` | BIN-HEADLESS | Inherits parent §I. The text is the lsof output the script will print. Sets `MODELTAP_LSOF=tests/fakes/lsof-<scenario>.sh`. |
| `Given the fixture's filesystem will return EBUSY for those <N> files only` | BIN-HEADLESS + FS | Sets `MODELTAP_TEST_EBUSY_PATHS=<colon-separated absolute paths>`. The HF plugin's `delete_one_at` test-only wrapper consults this env var and returns `io::Error::from_raw_os_error(libc::EBUSY)` for matching paths. **DELIVER-owned seam.** |
| `Given Devon has fixture "devon-hf-perm" with the repo "<author>/<repo>" (<N> files, <S> GB)` | FS | As above; one file's parent directory is at mode 0555. |
| `Given one model file "<file>" lives in a directory with mode <octal>` | FS | chmod on the file's containing directory. |
| `Given Devon has fixture "devon-hf-readonly" with the HF cache directory at mode <octal>` | FS | chmod on `${TMPDIR}/hf-cache-<scenario>/`. |
| `Given Devon has fixture "devon-hf-20files" with "<author>/<repo>" containing <N> model files` | FS | 20 sparse files, sizes irrelevant. |
| `Given Devon has fixture "devon-hf-allunique" with the repo "<author>/<repo>" (<N> files, <S> GB)` | FS | Shorthand combined builder. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon navigates the cursor to the folder header "<author>/<repo>"` | BIN-HEADLESS | Appends script: `wait_for: "<author>/<repo>"; key: until cursor row matches`. |
| `When Devon presses Shift+F` | BIN-HEADLESS | Appends `key: Shift+F` to the script. |
| `When Devon types "<text>"` | BIN-HEADLESS | Inherits parent §A. Appends `type: <text>`. |
| `When Devon presses Enter` | BIN-HEADLESS | Inherits parent §A. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the <N> model files are removed from the Hugging Face fixture directory` | FS | For each named/known file, `assert!(!path.exists())`. |
| `Then the <N> sidecar files are removed from the Hugging Face fixture directory` | FS | As above for sidecars. |
| `Then the now-empty "<dir>" directory tree is removed` | FS | `assert!(!dir.exists())` AND the parent `<HF_HOME>/hub/` directory exists (i.e., only this repo's subtree is gone). |
| `Then the Hugging Face path "<path>" no longer exists` | FS | `assert!(!path.exists())`. |
| `Then the Ollama path "<path>" still exists` | FS | `assert!(path.exists())`. |
| `Then the Ollama path stats to a live inode with the original SHA256 content` | FS | `stat(path).st_ino == world.pre_delete_inode` AND `sha256(path) == world.pre_delete_sha256`. |
| `Then the folder header no longer appears in the right pane` | SNAPSHOT | Captured frame's right pane does not contain `<author>/<repo>`. |
| `Then the folder header still appears in the right pane` | SNAPSHOT | Inverse. |
| `Then the right pane shows "<text>"` | SNAPSHOT | Inherits parent §E. |
| `Then the right pane lists "<text>"` | SNAPSHOT | Substring in the right pane region. |
| `Then the right pane hints "<text>"` | SNAPSHOT | Substring in the right pane region (used for the "press [F] again" hint). |
| `Then the dialog itemises "<text>"` | SNAPSHOT | Substring in the dialog body region. |
| `Then the dialog identifies the shared file as "<text>"` | SNAPSHOT | Substring. |
| `Then the dialog shows "<text>"` | SNAPSHOT | Substring in dialog body. |
| `Then a modal dialog opens titled "<text>"` | SNAPSHOT | Captured frame has a dialog region with a header matching `<text>`. |
| `Then no folder-delete dialog opens` | SNAPSHOT | Captured frame does not contain a folder-delete dialog region. |
| `Then no dialog opens` | SNAPSHOT | Inherits parent. |
| `Then the dialog closes with no changes` | SNAPSHOT + FS | Dialog gone in next frame; world.pre_action_inventory equals current dir manifest. |
| `Then no files are removed from the Hugging Face fixture directory` | FS | Current HF cache manifest equals `world.pre_action_inventory`. |
| `Then the Hugging Face fixture directory is unchanged` | FS | Same as above. |
| `Then the Hugging Face fixture directory is byte-identical pre and post (manifest equal)` | FS | Same — emphasises byte identity. |
| `Then the "[F]" indicator in the bottom bar is dimmed` | SNAPSHOT | Bottom-bar cell for "[F]" has the dim style attribute (per parent's color/style assertion mechanism). |
| `Then the Ollama fixture directory is unchanged` | FS | Manifest comparison on the Ollama tree. |

---

## B. Folder-delete dialog convenience steps

These are higher-level composite Given/When steps that bundle navigation + key presses, used by scenarios that don't care about the path to the dialog state.

| Step phrase | Seam | Behavior |
|---|---|---|
| `Given Devon has opened the folder-delete dialog for "<author>/<repo>"` | BIN-HEADLESS | Composite: launch in headless mode against the prevailing fixture, select HF, navigate to folder header, press Shift+F, wait for dialog. |
| `When Devon successfully confirms the folder-delete for "<author>/<repo>"` | BIN-HEADLESS | Composite: type the path, press Enter, wait for post-action summary. |
| `When Devon successfully folder-deletes "<author>/<repo>"` | BIN-HEADLESS | Same as above; shorter form. |
| `Given Devon has completed a folder-delete against fixture "<name>" for "<author>/<repo>"` | BIN-HEADLESS | Composite: full happy-path flow ending at the post-action summary. Used as precondition for scenarios that assert on the summary. |
| `Given Devon completed a partial folder-delete leaving <N> EBUSY files in "<author>/<repo>"` | BIN-HEADLESS + FS | Composite: runs the M4 partial-failure flow, asserts the partial summary appeared, leaves the 2 files on disk. |
| `Given the holding tool has been closed` | BIN-HEADLESS | Removes the `MODELTAP_TEST_EBUSY_PATHS` env var (or rewrites the fake-lsof to report empty). |
| `Given the next inventory rebuild lists the folder header with <N> remaining files` | BIN-HEADLESS | Triggers a re-discovery (parent's `[r] refresh` shortcut) and asserts the folder header shows `<N> files`. |

---

## C. Plugin contract dispatch (M5 — capability boundary)

| Step phrase | Seam | Behavior |
|---|---|---|
| `Given Devon has fixture "devon-multi-tool" with the <plugin> plugin installed` | FS | Inherits parent. |
| `Given the orchestrator attempts a folder-delete dispatch against the <plugin> plugin` | APP | DELIVER builds a `FolderDeletePlan` for an arbitrary HF folder, then invokes `<plugin>.delete_folder(&plan)` directly via the trait object. This is a Layer A scenario for end-user observability AND a Layer B contract check; the implementation lives in the orchestrator's test path. |
| `When the <plugin> plugin's Tool::delete_folder is invoked through the orchestrator` | APP | Calls `Tool::delete_folder` and captures the `Result`. |
| `Then the orchestrator receives DeleteError::Unsupported with tool == "<plugin>"` | APP | Asserts `result == Err(DeleteError::Unsupported { tool: ToolId("<plugin>") })`. |
| `Then no filesystem mutation occurs in the <plugin> fixture directory` | FS | Manifest comparison on the plugin's fixture tree. |
| `Then the right pane shows "<plugin> does not support folder-delete"` | SNAPSHOT | If the orchestrator surfaces the Unsupported error to the UI (it should — for AC-5 coverage), the captured frame contains this text. **DELIVER decision:** this assertion may be relaxed to a JSONL log assertion if the UI never reaches a path where Unsupported can be observed (e.g., if AC-5's "Shift+F dimmed when not HF" prevents the dispatch from happening at all). In that case, replace this step with `Then no folder-delete UI flow is triggered`. |

---

## D. KPI assertions (deltas to parent `kpi.rs`)

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the JSONL log "action.folder_delete" event has "keystroke_count" less than or equal to <N>` | JSONL | Reads `${LOG_DIR}/launch.log`, finds the `action.folder_delete` event, asserts `event.keystroke_count <= N`. |
| `Then the JSONL log "action.folder_delete" event has "keystroke_count" independent of the folder's file_count` | JSONL + arithmetic | Parameterized assertion: for the 20-file scenario AND a 5-file companion scenario, `keystroke_count` must be within the same bound. **Note:** the cucumber-rs framework may need to run two scenarios under the same `@property` tag and compare across them. DELIVER may simplify to "keystroke_count <= 40 AND keystroke_count > 32" instead of cross-scenario comparison. |
| `Then the JSONL log "action.folder_delete" event has "outcome" == "success"` | JSONL | Field equality. |
| `Then the JSONL log "action.folder_delete" event has "outcome" == "cancelled_mismatch"` | JSONL | Field equality. |
| `Then no DeleteOutcome is produced for any file in the folder` | JSONL | Asserts the `action.folder_delete` event has `outcomes_count == 0` (the orchestrator records this even on aborted dialogs, per parent's instrumentation convention). |

---

## E. Cross-cutting integration steps (integration-checkpoints.feature)

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given any FolderGroup built from any HF fixture` | BIN-HEADLESS | Launches headless against an arbitrary HF fixture; the next assertion holds over the resulting FolderGroup. **`@property` tag:** DELIVER may replace this E2E scenario with a proptest in `modeltap-core/tests/folder_group_proptest.rs` generating synthetic `Inventory` values directly. Spec applies either way. |
| `Given any populated HF folder group built from fixture "<name>"` | BIN-HEADLESS | Same as above with named fixture. |
| `Given the per-file classification has run for that folder` | APP | DELIVER's step calls `classify_unique_vs_shared(folder, inventory, capabilities)` under the hood. |
| `When the folder header row renders` | SNAPSHOT | Captures the frame after first paint. |
| `When the folder-delete dialog renders` | SNAPSHOT | Captures the frame after `Shift+F`. |
| `Then "folder_group.file_count" equals "<expr>"` | APP | Parses the dialog header for the file count; computes the RHS from fixture metadata; asserts equality. |
| `Then "folder_group.bytes_to_reclaim + folder_group.bytes_to_retain" equals "folder_group.total_bytes" within rounding tolerance of <N> byte(s)` | APP | Parses the dialog's Reclaim and Retained lines into u64; sums; compares to the fixture's known total. |
| `Then the new "total.disk_usage" equals "<expr>" within rounding tolerance of <N> byte(s)` | APP + SNAPSHOT | Reads the summary-bar value from the captured frame; computes the RHS; asserts within tolerance. |
| `Then "total.disk_usage" equals the sum of "tool.disk_usage" for every installed tool within rounding tolerance of <N> byte(s)` | SNAPSHOT | Reads all per-tool disk_usage values from the left-pane frame; sums; compares to the summary bar. |
| `Then for every previously-shared file, the other tool's path still stats to a live inode` | FS | For each `(hf_path, other_path)` pair recorded in `world.pre_delete_shared_files`, `assert!(other_path.exists())` and `stat(other_path).st_ino` matches the recorded pre-delete inode. |
| `Then the inode of the other tool's path matches the inode it had before the folder-delete` | FS | Same as above; more explicit. |
| `Then the HF plugin's "list_models" output contains no entry whose id_in_tool starts with "<prefix>"` | APP | After a re-discovery (or by reading the next captured frame), DELIVER invokes the HF plugin's `discover()` directly (this is a Layer B-ish hook) OR parses the right-pane for the prefix. Either is acceptable; spec allows both. |
| `Then the HF plugin's "list_folder_groups" output contains no entry with path "<path>"` | APP | Same; folder-groups are computed by `logic::folder_group::group_by_hf_repo` over discover()'s output. |
| `Then the comparator reads "folder_group.path" from the dialog's bound state` | CORE | This is a code-inspection assertion in spirit; at the test layer, DELIVER asserts via PROPERTY: take any folder with a unique path P, open the dialog, type any string Q where Q != P, observe rejection. The cardinality of the rejected-string space exceeds any reasonable hardcoded-literal heuristic. Spec calls this out as a property-test-shaped check. |
| `Then no literal repo path appears inline in the dispatch code` | CORE | A grep test in `crates/modeltap-tui/tests/lint.rs` that asserts no string matching `^[a-zA-Z0-9._-]+/[a-zA-Z0-9._-]+$` appears in the keymap or dispatch source. DELIVER-owned test. Spec asserts this as a property. |
| `Then no folder-delete dialog opens` | SNAPSHOT | As above. |
| `Then the next inventory rebuild runs` | BIN-HEADLESS | Triggers a re-discovery; waits for completion. |
| `Then the folder-group-bulk-delete feature is merged into modeltap` | (precondition) | Trivially true at test time; this is a regression-gate scenario meta-statement. DELIVER asserts by running the parent's `@walking-skeleton` tagged subset of `master-acceptance.feature` and confirming all pass. |
| `When the parent acceptance suite runs against fixture "devon-multi-tool"` | BIN-HEADLESS | `cargo test --test acceptance -- --tag '@walking-skeleton and not @us-05c'`. |
| `Then every scenario in <file> tagged @walking-skeleton still passes` | BIN | Asserts the test invocation's exit code is 0. |
| `Then no parent scenario produces a new failure attributable to the folder-delete code paths` | BIN | Same; supplementary diagnostic. |

---

## F. Pre-flight refusal steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `Given Devon has launched modeltap and the folder header is visible` | BIN-HEADLESS | Composite: launch + select HF + wait for folder header in right pane. |
| `Given an out-of-band process has removed the on-disk "<dir>" directory tree` | FS | After launch and after the folder header is captured in the frame, the test step calls `std::fs::remove_dir_all(<dir>)`. The inventory has NOT yet been refreshed, so the stale folder header is still in the TUI state. |
| `When Devon presses Shift+F on the now-stale folder header` | BIN-HEADLESS | Same as `When Devon presses Shift+F`. |
| `Then no folder-delete dialog opens` | SNAPSHOT | As above. |

---

## G. Step-Definition File Layout (DELIVER recommendation)

Per `nw-bdd-methodology` "Step Organization by Domain", the new step files extend the parent layout:

```
crates/modeltap-app/tests/acceptance/
├── world.rs                    # MODIFIED — add hf_cache_root, ebusy_paths,
│                                pre_action_inventory, pre_action_disk_usage
├── steps/
│   ├── ... (existing parent step files) ...
│   ├── folder_delete.rs        # NEW — Sections A, B, F above
│   ├── plugin_contract.rs      # MODIFIED — add Section C steps (M5)
│   ├── kpi.rs                  # MODIFIED — add Section D steps (action.folder_delete event)
│   ├── integration.rs          # NEW — Sections E (cross-cutting)
│   └── discovery.rs            # MODIFIED — folder header row rendering steps
└── fixtures/
    ├── build.sh                # MODIFIED — add the 6 new named fixtures
    ├── devon-hf-allunique/     # NEW
    ├── devon-hf-mixed/         # NEW
    ├── devon-hf-busy/          # NEW
    ├── devon-hf-perm/          # NEW
    ├── devon-hf-readonly/      # NEW
    ├── devon-hf-20files/       # NEW
    └── fakes/
        └── lsof-busy-ollama-files.sh   # NEW — emits "ollama PID 4421 holds <file1> <file2>"
```

Each step file is a self-contained module of `#[given(...)] / #[when(...)] / #[then(...)]` functions over the shared `World` type. cucumber-rs auto-discovers them.

---

## H. Test harness types referenced

For DELIVER's reference, the following harness types are mentioned across this spec:

| Type | Purpose | Where it lives |
|---|---|---|
| `HfFixture` | Builder for `${TMPDIR}/hf-cache-<scenario>/` trees. Methods: `with_repo(author, repo)`, `with_model(file, size, kind)`, `with_sidecar(file, size)`, `with_shared_into_ollama(file, ollama_dir)`, `with_readonly_root()`. Returns a `Drop`-cleaning handle. | `tests/src/fixtures/hf_fixture.rs` (NEW) |
| `KeyEventDriver` | Inherited from parent. Constructs `--script` input for the headless binary; supports `key`, `type`, `wait_for`, `Shift+F` modifiers. | Existing in parent's `tests/src/lib.rs` |
| `HeadlessTuiHarness` | Inherited from parent. Wraps `assert_cmd::Command` + script + frame capture + JSONL log reader. | Existing in parent's `tests/src/lib.rs` |
| `DirManifest` | Snapshot of a directory tree (recursive `(path, size, mtime)`) for pre/post assertions. **New in this feature** because folder-delete asserts on multiple-file unchanged-state. | `tests/src/fixtures/dir_manifest.rs` (NEW) |
| `MockOtherToolPlugin` | Inherited from parent. Used in M5 if the orchestrator dispatch test needs a synthetic non-HF plugin. | Existing in `modeltap-core::tests::mocks` |

---

## I. Implementation discipline notes for DELIVER

1. **One scenario enabled at a time.** Quinn's standard `@skip` discipline: only `@walking-skeleton` is enabled initially. After M1 passes against the real HF plugin and real fixture, DELIVER removes `@skip` from M2's first scenario, runs RED → GREEN, removes from the next, etc.
2. **Step phrases are STABLE.** The phrasing in this skeleton matches the phrasing in the feature files. DELIVER MUST NOT alter phrasing during implementation — that breaks the executable-spec round-trip. If a phrase is awkward in Rust step code, propose a phrase change in a follow-up PR.
3. **All assertions read observable behavior.** Per Mandate 1 and critique Dim 7, every Then step here asserts either (a) a return value from a driving port call (the `modeltap` binary's exit/stdout/JSONL), (b) an observable filesystem state (a path exists or doesn't), or (c) a captured TestBackend frame substring. No assertion reads `_internal_field` or `mock.called`. Verified by grep over the implementation during DELIVER review.
4. **EBUSY simulation seam is well-isolated.** The `MODELTAP_TEST_EBUSY_PATHS` env var seam is the ONLY test-only code path in the HF plugin. DELIVER may gate it behind `#[cfg(any(test, feature = "test-harness"))]` to ensure release builds do not include it. Spec is agnostic; either approach is acceptable.
