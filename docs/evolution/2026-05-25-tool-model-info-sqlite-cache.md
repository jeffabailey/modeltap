# Evolution — tool-model-info-sqlite-cache

**Closure date:** 2026-05-25
**Wave matrix:** DISCUSS / DESIGN / DISTILL / DELIVER — all complete and peer-reviewed (DEVOPS skipped per D4; no new CI/CD or deployment changes)
**Stories closed:** US-21, US-22, US-23, US-24, US-25, US-26 (Release 2). US-27 deferred to Release 3.
**Integration ACs closed:** INT-INFO-1..9.

## 1. Feature summary

A SQLite-backed inventory cache plus K5 pre-mutate revalidation. Warm-start launches paint cached inventory in under 150 ms; a background reconcile job validates and refreshes per-tool inventory atomically. Every destructive code path (`unify`, `zap`, `delete_one`, `delete_folder`) calls `revalidate::pre_mutate` before invoking the plugin, statically enforced by architecture-lint R9.

New crate: `modeltap-store`. New `Tool` trait methods: `inspect_tool` / `inspect_model`. New TUI screens: tool-detail and model-detail. New keymap: `[r]` (refresh tool) and `[Shift+R]` (refresh all). New summary-bar provenance ("Cache: 2 mins ago" / "Just refreshed" / "Refreshing now…").

## 2. Business context

The parent `modeltap-tui` v0.2.x shipped under ADR-003 — **stateless rediscovery on every launch**. Devon Park (the primary persona) opens modeltap dozens of times per day; the ~1.15 s cold-start cost was acceptable per-launch but punishing across a workflow, and detail screens that re-introspect on every open felt slow.

This feature **reverses ADR-003**, replacing it with ADR-015's paint-on-read / revalidate-on-mutate cache model. The reversal was explicitly anticipated by ADR-003's "Negative Consequence" clause:

> "Users with very large libraries (1000+ models) may notice discovery latency. If users complain, ADR-003 may be revisited."

The reversal would have been risky without **K5 (zero accidental data loss)** as a hard guardrail. The cache is paint-only on read paths; the filesystem remains authoritative on every mutate. The K5 invariant is now defended by three layers (see §3).

## 3. Key architectural decisions

| ADR | Title | Effect |
|---|---|---|
| ADR-015 | State Model — SQLite-Backed Cache With Pre-Mutate Revalidation | Adopts SQLite (`rusqlite` + `rusqlite_migration`) at `$XDG_DATA_HOME/modeltap/cache.sqlite`. Defines the paint-only / revalidate-on-mutate rule. **Supersedes ADR-003.** |
| ADR-016 | Tool Trait Inspect Extension | Adds `inspect_tool` and `inspect_model` to the `Tool` trait with a `NotSupported` default impl, so community plugins don't pay an integration tax. |
| ADR-017 | Schema Migration Strategy | Forward-only migrations via `rusqlite_migration`; corruption-recovery routine renames the cache file aside and falls back to cold-start with a recovery banner. |
| ADR-018 | SHA256 Persistence | Splits SHA256 persistence into Release 2 (`cache_models.sha256` column, derived lazily) and Release 3 (`cache_sha256` table with a background hash pool). **US-27 deferred to Release 3.** |

### K5 invariant — three layers of defense

1. **Store-side primitive:** `Cache::verify_against_fs(model_id)` re-stats every `cache_model_files` row and compares `(mtime, size, inode, dev)`. Returns `Match` / `Drift { fresh }` / `Gone`. Lives in `modeltap-store`.
2. **App-side orchestrator:** `revalidate::pre_mutate` in `modeltap-app` consumes the verify result and either proceeds, re-introspects-and-writeback, or aborts with auto-refresh.
3. **Static safety net:** Architecture-lint R9 in `crates/modeltap-app/tests/architecture.rs` AST-walks `actions/*` and `orchestration/*`, failing CI if any new destructive-method call site lacks a same-fn-body `pre_mutate` invocation.

Companion lints R7 (only `modeltap-app` may depend on `modeltap-store`; TUI must not know SQLite exists) and R8 (`modeltap-store` carries no `tokio` / `ratatui` / `crossterm`, neither runtime nor dev-dep — the async hop happens at the app boundary via `tokio::task::spawn_blocking`) keep the layering honest.

### Other decisions extracted from DISTILL wave-decisions

- **D5 — Walking skeleton Strategy B:** real I/O against fixture-populated tempdirs. An `InMemoryCache` would not pass the two-process walking skeleton — file existence on disk and `schema_version` PRAGMA are asserted across process boundaries.
- **D6 — `MODELTAP_CACHE_PATH` test seam:** every scenario points the cache at a per-scenario tempdir. One scenario exercises the production `dirs::data_dir()` resolver via `XDG_DATA_HOME`.
- **D7 — Pre-mutate revalidation observed at user-visible layer:** Layer A scenarios assert dialog text and observable outcomes, not call-graph instrumentation. R9 is the lint-level proof; scenarios are the user-outcome proof.
- **D8 — Two-process concurrency tests:** real `modeltap` binaries via `assert_cmd`, second waits on `PRAGMA busy_timeout=5000`. Debug-only `--debug-hold-write-lock-ms` flag gated behind `cfg(any(test, feature = "test-harness"))`.
- **D10 — Release 3 deferral:** every `@us-27` scenario carries `@release-3 @skip`. The 3 scenarios in `sha256-persistence.feature` are guarded by the regression gate (Step 06-02) to prevent accidental un-skip.

## 4. Steps completed — canonical audit trail (git history)

The full step list, most recent first, with commit SHAs:

```
93be14e  Step 06-02  mutation kill-rate ≥80% + parent regression gate + lat.md + CHANGELOG (Phase 06 + DELIVER done)
6d51817  Step 06-01  R7/R8/R9 architecture lints + release-build absence checks
d6b29ca  Step 05-04  pre-mutate drift re-introspect + gone auto-refresh + cache-state-model scenarios
16b7efd  Step 05-03  US-24 manual refresh hotkeys + US-25 summary-bar provenance
3255116  Step 05-02  pre-mutate revalidator + 4-site wiring (K5 invariant, part 2/2)
b12223a  Step 05-02  Cache::verify_against_fs + FileStat::matches + revalidator fixtures (part 1/2)
db9a637  Step 05-01  background reconcile orchestrator + atomic per-tool writes + last-known-good
dada88a  Step 04-05  launch metrics instrumentation + K-INFO budget cucumber (closes Phase 04)
33a1e59  Step 04-04  concurrent-process WAL + busy_timeout + MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS seam
a906f33  Step 04-03  per-tool TTL eligibility + XDG_DATA_HOME + transient I/O fallback
2dec6ca  Step 04-02  --no-cache CLI flag + cache.enabled opt-out + DirManifest invariant
e6e582a  Step 04-01  Cache::open recovery routine for SQLITE_CORRUPT / downgrade / migration failure
75bbb1b  Step 03-03  inspect_model contract harness + INT-INFO-8 inspect_model panic isolation (closes)
e301cbd  Step 03-02  GGUF v3 header parser + lm-studio inspect_model (part 3/N)
25f2baf  Step 03-02  HF inspect_model config.json reader (part 2/N)
e2e320e  Step 03-02  Ollama inspect_model manifest reader (part 1/N)
0ac9868  Step 03-01  model-detail cucumber driver — 1 active + 4 deferred (part 3/3, closes)
03b6143  Step 03-01  Msg::OpenModelDetail + Msg::ReintrospectModel (part 2/N)
4b4197c  Step 03-01  model detail Metadata section + open_model_detail orchestration (part 1/N)
948ed04  Step 02-03  INT-INFO-8 panic-isolation cucumber scenario (part 3/3, closes)
bd2a975  Step 02-03  inspect_tool panic-isolation orchestrator boundary (part 2/3)
12f9559  Step 02-03  inspect_tool plugin-contract harness + 5 plugin tests (part 1/2)
c8c6c6e  Step 02-02  Ollama + HF inspect_tool overrides + user-config search paths + reconcile last_error capture (closes)
1ac5a82  Step 02-01  tool detail Msg dispatch + cucumber acceptance (part 3/3, closes)
49ab9f5  Step 02-01  tool detail orchestration with merge logic and unit tests (part 2/3)
c4cb979  Step 01-05  walking-skeleton M1 scenario green — Phase 01 exit gate
1985000  Step 01-04  warm-start orchestration and cache-path resolver
5cb4b50  Step 01-03  promote async-trait to feature-gated regular dep (follow-up)
76fed8d  Step 01-03  in-process TestTool plugin + MODELTAP_TEST_PLUGINS registry seam
c6f2deb  Step 01-02  modeltap-store new crate: Cache::open, schema migration, ToolsRepo / ModelsRepo CRUD
5c952ec  Step 01-01  inspect domain types and Tool trait extension
```

### Known artifact — DES execution-log drift

The DES execution-log at `docs/feature/tool-model-info-sqlite-cache/deliver/execution-log.json` has missing phase entries for **9 steps**: 03-01, 03-02, 03-03, 04-01, 04-02, 04-03, 04-04, 04-05, and 05-01. The work for those steps shipped — every commit above lands on `main` with the canonical `Step-ID:` trailer in its message body — but earlier turn-limited crafter dispatches did not back-fill every phase event into the log.

**Treat git history as the canonical audit trail for this feature.** The DES log is a partial record; do not interpret missing entries as missing work. The pre-dispatch gate from `nw-finalize` was explicitly overridden by the parent orchestrating session before this finalize ran.

## 5. Lessons learned

1. **Large steps benefit from multi-commit landings.** Steps 03-01, 03-02, and 05-02 each shipped as multiple commits within the same roadmap step (parts 1..N). The single-commit-per-step ideal in `nw-deliver-orchestration` is a guideline, not a hard rule; breaking a step into 3 atomic commits when each commit is independently green is healthier than one 600-line wad.

2. **macOS Gatekeeper first-run scan can stall `cargo test --workspace` for 30+ minutes.** Step 02-03 hit this directly (35-minute hang on `cargo test -p modeltap-plugin-ollama --test inspect_tool_contract` before re-dispatch). Two mitigations are documented in `CLAUDE.md` § "Running Tests Fast on macOS": (a) add the terminal emulator to *System Settings → Privacy & Security → Developer Tools* to skip the kernel scan; (b) `scripts/test.sh` warms binaries via `--list` in parallel batches.

3. **DES audit-log discipline drifts under turn pressure.** When a crafter is turn-limited, the COMMIT trailer lands in git but earlier phases (PREPARE / RED_ACCEPTANCE / RED_UNIT / GREEN events) may not be logged back to `execution-log.json`. Git trailers (`Step-ID:`) are the canonical record; the DES log is best-effort. Future tooling could close this gap by writing the DES event post-commit from a git hook.

4. **K5 invariant separation pays off.** Store-side `Cache::verify_against_fs` is the primitive; app-side `pre_mutate` is the orchestrator; R9 static lint is the safety net. The three layers caught issues at different review stages — a contributor adding a 5th destructive call site (test fixture) hit R9 in CI, not in a runtime regression. Three layers of defense is right-sized; two would have left the static guarantee unproven, four would have been ceremony.

5. **Strategy B walking skeleton survived intent re-reads.** The litmus test in DISTILL D5 — "would the skeleton still pass if we substituted an `InMemoryCache`?" — was honest. It does not. The walking skeleton genuinely proves the SQLite-on-disk path resolution and the WAL init across processes. This is a template for future "is the walking skeleton real?" reviews.

## 6. Issues encountered

1. **Cargo flock deadlock between `cargo test --workspace` and `cargo run --package xtask`.** Acceptance tests under `tests/acceptance/release_process/` originally invoked `cargo run --package xtask` from inside `cargo test --workspace`. Both contend for the same exclusive `target/.cargo-lock`; the child blocked behind the parent's compile-time lock. Fixed in `tests/src/lib.rs::xtask_in()` by invoking the prebuilt `<workspace>/target/debug/xtask` binary directly — no cargo, no flock. The four duplicated definitions in `recovery.rs`, `release_prep.rs`, `walking_skeleton_e2e.rs`, and `bump_tap_formula.rs` collapsed to a single lib import. See `CLAUDE.md` § "Running Tests Fast on macOS" point (1).

2. **FK constraint bug in `DevonCacheMtimeDriftFixture` and `DevonCacheFileGoneFixture`.** Both fixtures wrote `cache_model_files` rows without a matching parent `cache_tools` row, producing a SQLITE_CONSTRAINT_FOREIGNKEY at scenario setup that masqueraded as a revalidator failure. Surfaced in Step 05-04 when the drift / gone scenarios first ran end-to-end.

3. **Pre-existing `modeltap-tui` clippy debt.** Cleared in Step 06-02 alongside the mutation-testing kill-rate work. None of the lints were introduced by this feature; they were latent debt surfaced by the workspace-wide `clippy -- -D warnings` discipline added in Step 06-01.

## 7. Deferred (backlog)

### Visible-dialog UX from Step 05-04

The behavioural core for K5 revalidation is fully wired and tested; only the visible TUI dialog strings remain. These were gated on the same `launch.log` timing seam that currently `#[ignore]`s the `manual_refresh.rs` perf scenarios:

- "Re-introspecting before proceeding…" string on the drift path.
- Reclaim re-confirm annotation in the dialog body.
- "file no longer exists; refreshing inventory" line on the right-pane Gone path.
- `LastAction::CacheStale` variant — currently 6 `// TODO` sites at `crates/modeltap-app/src/interactive.rs:798-851`.

These ship safely as a follow-up because the underlying state machine and revalidator are already correct; only the user-visible string is missing. No production safety regression while they sit on the backlog.

### US-27 — persistent SHA256 (Release 3)

The 3 scenarios in `distill/features/sha256-persistence.feature` are tagged `@us-27 @release-3 @skip` per ADR-018 / DISTILL D10. The regression gate (Step 06-02) statically asserts these tags remain in place to prevent accidental un-skip. Release 3 work is on hold pending Release 2 dogfood.

### Three DISTILL D12 follow-ups DELIVER chose / closed

For completeness — these are now resolved but worth recording:

1. `--debug-hold-write-lock-ms` flag shipped in Step 04-04 as `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS` env var, cfg-gated via `cfg(any(test, feature = "test-harness"))`.
2. `@perf` tag is hard-skipped in CI default runs (recommendation accepted).
3. `MODELTAP_OLLAMA_API_URL` test stub uses `MODELTAP_OLLAMA_VERSION=0.6.4` env-var short-circuit in the Ollama plugin (recommendation accepted, no wiremock dependency).

## 8. Migrated permanent artifacts

| Artifact | Permanent location |
|---|---|
| `architecture-design.md` | `docs/architecture/tool-model-info-sqlite-cache/architecture-design.md` |
| `component-boundaries.md` | `docs/architecture/tool-model-info-sqlite-cache/component-boundaries.md` |
| `data-models.md` | `docs/architecture/tool-model-info-sqlite-cache/data-models.md` |
| `technology-stack.md` | `docs/architecture/tool-model-info-sqlite-cache/technology-stack.md` |
| `journey-info-and-cache.yaml` | `docs/ux/tool-model-info-sqlite-cache/journey-info-and-cache.yaml` |
| `journey-info-and-cache-visual.md` | `docs/ux/tool-model-info-sqlite-cache/journey-info-and-cache-visual.md` |
| ADR-015..ADR-018 | already at `docs/adrs/` (flat, cross-feature) — verified, no copy needed |

The `distill/walking-skeleton.md` source file does not exist for this feature; the walking-skeleton specification lives inline in `distill/features/walking-skeleton.feature` and in `lat.md/walking-skeleton-acceptance.md`. No migration performed.

Wave-artifact workspace at `docs/feature/tool-model-info-sqlite-cache/` is **preserved** per `nw-finalize` Phase C contract — the wave matrix derives feature status from this directory. Only ephemeral session markers were removed (see commit message of the workspace-cleanup commit, if separately landed).
