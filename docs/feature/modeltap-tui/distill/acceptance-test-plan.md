# Acceptance Test Plan — modeltap-tui

**Wave:** DISTILL (5 of 6)
**Author:** Quinn (nw-acceptance-designer)
**Date:** 2026-04-28
**Authoritative inputs:** intake-brief.md, DISCUSS artifacts (user-stories.md, acceptance-criteria.md, journey-cleanup-and-unify.feature), DESIGN artifacts (architecture-design.md, component-boundaries.md, data-models.md, ADR-001..009), DEVOPS artifacts (kpi-instrumentation.md, ci-pipeline.md, platform-design.md).

This document describes how DELIVER will turn the refined Gherkin (`features/*.feature`) into running tests against a real `modeltap` binary. It is the contract between DISTILL and DELIVER.

## 1. Test Framework Recommendation

### Primary: `cucumber-rs` for Gherkin-driven E2E

Rationale:

- The DISCUSS wave already produced 21 Given-When-Then scenarios in `journey-cleanup-and-unify.feature`. Preserving that format end-to-end (DISCUSS → DISTILL → DELIVER tests → living documentation) keeps the artifact chain cheap. A non-Gherkin framework would force re-translation, losing fidelity.
- `cucumber-rs` (the maintained Rust port of Cucumber) supports `World` types, async step functions, and tagged scenario filtering (`@walking-skeleton`, `@release-1`, etc.).
- The `World` type holds the test fixtures (temp dir handle, env vars, captured stdout/stderr, parsed JSONL events).

### Secondary tools used inside step definitions

- **`assert_cmd`** — drives the actual `modeltap` binary as a subprocess. Used by `When the user runs "modeltap …"` steps. `assert_cmd::Command::cargo_bin("modeltap")` provides the binary built by `cargo test`.
- **`expectrl`** — for interactive TUI flows (sending keystrokes, waiting for screen content). Pseudo-terminal driver. Wraps `portable-pty`. Used when a scenario types confirmation (`llama-cli`-then-Enter) into a running TUI.
- **`insta`** — terminal frame snapshots for the headless TUI mode. Used by scenarios that assert the rendered two-pane layout matches a golden frame. `insta::assert_snapshot!` for plain text frames; `insta::assert_yaml_snapshot!` for structured data extracts.
- **`predicates`** — for stdout/stderr substring matching in `assert_cmd` chains.
- **`tempfile`** — `tempfile::TempDir` per scenario for the synthetic tool home.
- **`serde_json`** — parse the JSONL log lines for KPI assertions.

### Why NOT each alternative

- **Hand-rolled `assert_cmd` only** — viable but discards Gherkin. The DISCUSS scenarios become test names instead of executable specs. Loses living documentation.
- **`insta` only (snapshot diffs)** — proves the TUI renders pixels but says nothing about whether `z` actually deletes files. Snapshots are a complement, not a replacement.
- **`pytest-bdd`** — Python-driving-Rust adds language boundary friction; cucumber-rs avoids it.

### Test pyramid placement

```
                      ┌───────────────────────────────────────────────┐
                      │  Acceptance (E2E)  cucumber-rs + assert_cmd   │  ~ 60-70 scenarios
                      │  drives the real `modeltap` binary against    │
                      │  fixture-populated temp dirs                  │
                      └───────────────────────────────────────────────┘
                ┌─────────────────────────────────────────────────────────────┐
                │  Plugin contract tests  per-plugin tests/contract.rs        │  ~ 7 tests × 4 plugins
                │  parameterized over T: Tool from modeltap-core              │
                └─────────────────────────────────────────────────────────────┘
        ┌────────────────────────────────────────────────────────────────────────────┐
        │  TUI snapshot tests (insta)  ratatui::backend::TestBackend rendered frames  │  ~ 15-20 snapshots
        │  asserts visual layout for: empty pane, populated pane, dialogs, errors    │
        └────────────────────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────────────────────┐
│  Unit tests (modeltap-core pure functions)  compute_indicator, group_by_dedup_key, plans     │  many
│  property tests via proptest for invariants                                                  │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

The acceptance designer (this wave) owns the top layer (cucumber-rs scenarios). DELIVER's software-crafter owns the lower three layers. The plugin contract test specification (this wave's `plugin-contract-spec.md`) describes the contract any plugin must satisfy; DELIVER implements one parameterized test that all 4 plugins inherit.

## 2. Test Taxonomy and Story-to-Layer Map

| Layer | Mechanism | Drives | Owns |
|---|---|---|---|
| **A — E2E acceptance** | cucumber-rs runs `.feature` files; steps drive `modeltap` binary via `assert_cmd` / `expectrl`; assertions check stdout, exit code, file mutations on the temp tree, JSONL log lines | The 20 user stories' "Devon does X, sees Y" outcomes | DISTILL writes scenarios; DELIVER writes step definitions |
| **B — Plugin contract** | One parameterized test in `modeltap-core/tests/plugin_contract.rs` instantiated per plugin (`crates/plugins/<n>/tests/contract.rs` calls into it with the plugin's fixture dir) | US-18 trait stability; INT-3, INT-4 invariants per plugin | DISTILL writes the contract spec (this wave); DELIVER implements |
| **C — Unit (modeltap-core)** | Standard `#[test]` against pure functions — `compute_indicator`, `group_by_dedup_key`, `build_unify_plan`, `build_zap_plan` | The pure-logic invariants behind US-04, US-09, US-13, US-14 | DELIVER's software-crafter writes per inner-loop TDD |
| **D — TUI snapshot** | `ratatui::backend::TestBackend` renders the same view tree the real terminal would; `insta::assert_snapshot!` captures the buffer as text | US-03 layout, US-04 row format, US-08 bottom bar, US-13 detail screen layout, US-16 red `!` symbol | DELIVER writes per inner loop; one canonical golden frame per state |

### Story → Layer coverage matrix (full)

See § 7 for the complete table. Summary of how each story is covered:

| Story | E2E (A) | Contract (B) | Unit (C) | Snapshot (D) |
|---|:---:|:---:|:---:|:---:|
| US-01 launch & quit | ✓ | | | ✓ |
| US-02 Ollama discover | ✓ | ✓ | | |
| US-03 two-pane layout | ✓ | | | ✓ |
| US-04 row metadata | ✓ | | ✓ | ✓ |
| US-05 zap-all (typed confirm) | ✓ | ✓ | ✓ | ✓ |
| US-05b zap-one | ✓ | ✓ | ✓ | ✓ |
| US-06 last-action message | ✓ | | | ✓ |
| US-07 llama-cli discover | ✓ | ✓ | | |
| US-08 bottom bar | ✓ | | | ✓ |
| US-09 indicator engine | ✓ | | ✓ | |
| US-10 unify | ✓ | ✓ | ✓ | ✓ |
| US-11 totals refresh | ✓ | | ✓ | |
| US-12 HF discover | ✓ | ✓ | | |
| US-13 detail screen | ✓ | | | ✓ |
| US-14 dry-run preview | ✓ | | ✓ | ✓ |
| US-15 LM Studio discover | ✓ | ✓ | | |
| US-16 format-locked `!` | ✓ | | ✓ | ✓ |
| US-17 running-tool detect | ✓ | | ✓ | |
| US-18 plugin trait | ✓ | ✓ | | |
| US-19 cross-fs fallback | ✓ | | ✓ | |
| US-20 cross-platform paths | ✓ | ✓ | | |

Every story has at least one E2E scenario (Layer A) — that is the DISTILL ownership. Layer B/C/D coverage is a DELIVER recommendation, not a DISTILL gate.

## 3. Fixture Strategy

### Principle: synthetic-but-realistic temp trees, never live tool installations

Acceptance tests run on developer machines AND CI runners. We cannot require Ollama/llama-cli/HF/LM Studio to be installed on either. Each scenario builds a temp directory tree that mimics the on-disk layout of the relevant tools, then points the binary at it via env vars.

### Env var contract (the seam)

Each plugin honors a per-plugin env var that overrides its discovery root. This is in addition to the production default discovery path.

| Plugin | Production default | Test override env |
|---|---|---|
| ollama | `~/.ollama/models/` | `MODELTAP_OLLAMA_DIR` |
| llama-cli | `~/llms/`, `~/models/` (+ config) | `MODELTAP_LLAMACLI_DIRS` (colon-separated) |
| hf | `$HF_HOME/hub/` or `~/.cache/huggingface/hub/` | `HF_HOME` (existing standard env var; reuse) |
| lm-studio | `~/.cache/lm-studio/models/` or `~/.lmstudio/models/` | `MODELTAP_LMSTUDIO_DIR` |

In addition:

| Env | Purpose |
|---|---|
| `MODELTAP_HEADLESS=1` | Headless mode: no TUI render loop; emit timing JSON to stdout; exit after first paint or after a single Msg from `--script` (see §4) |
| `MODELTAP_LOG_DIR=<path>` | Override `~/.modeltap/` so tests write to temp dir |
| `MODELTAP_FIXTURES=<name>` | Selects a pre-built fixture tree under `tests/fixtures/<name>/` (used by the K3 benchmark) |
| `MODELTAP_FORCE_PLATFORM=linux\|macos` | Forces the platform identifier in JSONL logs; lets one CI runner generate fixtures for both platforms |
| `NO_COLOR=1` | Standard env; suppresses ANSI color codes (US-04 AC, US-16 AC) |
| `MODELTAP_LSOF=<bin>` | Override the lsof binary path; lets a fake-lsof script simulate "tool running" without actually running a tool |

### Fixture builder script

`tests/fixtures/build.sh` (to be created in DELIVER) generates synthetic trees. The script is the contract; the test suite calls it before any scenario:

```bash
# Build all named fixture trees (idempotent).
./tests/fixtures/build.sh all

# Build just one (test isolation).
./tests/fixtures/build.sh devon-multi-tool
```

Named fixture trees:

| Name | Contents | Used by |
|---|---|---|
| `devon-multi-tool` | Ollama (12 models, 47.3 GB), llama-cli (6, 21.4 GB), HF (31, 78.2 GB), LM Studio (9, 38.7 GB). Includes 1 model (Mistral-7B-v0.3 q4_K_M GGUF) present in 3 tools with identical SHA256, 1 AWQ-only-in-HF model, 1 corrupt GGUF, 1 broken HF symlink. | Most happy-path scenarios |
| `devon-empty` | All four tool dirs exist but empty. | Empty-state scenarios |
| `devon-only-ollama` | Only `~/.ollama/` exists; others absent. | "not installed" scenarios |
| `devon-permission-denied` | Ollama dir exists but `chmod 000`. | Error-path scenarios |
| `devon-cross-fs` | llama-cli files placed under a separately-mounted tmpfs (or via `mount --bind` simulation; see below). | US-19 cross-fs scenarios |
| `k3-bench` | 200 models split across 4 tools. | CI K3 benchmark |
| `riley-fifth-plugin` | Same as `devon-multi-tool` but with a 5th plugin "atomic-chat" registered. | US-18 plugin extensibility scenarios |

#### How the script fakes model files

For most scenarios the SIZE of files matters, not the bytes. A 4.4 GB GGUF file would balloon the fixture tree. Strategy:

- Default: **sparse files** (`truncate -s 4400000000 mistral.gguf`) — file system reports the size; only metadata blocks consumed. Tests can stat for size; SHA256 of a sparse file is deterministic (all zero bytes).
- For SHA256-equality scenarios (Mistral in 3 tools): build the file ONCE in a shared blob dir, then `cp --reflink=auto` (Linux btrfs/xfs) OR `cp -c` (macOS APFS) OR plain `cp` (worst case — only paid for the 1 setup) to each tool dir. SHA256 will match because content matches.
- For GGUF header parsing tests (US-07): write a real minimal GGUF header (magic bytes, version, key-value pairs) followed by sparse padding. Header parser sees a valid GGUF; size on disk is the sparse-reported size.
- For corrupt-file tests: 100-byte file with invalid magic.
- For broken symlinks: `ln -s /nonexistent target` then leave dangling.

Total fixture tree on-disk usage: < 100 MB even for `devon-multi-tool`. Sparse means apparent size can be 200+ GB.

#### Cross-fs fixture (US-19)

The trick: simulate two filesystems without actually mounting. Approach in priority order:

1. **CI Linux runners:** create a small tmpfs mount under the temp dir (`mount -t tmpfs ...`), place `llama-cli`'s tree there. Different `st_dev` from the rest. Requires `sudo` on GitHub Actions Linux runners (available).
2. **macOS CI runners:** APFS volumes can be created with `diskutil apfs addVolume` but this is heavy. Alternative: use a sparse disk image (`hdiutil create -size 100m`, attach, format). Heavy.
3. **Fallback for both:** the FsProbe port can be substituted with a `FakeFsProbe` that ALWAYS reports cross-fs for paths matching a configured prefix. This is acceptance-test-internal and bypasses the OS — but it is the FsProbe ADAPTER that is mocked, not the production code path. The actual cross-fs detection logic in `modeltap-core::logic::plan` is exercised honestly. DECISION: use the FakeFsProbe approach for the cross-fs E2E scenarios (Mandate 4: pure logic exercised; impure detection mocked). Real cross-fs is exercised by the per-plugin contract test on Linux only, with the sudo-mount approach.

#### Running-tool fixture (US-17)

Same principle: fake `lsof` rather than actually starting `ollama serve`. `MODELTAP_LSOF=tests/fakes/lsof-running-ollama.sh` points to a script that emits a hard-coded "ollama PID 4421 holds file X" output. The FsProbe adapter reads from `MODELTAP_LSOF` if set; otherwise from system `lsof`. The fake-lsof script is the seam; production lsof is exercised by manual end-to-end UAT (per `production-readiness-checklist.md`), not E2E acceptance.

## 4. Headless TUI Mode Contract

DEVOPS specified `MODELTAP_HEADLESS=1`. This section is the contract.

### Goal

Make every E2E acceptance scenario runnable in CI without a TTY, without flakes from terminal initialization, and without expectrl pseudo-terminal pain for the common case.

### Behavior in headless mode

When `MODELTAP_HEADLESS=1`:

1. **No alternate-screen / raw-mode init.** The crossterm `enable_raw_mode()` calls are skipped.
2. **The render path runs against `ratatui::backend::TestBackend`** at a fixed 100×40 size, instead of `CrosstermBackend`.
3. **Each rendered frame is captured as plain text** (cells, no ANSI). The frame is appended to a buffer.
4. **Input is scripted, not terminal-driven.** A `--script <file>` flag accepts a sequence of keystrokes and synthetic events:
   ```
   # tests/scripts/zap-llama-cli.script
   wait_for: "Models in llama-cli"
   key: z
   wait_for: "DELETE 6 MODELS"
   type: llama-cli
   key: Enter
   wait_for: "Reclaimed"
   key: q
   ```
   The driver applies one event at a time, re-renders, captures the frame, then advances. Timeouts on `wait_for` produce a test failure with the most-recent frame attached.
5. **At quit, the binary emits a single JSON object to stdout** describing the session: `{"frames_captured": N, "exit_reason": "user_quit", "first_paint_ms": M, "events_logged": K, "log_path": "/tmp/...modeltap/launch.log"}`.
6. **Stderr stays human-readable** for debugging.

### What headless mode does NOT do

- It does NOT bypass the TUI logic. The same `update()` and `view()` functions run. Only the backend swaps.
- It does NOT skip plugin discovery. Plugins run normally against the fixture tree.
- It does NOT skip the JSONL log writer. The `~/.modeltap/launch.log` is written to `MODELTAP_LOG_DIR` (a tempdir per scenario).

### Why this design

This contract gives DELIVER a deterministic, scriptable, fast (< 100 ms per scenario) test harness while preserving the production code paths. The two existing departures from production are: TestBackend instead of Crossterm (rendering target), and scripted input instead of crossterm event polling (input source). Everything else — discovery, planning, mutation, JSONL emission — is identical to production.

For the small number of scenarios that genuinely require interactive timing (e.g., "Devon types `llama-cli` partially, then waits, then completes"), `expectrl` against a real PTY is used. These are tagged `@interactive` and run only on macOS CI (where PTY support is reliable in GitHub Actions).

### Headless flag is dev-only

Headless mode is gated behind a `--features headless` cargo feature OR an env-var check at startup. Production releases do not include the headless code path? — DECISION DEFERRED: DELIVER may choose either approach. Recommendation: include in release builds (small; useful for CI consumers) but document `MODELTAP_HEADLESS` only in the developer/contributor docs.

## 5. Coverage Matrix (Story → Scenarios → Layer → Fixture)

See `features/master-acceptance.feature` and per-story files for the actual scenarios. The matrix below maps story ID to expected scenario count and fixture/layer.

| Story | Scenarios | Tags | Layer | Fixture |
|---|---:|---|---|---|
| US-01 | 5 | `@walking-skeleton @us-01` | A + D | `devon-empty`, `devon-only-ollama` |
| US-02 | 5 | `@walking-skeleton @us-02 @release-1` | A + B | `devon-multi-tool`, `devon-only-ollama`, `devon-permission-denied` |
| US-03 | 4 | `@walking-skeleton @us-03` | A + D | `devon-multi-tool` |
| US-04 | 4 | `@release-1 @us-04` | A + D | `devon-multi-tool` |
| US-05 | 5 | `@walking-skeleton @us-05 @destructive` | A + B | `devon-multi-tool` |
| US-05b | 5 | `@release-2 @us-05b @destructive` | A + B | `devon-multi-tool` |
| US-06 | 4 | `@walking-skeleton @us-06` | A + D | `devon-multi-tool` |
| US-07 | 4 | `@release-1 @us-07` | A + B | `devon-multi-tool`, `devon-empty` (with config override) |
| US-08 | 3 | `@release-2 @us-08` | A + D | `devon-multi-tool` |
| US-09 | 4 | `@release-1 @us-09 @property` | A + C | `devon-multi-tool` |
| US-10 | 6 | `@release-2 @us-10` | A + B | `devon-multi-tool`, `devon-cross-fs` |
| US-11 | 3 | `@release-2 @us-11` | A | `devon-multi-tool` |
| US-12 | 4 | `@release-1 @us-12` | A + B | `devon-multi-tool` (broken symlink), `devon-multi-tool` (HF_HOME override) |
| US-13 | 4 | `@release-1 @us-13` | A + D | `devon-multi-tool` |
| US-14 | 3 | `@release-2 @us-14` | A + D | `devon-multi-tool`, `devon-cross-fs` |
| US-15 | 3 | `@release-1 @us-15` | A + B | `devon-multi-tool` |
| US-16 | 3 | `@release-1 @us-16` | A + D | `devon-multi-tool` |
| US-17 | 4 | `@release-2 @us-17` | A | `devon-multi-tool` (with fake-lsof) |
| US-18 | 4 | `@release-3 @us-18 @plugin-trait` | A + B | `riley-fifth-plugin`, `devon-multi-tool` (with panic plugin) |
| US-19 | 4 | `@release-2 @us-19 @cross-fs` | A | `devon-cross-fs` |
| US-20 | 3 | `@release-3 @us-20 @cross-platform` | A + B | `devon-multi-tool` (with `MODELTAP_FORCE_PLATFORM`) |
| **K3 latency (cross-cutting)** | 2 | `@k3-latency` | A | `k3-bench` |
| **JSONL invariants (cross-cutting)** | 4 | `@kpi-instrumentation` | A | `devon-multi-tool` |

**Total: ~85 scenarios** (DISTILL produced; DELIVER may reduce after first-implementation feedback). Walking-skeleton subset: ~22 scenarios across US-01, US-02, US-03, US-05, US-06.

Error-path ratio target: **40% minimum** (per critique-dimensions Dim 1).

## 6. Test Execution Model

### Local developer

```bash
# Build fixtures once (cached if unchanged).
./tests/fixtures/build.sh all

# Run all acceptance scenarios.
cargo test --test acceptance

# Run only walking-skeleton scenarios.
cargo test --test acceptance -- --tag @walking-skeleton

# Run a single story.
cargo test --test acceptance -- --tag @us-05

# Skip destructive scenarios (faster iteration).
cargo test --test acceptance -- --skip-tag @destructive
```

Each scenario gets its own `tempfile::TempDir` for `MODELTAP_OLLAMA_DIR` etc., copied or symlinked from the named fixture template. Scenarios are isolated; parallelism via cucumber-rs's worker pool.

### CI

The `ci.yml` "test" job runs `cargo test --workspace --locked` which includes the acceptance suite. The `k3-bench` job runs the K3 benchmark in isolation (see `k3-benchmark-spec.md`).

### The "smoke test in current env only" fast path

Per DISTILL handoff workflow, if a wave produces ≤ 3 scenarios, only one review pass and one env smoke test are required. **modeltap is well above that threshold (~85 scenarios) — full review pass + per-scenario fixture isolation applies.**

## 7. Mandate Compliance Notes (CM-A through CM-D)

This plan satisfies the four acceptance-test mandates as follows:

### CM-A — Hexagonal boundary (driving ports only)

Acceptance tests invoke through the actual `modeltap` binary. The binary's `main()` is the driving port. Scenarios send keystrokes (or scripted events in headless mode) and observe stdout, exit codes, and filesystem mutations. **No scenario imports `modeltap-core::logic::plan` directly.** Internal components (compatibility engine, plan builder) are exercised indirectly through the binary entrypoint.

Exception: the Plugin Contract test (Layer B) instantiates `Box<dyn Tool>` directly — this is a contract test, not an acceptance test, and the trait `Tool` IS the public port for plugin authors (per ADR-001). DISTILL specifies the contract; DELIVER implements.

### CM-B — Business language

Step phrases use Devon's vocabulary: "Devon launches modeltap", "the bottom bar shows", "the right pane lists", "Devon types", "modeltap reports Reclaimed". Forbidden terms in step text (DELIVER must not introduce): `HTTP`, `JSON` (except in KPI-assertion steps that explicitly check the JSONL log), `database`, `endpoint`, `function`, `method call`, `class`, `assert_eq!`, `unwrap`. Per the grep check in CM verification.

### CM-C — User journey completeness

Walking-skeleton scenarios trace a complete user journey (Devon launches → sees inventory → zaps → sees reclaim message → quits). Focused scenarios stay anchored to user observable outcomes (always assert what Devon SEES, not what the system internally computes).

### CM-D — Pure function extraction

The architecture (per `architecture-design.md` § 4.3) already separates pure logic in `modeltap-core::logic::*` from impure I/O in plugins and adapters. The acceptance tests (Layer A) exercise impure I/O through the binary; DELIVER's unit tests (Layer C) exercise pure logic directly without fixtures. **No fixture parametrization at the acceptance layer beyond the named-fixture-tree level** (which IS the adapter layer parametrization, per the mandate).

### Walking Skeleton Strategy declaration

**Strategy: B (real I/O against fixture-populated temp dirs).** modeltap is a local desktop CLI; "real I/O" means real filesystem operations against synthetic-but-realistic temp trees. There is no costly external dependency to mock (no cloud, no network in v1). This is the correct strategy per critique-dimensions Dim 9: walking skeletons exercise real plugin discovery against real on-disk layouts. InMemory test plugins are reserved for Layer B (Plugin Contract Test) where a generic `T: Tool` substitution is needed; they are NOT used for walking-skeleton acceptance scenarios.

## 8. What DELIVER Inherits from This Plan

1. The `.feature` files in `features/` — directly executable once step definitions exist.
2. The step-definition skeleton in `step-definitions-skeleton.md` — describes every Given/When/Then phrase and what it should assert.
3. The plugin contract spec in `plugin-contract-spec.md` — the test that every plugin must pass.
4. The K3 benchmark spec in `k3-benchmark-spec.md` — what to assert and against what fixture.
5. The fixture-builder contract — script name and named-fixture inventory in §3.
6. The headless mode contract — env var name, behavior, output schema in §4.

## 9. What DELIVER Decides

- The exact step-definition Rust code (this wave specifies what each step asserts; DELIVER writes the Rust).
- The fixture-builder script implementation (this wave specifies the named trees; DELIVER writes the bash).
- Whether headless mode is gated by feature flag or env var only (recommendation: env var only).
- The `expectrl`-based interactive scenario implementations for the `@interactive` tag.
- The ratatui golden-frame snapshot files (Layer D) — too implementation-specific for DISTILL.

## 10. Risks and Open Questions

| # | Item | Mitigation |
|---|---|---|
| R1 | cucumber-rs Gherkin support for tags + outline tables: confirm version supports `@tag` filtering and Scenario Outline. | DELIVER pins `cucumber = "0.21"` or later (verified at writing). |
| R2 | Sparse files behave differently across filesystems (some refuse `truncate -s 200G`); APFS handles them, ext4 does, tmpfs has limits. | Cap sparse sizes at 50 GB; use real small files for tests that need actual content reads. |
| R3 | TestBackend output is character cells; ANSI color is lost. Snapshot tests for US-04 / US-16 color assertions need a different approach. | Use a `Style`-aware capture: `TestBackend` exposes `Buffer` which has cell styles; assertions check `cell.style.fg == Color::Red` not just text presence. |
| R4 | The fake-lsof seam (`MODELTAP_LSOF`) must be a per-launch env var honored by the FsProbe adapter — DELIVER must wire this. | Documented as a step-definition assertion in `step-definitions-skeleton.md`. |
| R5 | Cross-fs `mount -t tmpfs` requires sudo on Linux CI; fallback FakeFsProbe approach removes the sudo dependency for E2E but loses real-cross-fs proof at the E2E level. | Real-cross-fs proof is moved to per-plugin contract test (`plugin-contract-spec.md` § 3.7) which runs only on Linux CI with `sudo`. |
| OQ-1 | Should `modeltap` ship with a `--script` flag in production, or only in test builds? | RECOMMENDATION: ship in production (small surface, useful for users automating via `expect`). Final call: DELIVER's PR-time decision. |
| OQ-2 | Insta snapshots for color: capture as ANSI-encoded text (lossy if terminal supports more colors than the snapshot was taken under) or as structured `(text, fg, bg)` triples (verbose snapshot files)? | RECOMMENDATION: structured triples for color-critical tests (US-04, US-16); plain text for layout-only tests (US-03, US-13). |
