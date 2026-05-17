# Wave Decisions — tool-model-info-sqlite-cache (DISTILL)

**Wave:** DISTILL (5 of 6) — brownfield extension of `modeltap-tui`; closes per US-21..US-27 + INT-INFO-1..9
**Author:** Quinn (nw-acceptance-designer)
**Date:** 2026-05-17

These are the **acceptance-test-shape** decisions made during DISTILL, inside the locked-upstream constraints from DISCUSS, DESIGN, and the four ADRs (ADR-015, ADR-016, ADR-017, ADR-018). They are **NOT** architectural decisions (those belong in the ADRs). They are the executable-spec decisions that DELIVER's software-crafter inherits.

The four parent decision-points specified by the autonomous-mode handoff (D1..D4) are recorded here as PRE-RESOLVED, with rationale.

---

## D1 — Scope

**Decision: core (new component + 7 user stories, supersedes ADR-003).**

Rationale: closed by parent autonomy. Scope is the seven stories US-21..US-27 plus the nine cross-feature integration ACs INT-INFO-1..9. The DESIGN wave produced four ADRs (ADR-015..ADR-018) and introduced a new `modeltap-store` crate. This distill specifies the executable spec for the whole package; US-27 (Release 3) scenarios ship `@release-3 @skip` per the Release-3 deferral in `prioritization.md`.

---

## D2 — Framework

**Decision: cucumber-rs.**

Locked by parent `modeltap-tui` convention (`docs/feature/modeltap-tui/distill/acceptance-test-plan.md` §1) and re-locked by sibling `folder-group-bulk-delete/distill/acceptance-test-plan.md` §1. No alternative considered. The 28 scenarios in this feature's `journey-info-and-cache.feature` (DISCUSS) and the additional walking-skeleton + per-story + integration scenarios derived in this DISTILL are all written for `cucumber-rs`.

Secondary tools (inherited verbatim from parent): `assert_cmd`, `expectrl` (only for `@interactive`), `insta`, `predicates`, `tempfile`, `serde_json`, `rusqlite` (read-only verification in step assertions — see CM-B exception note).

---

## D3 — Integration

**Decision: real services (real tempdir filesystem + real SQLite + real plugins; no mocks at acceptance level).**

Rationale: closed by parent autonomy. Specifically:

| Service | Mode in acceptance tests |
|---|---|
| Filesystem | Real, via `tempfile::TempDir` per scenario. No `FakeFs` adapter at acceptance level. |
| SQLite | Real `rusqlite::Connection` against the per-scenario tempdir cache file. `Cache::open_in_memory()` is reserved for `modeltap-store` unit tests under `crates/modeltap-store/tests/`, NOT acceptance scenarios. The walking skeleton specifically uses a **real on-disk file** to prove path resolution + WAL init + migrate-v0-to-v1 wiring. |
| Tool plugins | Real `Box<dyn Tool>` instances. For the walking skeleton, an **in-process `TestTool`** is registered into the `Vec<Box<dyn Tool>>` to provide deterministic discovery output without requiring the four real plugin crates' fixture trees. The `TestTool` is a real plugin, not a mock — it implements the trait and writes real `discover()` output backed by the per-scenario tempdir. For the per-story scenarios that exercise plugin-native introspection (US-21, US-22), the real production plugins (`OllamaPlugin`, `HfPlugin`, `LmStudioPlugin`) are used against the parent's named fixture trees (`devon-multi-tool`, etc.). |
| Subprocess | None new. Ollama's `inspect_tool` may call `http://localhost:11434/api/version`; this is gated by `MODELTAP_OLLAMA_API_URL` (test override pointing at a fake-stub HTTP server when needed; default unset = real localhost; see env-var contract). |
| Network | None at acceptance level. Cache stays local per ADR-015 §4. |

**Concurrent-process scenarios (US-23 Scenarios 4, 5; US-26 Scenario 5):** two real `modeltap` processes launched in headless mode against the same per-scenario cache file. Real WAL contention exercised; no mocks.

**Strategy declaration (critique Dim 9a):** Strategy B (real I/O against fixture-populated temp dirs). Inherits from parent and sibling.

---

## D4 — Infrastructure Testing

**Decision: no.**

Rationale: closed by parent autonomy. DEVOPS wave was skipped for this feature (no new CI/CD or deployment changes; the new crate adds <1 MB to binary size and runs in the existing `cargo test --workspace` pipeline). No `environments.yaml` exists for this feature. Per critique Dim 8 Check B, the environments are inherited from the parent `modeltap-tui` feature (`clean`, `with-existing-cache`, `with-corrupted-cache`, `with-stale-config`); they are exercised by the per-story fixture choices rather than enumerated separately.

---

## D5 — Walking Skeleton Strategy

**Decision: Strategy B (real I/O against fixture-populated temp dirs).** Inherits from parent and sibling.

**Walking skeleton scenario** (`features/walking-skeleton.feature`, single scenario): drives the real `modeltap` binary in headless mode, registers an in-process `TestTool` plugin pointed at a tempdir model, performs cold-start discovery, persists to a **real on-disk** `cache.sqlite` (not `:memory:`), quits, relaunches against the same cache, asserts warm-start paint reads the persisted row and renders it in the headless TUI.

**Litmus test (Dim 9d):** if we deleted the real `modeltap-store` adapter and substituted an `InMemoryCache`, would the walking skeleton still pass? **NO.** The skeleton asserts on `cache.sqlite` existing on disk after process A exits, on the `schema_version` PRAGMA value, and on the row reappearing in the headless TUI of process B. An in-memory cache that shares no state across process boundaries fails every one of those assertions.

**Adapter integration coverage (Dim 9c):** the NEW driven adapters in this feature are:

| Adapter | Crate | Real I/O coverage |
|---|---|---|
| `Cache::open` (SQLite file lifecycle) | `modeltap-store` | Walking skeleton + every `@release-2` scenario in `cache-state-model.feature` |
| `Cache::write_tool` / `write_models` / `write_files` | `modeltap-store` | Walking skeleton + `cache-state-model.feature` reconcile/concurrency scenarios |
| `Cache::verify_against_fs` (the revalidator) | `modeltap-store` | Every `@us-26` scenario in `cache-state-model.feature` + every destructive integration scenario in `integration-checkpoints.feature` |
| `Migrator` (rusqlite_migration wrapper) | `modeltap-store` | `cache-state-model.feature` migration-forward + corruption-recovery scenarios |
| `OllamaPlugin::inspect_tool` / `inspect_model` | `plugins/ollama` | `tool-detail.feature` + `model-detail.feature` Ollama scenarios |
| `HfPlugin::inspect_tool` / `inspect_model` | `plugins/hf` | `tool-detail.feature` + `model-detail.feature` HF scenarios |
| `LmStudioPlugin::inspect_model` | `plugins/lm-studio` | `model-detail.feature` LM Studio scenario |
| `dirs::data_dir()` path resolution | `modeltap-app` adapter | `cache-state-model.feature` opt-out / `MODELTAP_CACHE_PATH` override scenarios |

Every NEW adapter has at least one `@real-io @adapter-integration` scenario.

---

## D6 — Cache file path resolution in tests

**Decision: every scenario sets `MODELTAP_CACHE_PATH=${TMPDIR}/modeltap-test-${SCENARIO_ID}/cache.sqlite`.**

The production resolver is `dirs::data_dir().join("modeltap/cache.sqlite")` per ADR-015 §4. Acceptance tests MUST NOT touch the real user data directory. The `MODELTAP_CACHE_PATH` env override (ADR-015 §4 + C-INFO-5) is the canonical test seam. **A single scenario** in `cache-state-model.feature` explicitly exercises the production `dirs::data_dir()` path resolution by setting `XDG_DATA_HOME=${TMPDIR}/xdg-data` and asserting that the cache lands at `${TMPDIR}/xdg-data/modeltap/cache.sqlite` — this proves the production resolver works, but every other scenario short-circuits via `MODELTAP_CACHE_PATH` to keep tests fast and isolated.

---

## D7 — Pre-mutate revalidation seam at the test layer

**Decision: scenarios assert the revalidator is exercised by observing user-visible outcomes, not by inspecting the call graph.**

ADR-015 §3 + architecture-lint R9 (per `architecture-design.md` §5.4) enforce that EVERY destructive code path calls `revalidate::pre_mutate` before mutation. The Layer A acceptance scenarios do NOT assert "R9 holds" directly — that is a unit-test-level lint, owned by DELIVER's `tests/architecture.rs`. Instead, the acceptance scenarios assert the **user-visible consequences** of revalidation:

- **Drift case** (`@us-26 @ac-26-6` scenario in `cache-state-model.feature`): scenario mutates a model file's mtime between the cache write and the action, presses `[u]`, asserts the dialog shows "Re-introspecting..." and the recompute happens before the action proceeds.
- **Gone case** (`@us-26 @ac-26-7` scenario): scenario deletes a model file out-of-band, presses `[d]`, asserts the action aborts with "file no longer exists; refreshing inventory" and asserts an automatic per-tool refresh runs.
- **Match case** (covered implicitly by every happy-path destructive scenario in the parent's `master-acceptance.feature` that continues to pass with cache enabled).

This is the Mandate 1 + critique Dim 7 discipline: assertions read **observable user outcomes** (dialog text, action aborted, file/no-file on disk), not internal state (`mock.verify.called`).

**The R9 architecture-lint** is DELIVER's responsibility — it lives in `tests/architecture.rs` as an AST-walk over `crates/modeltap-app/src/orchestration/`. The Layer A scenarios complement R9 by proving the revalidator is wired correctly at the user-observable level.

---

## D8 — Concurrent-process scenario implementation

**Decision: two real `modeltap` processes launched via `assert_cmd::Command`; the second waits for the first's WAL lock via `PRAGMA busy_timeout=5000` per ADR-015 §6.**

For US-23 Scenario "Two modeltap processes can read concurrently" and Scenario "Concurrent cache writes serialise via busy_timeout", the step harness:

1. Launches process A in headless mode against the per-scenario cache file. Process A holds a WAL read transaction open via the `--script` flag's `wait_for: <sentinel>` directive.
2. Launches process B in headless mode against the same cache file. Process B reads the same `cache_tools` rows and asserts both see consistent snapshots.
3. For the write-contention scenario, process A holds a `BEGIN IMMEDIATE` transaction open via a debug-only `--debug-hold-write-lock-ms <N>` flag (DELIVER-owned cargo feature `test-harness`). Process B's write attempt invokes the `busy_timeout`; the assertion is that B's write succeeds within 5 seconds AND neither process panics.

Rationale: this is the cleanest way to exercise real WAL semantics without bypassing the production code path. The `--debug-hold-write-lock-ms` flag is the one test-only seam in `modeltap-app` (analogous to the sibling feature's `MODELTAP_TEST_EBUSY_PATHS` env-var seam); it is gated behind `cfg(any(test, feature = "test-harness"))` to keep it out of release builds.

Alternative considered: simulate concurrency entirely inside one `modeltap` process via two `rusqlite::Connection` handles. Rejected because (a) it does not prove the WAL lock-file path resolution works across actual OS process boundaries, and (b) ADR-015 §6 specifically promises "two modeltap processes can read concurrently"; a single-process test would not validate the promise.

---

## D9 — KPI assertion tags

**Decision: time-bounded KPIs are encoded as `@k-info-N-<budget>` tags on the relevant scenarios; non-time-bounded KPIs are documented but not asserted at Layer A.**

Per `outcome-kpis.md` handoff notes:

| KPI | Tag | Assertion | Scenario |
|---|---|---|---|
| K-INFO-1 (warm-start ≤100 ms p90) | `@k-info-1-warm-100ms` | JSONL `launch.warm_paint_ms <= 150` (single-launch upper bound; p90 is a quarterly aggregate) | `cache-state-model.feature` walking-skeleton + warm-start scenario |
| K-INFO-2 (manual refresh ≤1 s p90) | `@k-info-2-refresh-1s` | JSONL `refresh.wall_clock_ms <= 1000` | `manual-refresh.feature` |
| K-INFO-4 (corruption recovery 100%) | `@k-info-4-recovery-100` | scenario PASSES = recovery succeeded (rename + cold-start + banner) | `cache-state-model.feature` corruption recovery |
| K-INFO-7 (cache overhead ≤50 ms) | `@k-info-7-overhead-50ms` | JSONL `launch.cache_open_ms + launch.cache_read_ms <= 100` | `cache-state-model.feature` warm-start scenario (companion assertion) |
| K3a (warm-start ≤150 ms) | `@k3a-warm-paint` | same as K-INFO-1 | walking-skeleton + warm-start |
| K3b (cold-start ≤1.15 s) | `@k3b-cold-start` | JSONL `launch.first_paint_ms <= 1150` | `cache-state-model.feature` cold-start scenarios |

K-INFO-3, K-INFO-5, K-INFO-6, K-INFO-8 are usage-pattern metrics measured over time; not asserted at Layer A per `outcome-kpis.md` handoff. Scenarios reference these KPIs in trailing comments for traceability but do not encode assertions.

**`@perf` tag** is applied to scenarios whose pass/fail depends on wall-clock latency assertions; DELIVER may run these via `cargo test --release -p modeltap-acceptance -- @perf` to avoid debug-build noise. The default `cargo test` run skips `@perf` scenarios via a default `--skip-tag @perf` argument in `tests/acceptance/runner.rs`.

---

## D10 — Release 3 (US-27) deferral mechanism

**Decision: every `@us-27` scenario also carries `@release-3 @skip`.**

Per `prioritization.md`, US-27 ships in Release 3, after Release 2 dogfoods. The five scenarios in `sha256-persistence.feature` are tagged `@us-27 @release-3 @skip`. DELIVER removes the `@skip` tag scenario-by-scenario when the feature is unblocked (post-Release-2 dogfood window). The `@release-3` tag enables a single CI command (`cargo test -- --skip-tag @release-3`) to keep R3 scenarios out of the default test run.

ADR-018's seam is honored: `cache_models.sha256` column (Release 2) is exercised by US-26 scenarios that mention dedup keys; the `cache_sha256` table (Release 3) is exercised only by `sha256-persistence.feature`. The Release 2 surface ships complete without the R3 table.

---

## D11 — Scenario count and error-path ratio

**Decision: 40 scenarios total across 7 feature files; error/edge path ratio 50% (20 of 40).**

| File | Total | Walking-skeleton | Happy | Error/edge | Property |
|---|---:|---:|---:|---:|---:|
| `walking-skeleton.feature` | 1 | 1 | 1 | 0 | 0 |
| `cache-state-model.feature` | 15 | 0 | 4 | 11 | 0 |
| `tool-detail.feature` | 5 | 0 | 2 | 3 | 0 |
| `model-detail.feature` | 5 | 0 | 3 | 2 | 0 |
| `manual-refresh.feature` | 4 | 0 | 3 | 1 | 0 |
| `sha256-persistence.feature` | 3 | 0 | 1 | 1 | 1 |
| `integration-checkpoints.feature` | 7 (one is a Scenario Outline with 4 examples — counted as 1 logical scenario) | 0 | 2 | 5 | 2 |
| **Total** | **40** | **1** | **16** | **23** | **3** |

The `walking-skeleton.feature` scenario is also tagged `@happy` (it traces the simplest E2E success journey); the count-as-happy in the matrix above double-counts (the WS appears in both columns 2 and 3). Net unique scenarios: 40. Net error/edge: 20 (50%), well above the 40% minimum per critique Dim 1.

Note: integration-checkpoints.feature includes a `Scenario Outline` covering 4 destructive actions (unify, zap, delete_one, folder_delete). The cucumber-rs runner treats each row as a separate executed scenario; for the matrix above, the outline counts as 1 logical scenario with 4 example rows, yielding 7 logical scenarios in the file.

---

## D12 — Three follow-ups deferred to DELIVER

These are listed in `acceptance-test-plan.md` § "What DELIVER Decides"; recapped here for explicit handoff visibility:

1. The exact `--debug-hold-write-lock-ms` flag implementation (cfg-gated; D8).
2. Whether the `@perf` tag is hard-skipped in CI default runs or run-and-warn (recommend: hard-skip; DELIVER decides).
3. Whether the `MODELTAP_OLLAMA_API_URL` test stub uses a real `wiremock`-style HTTP server or a simple `MODELTAP_OLLAMA_VERSION=0.6.4` env-var short-circuit in the Ollama plugin (recommend: env-var short-circuit; DELIVER decides).

None are blockers for DELIVER; spec accommodates either choice in each case.

---

## D13 — Reviewer escalations (anticipated)

Two findings that may surface during peer review which the acceptance-designer-reviewer (Sentinel) should be aware are SCOPED OUT:

1. **KPI measurability (escalate to PO-reviewer at DELIVER post-merge gate):** K-INFO-3, K-INFO-5, K-INFO-6 are usage-pattern metrics measured over time, not in single-scenario tests; they appear as trailing comments in scenarios for traceability. If a reviewer wants K-INFO-3 in a scenario, that is `@escalate:po-reviewer` per critique-dimensions § Reviewer Scope Boundaries.

2. **Infrastructure readiness (escalate to PA-reviewer):** the `--debug-hold-write-lock-ms` flag and `MODELTAP_OLLAMA_API_URL` env-var are test-only seams. If a reviewer wants them modelled as ports instead, that is `@escalate:pa-reviewer`.
