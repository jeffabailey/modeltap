# Evolution Archive — gpt4all-plugin

**Feature**: gpt4all-plugin (5th production plugin: GPT4All)
**Wave**: DELIVER (final, wave 6 of 6)
**Date completed**: 2026-05-03
**Status**: ✅ APPROVED — production-ready

## Outcome

Devon now sees a `gpt4all` slot in the modeltap left pane alongside ollama / hf / lm-studio / atomic-chat. The plugin walks GPT4All's two default directories (Python SDK at `~/.cache/gpt4all/` plus the desktop chat app at OS-specific paths under `nomic-ai/gpt4all-chat/`), discovers `*.gguf` files, and integrates transparently with the existing dedup/unify infrastructure from `cross-tool-model-unify`. A GPT4All blob with the same SHA256 as an Ollama blob shows the `=` glyph and unifies via the existing `[u]` keypress flow.

## Delivery Summary

- **Commits**: 9 (all 8 roadmap steps + 1 mutation hardening pass)
- **Tests**: 592 → 627 passed (+35 new), 0 failed, 0 ignored
- **Lines changed**: ~2300 across ~35 files (new plugin crate + composition root wiring + 26 test-file env-var carving + 2 new acceptance tests)
- **Mutation kill rate**: 100% (53/53 viable mutations killed; 16 unviable)
- **Time wall-clock**: ~2 hours (single session)

## Quality Gates Passed

| Phase | Gate | Result |
|-------|------|--------|
| Phase 1 — Roadmap | auto-approved (small bounded plugin add) | ✓ |
| Phase 2 — Execute | 8/8 steps full TDD | ✓ |
| Phase 3 — Refactor | dead code removed during mutation pass; no separate refactor needed | ✓ (folded into Phase 5) |
| Phase 4 — Adversarial Review | APPROVED, 0 critical/major findings | ✓ |
| Phase 5 — Mutation | **100% kill rate** (53 killed / 0 missed / 16 unviable) — gate ≥80% | ✓ |
| Phase 6 — Integrity Verification | exit 0 (roadmap schema warnings informational only) | ✓ |
| Phase 7 — Finalize | this document + final commit | (in progress) |

## Roadmap

8 steps in 1 phase:

| Step | Commit | Theme |
|------|--------|-------|
| 01-01 | `c274474` | Crate skeleton (Cargo.toml + lib.rs + inventory::submit!) |
| 01-02 | `ff27cc1` | paths + config (env-var override per-OS defaults) |
| 01-03 | `660063d` | discover() walking *.gguf files |
| 01-04 | `557af7c` | Wire into composition root (deliberately broke 3 v1 count assertions) |
| 01-06 | `179106e` | Sweep plugin-count assertions + env carving across 26 test files |
| 01-05 | `e3a224d` | link/delete_one/delete_all (ADR-008 EXDEV → CrossFilesystem) |
| 01-07 | `57f9f03` | us_gpt4all_discovery acceptance scenarios |
| 01-08 | `b249cf3` | us_gpt4all_cross_tool_unify (proves full value prop end-to-end) |
| (post) | `f82cfd5` | Mutation hardening: 52.8% → 100% kill rate |

(Note: 01-06 executed before 01-05 to restore green workspace — see review M1.)

## Storage Layout (verified during intake, 2026-05-02)

| Front-end | Path |
|-----------|------|
| Python SDK | `~/.cache/gpt4all/` (all platforms) |
| Desktop app (macOS) | `~/Library/Application Support/nomic-ai/gpt4all-chat/` |
| Desktop app (Linux/WSL) | `~/.local/share/nomic-ai/gpt4all-chat/` |

Both layouts are flat directories of `*.gguf` files. No manifest layer. Env var `MODELTAP_GPT4ALL_DIRS` (colon-separated) overrides defaults.

## Architectural Decisions Honored

- **ADR-001 (Tool trait frozen)**: gpt4all is `impl Tool` with all 6 methods; no trait extension.
- **ADR-005 (spawn_blocking for sync I/O)**: discover/link/delete wrapped in `tokio::task::spawn_blocking`.
- **ADR-008 (orchestrator owns cross-fs decision)**: EXDEV → `LinkError::CrossFilesystem` (NOT silent auto-copy). Crafter caught this nuance in 01-05 — the roadmap text said "fall back to copy" but ADR-008 explicitly puts that decision on the orchestrator's CrossFsChoice dialog.
- **ADR-013 (background hash pool)**: GPT4All blobs flow through unmodified; cross-tool dedup test (`us_gpt4all_cross_tool_unify`) proves it.
- **ADR-014 (synthetic slots)**: No new synthetic slots; `[All Unified]` picks up gpt4all unifications transparently.

## Non-Obvious Wins

1. **No core changes required**. The cross-tool-model-unify infrastructure (hash pool, dedup classifier, unify orchestrator, synthetic [All Unified] slot, post-unify reactivity) all worked unchanged for gpt4all. Adding a 5th plugin only required: (a) new plugin crate, (b) one Cargo.toml dep, (c) one `as _` import line. The Tool trait contract paid for itself.

2. **Step 01-06 was its own step**. Splitting "wire composition root" (01-04) from "sweep test assertions" (01-06) into separate commits made each diff surgical and reviewable. The temporary 3-test failure between 01-04 and 01-06 was a deliberate trade-off documented in the roadmap.

3. **ADR deviation caught at implementation time**. The 01-05 crafter compared the roadmap text against ADR-008 + existing plugin patterns (lm-studio, ollama, loose-gguf — all return `CrossFilesystem` error) and chose to follow the established pattern over the roadmap text. Good craftsmanship.

4. **Mutation hardening one-shot**: Initial run was 52.8% kill rate (25 missed). One focused pass added 19 tests + cleaned up dead code (defensive guards) → 100%. Tests are mutation-driven and tight, not just coverage.

## Lessons Learned

1. **Small bounded features can skip nWave waves cleanly.** This delivery had no DISCUSS/DESIGN/DISTILL — the plugin contract was already established (ADR-001), and the intake brief was the sole prior-wave context. The architect produced an 8-step roadmap directly from the brief. Workflow scaled down naturally.

2. **Multi-step TDD with intentional inter-step RED.** Step 01-04 deliberately committed wiring that broke 3 v1 tests; step 01-06 unblocked them. This Outside-In RED-then-GREEN at the workspace level is valid TDD discipline — it's not "test breaking" but "isolating the change."

3. **Env-var pinning is now a tax on every plugin add.** The 26-file env-carving sweep in 01-06 is mechanical but unavoidable until either (a) tests share a common pinning helper, or (b) modeltap-app provides a "no plugins discover" test mode. Worth considering as a follow-up.

## Files Modified (Top-Level)

- `plugins/gpt4all/`: 6 source files + 1 test file (new crate)
- `crates/modeltap-app/`: 2 source files (Cargo.toml + main.rs), 26 test files updated
- `crates/modeltap-app/tests/acceptance/`: 2 new acceptance test files (us_gpt4all_discovery, us_gpt4all_cross_tool_unify)
- `Cargo.toml` (workspace root): added `plugins/gpt4all` to members
- `docs/feature/gpt4all-plugin/`: full intake + roadmap + execution log + adversarial review

## Next Iteration

Returns to **DISCOVER** for the next feature, OR `/nw:new` for a new feature.
