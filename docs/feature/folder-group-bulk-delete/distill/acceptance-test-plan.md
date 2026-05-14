# Acceptance Test Plan — folder-group-bulk-delete

**Wave:** DISTILL (5 of 6) — brownfield extension of `modeltap-tui`
**Author:** Quinn (nw-acceptance-designer)
**Date:** 2026-05-11
**Authoritative inputs:**
- DISCUSS: `docs/feature/folder-group-bulk-delete/discuss/{user-stories.md,acceptance-criteria.md,requirements.md,journey-folder-group-delete.feature,shared-artifacts-registry.md,outcome-kpis.md}`
- DESIGN: `docs/feature/folder-group-bulk-delete/design/{architecture-design.md,component-boundaries.md,data-models.md}` and `docs/adrs/ADR-010-folder-group-delete-hf-capability.md`
- Project convention: `docs/feature/modeltap-tui/distill/{acceptance-test-plan.md,step-definitions-skeleton.md,plugin-contract-spec.md,features/master-acceptance.feature}`
- Project `CLAUDE.md`

This plan is **additive** to the parent's acceptance-test-plan. It specifies only the deltas required for US-05c. The parent's framework, fixture strategy, headless-mode contract, env-var contract, and step-organization layout are inherited unchanged.

---

## 1. Test Framework — Inherited

**Same as parent.** `cucumber-rs` for Gherkin-driven E2E; `assert_cmd` for the binary; `tempfile::TempDir` for the per-scenario root; `insta` for snapshots; `serde_json` for JSONL log parsing; `predicates` for substring matches; `expectrl` reserved for the rare `@interactive` scenarios. The parent's test pyramid (E2E → Plugin contract → Unit → TUI snapshot) is reused verbatim.

**Project test infrastructure used:**
- Acceptance crate at `tests/` (`modeltap-acceptance`).
- HF cache driven via tempdir fixtures mimicking `<HF_HOME>/hub/models--<author>--<repo>/` layout.
- Headless TUI via `ratatui::backend::TestBackend`; `MODELTAP_HEADLESS=1` + `--script` flag (parent's contract).
- Plugin-contract tests under each plugin's `tests/` directory parameterized over `T: Tool` from the parent's `plugin_contract` harness.

**Rust-native gherkin discipline:** the feature files in `features/` are written for `cucumber-rs`. No pytest-bdd, no Cucumber-JS, no SpecFlow — locked per the parent convention.

---

## 2. Test Layer Map — US-05c only

| Layer | Mechanism | What this feature adds | Owns |
|---|---|---|---|
| **A — E2E acceptance** | cucumber-rs runs `features/folder-group-delete.feature` + `features/integration-checkpoints.feature`; steps drive `modeltap` binary in headless mode against `tempfile::TempDir`-built HF cache trees | 15 scenarios in `folder-group-delete.feature`; 10 scenarios in `integration-checkpoints.feature` | DISTILL writes scenarios; DELIVER writes step defs |
| **B — Plugin contract** | One new parameterized test `crates/modeltap-core/tests/folder_delete_contract.rs` extending the parent's `plugin_contract` harness with the `delete_folder` contract | HF plugin: contract path (b) — honors folder-delete invariants. Ollama / llama-cli / lm-studio: contract path (a) — returns `Err(DeleteError::Unsupported)`. See `plugin-contract-spec.md`. | DISTILL writes the spec; DELIVER implements |
| **C — Unit (modeltap-core)** | Standard `#[test]` against `logic::folder_group::{group_by_hf_repo, classify_unique_vs_shared, build_folder_delete_plan}` | Property tests for single-engine invariant (D-FGD-4 / AC-13). Unit tests for INT-FGD-2, INT-FGD-3 reclaim-math invariants. | DELIVER's software-crafter writes per inner-loop TDD |
| **D — TUI snapshot** | `ratatui::backend::TestBackend` + `insta` | Folder header row (collapsed / expanded), `[F]` shortcut in bottom bar (dimmed when not applicable), folder-delete dialog body (all-unique, mixed, sidecar-only variants), partial-failure post-action summary | DELIVER writes per inner loop |

### Story → Layer coverage matrix

| Story | E2E (A) | Contract (B) | Unit (C) | Snapshot (D) |
|---|:---:|:---:|:---:|:---:|
| US-05c | ✓ (15 + 10 scenarios) | ✓ (HF: full; non-HF: Unsupported) | ✓ (folder_group module) | ✓ (folder header + dialog + post-action) |

Every AC traces to at least one Layer A or Layer B scenario; see the AC traceability matrix in §6 below.

---

## 3. Fixture Strategy

### Principle (inherited): synthetic-but-realistic HF cache trees built via `tempfile::TempDir`

Each scenario builds an HF cache tree at `${TMPDIR}/hf-cache-${SCENARIO_ID}/hub/`, populates it with `models--<author>--<repo>/` directories matching the real HF layout, and points the binary at it via `HF_HOME` (the standard HF env var, reused per the parent contract).

**Per parent convention:** model files are sparse (`truncate -s <size>`) for size-only tests; SHA256-equality scenarios use `cp` (or `cp --reflink=auto` on btrfs/xfs) from a shared blob to each tool's directory; the GGUF magic-bytes header is written for tests that need format detection.

### Named fixture trees this feature adds

| Name | Contents | Used by milestone |
|---|---|---|
| `devon-hf-allunique` | 1 HF repo `bartowski/Llama-3.2-1B-Instruct-GGUF` with 2 model files (808 MB + 1.3 GB) unique to HF + 3 sidecars (README.md, .imatrix, .gguf.urls). All files in this repo only. | M1 walking skeleton, M2 confirmation safety |
| `devon-hf-mixed` | 1 HF repo `bartowski/Llama-3.2-1B-Instruct-GGUF` with 19 unique model files (13.2 GB total) + 1 shared model file `Llama-3.2-1B-Instruct-Q4_K_M.gguf` (808 MB) hardlinked into a sibling Ollama fixture tree + 3 sidecars. | M3 mixed shared/unique |
| `devon-hf-busy` | Same shape as `devon-hf-allunique` extended to 21 files. `MODELTAP_LSOF` points at a fake-lsof script reporting 2 specific files as held by Ollama. The fixture's filesystem returns EBUSY for those 2 files only (achieved by leaving them open in a sibling child process kept alive for the scenario duration). | M4 partial failure (EBUSY) |
| `devon-hf-perm` | 1 HF repo with 5 model files where one file's containing directory has mode 0555 (file unlinkable by no user). | M4 partial failure (permission denied) |
| `devon-hf-readonly` | 1 HF repo plus the entire HF cache root at mode 0555. | Integration checkpoint: pre-flight refusal (AC-15) |
| `devon-hf-20files` | 1 HF repo with 20 model files (sparse, sizes irrelevant). | M6 KPI: keystroke count bounded |
| (reused) `devon-multi-tool` | Parent fixture; reused for M5 capability boundary (Shift+F dispatched against Ollama / llama-cli / lm-studio plugins). | M5 |

### How EBUSY is simulated portably

Two options, chosen per platform:

1. **Linux (CI primary):** the fixture-builder spawns a sibling helper process that `open()`s the 2 target files with `O_RDWR` and holds them. `unlink(2)` on Linux succeeds even on open files, so for the EBUSY path the fixture uses **`flock(LOCK_EX)`** plus a wrapper in the HF plugin's test harness that checks for advisory locks and returns EBUSY-equivalent. The plugin code path is not platform-specific; the simulation is.
2. **macOS (developer machines):** same approach — the sibling holds the file open. APFS allows unlink-while-open but a wrapped `remove_file` call in the test harness can return a synthetic EBUSY when configured.

**Alternative if the above is too invasive:** add a `MODELTAP_TEST_EBUSY_PATHS=path1:path2` env var honored by the HF plugin's `delete_one_at` helper to synthesize EBUSY for the listed paths. This is acceptance-test-internal and is the test-double seam — production code path is unchanged. **Recommended for first DELIVER pass** (less ceremony than real `flock`).

### Cross-tool hardlink fixture for INT-FGD-4

For `devon-hf-mixed`, the shared file is created ONCE under a shared blob dir, then **hardlinked** (`ln`) into both the HF cache's `blobs/` directory and the Ollama fixture's `blobs/` directory. `stat()` on both paths must return the same `st_ino` pre-delete. The scenario then asserts the Ollama-side `st_ino` is still live post-delete.

### Env vars used (delta vs parent)

| Env | Purpose |
|---|---|
| `HF_HOME` | Inherited; points at the per-scenario tempdir HF cache root. |
| `MODELTAP_OLLAMA_DIR` | Inherited; used by `devon-hf-mixed` to set up the sibling Ollama tree for hardlink survival. |
| `MODELTAP_TEST_EBUSY_PATHS` | NEW; colon-separated list of absolute paths the HF plugin's `delete_one_at` synthesizes EBUSY for during folder-delete tests. Honored only when `MODELTAP_HEADLESS=1` is also set (defense against accidental production leakage). |
| `MODELTAP_LSOF` | Inherited; points at the fake-lsof script for the running-tool soft warning. |
| `MODELTAP_HEADLESS=1` | Inherited; required for every scenario in this file. |
| `MODELTAP_LOG_DIR` | Inherited; tempdir per scenario. |

---

## 4. Walking Skeleton — Strategy Declaration

**Strategy: B (real I/O against fixture-populated temp dirs).** Inherits from parent.

For US-05c specifically, the walking skeleton:
1. Runs the real `modeltap` binary in headless mode.
2. Drives input through the `--script` mechanism.
3. Reads from a **real** HF cache tempdir fixture (`devon-hf-allunique`).
4. Invokes the **real** HF plugin's `delete_folder` override (not a mock).
5. Asserts file unlinks happened on the real filesystem via `path.exists() == false`.
6. Asserts the post-action summary by reading the captured TestBackend frame and the JSONL log line.

**Litmus test (critique-dimensions Dim 9d):** if we deleted the real HF adapter and substituted an InMemoryHfPlugin, would the walking skeleton still pass? **No.** The skeleton asserts on `path.exists()` against the real tempdir tree, which an in-memory plugin would not mutate. Strategy B is honored.

**Adapter integration coverage (Dim 9c):** the only NEW driven adapter this feature adds is `HfPlugin::delete_folder` (and its internal `enumerate_sidecars` + `delete_one_at` reuse). The walking skeleton tag `@adapter-integration` covers it. The non-HF plugins' `delete_folder` is the trait default — covered by the M5 plugin-contract scenario which exercises real plugin instances.

---

## 5. Test Execution Model

### Local developer

```bash
# Build fixtures (idempotent; extends parent's build.sh with the new fixtures above).
./tests/fixtures/build.sh devon-hf-allunique devon-hf-mixed devon-hf-busy devon-hf-perm devon-hf-readonly devon-hf-20files

# Run all folder-group-delete scenarios.
cargo test --test acceptance -- --tag @us-05c

# Run only the walking skeleton.
cargo test --test acceptance -- --tag '@walking-skeleton and @us-05c'

# Run a specific milestone.
cargo test --test acceptance -- --tag @milestone-3
```

### CI

The existing `ci.yml` "test" job runs `cargo test --workspace --locked`. This picks up the new scenarios automatically. Two notes:

1. The `folder_delete_contract.rs` test in `modeltap-core/tests/` is parameterized over the 4 plugins — it adds ~1 minute to the CI run (lightweight tempdir setup).
2. The `EBUSY` simulation env var (`MODELTAP_TEST_EBUSY_PATHS`) is opt-in; CI sets it only for the `@infrastructure-failure` scenarios.

### Smoke-test fast path

Per Quinn's protocol: fast path applies when total scenarios ≤ 3. This feature has 25 scenarios (15 + 10). **Full review pass + per-scenario fixture isolation applies.**

---

## 6. AC Traceability Matrix

Every US-05c.AC must trace to at least one scenario tag. Filing AC → scenario:

| AC | Tag(s) on scenario | Where it lives |
|---|---|---|
| AC-1 (folder grouping in right pane) | parent's `@us-12` already covers HF cache discovery; folder grouping is rendered behavior — DELIVER's Layer D snapshot tests verify; one E2E scenario uses it implicitly via Background ("the right pane shows the folder header") | M1 walking skeleton (implicit precondition); explicit Layer D coverage |
| AC-2 (folder header content) | implicit precondition of M1 ("navigates to folder header"); Layer D snapshot |
| AC-3 (folder header cursor-targetable) | implicit precondition of M1 ("navigates the cursor to the folder header"); Layer D snapshot |
| AC-4 (Shift+F opens dialog within 200ms) | `@ac-4` in M1 — explicit; performance assertion via JSONL `action.folder_delete.dialog_open_ms < 200` event |
| AC-5 (Shift+F no-op on non-folder rows / non-HF tool) | `@ac-5` in M2 + M5 |
| AC-6 (dialog body content) | `@ac-6` in M3 |
| AC-7 (Reclaim + Retained == total) | `@ac-7` in M3 + cross-cutting `@int-fgd-3` |
| AC-8 (byte-exact typed confirmation) | `@ac-8` in M1, M2 (×2 — wrong path, trailing slash), M6 property |
| AC-9 (Esc cancels) | `@ac-9` in M2 |
| AC-10 (per-file unlink semantics) | `@ac-10` in M1 + M3 |
| AC-11 (empty directory tree removed) | `@ac-11` in M1 |
| AC-12 (partial failure handling) | `@ac-12` in M4 (all 3 scenarios) |
| AC-13 (single-engine classification) | `@ac-13` in M3 property |
| AC-14 (HF plugin owns sidecar enumeration) | covered by plugin-contract-spec.md §3.B (HF contract path) |
| AC-15 (pre-flight read-only refusal) | `@ac-15` in integration-checkpoints |
| AC-16 (post-action summary content) | `@ac-16` in M1, M3, M4 |
| AC-17 (summary bar 500ms refresh) | `@int-fgd-6` in integration-checkpoints + parent's US-11 invariant |
| AC-18 (`[F]` shortcut in bottom bar, dimmed when not applicable) | `@ac-5` scenario asserts the dim; Layer D snapshot |
| AC-19 (`[F]` in SHORTCUT_TABLE single source) | covered by parent's US-08 invariant; no new scenario needed (regression-only) |
| AC-20 (folder-vanished pre-flight refusal) | `@ac-20` in integration-checkpoints |

**Integration ACs:**

| INT-FGD | Tag | Where |
|---|---|---|
| INT-FGD-1 (total == sum of tool disk_usage) | `@int-fgd-1` | integration-checkpoints |
| INT-FGD-2 (file_count = models + sidecars) | `@int-fgd-2` | integration-checkpoints (property) |
| INT-FGD-3 (reclaim + retain = total) | `@int-fgd-3` | integration-checkpoints (property) |
| INT-FGD-4 (cross-tool hardlink survives) | `@int-fgd-4` | M3 + integration-checkpoints |
| INT-FGD-5 (list_models excludes deleted folder) | `@int-fgd-5` | integration-checkpoints |
| INT-FGD-6 (total decreases by bytes_reclaimed) | `@int-fgd-6` | integration-checkpoints |
| INT-FGD-7 (comparator reads folder_group.path) | `@int-fgd-7` | integration-checkpoints (property) |
| INT-FGD-8 (parent regression gate) | `@int-fgd-8` | integration-checkpoints |

**Coverage gate:** every AC and every INT-FGD has at least one scenario tag. Zero blocker findings on critique-dimension 4 (Coverage Completeness) and 8a (Story-to-Scenario mapping).

---

## 7. What DELIVER Inherits From This Plan

1. **Feature files** — `features/folder-group-delete.feature` (15 scenarios) and `features/integration-checkpoints.feature` (10 scenarios).
2. **Step-definition skeleton** — `step-definitions-skeleton.md` (deltas vs parent only).
3. **Plugin-contract spec** — `plugin-contract-spec.md` (the `delete_folder` contract for the 4 plugins).
4. **Fixture inventory** — 6 new named fixtures listed in §3 above.
5. **Env-var contract delta** — `MODELTAP_TEST_EBUSY_PATHS` for partial-failure simulation.
6. **Strategy declaration** — Strategy B; walking skeleton uses real HF plugin + real tempdir filesystem.
7. **Walking-skeleton scenario** — `M1: Devon deletes an all-unique HF repo folder and reclaims disk`.

---

## 8. What DELIVER Decides

- The exact step-definition Rust code (this wave specifies what each step asserts; DELIVER writes the code).
- The exact `enumerate_sidecars` heuristic body in the HF plugin (this wave specifies the AC: sidecars enumerated per AC-14 / B-FGD-2; the suffix list and HF-internal refs/blobs heuristic is DELIVER's call against the DISCUSS examples).
- The exact form of the `MODELTAP_TEST_EBUSY_PATHS` mechanism (env var on the `delete_one_at` wrapper, or a separate `FakeFsOps` adapter — DELIVER may choose).
- The exact upper bound for `keystroke_count` in the M6 KPI scenario — `wave-decisions.md` records **40** as a defensible upper bound for a typical 30-char repo path (`Shift+F` counted at parent level → 30 chars path → Enter ≈ 32 keys, with 8 keys of headroom for corrections). DELIVER may tighten after first measurement.

---

## 9. Mandate Compliance Notes (CM-A through CM-D)

### CM-A — Hexagonal boundary (driving ports only)

All E2E scenarios invoke through the `modeltap` binary (the driving port). The Plugin Contract test (Layer B) invokes `Box<dyn Tool>` — the public port for plugin authors per ADR-001 and ADR-010. **No scenario imports `modeltap-core::logic::folder_group` directly.** The single-engine invariant (M3 @property scenario, AC-13) is asserted from the outside: every classification observable in the dialog text matches what `compute_indicator` would have produced for the same input.

### CM-B — Business language

Step phrases use Devon's vocabulary: "Devon presses Shift+F", "Devon types the folder path", "the dialog itemises", "the folder header no longer appears", "Reclaimed", "Retained". Technical terms confined to:
- `JSONL log` assertions (parent contract; explicit instrumentation steps);
- `stat()` / `inode` in cross-tool hardlink scenarios (these ARE the user-observable semantics for "the Ollama copy still works");
- `EBUSY` / `permission denied` in error scenarios (the reasons surfaced to the user verbatim in the post-action summary — these ARE the business language).

Forbidden terms in step text (DELIVER must not introduce): `HTTP`, `database`, `endpoint`, `function`, `method call`, `class`, `assert_eq!`, `unwrap`, `Vec<DeleteOutcome>`, `FolderClassification` (the type names appear only in `plugin-contract-spec.md` where they ARE the public contract). Verified by `grep` over the final feature files.

### CM-C — User journey completeness

The walking skeleton M1 traces a complete user journey: Devon launches → navigates → presses Shift+F → types confirmation → presses Enter → sees reclaim message → folder header disappears. Every focused scenario asserts what Devon SEES, not what the system internally computes (with the exception of the M3 property and the integration-checkpoint properties, which assert universal invariants over observable behavior).

### CM-D — Pure function extraction

The architecture (per `architecture-design.md` § 4.3) already separates pure logic in `modeltap-core::logic::folder_group::{group_by_hf_repo, classify_unique_vs_shared, build_folder_delete_plan}` from impure I/O in `plugins/hf::folder_delete::*` and `modeltap-app::orchestration::execute_folder_delete`. The acceptance tests (Layer A) exercise impure I/O through the binary. The unit tests (Layer C, DELIVER-owned) exercise pure logic directly without fixtures. **No fixture parametrization at the acceptance layer beyond the named-fixture-tree level**, which IS the adapter layer parametrization per the mandate.

Pure functions inventory (for DELIVER's unit-test plan):

| Function | Location | Pure? |
|---|---|---|
| `group_by_hf_repo` | `modeltap-core::logic::folder_group` | YES — no I/O, deterministic |
| `classify_unique_vs_shared` | `modeltap-core::logic::folder_group` | YES — calls pure `compute_indicator` |
| `build_folder_delete_plan` | `modeltap-core::logic::folder_group` | YES — pure arithmetic over typed inputs |
| `FolderGroup::total_bytes` | `modeltap-core::types` | YES — pure sum |
| `FolderGroup::file_count` | `modeltap-core::types` | YES — pure len |

Impure-but-isolated adapters:

| Adapter | Location | Driven by |
|---|---|---|
| `HfPlugin::delete_folder` | `plugins/hf::folder_delete` | real filesystem in M1 walking skeleton; tempdir fixture |
| `HfPlugin::enumerate_sidecars` | `plugins/hf::folder_delete` | real `walkdir` over the tempdir repo tree |
| `FsProbe::processes_holding` | parent's `modeltap-app::adapters::fs_probe` | `MODELTAP_LSOF` fake-lsof script (parent contract) |

---

## 10. Risks and Open Questions

| # | Item | Mitigation |
|---|---|---|
| R1 | EBUSY simulation cross-platform (Linux vs macOS) — `flock` semantics differ; `unlink-while-open` differs | Recommended path: `MODELTAP_TEST_EBUSY_PATHS` env-var test seam in the HF plugin's `delete_one_at` wrapper. DELIVER decides; spec is in §3 above. |
| R2 | `EBUSY` for the partial-failure path is the only @infrastructure-failure case — `permission denied` is covered by `mode 0555` which is portable | No mitigation needed; permission-denied scenario uses real chmod and works identically on macOS / Linux. |
| R3 | Sidecar enumeration heuristic owned by HF plugin (AC-14) — different HF versions may add new sidecar types. The walking skeleton's 3 sidecars are an exemplar, not exhaustive. | Documented as DELIVER decision (see § 8 above). The contract test (plugin-contract-spec.md) covers "all files in the repo directory tree that are not model files" semantics; the suffix list is implementation detail. |
| R4 | Out-of-band folder deletion (AC-20) requires racing the file system between launch and `Shift+F` — testing this deterministically is tricky | Mitigation: the integration-checkpoint scenario for AC-20 deletes the folder AFTER launch but BEFORE the Shift+F press — a deterministic ordering via the script driver. The race-condition risk between Shift+F and dialog-open is acknowledged but not test-targetable at the E2E layer; the plugin contract test in `crates/modeltap-core/tests/folder_delete_contract.rs` covers the "folder vanished mid-execution" case (returns `NotFound` per file). |
| R5 | The `Tool::delete_folder` default-method silently masks a missing override on a hypothetical future folder-aware plugin | Covered by M5 plugin-contract scenario: each plugin's contract is "EITHER `Unsupported` OR honors folder-delete invariants". A future 5th tool's PR must add its own contract test instance. |
| OQ-1 | Should the M6 keystroke-count upper bound be 40 (this plan), 35 (outcome-kpis.md typical), or be expressed as `35 + tolerance` to absorb terminal-side reflow events? | RECOMMENDATION: 40 as the test assertion (with 8 keys of headroom over the 32-key typical). Wave-decisions captures this; DELIVER may tighten after first measurement against real input traces. |
| OQ-2 | Should AC-15 (HF cache read-only) extend to PARTIAL read-only (cache root writable but one repo dir read-only)? | RECOMMENDATION: out of scope for v1. AC-15 is "entire cache read-only" per requirements.md. The per-file permission-denied case (M4 third scenario) covers a single read-only subdir within an otherwise-writable cache. |
