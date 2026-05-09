# Evolution Archive — arrow-keys-navigate-tools

**Feature**: arrow-keys-navigate-tools (focus-aware Up/Down dispatch in left pane)
**Wave**: DELIVER (single-feature increment, post-v1)
**Date completed**: 2026-05-09
**Status**: APPROVED — production-ready
**Step-ID**: 01-01 (single-step roadmap)

## Outcome

When the left (tools) pane has focus, the Up/Down arrow keys now navigate the
vertical list of tools instead of moving the row cursor in the right (models)
pane. The bottom help bar tells the truth in both focus states:

- Left focused: `[up/down] tools  [tab] focus  [<-/->] tools  ...`
- Right focused: `[up/down] models [tab] focus  [<-/->] tools  ...`

Tab continues to toggle focus and Left/Right continue to step the tool selection
regardless of focus, so muscle-memory shortcuts are preserved. Inside the unify
dialog (US-U5) Up/Down are completely unchanged — `dispatch_in_dialog` was not
touched.

## Why It Was Needed

In the v1 TUI the Up/Down arrows always navigated the right-pane model list,
even when the left tool pane had focus. The bottom help bar reinforced this by
labelling them `[up/down] models` unconditionally. The result was a paper-cut
UX bug:

- Vertical arrows did nothing useful for the most natural target (the visually
  vertical tool list on the left), forcing users to learn that Tab+Left/Right
  was the only way to change tools.
- The help bar's static label was a small but real lie when the left pane was
  active.

This feature closes that gap: vertical arrows for the vertical list, with the
bar honestly reflecting which list they will move.

## Implementation Summary

Two pure changes in `modeltap-tui` plus a thin composition-root threading change
in `modeltap-app`:

1. **`crates/modeltap-tui/src/keymap.rs`**
   - New entry point `dispatch_focus_aware(key: KeyEvent, focus: FocusPane) -> Msg`
     replaces the old focus-blind `dispatch` for the main screen path. When
     `focus == FocusPane::Left`, `KeyCode::Up` returns `Msg::SelectPrevTool` and
     `KeyCode::Down` returns `Msg::SelectNextTool`; when `focus == FocusPane::Right`,
     they fall through to the existing row-navigation behaviour
     (`Msg::SelectPrevRow` / `Msg::SelectNextRow`).
   - New helper `up_down_bar_label(focus) -> &'static str` returns
     `"[up/down] tools"` or `"[up/down] models"` so the bar renderer and the
     dispatch path share one source of truth for the wording.
   - `SHORTCUT_TABLE` invariant preserved: the table still has a single
     `[up/down]` entry; the focus-aware behaviour is layered on top via a
     post-lookup substitution branch in `render_bottom_bar`. Existing tests
     `shortcut_table_drives_both_render_label_and_dispatch_msg` and
     `int_6_invariant_every_visible_bar_key_dispatches_to_non_noop` were
     extended to cover both focus states.

2. **`crates/modeltap-tui/src/render/bottom_bar.rs`**
   - `BarContext` now carries `focus: FocusPane` (plumbed through `for_state`).
   - `render_bottom_bar` performs a focus-aware substitution for the Up/Down
     label spans only (lines 131–132). All other shortcut spans are
     untouched, so the SHORTCUT_TABLE-as-source-of-truth invariant holds.

3. **`crates/modeltap-app/src/interactive.rs` and `crates/modeltap-app/src/headless.rs`**
   - Composition root now passes `state.focus` through to
     `dispatch_focus_aware` (and to its `token_to_msg` mirror in the headless
     script driver). The headless mirror is intentional: `headless.rs` runs a
     scripted-token loop that does not see real `KeyEvent` values, so its own
     small focus-aware translation table mirrors the keymap. The mirror is
     documented inline.

4. **`crates/modeltap-tui/tests/architecture.rs`**
   - Added `[up/down] tools` to `FORBIDDEN_TOKENS` so any future refactor that
     accidentally hard-codes the focus-blind label outside `up_down_bar_label`
     will trip the architecture lint.

5. **Snapshot updates** in
   `crates/modeltap-tui/tests/snapshots/render_delete_one_dialog__delete_one_dialog_*.snap`
   for the bottom-bar wording change.

## Acceptance Criteria

All eleven roadmap ACs are satisfied with extended test coverage:

- [x] AC-1: When `FocusPane::Left` is active, `KeyCode::Up` dispatches `Msg::SelectPrevTool`.
- [x] AC-2: When `FocusPane::Left` is active, `KeyCode::Down` dispatches `Msg::SelectNextTool`.
- [x] AC-3: When `FocusPane::Right` is active, `KeyCode::Up` dispatches `Msg::SelectPrevRow` (regression: existing behaviour preserved).
- [x] AC-4: When `FocusPane::Right` is active, `KeyCode::Down` dispatches `Msg::SelectNextRow` (regression: existing behaviour preserved).
- [x] AC-5: `KeyCode::Left` dispatches `Msg::SelectPrevTool` and `KeyCode::Right` dispatches `Msg::SelectNextTool` regardless of focus.
- [x] AC-6: `KeyCode::Tab` dispatches `Msg::ToggleFocus` regardless of focus (no regression).
- [x] AC-7: Bottom-bar Up/Down label reads `[up/down] tools` when `FocusPane::Left` and `[up/down] models` when `FocusPane::Right`.
- [x] AC-8: SHORTCUT_TABLE source-of-truth invariant preserved: `shortcut_table_drives_both_render_label_and_dispatch_msg` and `int_6_invariant_every_visible_bar_key_dispatches_to_non_noop` pass, extended to cover both focus states.
- [x] AC-9: Composition root in `modeltap-app` (`interactive.rs` and `headless.rs`) passes `state.focus` to the focus-aware dispatch entry point.
- [x] AC-10: Dialog dispatch (`dispatch_in_dialog`) unchanged: arrow keys in the unify dialog continue to drive `UnifyDialogSelectPrev/Next` per US-U5.
- [x] AC-11: `cargo fmt --all` clean, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo test --workspace` all green.

## Key Engineering Decisions

1. **Single-step decomposition (not split).** The roadmap reviewer considered
   splitting dispatch from label rendering into separate steps. Rejected:
   the truthfulness invariant ("the bar's label matches what the dispatch will
   actually do") couples the two concerns at the test boundary. A single TDD
   cycle that lands both keeps the SHORTCUT_TABLE invariant tests honest at
   every commit boundary.

2. **Renamed dispatch entry point to `dispatch_focus_aware(key, focus)`.**
   Rather than overloading the existing `dispatch(key)` with a second arity,
   the new public function has its own name. Callers that only ever needed
   focus-blind dispatch (the dialog path) keep using the unchanged
   `dispatch_in_dialog`. This makes the call-graph self-documenting: any
   `dispatch_focus_aware` call site is contractually obligated to thread the
   focus through.

3. **Cross-crate `token_to_msg` mirror in `headless.rs`.** The headless script
   driver translates string tokens (e.g. `"up"`, `"down"`) into `Msg` values
   without ever constructing a `KeyEvent`. Rather than route through
   `dispatch_focus_aware` (which would require synthesising `KeyEvent`s), the
   mirror table in `headless.rs` performs the same focus-aware branch
   directly. The duplication is small (two lines), well-commented, and
   covered by the existing US-03 acceptance test
   `us_03_two_pane_navigation` which now exercises both focus states.

4. **Architecture-lint guard rail.** Added `[up/down] tools` to
   `FORBIDDEN_TOKENS` in `crates/modeltap-tui/tests/architecture.rs`. Any
   future refactor that hard-codes the label outside `up_down_bar_label`
   will fail the architecture test before it can land. This is the
   "shift-left quality gate" pattern: catch the regression at the lint
   stage, not in a downstream snapshot diff.

## Mutation Testing Outcome

Per-feature kill-rate gate (CLAUDE.md): ≥80% required.

| Run | Caught | Missed | Kill rate |
| --- | --- | --- | --- |
| Initial (pre-fix) | 33 | 18 | 64.71% — FAIL |
| Final (after `unit_mutation_kill_keymap_bottom_bar.rs`) | 49 | 2 | **96.08% — PASS** |

The two surviving mutants are both in the file-private helper `no_color_active`
in `bottom_bar.rs:204` (mutating its return to `true` or `false`). Both are
provably **equivalent mutants**: `no_color_active()` is currently called once,
its return value is bound to a leading-underscore parameter
(`_no_color: bool`) inside `render_bottom_bar`, and that parameter is unused
by the function body (styling is governed by `Modifier::DIM` /
`Modifier::CROSSED_OUT` which ratatui automatically downgrades when the
terminal advertises `NO_COLOR`). Killing these would require either (a)
adding I/O assertions to unit tests (violates the test pyramid) or
(b) removing the unused parameter (production-code change, out of scope for
Phase 5). Documented in `docs/feature/arrow-keys-navigate-tools/deliver/mutation/mutation-report.md`.

## Quality Gates Passed

| Phase | Gate | Result |
| --- | --- | --- |
| Phase 1 — Roadmap | reviewer approved (`nw-solution-architect-reviewer`, 2026-05-09T00:25:00Z) | PASS |
| Phase 2 — Execute | full TDD (PREPARE → RED_ACCEPTANCE → RED_UNIT → GREEN → COMMIT) on the single step | PASS |
| Phase 3 — L1-L4 Refactor | no actionable smells; no commit | PASS (no-op) |
| Phase 4 — Adversarial Review | rejected (test-circularity flag) → revision applied → consolidated test in `113f5fd` | PASS after revision |
| Phase 5 — Mutation | 96.08% kill rate (49/51 viable); 2 equivalents documented | PASS |
| Phase 6 — Integrity Verification | `All 1 steps have complete DES traces` | PASS |
| Phase 7 — Finalize | this document | in progress |

## Commit Chain

The feature lands as exactly three commits on `main`:

1. **`3594c5c feat(tui): focus-aware up/down dispatch in left pane`**
   — Main implementation. Adds `dispatch_focus_aware`, `up_down_bar_label`,
   `BarContext.focus` field, the substitution branch in `render_bottom_bar`,
   composition-root threading in `interactive.rs`/`headless.rs`, the
   architecture-lint addition, and full TDD test coverage. 10 files,
   +394/-41.

2. **`113f5fd test(tui): consolidate focus-aware label assertions per reviewer feedback`**
   — Phase 4 revision. Consolidates the
   `render_bottom_bar_main_up_down_label_flips_with_right_focus` assertion
   pair into a single non-circular test for visibility on isolated read.
   No production code change. 1 file (`unit_tdd_us08.rs`), +35/-34.

3. **`f5e968a test(tui): kill surviving mutants in keymap focus-aware dispatch`**
   — Phase 5 mutation hardening. Adds targeted unit tests that kill the
   16 pre-existing mutants pulled into scope by the file globs (Ctrl+C
   uniqueness, dialog routing, BarContext::for_state Detail capture,
   strikethrough assertions, multi-tool detail bar). Brings kill rate
   from 64.71% to 96.08%. No production code change.

## File-Level Summary of What Changed

Production code (3 files):
- `crates/modeltap-tui/src/keymap.rs` — added `dispatch_focus_aware`,
  `up_down_bar_label`; preserved `dispatch_in_dialog` and `SHORTCUT_TABLE`.
- `crates/modeltap-tui/src/render/bottom_bar.rs` — added `BarContext.focus`;
  added focus-aware substitution branch in `render_bottom_bar`.
- `crates/modeltap-app/src/interactive.rs` — wired `state.focus` to
  `dispatch_focus_aware`.
- `crates/modeltap-app/src/headless.rs` — focus-aware mirror in token table.

Tests (5 files + 2 snapshots):
- `crates/modeltap-tui/src/keymap.rs` (in-module unit tests)
- `crates/modeltap-tui/tests/unit_tdd_us03.rs` (focus-aware dispatch ACs)
- `crates/modeltap-tui/tests/unit_tdd_us08.rs` (bar-label substitution; consolidated in 113f5fd)
- `crates/modeltap-tui/tests/architecture.rs` (FORBIDDEN_TOKENS guard rail)
- `crates/modeltap-tui/tests/unit_mutation_kill_keymap_bottom_bar.rs` (added in f5e968a)
- `crates/modeltap-app/tests/acceptance/us_03_two_pane_navigation.rs` (extended)
- `crates/modeltap-tui/tests/snapshots/render_delete_one_dialog__delete_one_dialog_{shared,unique}_mode.snap` (label updates)

## Mutation Report

Full mutation-testing artefact preserved at:
`docs/evolution/arrow-keys-navigate-tools/mutation-report.md` (migrated from
the workflow-only `docs/feature/arrow-keys-navigate-tools/deliver/mutation/`
which was removed during finalize).

The report contains the full per-mutant breakdown, equivalence proof for the
two `no_color_active` survivors, and the reproduction command.
