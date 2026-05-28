# Evolution — folder-group-bulk-delete

**Closure date:** 2026-05-27 (code landed 2026-05-12; mutation gate closed 2026-05-13; finalize archived 2026-05-27)
**Wave matrix:** DISCUSS / DESIGN / DISTILL / DELIVER — all complete and peer-reviewed (DEVOPS skipped per D8; no new CI/CD or deployment changes)
**Stories closed:** US-05c — Delete a whole Hugging Face folder group (Release 1; HF plugin only in v1)
**Integration ACs closed:** INT-FGD-1..8 (parent regression INT-FGD-8 included)

## 1. Feature summary

`Shift+F` on a Hugging Face folder-header row opens a typed-confirmation dialog. The user types the full `<author>/<repo>` path (byte-exact, case-sensitive). On confirmation, the HF plugin sweeps every model file plus enumerated sidecars (`README.md`, `LICENSE`, `.imatrix`, `.gguf.urls`, `refs/`, `blobs/`). Unique files are unlinked; shared files have only their HF-side snapshot symlink removed (cross-tool hardlinks survive). The post-action summary reports `Reclaimed: <X> GB` and `Retained: <Y> GB` using the same vocabulary as US-05 / US-05b.

This is the third delete granularity: `[z]` (whole tool, US-05) and `[d]` (single model, US-05b) already existed. `[F]` (folder, US-05c) fills the gap that Devon hit dozens of times per session — discarding a whole `<author>/<repo>/` folder with all its quant variants in one motion (~35 keystrokes total, independent of file count) instead of N × 22 keystrokes through `[d]`.

## 2. Business context

Devon Park's primary HF-cache cleanup task today is "I'm done with this repo, drop the whole thing." Without `[F]`, doing that against a 20-file folder is ~440 keystrokes and 60-180 seconds through the `[d]` loop. With `[F]`, ~35 keystrokes and (per K-FGD-1) p50 ≤ 15 s end-to-end.

Three KPIs framed the work:

| KPI | Target | Baseline |
|---|---|---|
| K-FGD-1 `time_to_reclaim_repo_p50_seconds` | p50 ≤ 15 s, p90 ≤ 30 s (21-file repo) | US-05b loop: 60-180 s |
| K-FGD-2 `keystrokes_per_repo_delete` | ~35 keystrokes, file-count-independent | US-05b loop: ~22 × N_files |
| K-FGD-3 `mis_target_rate` (guardrail) | < 1% of dialog opens; 0 accidental wrong-folder deletes in 90 days | N/A (no feature) |

Feature also drives parent KPIs K1 (bytes reclaimed per session) and K5 (no accidental loss).

## 3. Key architectural decisions

| ADR | Title | Effect |
|---|---|---|
| ADR-010 | Folder-Group Delete — HF Capability via Trait Default-Method | Adds `Tool::delete_folder` with a default body returning `Err(DeleteError::Unsupported)`. HF overrides; the other three plugins inherit zero source changes. Closes Q-FGD-1 (trait shape) and Q-FGD-2 (concurrency = inherit ADR-009 per-file detect-and-prompt-then-retry). |

No other ADRs were authored. The remaining decisions live in the DISCUSS and DISTILL wave-decision artifacts, extracted below.

### Discuss-wave decisions (D-FGD-1..7)

- **D-FGD-1 Hotkey `Shift+F` (`[F]`).** Visual distance from `[d]` / `[z]`; mnemonic "folder"; uppercase signals bulk operation.
- **D-FGD-2 Typed-confirm = full `<author>/<repo>` path.** Mirrors US-05 (typed-confirm is the strongest cheap guard for irreversible bulk ops). Recognition over recall — the dialog displays the exact string. Case-sensitive, byte-exact. Trailing slash is a mismatch (D2 in DISTILL).
- **D-FGD-3 Collapsible folder header row.** Always-on grouping (not a display toggle). `[+]` / `[-]` leftmost; folder headers are cursor-targetable; sidecar children are dim-prefixed `.` and not cursor-targetable. Single-file folders collapse to the pre-existing US-04 row format (backward-compatible).
- **D-FGD-4 Per-file shared/unique via parent's US-09 compute_indicator.** Single-engine invariant — `classify_unique_vs_shared` is a thin pure adaptor; no parallel dedup logic. Enforced by construction.
- **D-FGD-5 Sidecar sweep is mandatory, HF-plugin-owned.** `README.md`, `LICENSE` / `LICENSE.md`, `.imatrix`, `.gguf.urls` / `.urls`, `refs/`, `blobs/`. Partial sweeps (models only, leaving sidecars) are NOT offered. Suffix list lives only in `plugins/hf` so HF version churn doesn't ripple into `modeltap-core`.
- **D-FGD-6 Partial-failure continue-and-report, no rollback.** Successfully-deleted files stay deleted; failed files remain on disk with reason captured. Re-run is cheap because inventory rebuilds on next launch.
- **D-FGD-7 Reclaim accounting matches parent vocabulary** (`Reclaimed` = unique + sidecar bytes; `Retained` = shared-but-HF-side-removed bytes).

### Distill-wave decisions (D1..D10)

- **D1 Strategy B walking skeleton** — real I/O against fixture-populated tempdirs. M1 asserts `path.exists() == false`; substituting an `InMemoryHfPlugin` would not mutate the filesystem and the test would fail by construction.
- **D2 Trailing slash on typed path = mismatch.** No normalization. The dialog tells the user the exact string; ambiguity is the failure mode the typed-confirm pattern exists to prevent.
- **D3 K-FGD-2 keystroke bound = 40** (33 chars for `bartowski/Llama-3.2-1B-Instruct-GGUF` + Enter + small correction allowance). Same bound for 5-file folder — file-count independence is the property.
- **D4 `MODELTAP_TEST_EBUSY_PATHS` env-var test seam**, gated behind `cfg(any(test, feature = "test-harness"))`. Phase 5b mutation results confirm release builds do not ship the seam string.
- **D5 M5 capability boundary tested at both layers** — Layer A asserts user-observable behavior (right pane shows `Ollama does not support folder-delete`); Layer B asserts trait-level `Err(DeleteError::Unsupported)` via the plugin-contract harness.
- **D6 `@property` tag = DELIVER discretion** to implement as proptest invariants or concrete-example scenarios per case. Three landed as proptests in `modeltap-core/tests/folder_group_proptest.rs`; AC-8 stayed as E2E mutation classes; INT-FGD-7 went hybrid (E2E + regex lint).
- **D7 60% error-path scenario ratio** (9 of 15 in `folder-group-delete.feature`) — well above the 40% minimum. Typed-confirm features are intrinsically error-path-heavy.
- **D8 DEVOPS skipped.** Brownfield additive feature; no new environments, no new platform coverage. Parent's macOS / Linux / WSL contract inherits.
- **D9 Three DELIVER-discretionary follow-ups landed** — exact sidecar suffix list (frozen at the 5 above), seam gating (chose `cfg(any(test, feature = "test-harness"))`), M5 Layer A wording (chose user-observable text over JSONL fallback).

## 4. Steps completed — canonical audit trail (git history)

16 step commits + 1 follow-up fix + 1 mutation-gap closure, in chronological order:

```
e3996d1  Step 01-01  core::FolderGroup types + Tool::delete_folder default body
400f63d  Step 01-02  pure folder-group logic (group_by_hf_repo / classify / plan)
dce769f  Step 01-03  HF plugin delete_folder happy path
8796beb  Step 01-04  TUI folder-header row + Shift+F keymap + folder confirm dialog
a33c23c  (follow-up)  fix(tui): keep bottom-bar within 80 cols by shortening [F] label
af258a8  Step 01-05  orchestrate folder-delete + M1 walking-skeleton green (Phase 01 exit)
59e14c0  Step 02-01  wrong-path + trailing-slash + Esc cancel paths
ece09b6  Step 02-02  Shift+F is no-op on non-HF tools + dim [F] indicator
149d383  Step 03-01  itemized mixed-folder dialog + post-action accounting
39b0072  Step 03-02  mixed-folder delete preserves cross-tool hardlinks
2eb2da5  Step 03-03  @property proptests for folder-group invariants
e4b88d6  Step 04-01  partial-failure handling + EBUSY test seam
37c5d31  Step 04-02  idempotent folder-delete retry after busy resolved
254fe17  Step 04-03  pre-flight refusals for read-only cache + vanished folder
05fc344  Step 05-01  folder-delete plugin-contract tests + M5 boundary (3 non-HF plugins)
12e5852  Step 06-01  KPI instrumentation + keystroke-count + mis-target invariant
49ffe78  Step 06-02  integration checkpoints + feature exit gate (Phase 06 + DELIVER done)
466369a  (Phase 5b)   test(hf): kill surviving mutants in classify_sidecar / path_starts_with_subdir
```

## 5. Adversarial review (Phase 4)

**Verdict: APPROVED. Zero defects** (Critical 0 / High 0 / Medium 0 / Low 0).

Verified strengths:
- ADR-010 default-method extension preserves "add a 5th tool = zero changes outside the plugin crate."
- AC-13 single-engine invariant enforced by construction.
- AC-14 sidecar suffix list hermetically sealed in `plugins/hf/src/folder_delete.rs`.
- `MODELTAP_TEST_EBUSY_PATHS` seam release-absent (`strings target/release/modeltap | grep ...` empty).
- ADR-009 ref-counting keeps cross-tool hardlinked inodes alive when HF unlinks its side (INT-FGD-4).
- Port-to-port testing discipline — acceptance tests drive the `modeltap-app` binary; plugin-contract tests drive `Box<dyn Tool>`; zero internal-class tests.
- Cargo-flock fix applied — `modeltap_app_in` invokes `target/debug/modeltap-app` directly, not `cargo run`.

Plugin-contract substitution note: `plugins/llama-cli` does not exist in this workspace; M5 substituted `atomic-chat` as the third non-HF plugin. The substitution is recorded in the Step 05-01 commit and preserves M5 intent (the trait-level default-body assertion is identical across any non-HF plugin).

## 6. Mutation testing results (Phase 5/5b)

**PASS** after the Phase 5b test-gap closure.

| Target | Kill rate (spec) | Kill rate (strict) | Gate |
|---|---|---|---|
| `crates/modeltap-core/src/logic/folder_group.rs` | 88.24% (15/17) | 83.33% (10/12) | PASS |
| `plugins/hf/src/folder_delete.rs` (production only) | 100.00% (24/24) | 100.00% (21/21) | PASS |
| Overall (production only) | 100.00% (41/41) | 100.00% (33/33) | PASS |

The 6 surviving mutants live inside `is_test_ebusy_path` — the cfg-gated test seam (D4) — and are out of scope for production kill-rate measurement.

Phase 5b added `plugins/hf/tests/folder_delete_classify_sidecar.rs`: a 3-test file (one parametrized over 10 (path, expected-kind) tuples + two single-case tests pinning the `path_starts_with_subdir` branches). The unit tests drive `classify_sidecar` / `path_starts_with_subdir` through the public `enumerate_sidecars` port — port-to-port at domain-function scope, no module-private access. Production code untouched.

## 7. Issues encountered & lessons learned

1. **Bottom-bar 80-col regression (a33c23c).** The Step 01-04 commit landed `[F] folder-delete` in the bottom bar, which combined with the existing `[d] [z] [u]` etc. broke the 80-col invariant. Fix: shorten to `[F]`. Lesson: **AC-19 "single source SHORTCUT_TABLE drives both render and dispatch" needs a per-edit width assertion**, not just a render snapshot. A unit test on `SHORTCUT_TABLE.iter().map(label_len).sum::<usize>() + separators <= 80` would have caught this in RED.
2. **`compute_indicator` Tentative-key behavior had to be re-verified.** Risk R1 in `architecture-design.md` §10 noted that classification under Tentative dedup keys (SHA256 not yet computed at dialog open) could mis-classify shared as unique. Reviewer verified against `crates/modeltap-core/src/logic/compatibility.rs` "Decision rules" #2 — "when SHA256 is None for either side, the engine MUST NOT classify as Shared" — and the invariant holds. Lesson: load-bearing claims about parent-engine behavior need a citation, not a paraphrase, in design docs.
3. **`classify_sidecar` boolean-OR mutations escaped initial pass.** README/LICENSE/`.urls` alternation arms in `classify_sidecar` were only exercised at integration scope. Mutation testing surfaced 5 surviving mutants on `||` → `&&` flips that the integration suite happened to not cover because no test fixture contained a README. Lesson: **per-suffix parametrized unit tests are cheaper than integration coverage** for pure classifier functions, and they're the natural port-to-port target.
4. **`is_test_ebusy_path` cfg-gating discipline.** The 6 surviving mutants in the test seam are by design — but the reviewer should have a `mutants.toml` skip rule (`exclude_re = ["is_test_ebusy_path"]`) to avoid recurring confusion. Listed as a future-maintenance follow-up.
5. **DISTILL D8 "no DEVOPS context" worked cleanly for a brownfield additive feature.** Future brownfield features that touch no environments / no new platform code should follow this pattern (skip DEVOPS, document why in DISTILL wave-decisions).

## 8. Migrated permanent artifacts

| Source (workspace) | Destination |
|---|---|
| `design/architecture-design.md` | `docs/architecture/folder-group-bulk-delete/` |
| `design/component-boundaries.md` | `docs/architecture/folder-group-bulk-delete/` |
| `design/data-models.md` | `docs/architecture/folder-group-bulk-delete/` |
| `design/technology-stack.md` | `docs/architecture/folder-group-bulk-delete/` |
| `distill/features/folder-group-delete.feature` | `docs/scenarios/folder-group-bulk-delete/` |
| `distill/features/integration-checkpoints.feature` | `docs/scenarios/folder-group-bulk-delete/` |
| `distill/acceptance-test-plan.md` | `docs/scenarios/folder-group-bulk-delete/` |
| `distill/plugin-contract-spec.md` | `docs/scenarios/folder-group-bulk-delete/` |
| `discuss/journey-folder-group-delete.yaml` | `docs/ux/folder-group-bulk-delete/` |
| `discuss/journey-folder-group-delete-visual.md` | `docs/ux/folder-group-bulk-delete/` |
| `discuss/journey-folder-group-delete.feature` | `docs/ux/folder-group-bulk-delete/` |

ADR-010 already lives at `docs/adrs/ADR-010-folder-group-delete-hf-capability.md` (was authored there at DESIGN-time, not in the feature workspace). No ADR migration needed.

Discarded (process scaffolding, captured in this doc): `deliver/{execution-log.json, roadmap.json, roadmap-summary.md, phase4-review.md, phase5-mutation-results.md}`, `design/peer-review.md`, `distill/{acceptance-review.md, step-definitions-skeleton.md}`, `discuss/{dor-checklist.md, prioritization.md, shared-artifacts-registry.md, peer-review.md, story-map.md, acceptance-criteria.md, requirements.md, user-stories.md, outcome-kpis.md}`, all `*/wave-decisions.md`. The `discuss/*` business artifacts (requirements, ACs, KPIs) are not migrated because the user-facing behavior is now codified in the .feature files (migrated to `docs/scenarios/`) and the architecture is codified in the design docs (migrated to `docs/architecture/`). Future contributors who want to know "why was it built this way" land here; "what does it do" lands in the scenarios.

## 9. Next-feature impact

- The `Tool::delete_folder` default-method pattern is reusable for any future "destructive bulk capability" that's tool-specific (e.g., Ollama-specific blob-GC, llama-cli-specific cache prune).
- The `MODELTAP_TEST_*` env-var seam pattern (D4) cleanly handled fault injection in a CLI tool without a `FakeFsOps` port; both `MODELTAP_TEST_EBUSY_PATHS` here and `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS` in the SQLite-cache feature follow the same shape and gating discipline.
- The single-engine invariant (D-FGD-4 / AC-13) is now the precedent for any future "per-X compatibility decision that needs to be consistent with the row indicator" — call the same `compute_indicator`, don't shadow it.
