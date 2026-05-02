# Evolution Archive — cross-tool-model-unify

**Feature**: cross-tool-model-unify (UMR-style cross-tool unification via hardlinks)
**Wave**: DELIVER (final, wave 6 of 6)
**Date completed**: 2026-05-02
**Status**: ✅ APPROVED — production-ready

## Outcome

Devon (the persona) can now press `[u]` from the main view on a row marked `=` and watch the duplicated model file collapse from N separate inodes onto one shared inode across multiple AI tools (Ollama, HF cache, LM Studio, Atomic Chat). The summary bar honestly reports `Dedup-able: X.Y GB` (replacing the v1 lie of always `0 B`); pressing `u` builds a plan, opens a dialog with mates pre-populated, and on `Enter` reclaims disk via `link()`. A new `[All Unified]` synthetic left-pane slot lets users audit cumulative unification state.

## Delivery Summary

- **Commits**: 28 (across 26 roadmap steps + 1 log-format housekeeping + 1 refactor pass)
- **Tests**: 433 → 568 passed (+135), 0 failed, 0 ignored
- **Lines changed**: ~6500 insertions across 65 files
- **Test budget**: well under the 86-unit-test budget (used ~31 explicit unit tests; behavioral coverage dominantly via real-IO acceptance tests)
- **Time wall-clock**: 2 sessions over ~2 days

## Quality Gates Passed

| Phase | Gate | Result |
|-------|------|--------|
| Phase 1 — Roadmap | review approved (1 attempt) | ✓ |
| Phase 2 — Execute | 26/26 steps full TDD (PREPARE/RED_ACCEPTANCE/RED_UNIT/GREEN/COMMIT) | ✓ |
| Phase 3 — Refactor | 1 high-value refactor (`format_size` consolidation) | ✓ partial |
| Phase 4 — Adversarial Review | APPROVED, 0 findings | ✓ |
| Phase 5 — Mutation | 88.2% kill rate (30 killed / 4 missed / 2 unviable) — gate ≥80% | ✓ |
| Phase 6 — Integrity Verification | exit 0, all 26 steps audit-clean | ✓ |
| Phase 7 — Finalize | this document + final commit | (in progress) |

## Walking Skeleton

The walking-skeleton scenario `devon_reclaims_disk_by_unifying_a_duplicated_model_from_the_main_view` (us_u1_walking_skeleton.rs) is the single end-to-end proof:
1. Two-tool fixture: identical 4096-byte payload at separate inodes (Ollama blob + HF cache blob)
2. Launch with `MODELTAP_HEADLESS=1` and script `<hash-complete>u<enter>q`
3. Hash pool runs to completion (sentinel resolves)
4. `u` keypress → glyph-aware dispatch → unify dialog opens
5. `<enter>` confirms → `actions::unify::run` executes
6. `q` quits cleanly within 200ms shutdown budget
7. **Post-condition**: ollama + hf blobs share one inode; JSONL log shows exactly 1 `action.unify` event with `outcome="success"`

## Key Architectural Decisions Locked

- **ADR-013**: Background SHA256 hash pool with `min(num_cpus, 4)` workers, 250ms throttle, 200ms cancellable shutdown via `tokio_util::sync::CancellationToken`. CPU-bound work via `spawn_blocking`. `MODELTAP_HASH_WORKERS` undocumented escape hatch.
- **ADR-014**: Synthetic left-pane slots (`[All Unified]`) are render-only — never `impl Tool`, never in plugin registry. `LeftPaneSlot::Real(ToolView) | LeftPaneSlot::Synthetic(SyntheticSlot)` heterogeneous list.
- **ADR-002**: SHA256 conservative-when-uncertain — failed hashes classify as Unique (not DedupAble), preserving the safety property.
- **ADR-003**: Stateless rediscovery preserved — hash cache lives in process memory only; no persistent index file.

## Non-Obvious Technical Wins (Mid-Step Fix-Forwards)

1. **Step 01-08**: `hash_pool/queue.rs` used bare `tokio::spawn` outside runtime context; the headless main is sync. Fixed by passing `runtime.handle()` through `queue::build()`. Diagnosed mid-step after 25+ acceptance regressions.
2. **Step 01-08 (second)**: Headless runtime was `current_thread`; workers couldn't progress while script driver sync-polled `<hash-complete>`. Fixed by switching to `new_multi_thread`.
3. **Step 01-12**: Pure update layer can't access on-disk paths (AppState carries hashes/inodes only); composition root now resolves plan paths from `discovered` inventory before invoking `actions::unify::run`. Layered split kept the pure/IO boundary intact.
4. **Step 02-02**: `right_pane.rs::build_dedup_inventory` had its own inventory builder that hard-coded `content_hash: None` and ignored hash-pool results. Real bug surfaced and fixed.
5. **Step 03-01**: Detail-screen seam reused for `[All Unified]` informational dialog (`UnifyMode::AlreadyUnified` derived automatically by `from_plan` when all links report `already_linked`).

## Mutation Coverage Notes

4 missed mutants in `crates/modeltap-core/src/logic/dedup.rs`:
- `:174:22` (`==` → `!=`) — self-skip in peer matching
- `:174:37` (`&&` → `||`) — self-skip
- `:174:62` (`==` → `!=`) — self-skip
- `:294:42` (`&&` → `||`) — dedup_summary peer matching

Future hardening: add edge-case unit tests for the self-skip + same-tool-skip branches in `compute_dedup_glyph` (e.g., target with same id_in_tool as a peer in a DIFFERENT tool — currently not asserted).

## Roadmap

26 steps in 6 phases:

| Phase | Steps | Theme |
|-------|-------|-------|
| 01 (Walking Skeleton) | 12 | Domain types → pure dedup fns → AppState refactor → summary_bar wiring → glyph column → hash-pool Msg variants → background SHA256 hash pool → composition-root wiring → harness sentinel → glyph-aware u-keypress → reclassify → activate WS |
| 02 | 2 | Unignore us_u2 (dedup-able-bytes) + us_u3 (glyphs) |
| 03 | 3 | Unignore us_u4 (u-from-main), unify dialog reclaim preview (us_u5 dialog body), us_u5 verify |
| 04 | 3 | collect_unified_rows pure fn → [All Unified] render → us_u7 unignore |
| 05 | 3 | Transient (was X) summary delta → partial-success path → us_u6 unignore |
| 06 | 3 | [All Unified] empty-state (us_u8) → Detail inode proof (us_u9) → partial-success toast (us_u10) |

## Lessons Learned

1. **DES finalization re-dispatch pattern works.** Crafters timed out mid-COMMIT on ~7 of 26 steps; a focused finalization re-dispatch with the full 9-section DES template + explicit "skip phases already logged" instruction reliably closed each one.
2. **Pure/IO boundary needs explicit path-resolution seam.** Steps 01-10 + 01-12 revealed that pure update handlers can't access filesystem state, so the composition root must enrich plans before invoking actions. The `discovered` slice + `resolve_plan_paths` helper is the canonical seam.
3. **Multi-thread tokio is required for sync-driver + async-workers.** A `current_thread` runtime deadlocks when the driver loop sync-polls for completion; `new_multi_thread` is non-negotiable.
4. **Synthetic left-pane slots ≠ Tool plugins.** ADR-014's render-only constraint kept the action orchestrators clean. Defensive `if name == "All Unified" skip` would have spread across the codebase; the type-system enforcement avoided that.
5. **Reviewer hot path.** Phase 4 adversarial review running on Haiku flagged zero issues across 28 commits — strong signal that TDD discipline + DES enforcement caught problems early.

## Files Modified (Top-Level)

- `crates/modeltap-core/`: 5 source files (domain types, dedup logic), 4 test files
- `crates/modeltap-tui/`: 14 source files (app_state, dialogs, render modules, update handlers), 7 test files
- `crates/modeltap-app/`: 9 source files (hash_pool/, reclassify, headless/interactive wiring), 13 test files
- `docs/feature/cross-tool-model-unify/`: full DISCUSS / DESIGN / DISTILL / DELIVER artifact set (pre-existing)
- `docs/adrs/`: ADR-013 + ADR-014 (added pre-DELIVER)

## Next Iteration

Returns to **DISCOVER** for the next feature, or marks the project ready for v1.0 release.
