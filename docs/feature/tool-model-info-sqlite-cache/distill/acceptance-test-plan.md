# Acceptance Test Plan — tool-model-info-sqlite-cache

**Wave:** DISTILL (5 of 6) — brownfield extension of `modeltap-tui`
**Author:** Quinn (nw-acceptance-designer)
**Date:** 2026-05-17
**Authoritative inputs:**
- DISCUSS: `docs/feature/tool-model-info-sqlite-cache/discuss/{user-stories.md,acceptance-criteria.md,requirements.md,journey-info-and-cache.feature,outcome-kpis.md,shared-artifacts-registry.md,prioritization.md}`
- DESIGN: `docs/feature/tool-model-info-sqlite-cache/design/{architecture-design.md,component-boundaries.md,data-models.md,technology-stack.md}` and `docs/adrs/{ADR-015,ADR-016,ADR-017,ADR-018}.md`
- Project convention: `docs/feature/modeltap-tui/distill/{acceptance-test-plan.md,step-definitions-skeleton.md,plugin-contract-spec.md,features/master-acceptance.feature}`
- Sibling convention: `docs/feature/folder-group-bulk-delete/distill/{acceptance-test-plan.md,step-definitions-skeleton.md,plugin-contract-spec.md,wave-decisions.md,acceptance-review.md,features/*.feature}`
- Project `CLAUDE.md`

This plan is **additive** to the parent's acceptance-test-plan. It specifies only the deltas required for US-21..US-27 plus the nine cross-feature integration ACs INT-INFO-1..9. The parent's framework, fixture strategy, headless-mode contract, env-var contract, and step-organization layout are inherited unchanged.

---

## 1. Test Framework — Inherited

**Same as parent and sibling.** `cucumber-rs` for Gherkin-driven E2E; `assert_cmd` for the binary; `tempfile::TempDir` for the per-scenario root; `insta` for snapshots; `serde_json` for JSONL log parsing; `predicates` for substring matches; `expectrl` reserved for the rare `@interactive` scenarios.

**New for this feature:** `rusqlite` in scope as a **test-only verification dependency** of the acceptance crate (`tests/Cargo.toml`'s `[dev-dependencies]`). Step assertions that need to verify the cache file's schema version or row count read it directly via a read-only `rusqlite::Connection` — this is acceptable because the acceptance crate is the COMPOSITION-ROOT consumer of `modeltap-store` in the test harness; the production code path through `Cache::open()` is also exercised. Step assertions that read the cache directly are tagged `@cache-introspection` and limited to the minimum needed to prove the contract (e.g., "`PRAGMA user_version` equals 1 after migration").

**Test pyramid (inherited verbatim from parent, extended for this feature):**

```
                      ┌─────────────────────────────────────────────────┐
                      │  Acceptance (E2E)  cucumber-rs + assert_cmd     │  ~ 30 new scenarios
                      │  drives real `modeltap` binary against          │
                      │  fixture-populated temp dirs + real cache.sqlite │
                      └─────────────────────────────────────────────────┘
                ┌─────────────────────────────────────────────────────────────┐
                │  Plugin contract tests  per-plugin tests/inspect_contract.rs │  ~ 6 cases × 4 plugins
                │  parameterized over T: Tool from modeltap-core               │
                └─────────────────────────────────────────────────────────────┘
        ┌─────────────────────────────────────────────────────────────────────────────┐
        │  modeltap-store internal tests  crates/modeltap-store/tests/*.rs            │  ~ 12 tests
        │  corruption.rs, migration.rs, revalidate.rs, concurrent.rs, sha256_writeback│
        └─────────────────────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│  Unit tests (modeltap-core + modeltap-store pure)  pure-function unit tests + proptest      │  many
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

The acceptance designer (this wave) owns the top layer. DELIVER's software-crafter owns the lower three. The plugin contract spec (`plugin-contract-spec.md`) describes the inspect contract every plugin must satisfy.

---

## 2. Test Layer Map — US-21..US-27 + INT-INFO-*

| Layer | Mechanism | What this feature adds | Owns |
|---|---|---|---|
| **A — E2E acceptance** | cucumber-rs runs the 7 `.feature` files; steps drive `modeltap` binary in headless mode against `tempfile::TempDir`-built fixture trees AND a real on-disk `cache.sqlite` per scenario | 40 scenarios across 7 files (see wave-decisions.md §D11 + §6 below) | DISTILL writes scenarios; DELIVER writes step defs |
| **B — Plugin contract** | One new parameterized test `crates/modeltap-core/tests/inspect_contract.rs` extending the parent's `plugin_contract` harness with the `inspect_tool` + `inspect_model` contract | HF, Ollama, LM Studio: contract path (b) — honor inspect invariants. llama-cli, atomic-chat, gpt4all: contract path (a) — return `Err(InspectError::Unsupported)`. See `plugin-contract-spec.md`. | DISTILL writes the spec; DELIVER implements |
| **C — modeltap-store internals** | `crates/modeltap-store/tests/*.rs` integration tests with `tempfile`-backed cache files + `:memory:` opener | corruption.rs (4 modes), migration.rs (forward matrix), revalidate.rs (drift quad), concurrent.rs (WAL + busy_timeout), sha256_writeback.rs (R3) | DELIVER's software-crafter writes per inner-loop TDD |
| **D — Unit (modeltap-core + modeltap-store pure)** | Standard `#[test]` against pure functions + proptest invariants | Pure inspect-result construction (`ToolDetail`, `ModelDetail` builders); pure validity-quad comparison (`FileStat::matches`); pure TTL eligibility (`CachedTool::is_ttl_eligible(now, ttl_secs)`) | DELIVER's software-crafter writes per inner-loop TDD |

### Story → Layer coverage matrix

| Story | E2E (A) | Contract (B) | Store-internals (C) | Unit (D) |
|---|:---:|:---:|:---:|:---:|
| US-21 | ✓ (5 scenarios) | ✓ (inspect_tool contract) | | ✓ (ToolDetail builders) |
| US-22 | ✓ (5 scenarios) | ✓ (inspect_model contract) | | ✓ (ModelDetail builders) |
| US-23 | ✓ (walking-skeleton + 11 in cache-state-model + 2 in integration) | | ✓ (corruption.rs, migration.rs, concurrent.rs) | ✓ (FileStat, CacheOpenResult variants) |
| US-24 | ✓ (4 scenarios) | | | ✓ (provenance formatting) |
| US-25 | ✓ (walking-skeleton + cache-state-model warm-start scenarios) | | ✓ (concurrent.rs read paths) | ✓ (TTL eligibility) |
| US-26 | ✓ (cache-state-model US-26 + integration-checkpoints US-26) | | ✓ (revalidate.rs quad detection) | ✓ (ValidationResult enum) |
| US-27 | ✓ (3 scenarios in sha256-persistence; `@release-3 @skip` until R3) | | ✓ (sha256_writeback.rs, R3) | ✓ (cache_sha256 row builders, R3) |
| INT-INFO-1..9 | ✓ (integration-checkpoints 7 scenarios) | | | |

Every per-story AC and every INT-INFO-* AC traces to at least one Layer A scenario. See §6 below for the full AC traceability matrix.

---

## 3. Fixture Strategy

### Principle (inherited): synthetic-but-realistic temp trees via `tempfile::TempDir`

Per parent convention. Each scenario builds:

1. A per-scenario root directory `${TMPDIR}/modeltap-test-${SCENARIO_ID}/`.
2. Underneath: tool fixture trees (`ollama/`, `hf/`, `lm-studio/`, `llms/`), an `xdg-data/modeltap/` directory for cache, and a `modeltap-home/` directory for `~/.modeltap/diagnostics.log`.
3. The binary is launched with `MODELTAP_CACHE_PATH=${ROOT}/xdg-data/modeltap/cache.sqlite` and `MODELTAP_LOG_DIR=${ROOT}/modeltap-home`.

### Named fixture trees this feature ADDS (parent's fixtures still reused for US-21 / US-22 plugin-native scenarios)

| Name | Contents | Used by |
|---|---|---|
| `devon-cache-empty` | All fixture tool dirs populated with the parent's `devon-multi-tool` content. NO `cache.sqlite` exists. | walking-skeleton; cache-state-model cold-start |
| `devon-cache-warm` | Same as `devon-cache-empty` plus a pre-populated `cache.sqlite` written by a prior `modeltap` launch (built by the fixture script via running `modeltap --no-tui --warm-cache-seed` in fixture-build mode). Cache age is configurable per scenario via `MODELTAP_CACHE_AGE_OVERRIDE=<seconds>` to drive TTL eligibility. | cache-state-model warm-start; manual-refresh; sha256-persistence (R3) |
| `devon-cache-corrupt` | Cache file present but contains 16 KB of random bytes. `Cache::open()` returns `SQLITE_CORRUPT`. | cache-state-model corruption recovery |
| `devon-cache-future-v` | Cache file present with `PRAGMA user_version = 99` (a future schema version). | cache-state-model downgrade recovery |
| `devon-cache-old-v` | Cache file present with `PRAGMA user_version = 0` (pre-migration). | cache-state-model forward-migration |
| `devon-cache-stale-tool` | `cache.sqlite` populated; one tool's `cache_tools.last_scan_at` set to >24h ago to trigger per-tool TTL cold-start while other tools paint from cache. | cache-state-model mixed warm/cold |
| `devon-mistral-gguf` | Single GGUF v3 file with a real (minimal) GGUF header containing `general.architecture=llama`, `general.quantization_version=2`, `llama.context_length=32768`, `llama.embedding_length=4096`. Sparse padding to 4.4 GB. | model-detail US-22 GGUF metadata |
| `devon-mistral-corrupt-gguf` | Truncated GGUF: 100 bytes containing the magic only, then truncated. | model-detail US-22 introspection failure |
| `devon-hf-with-config-json` | HF cache repo with a `config.json` containing `model_type=llama`, `architectures=["LlamaForCausalLM"]`, `hidden_size=4096`, `num_attention_heads=32`, `num_hidden_layers=32`. | model-detail US-22 HF metadata |
| `devon-ollama-manifest` | `~/.ollama/models/manifests/registry.ollama.ai/library/llama3/8b-instruct-q4_K_M` with a real Ollama manifest JSON containing `config.architecture`, `parameters`, `template`. | model-detail US-22 Ollama metadata |
| `devon-tool-error-ollama` | Ollama fixture dir with `manifests/` set to mode 0000 (permission denied on read). | tool-detail US-21 error scenario |
| `devon-llamacli-userconfig` | llama-cli fixture with `~/.modeltap/config.toml` containing `[plugins.llama-cli] search_paths = ["/data/models"]`; `/data/models` is created as a subdirectory of the per-scenario tempdir. | tool-detail US-21 search-paths |
| `devon-cache-mtime-drift` | Cache present and warm; one model file's mtime is mutated after the cache was written (via `filetime::set_file_mtime`) before the scenario presses `[u]`. | cache-state-model + integration-checkpoints pre-mutate drift |
| `devon-cache-file-gone` | Cache present and warm; one model file deleted after cache write. | cache-state-model + integration-checkpoints pre-mutate gone |

Plus the parent's `devon-multi-tool`, `devon-empty`, `devon-only-ollama`, `devon-permission-denied` fixtures and the sibling's `devon-hf-allunique`, `devon-hf-mixed` (reused unchanged).

### Env vars added by this feature

| Env | Purpose |
|---|---|
| `MODELTAP_CACHE_PATH` | Overrides `dirs::data_dir().join("modeltap/cache.sqlite")` for the launch. Honored per ADR-015 §4 / C-INFO-5. SET in every scenario. |
| `MODELTAP_CACHE_AGE_OVERRIDE` | Test-only. Sets `cache.tools.last_scan_at` to `now() - <seconds>` after the fixture writes the cache, used to drive per-tool TTL eligibility tests. Gated by `cfg(any(test, feature = "test-harness"))`. |
| `MODELTAP_OLLAMA_API_URL` | Overrides the production `http://localhost:11434/api/version` lookup; default unset = real localhost. Acceptance scenarios set this to a fake `http://127.0.0.1:<port>` stub OR (DELIVER's call per `wave-decisions.md` §D12) skip the HTTP call entirely via a parallel `MODELTAP_OLLAMA_VERSION=<n>` env var. |
| `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS` | Test-only. When set on the binary, causes the per-tool reconcile write transaction to sleep `N` milliseconds before COMMIT, used by the concurrent-write contention scenario. Gated by `cfg(any(test, feature = "test-harness"))`. |
| `XDG_DATA_HOME` | Standard XDG env var. Used by exactly ONE scenario (the `dirs::data_dir()` resolution proof) to verify the production resolver lands the cache file at `${XDG_DATA_HOME}/modeltap/cache.sqlite`. |

Inherited from parent (unchanged use): `MODELTAP_HEADLESS=1`, `MODELTAP_LOG_DIR`, `MODELTAP_OLLAMA_DIR`, `MODELTAP_LLAMACLI_DIRS`, `HF_HOME`, `MODELTAP_LMSTUDIO_DIR`, `MODELTAP_LSOF`, `NO_COLOR=1`.

### The walking-skeleton in-process `TestTool`

The walking-skeleton scenario uses an in-process `TestTool` plugin (not one of the four real production plugins) to keep the seam tight on cache wiring rather than plugin-specific introspection. The `TestTool`:

- Implements `Tool` from `modeltap-core` with all 9 methods (existing 7 + `inspect_tool` + `inspect_model`).
- `discover()` returns one model file at a path the harness writes ahead of time into `${ROOT}/test-tool/models/`.
- `inspect_tool()` returns `Ok(ToolDetail { detected_version: Some("test-1.0.0".into()), ... })`.
- `inspect_model()` returns `Ok(ModelDetail { metadata_kv: { "test.kind" → "synthetic" }, ... })`.
- Lives in `tests/src/test_tool.rs` (a NEW module in the acceptance crate). Registered into the orchestrator's `Vec<Box<dyn Tool>>` via a `MODELTAP_TEST_PLUGINS=test-tool` env var that the binary honors only in `cfg(any(test, feature = "test-harness"))` builds.

This is the **walking-skeleton-only** seam. Every other scenario uses real production plugins.

---

## 4. Walking Skeleton — Strategy Declaration

**Strategy: B (real I/O against fixture-populated temp dirs).** Declared in `wave-decisions.md` §D5. Inherits from parent and sibling.

For this feature, the walking skeleton:

1. Runs the real `modeltap` binary in headless mode (`MODELTAP_HEADLESS=1`).
2. Drives input through the `--script` mechanism (parent contract).
3. Reads from a **real** `tempfile::TempDir` fixture (`devon-cache-empty`).
4. Registers the **real** in-process `TestTool` plugin (via `MODELTAP_TEST_PLUGINS=test-tool`).
5. Performs cold-start discovery; writes one model row to a **real on-disk** `cache.sqlite` at `MODELTAP_CACHE_PATH`.
6. Quits process A.
7. Launches process B against the same `MODELTAP_CACHE_PATH`.
8. Asserts:
   - `cache.sqlite` exists on disk (filesystem read).
   - `PRAGMA user_version == 1` (read via test-only `rusqlite::Connection`).
   - `cache_models` contains exactly 1 row matching the TestTool's model (read via test-only `rusqlite::Connection`).
   - Process B's headless TUI frame contains the model's display name (read from captured TestBackend frame).
   - Process B's JSONL log shows `launch.warm_paint_ms <= 150` (K-INFO-1).

**Litmus test (Dim 9d):** if we deleted the real `modeltap-store` adapter and substituted an `InMemoryCache` that lives only in process A's memory, would the walking skeleton still pass? **NO.** Process B starts with no in-memory state; it must read the cache file from disk. The walking skeleton's assertions read `cache.sqlite` existence, the `PRAGMA user_version` value, and the row presence — all of which fail without a real on-disk SQLite file. Strategy B is honored.

**Adapter integration coverage (Dim 9c):** the walking skeleton covers `Cache::open`, `Cache::write_tool`, `Cache::write_models`, `Cache::tools`, `Cache::models_for_tool`, the path resolver (`dirs::data_dir()` is short-circuited by `MODELTAP_CACHE_PATH` in the walking-skeleton; a separate `cache-state-model.feature` scenario exercises the unconfigured resolver path), the `Migrator` (v0 → v1), and the in-process `TestTool`'s `Tool::inspect_*` defaults via the `TestTool`'s real overrides.

Adapters NOT covered by the walking skeleton (covered elsewhere): `OllamaPlugin::inspect_*` (covered by `tool-detail.feature` + `model-detail.feature` Ollama scenarios), `HfPlugin::inspect_*` (HF scenarios), `LmStudioPlugin::inspect_*` (LM Studio scenario), `Cache::verify_against_fs` (covered by every `@us-26` scenario in `cache-state-model.feature` and the destructive scenarios in `integration-checkpoints.feature`), corruption recovery (covered by the dedicated corruption-recovery scenarios in `cache-state-model.feature`).

---

## 5. Test Execution Model

### Local developer

```bash
# Build fixtures (idempotent; extends parent's build.sh).
./tests/fixtures/build.sh devon-cache-empty devon-cache-warm devon-cache-corrupt devon-cache-future-v devon-cache-old-v devon-cache-stale-tool devon-mistral-gguf devon-mistral-corrupt-gguf devon-hf-with-config-json devon-ollama-manifest devon-tool-error-ollama devon-llamacli-userconfig devon-cache-mtime-drift devon-cache-file-gone

# Run the walking skeleton only.
cargo test --test acceptance -- --tag '@walking-skeleton and @us-21-cache'

# Run all this-feature scenarios.
cargo test --test acceptance -- --tag '@us-21 or @us-22 or @us-23 or @us-24 or @us-25 or @us-26'

# Run only Release-1 scenarios (US-21, US-22 inspection — ships first).
cargo test --test acceptance -- --tag @release-1

# Run only Release-2 scenarios (cache infra).
cargo test --test acceptance -- --tag @release-2

# Skip Release-3 / @skip scenarios in CI default.
cargo test --test acceptance -- --skip-tag @skip
```

### CI

The existing `ci.yml` "test" job runs `cargo test --workspace --locked`. It picks up the new scenarios automatically. Two notes:

1. The `concurrent.rs` tests in `crates/modeltap-store/tests/` exercise two real `modeltap` processes via `assert_cmd`; CI must allow ≥2 parallel test jobs without exhausting file descriptors.
2. The `@perf` tag (K-INFO-1, K-INFO-7) scenarios run in a dedicated `cargo test --release -p modeltap-acceptance -- --tag @perf` invocation, NOT in the default `cargo test` debug-build run. Debug-build latencies routinely exceed the 150 ms warm-paint budget; the assertion would be a false positive.

### Smoke-test fast path

Per Quinn's protocol: fast path applies when total scenarios ≤ 3. This feature has 40 scenarios. **Full review pass + per-scenario fixture isolation applies.**

---

## 6. AC Traceability Matrix

Every US-21..US-27 AC and every INT-INFO-* AC must trace to at least one scenario tag.

### Per-story ACs

| AC | Tag(s) on scenario | Feature file |
|---|---|---|
| AC-21-1 (Enter opens tool detail < 100 ms) | `@us-21 @ac-21-1 @perf @k-info-1-warm-100ms` | tool-detail.feature |
| AC-21-2 (detail fields) | `@us-21 @ac-21-2` | tool-detail.feature |
| AC-21-3 (Option<String> version) | `@us-21 @ac-21-3` | tool-detail.feature |
| AC-21-4 (Last error field) | `@us-21 @ac-21-4` | tool-detail.feature |
| AC-21-5 (search-paths provenance) | `@us-21 @ac-21-5` | tool-detail.feature |
| AC-21-6 (`[r]` re-runs discovery) | `@us-21 @ac-21-6` | tool-detail.feature |
| AC-21-7 (`[Esc]` cursor preserved) | `@us-21 @ac-21-7` (implicit in every US-21 scenario; explicit in one) | tool-detail.feature |
| AC-21-8 (bottom-bar shortcuts on detail screen) | `@us-21 @ac-21-8` | tool-detail.feature |
| AC-21-9 (plugin panic isolation) | `@us-21 @ac-21-9 @int-info-8` | integration-checkpoints.feature |
| AC-22-1 (Enter opens model detail < 100 ms cached) | `@us-22 @ac-22-1 @perf @k-info-1-warm-100ms` | model-detail.feature |
| AC-22-2 (re-introspect < 1 s) | `@us-22 @ac-22-2 @perf` | model-detail.feature |
| AC-22-3..AC-22-5 (Metadata section content) | `@us-22 @ac-22-3 @ac-22-4 @ac-22-5` | model-detail.feature |
| AC-22-6 (BTreeMap<String,String>) | covered by plugin-contract spec § 3.B (no Layer A scenario; type-shape is plugin-contract concern) | plugin-contract-spec.md |
| AC-22-7 (un-introspectable degrades) | `@us-22 @ac-22-7` | model-detail.feature |
| AC-22-8 (`[r]` re-introspect) | `@us-22 @ac-22-8` | model-detail.feature |
| AC-22-9 (`[Esc]` cursor preserved) | `@us-22 @ac-22-9` (implicit; one explicit scenario) | model-detail.feature |
| AC-22-10 (bottom-bar shortcuts) | `@us-22 @ac-22-10` | model-detail.feature |
| AC-23-1 (cache path via `dirs::data_dir`; MODELTAP_CACHE_PATH override) | `@us-23 @ac-23-1` | cache-state-model.feature |
| AC-23-2 (WAL + busy_timeout) | `@us-23 @ac-23-2` + asserted implicitly by concurrent scenarios | cache-state-model.feature |
| AC-23-3 (`PRAGMA user_version` checked) | `@us-23 @ac-23-3` + walking-skeleton | walking-skeleton.feature, cache-state-model.feature |
| AC-23-4 (forward migrations) | `@us-23 @ac-23-4` | cache-state-model.feature |
| AC-23-5 (downgrade rename) | `@us-23 @ac-23-5` | cache-state-model.feature |
| AC-23-6 (SQLITE_CORRUPT rename) | `@us-23 @ac-23-6 @k-info-4-recovery-100` | cache-state-model.feature |
| AC-23-7 (recovery banner) | `@us-23 @ac-23-7` (asserted in AC-23-5 and AC-23-6 scenarios) | cache-state-model.feature |
| AC-23-8 (`--no-cache` zero bytes) | `@us-23 @ac-23-8 @int-info-5` | cache-state-model.feature, integration-checkpoints.feature |
| AC-23-9 (config `cache.enabled = false`) | `@us-23 @ac-23-9 @int-info-5` | cache-state-model.feature |
| AC-23-10 (two-process WAL) | `@us-23 @ac-23-10 @concurrent` | cache-state-model.feature |
| AC-23-11 (cache never blocks launch) | walking-skeleton implicitly proves; all `@k-info-4-recovery-100` scenarios assert | cache-state-model.feature |
| AC-23-12 (cache stays local) | code review concern; no Layer A scenario required | (out-of-scope for E2E) |
| AC-24-1 (provenance always shown) | `@us-24 @ac-24-1` | manual-refresh.feature |
| AC-24-2 (reconciling... suffix) | `@us-24 @ac-24-2` (asserted in `@ac-24-1` scenario) | manual-refresh.feature |
| AC-24-3 (`[r]` per-tool reconcile) | `@us-24 @ac-24-3 @k-info-2-refresh-1s @perf` | manual-refresh.feature |
| AC-24-4 (`[Shift+R]` all parallel) | `@us-24 @ac-24-4 @perf` | manual-refresh.feature |
| AC-24-5 (no-op when dialog open) | `@us-24 @ac-24-5` | manual-refresh.feature |
| AC-24-6 (bottom-bar shortcuts) | covered by parent US-08 invariant + INT-INFO-2 | integration-checkpoints.feature |
| AC-24-7 (provenance update post-refresh) | `@us-24 @ac-24-7` (asserted in AC-24-3 scenario) | manual-refresh.feature |
| AC-24-8 (cache.tools.last_scan_at update) | `@us-24 @ac-24-8 @cache-introspection` (asserted in AC-24-3) | manual-refresh.feature |
| AC-24-9 (refresh latency ≤ 1 s) | `@us-24 @k-info-2-refresh-1s @perf` (asserted in AC-24-3) | manual-refresh.feature |
| AC-25-1 (warm-start ≤ 100 ms) | `@us-25 @ac-25-1 @k-info-1-warm-100ms @k3a-warm-paint @perf` + walking-skeleton | walking-skeleton.feature, cache-state-model.feature |
| AC-25-2 (per-tool TTL) | `@us-25 @ac-25-2` | cache-state-model.feature |
| AC-25-3 (cold-start preserves parent K3) | `@us-25 @ac-25-3 @k3b-cold-start @perf` | cache-state-model.feature |
| AC-25-4 (mixed warm/cold per-tool) | `@us-25 @ac-25-4` | cache-state-model.feature |
| AC-25-5 (provenance from MAX(last_scan_at)) | asserted in AC-25-1 + AC-25-4 scenarios | cache-state-model.feature |
| AC-25-6 (transient I/O falls back to cold-start) | `@us-25 @ac-25-6 @infrastructure-failure` | cache-state-model.feature |
| AC-25-7 (--no-cache skips warm-paint) | `@us-25 @ac-25-7 @int-info-5` | integration-checkpoints.feature |
| AC-26-1 (background reconcile runs) | `@us-26 @ac-26-1` (implicit in walking-skeleton + every warm-start scenario) | walking-skeleton.feature |
| AC-26-2 (atomic per-tool write) | `@us-26 @ac-26-2 @cache-introspection` | cache-state-model.feature |
| AC-26-3 (failed reconcile preserves last-known-good) | `@us-26 @ac-26-3 @infrastructure-failure` | cache-state-model.feature |
| AC-26-4 (silent ack indicator) | `@us-26 @ac-26-4` | cache-state-model.feature |
| AC-26-5 (pre-mutate revalidation: stat quad) | `@us-26 @ac-26-5 @int-info-4` (asserted in drift + gone scenarios) | cache-state-model.feature, integration-checkpoints.feature |
| AC-26-6 (drift → re-introspect) | `@us-26 @ac-26-6` | cache-state-model.feature |
| AC-26-7 (gone → abort + refresh) | `@us-26 @ac-26-7` | cache-state-model.feature |
| AC-26-8 (every mutation site guarded) | architecture-lint R9 (DELIVER-owned); not a Layer A scenario | (out-of-scope for E2E) |
| AC-26-9 (reconcile ≤ 1.15 s) | `@us-26 @perf` (asserted in AC-26-1 scenario) | cache-state-model.feature |
| AC-27-1..AC-27-8 | `@us-27 @release-3 @skip` (3 scenarios) | sha256-persistence.feature |

### Cross-feature integration ACs

| INT-INFO | Tag | Where |
|---|---|---|
| INT-INFO-1 (K3a + K3b sub-KPIs) | `@int-info-1 @k3a-warm-paint @k3b-cold-start` | integration-checkpoints.feature |
| INT-INFO-2 (keymap SHORTCUT_TABLE) | covered by parent's US-08 invariant; one scenario re-asserts dim/bright discipline for the new `[r]`/`[Shift+R]`/`[Enter]` entries | integration-checkpoints.feature |
| INT-INFO-3 (total == sum(tool.disk_usage) during reconcile) | `@int-info-3 @property` | integration-checkpoints.feature |
| INT-INFO-4 (every destructive action runs revalidator) | `@int-info-4 @us-26` | integration-checkpoints.feature |
| INT-INFO-5 (--no-cache true bypass) | `@int-info-5 @ac-23-8 @us-23` | cache-state-model.feature, integration-checkpoints.feature |
| INT-INFO-6 (--version succeeds with corrupted cache) | `@int-info-6 @infrastructure-failure` | integration-checkpoints.feature |
| INT-INFO-7 (folder-group-bulk-delete `[F]` runs revalidator) | `@int-info-7 @us-26 @us-05c` | integration-checkpoints.feature |
| INT-INFO-8 (plugin panic in inspect_* caught) | `@int-info-8 @us-21 @ac-21-9 @plugin-trait` | integration-checkpoints.feature |
| INT-INFO-9 (vocabulary consistency) | covered by code review + parent invariant; one scenario asserts a sample of the new terms appears in TUI help output | integration-checkpoints.feature |

**Coverage gate:** every AC and every INT-INFO has at least one scenario tag (or is explicitly out-of-scope for E2E with the alternative coverage cited). Zero blocker findings on critique-dimension 4 (Coverage Completeness) and 8a (Story-to-Scenario mapping).

---

## 7. What DELIVER Inherits From This Plan

1. **Feature files** (7 total under `features/`):
   - `walking-skeleton.feature` (1 scenario; the WS exit gate)
   - `cache-state-model.feature` (11 scenarios; US-23 / US-25 / US-26)
   - `tool-detail.feature` (5 scenarios; US-21)
   - `model-detail.feature` (5 scenarios; US-22)
   - `manual-refresh.feature` (4 scenarios; US-24)
   - `sha256-persistence.feature` (3 scenarios; US-27; all `@release-3 @skip`)
   - `integration-checkpoints.feature` (7 scenarios; INT-INFO-1..9)
2. **Step-definition skeleton** — `step-definitions-skeleton.md` (deltas vs parent only).
3. **Plugin-contract spec** — `plugin-contract-spec.md` (the `inspect_tool` + `inspect_model` contract for the 6 plugins).
4. **Fixture inventory** — 14 new named fixtures listed in §3.
5. **Env-var contract delta** — `MODELTAP_CACHE_PATH`, `MODELTAP_CACHE_AGE_OVERRIDE`, `MODELTAP_OLLAMA_API_URL`, `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS`, `MODELTAP_TEST_PLUGINS=test-tool`.
6. **Strategy declaration** — Strategy B; walking skeleton uses real `modeltap-store` + real tempdir filesystem + in-process `TestTool` plugin (walking-skeleton ONLY).
7. **Walking-skeleton scenario** — the one in `walking-skeleton.feature`.

---

## 8. What DELIVER Decides

- The exact step-definition Rust code (this wave specifies what each step asserts; DELIVER writes the Rust).
- The `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS` implementation (cfg-gated; D8 in wave-decisions.md).
- The exact GGUF-header-parser path (use the `gguf` crate vs. hand-rolled minimal parser; ADR-016 implementation-guidance).
- The exact Ollama-version detection HTTP path (with/without the `MODELTAP_OLLAMA_API_URL` stub seam; D12 in wave-decisions.md).
- The exact `TestTool` implementation shape in the acceptance crate (this spec asserts what the WS exercises; DELIVER writes the trait impl).
- The fixture-builder script additions for the 14 new named fixtures (parent owns `tests/fixtures/build.sh`).

---

## 9. Mandate Compliance Notes (CM-A through CM-D)

### CM-A — Hexagonal boundary (driving ports only)

All E2E scenarios invoke through the `modeltap` binary (the driving port). The Plugin Contract test (Layer B) invokes `Box<dyn Tool>::inspect_*` — the public port for plugin authors per ADR-001 / ADR-016. **No scenario imports `modeltap-store::*` directly** except for the `@cache-introspection` step assertions that open a READ-ONLY `rusqlite::Connection` to verify `PRAGMA user_version` and row counts — this is a test-utility, not a production-code import, and is gated to the acceptance crate's `[dev-dependencies]`.

Verified by inspection of `features/*.feature` and `step-definitions-skeleton.md`.

### CM-B — Business language

Step phrases in Gherkin use Devon's vocabulary: "Devon runs `modeltap`", "the cache contains the inventory from the previous launch", "the summary bar reads", "the recovery banner appears", "Devon presses Shift+R", "the cache was corrupted at byte 4096", "the model file no longer exists". Technical terms confined to:

- `PRAGMA user_version`, `WAL`, `busy_timeout`, `SQLITE_CORRUPT` — these ARE the user-facing log lines and recovery-banner contents per ADR-015 §5 and the journey draft.
- `cache.sqlite`, `.corrupt-<timestamp>`, `.future-version-<n>` — these ARE the user-visible filenames in the recovery banner.
- `(mtime, size, inode, dev)` — these ARE the cache safety contract per ADR-015 §3, surfaced verbatim in the journey and in any error message when the revalidator aborts.
- `JSONL log "launch.warm_paint_ms"` — instrumentation step (parent contract).
- `Tool::inspect_tool`, `InspectError::Unsupported` — these are the public ADR-016 type names plugin authors program against (Riley persona, US-18). Same convention as the sibling feature's `Tool::delete_folder` references.

Forbidden terms in step text (DELIVER must not introduce): `HTTP` (except in the one Ollama-API-version scenario where it IS the integration), `database`, `endpoint`, `function`, `method call`, `class`, `assert_eq!`, `unwrap`. Verified by grep over the final feature files.

### CM-C — User journey completeness

Walking skeleton traces a complete user journey: launch → discover → write to cache → quit → relaunch → cached inventory paints → user sees the model in the right pane. Every focused scenario asserts what Devon SEES (right-pane content, summary-bar text, recovery banner text, dialog text), not what the system internally computes. The pre-mutate revalidation scenarios assert observable consequences (action proceeds / aborts / refreshes), not internal call counts.

### CM-D — Pure function extraction

The DESIGN (per `architecture-design.md` §5) already separates pure logic from impure I/O:

| Component | Purity | Where exercised |
|---|---|---|
| `modeltap-core::domain::inspect::{ToolDetail, ModelDetail}` constructors | PURE — pure data | DELIVER's unit tests under `modeltap-core/tests/` |
| `modeltap-store::types::FileStat::matches(&FileStat)` (validity quad) | PURE — pure compare | DELIVER's unit test in `modeltap-store/tests/revalidate.rs` |
| `CachedTool::is_ttl_eligible(now, ttl_secs)` (TTL check) | PURE — pure arithmetic | DELIVER's unit test |
| `Cache::open` (SQLite lifecycle) | IMPURE — file I/O | Layer A walking skeleton + every `@release-2` scenario |
| `Cache::verify_against_fs` (revalidator) | IMPURE — `std::fs::metadata()` | Layer A drift/gone scenarios + `modeltap-store/tests/revalidate.rs` |
| `Migrator::to_latest` (migration runner) | IMPURE — SQL writes | Layer A migration scenario + `modeltap-store/tests/migration.rs` |
| Per-plugin `inspect_*` overrides | IMPURE — file/HTTP I/O | Layer B plugin contract + per-plugin Layer A scenarios |

The acceptance tests (Layer A) exercise impure I/O through the binary. DELIVER's unit tests (Layer D) exercise pure logic directly without fixtures. **No fixture parametrization at the acceptance layer beyond the named-fixture-tree level**, which IS the adapter parametrization per the mandate.

Pure functions inventory (for DELIVER's unit-test plan):

| Function | Location | Pure? |
|---|---|---|
| `FileStat::matches(&Self) -> bool` | `modeltap-store::types` | YES — pure compare |
| `CachedTool::is_ttl_eligible(now, ttl_secs)` | `modeltap-store::types` | YES — pure arithmetic |
| `ToolDetail::with_undetectable_version()` builder | `modeltap-core::domain::inspect` | YES — constructor |
| `ModelDetail::with_introspection_failure(detail)` builder | `modeltap-core::domain::inspect` | YES — constructor |
| `format_provenance(now, last_scan_at) -> String` ("just now", "<N> min ago", etc.) | `modeltap-tui::view::provenance` | YES — pure formatter |
| `compute_inventory_diff(cached, fresh) -> InventoryDiff` (silent-ack trigger) | `modeltap-core::logic` | YES — pure comparison |

Impure-but-isolated adapters:

| Adapter | Location | Driven by |
|---|---|---|
| `Cache::open` | `modeltap-store::open` | real on-disk SQLite + tempdir |
| `Cache::verify_against_fs` | `modeltap-store::revalidate` | real `std::fs::metadata()` against tempdir paths |
| `Migrator::to_latest` | `modeltap-store::migrate` | real rusqlite_migration on tempdir DB |
| `OllamaPlugin::inspect_tool` | `plugins/ollama::inspect` | real HTTP localhost (or stubbed via `MODELTAP_OLLAMA_API_URL`) |
| `HfPlugin::inspect_model` | `plugins/hf::inspect` | real `config.json` read from tempdir |
| `LmStudioPlugin::inspect_model` | `plugins/lm-studio::inspect` | real GGUF header / `model.json` read |
| `OllamaPlugin::inspect_model` | `plugins/ollama::inspect` | real manifest JSON read |

---

## 10. Risks and Open Questions

| # | Item | Mitigation |
|---|---|---|
| R1 | The walking skeleton's in-process `TestTool` is novel — it differs from the parent's pattern (which uses real plugins against fixture trees). Risk: DELIVER may misinterpret the seam. | Mitigation: `wave-decisions.md` §D5 + this plan §4 + the WS scenario itself are explicit; the `TestTool` lives in the acceptance crate only and is registered via `MODELTAP_TEST_PLUGINS=test-tool`. Production builds (`cargo build --release`) do not include the `TestTool`. |
| R2 | Concurrent-process scenarios (US-23 Scenarios 4, 5) require launching two real `modeltap` processes. On macOS CI, the per-binary Gatekeeper scan tax (per CLAUDE.md) doubles. | Mitigation: documented in CLAUDE.md "Running Tests Fast on macOS"; CI users should add their terminal to System Settings → Developer Tools. The concurrent scenarios are tagged `@concurrent`; CI may run them in a serial test job to avoid file-descriptor exhaustion. |
| R3 | `MODELTAP_CACHE_AGE_OVERRIDE` env-var seam is a test-only code path in `modeltap-store`. Risk: leakage into release builds. | Mitigation: gated behind `cfg(any(test, feature = "test-harness"))` per the sibling feature's `MODELTAP_TEST_EBUSY_PATHS` precedent. DELIVER decides between `cfg(test)` and the `test-harness` feature flag. |
| R4 | The GGUF-header fixture (`devon-mistral-gguf`) requires writing a real minimal GGUF header. Risk: the binary format is non-trivial. | Mitigation: parent's `tests/fixtures/build.sh` already writes GGUF magic bytes for US-07 / US-16 scenarios; this feature extends with the full KV-table section. DELIVER uses the `gguf` crate or hand-rolls per ADR-016 implementation guidance. |
| R5 | The Ollama HTTP API integration (`http://localhost:11434/api/version`) is real localhost in production. In CI, no Ollama daemon is running. Risk: every US-21 Ollama scenario times out on the HTTP call. | Mitigation: the Ollama plugin's `inspect_tool` MUST honor `MODELTAP_OLLAMA_API_URL` (test stub) OR `MODELTAP_OLLAMA_VERSION=<n>` (env-var short-circuit, DELIVER's call per D12). Tests set whichever applies. Timeout is 500 ms per ADR-016 implementation guidance; if no stub is set and localhost is unreachable, the plugin falls back to `detected_version = None` ("not detectable") — which is itself testable (AC-21-3 scenario). |
| R6 | The `cache.sqlite.corrupt-<timestamp>` filename uses a timestamp that varies by test-run. Scenarios match on a regex (per the journey draft). | Mitigation: assertions use `cache\.sqlite\.corrupt-\d{4}-\d{2}-\d{2}T\d{6}` regex. The test harness ignores the exact timestamp; only the filename shape matters. |
| OQ-1 | Should `XDG_DATA_HOME` resolution be tested on macOS (where `dirs::data_dir()` returns `~/Library/Application Support/`)? | RECOMMENDATION: ONE scenario verifies the resolver returns the platform-native path; the assertion checks for either `${XDG_DATA_HOME}/modeltap/cache.sqlite` (Linux) or `~/Library/Application Support/modeltap/cache.sqlite` (macOS) based on the runner's OS. DELIVER's call. |
| OQ-2 | Should the SQLite WAL files (`*.sqlite-wal`, `*.sqlite-shm`) be asserted-on in the `--no-cache` scenario? AC-23-8 says "ZERO bytes written to the cache file or its location." | RECOMMENDATION: yes; the `--no-cache` scenario asserts that `cache.sqlite`, `cache.sqlite-wal`, and `cache.sqlite-shm` all do NOT exist after the launch. Wave-decisions documents. |
| OQ-3 | The `MODELTAP_TEST_PLUGINS=test-tool` seam ships only in `cfg(any(test, feature = "test-harness"))` builds. Risk: confusion about which builds include the test plugin. | RECOMMENDATION: DELIVER documents the seam in `crates/modeltap-app/src/plugin_registry.rs` (or wherever the plugin list is wired). The walking-skeleton step harness sets the env var; no other test scenario does. |
