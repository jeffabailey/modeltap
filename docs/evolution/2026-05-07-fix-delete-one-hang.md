# Evolution Archive — fix-delete-one-hang

**Feature**: fix-delete-one-hang (delete-one keystroke `[d]` appeared to hang the production TUI)
**Wave**: DELIVER (single-phase bug-fix increment, post-v1)
**Date completed**: 2026-05-07
**Date finalized**: 2026-05-27
**Status**: APPROVED — shipped in v0.2.x line, validated in production
**Step-IDs**: 01-01, 01-02, 01-03 (three TDD cycles, one per root cause)

## Outcome

Pressing `[d]` (delete-from-one) in the production interactive TUI now does what the bottom bar advertises in every cursor position the bar offers it:

- **Main screen, real tool column selected**: a confirmation modal renders over the panes — Shared mode shows `[y]/[n]` footer, Unique mode shows a typed-input echo and `Type the model id, then [Enter]` instruction. The user can see the dialog and act on it.
- **Detail screen**: `[d]` opens the same dialog targeted at the focused tool's first registration. Previously a silent no-op.
- **Main screen, Unified virtual column**: the bottom bar now dims `[d]` so the keymap promise matches what the orchestrator can fulfil — no more silent press.

The fix also added the first TestBackend+`insta` snapshot test in the repo, exercising `modeltap_tui::view()` directly rather than going through the headless harness — closing the production-loop coverage gap that let the original defect slip through US-05b acceptance.

## Why It Was Needed

A field bug report — "pressing `d` hangs the UI and doesn't complete" — looked superficially like an async/`block_on` deadlock. RCA revealed three independent root causes, all of them production-loop-only:

- **Cause A — Missing render.** `state.delete_one_dialog` was constructed correctly but no `render/delete_one_dialog.rs` module existed and `layout.rs::view_main` never overlaid it. The event loop flipped into dialog-modal mode (`translate_key` at `interactive.rs:318-323`), so subsequent keystrokes routed through `dispatch_in_dialog` and silently mutated the invisible typed buffer. Only `Esc` could escape. Textbook "frozen UI" signature.
- **Cause B — Detail lift dead code in production.** `lift_delete_one_in_detail` existed in `headless.rs:918-961` and was invoked by the headless orchestrator + every US-05b acceptance test, but `interactive.rs::translate_key` only chained `lift_delete_one_in_main`. Detail-screen `[d]` fell through to `update.rs:224`'s no-op match arm.
- **Cause C — Bar advertises an unfulfillable promise.** `is_available_main` in `render/bottom_bar.rs` did not gate `[d]` on `current_tool_has_models`, so the bar showed `[d] delete-from-one` even when the cursor was on the Unified virtual column. `lift_delete_one_in_main` early-returns when `current_tool() is None` — silent no-op for the user.

The US-05b acceptance suite passed throughout because it drives `headless::run_scripted` (which sets `MODELTAP_HEADLESS_DETAIL_REGS` + `MODELTAP_HEADLESS_DELETE_TARGET` and bypasses `interactive.rs`'s lift chain) and asserts on filesystem state + JSONL events, never on a captured frame containing the dialog. The defects lived precisely in the gap between the headless harness and the production event loop.

## Implementation Summary

Three fixes, one TDD cycle each, sequential per RCA-cause ordering:

1. **`crates/modeltap-tui/src/render/delete_one_dialog.rs`** (NEW, 107 lines) — overlay renderer. Mirrors `render/unify_dialog.rs` structure (`Block` + `Paragraph` + centered `Rect` helper). Renders Shared-mode footer (`[y]/[n]/Esc`) vs Unique-mode (typed-input echo + `Type the model id, then [Enter]`).
2. **`crates/modeltap-tui/src/render/mod.rs`** — `pub mod delete_one_dialog;`.
3. **`crates/modeltap-tui/src/layout.rs`** — overlay guard added in two sites: `view_main` (Main view, before the running-tool overlay so running-tool wins layering) and the Detail branch of `view` (before its running-tool overlay).
4. **`crates/modeltap-app/src/interactive.rs`** — new `lift_delete_one_in_detail` ported from `headless.rs:918-961`, dropping the test-only `MODELTAP_HEADLESS_DELETE_TARGET` / `_ID_IN_TOOL` env-var seams. Targets `detail.registrations.first()` and uses `detail.registrations.len() >= 2` for `was_shared`. Chained after `lift_delete_one_in_main` in `translate_key`.
5. **`crates/modeltap-tui/src/render/bottom_bar.rs`** — `is_available_main` extended with `KeyCode::Char('d') => ctx.current_tool_has_models`, mirroring the existing `[u]`/`[z]` gating.
6. **New regression nets** — `crates/modeltap-tui/tests/render_delete_one_dialog.rs` (TestBackend + `insta` snapshot for Shared and Unique modes), `crates/modeltap-app/tests/unit_lift_delete_one_in_detail.rs` (Detail-lift unit test), `crates/modeltap-tui/tests/unit_bottom_bar_dim_d.rs` (both arms of the `is_available_main` predicate).

Fix commits (in DELIVER order):

| Step | Commit | Cause | Files |
|---|---|---|---|
| 01-01 | `b57f62d` `fix(tui): render delete-one confirmation dialog (RCA cause A)` | A | 4 changed (render module + layout overlays + Cargo.toml dev-deps + snapshot test) |
| 01-02 | `bd0abef` `fix(app): lift DeleteFromOne on Detail in interactive loop (RCA cause B)` | B | 1 changed (`interactive.rs`, +212 lines) |
| 01-03 | `2d7e67f` `fix(tui): dim [d] on Main when no real tool is selected (RCA cause C)` | C | 2 changed (`bottom_bar.rs` + unit test) |

Workspace-archive commit (during DELIVER, pre-finalize): `6da458b` `chore(deliver): archive fix-delete-one-hang RCA, roadmap, and DES log` — first introduction of the `lat.md/` seed in this repo.

## Key Decisions

1. **Three independent fixes, not one.** RCA branches A, B, C are at distinct call sites (render layer vs. orchestrator lift vs. keymap gating). Each is independently shippable and independently testable. Bundling would have made the regression-test design harder; sequencing them as three TDD cycles let each step's RED test target exactly one cause.
2. **Snapshot-test `view()` directly, not through the headless harness.** The headless harness drives `update()` and `view()` through scripted tokens and is a strong asset for state-transition + side-effect tests — but it bypasses the production `interactive.rs` lift chain. The bug existed precisely in that gap. The new `tests/render_delete_one_dialog.rs` constructs an `AppState` with `delete_one_dialog: Some(...)`, drives `modeltap_tui::view()` against a `TestBackend`, and asserts a captured frame snapshot. This is the canonical ratatui testing recipe ([ratatui.rs/recipes/testing/snapshots](https://ratatui.rs/recipes/testing/snapshots/)). `insta` was already declared as a workspace dependency but had never been instantiated; this feature was the first concrete use.
3. **`ratatui-testlib` deferred.** Considered and rejected for this fix. It targets PTY-level testing (terminal-size negotiation, image protocols, ANSI handling) which modeltap doesn't need, and pulls in Bevy ECS + an async runtime as transitive deps. The TestBackend+`insta` recipe covers every concrete defect this RCA identified.
4. **Detail lift in production drops env-var seams.** `MODELTAP_HEADLESS_DELETE_TARGET` / `_ID_IN_TOOL` are test-only knobs in the headless harness. The production port targets `detail.registrations.first()` directly. If Detail later grows row-cursor navigation, the "first registration" choice will need to be updated to match — flagged as a known follow-up in the RCA.
5. **No change to the `block_on` chain in `apply_effect`.** H3 (slow `plugin.discover()` on a multi-GB HF cache freezing the loop) was raised by RCA as a real *secondary* latent risk but is not the cause of this report. Time-budgeting the discover on the destructive-keystroke path is a separate concern, tracked but not landed here.

## Lessons Learned

- **A passing headless acceptance suite is not the same as a passing production loop.** Every defect in this RCA was invisible to US-05b because the harness's lift functions and render-overlay calls bypass the production code paths. The minimum repair is a small set of `view()`-direct tests (snapshot, not text-grep) that exercise the render layer at the production boundary. The harness should keep its role for state-transition and side-effect tests; it is not the right tool for "is this rendered?"
- **Silent-modal mode is the most misleading UI failure available.** When the event loop is in dialog-modal mode with no visible modal, the user has no affordance — the bottom bar still advertises Main keys, but every keystroke routes through `dispatch_in_dialog`. The signature reads as "the UI is frozen" even though the loop is correctly polling. Any future feature that adds a new dialog state must add a render overlay in the same commit (and ideally the snapshot test in the same commit too) — these are an inseparable atomic increment.
- **The bottom bar's `is_available` predicates are part of the keymap contract.** A key advertised by the bar should produce a visible effect; if it can't, the bar must dim it. The `[u]`/`[z]` gating pattern at `bottom_bar.rs:140-162` is the convention to follow; any new destructive keystroke should be added there.

## Risks / Follow-ups Deferred

- **H3 — slow `discover()` freezing the loop on post-confirmation path.** Real secondary issue, not triggered by the original report (it can only fire after the dialog resolves, which Causes A/B prevented). Recommended follow-up: time-budget the discover on destructive keystrokes or resolve paths before showing the dialog. Not in scope for this fix.
- **Detail row-cursor navigation.** If/when `DetailScreenState` grows multi-row navigation per-tool, the production `lift_delete_one_in_detail`'s "first registration" target will need to track the focused row.
- **Brief-highlight/toast on unbound keys.** RCA's Cause C had two solutions: (a) dim the bar entry (chosen, cheap, consistent), or (b) emit a brief-highlight/toast effect when the key would no-op. (a) shipped; (b) tracked separately as a UX improvement.

## Steps Completed

| Step | Title | Closed by |
|---|---|---|
| 01-01 | Render and overlay delete-one dialog | `b57f62d` |
| 01-02 | Lift `DeleteFromOne` on Detail screen in interactive | `bd0abef` |
| 01-03 | Dim `[d]` when no real tool selected | `2d7e67f` |

All three steps recorded `RED_ACCEPTANCE → GREEN → COMMIT` cycles in `deliver/execution-log.json` (`RED_UNIT` skipped per "RED_ACCEPTANCE is the unit-level coverage" — these are all small, one-cause fixes with a single snapshot or unit test). No mutation-testing gate (out of scope for a 3-step bug-fix increment).

## Links

- **Supporting artifact** (preserved in this archive): [`fix-delete-one-hang/rca.md`](./fix-delete-one-hang/rca.md) — full Toyota 5-Whys RCA with Branches A–F, hypothesis ranking, file:line references, ratatui integration-testing recommendation.
- **Workspace** (preserved per nWave convention so the wave matrix derives status): `docs/feature/fix-delete-one-hang/`.
- **Fix commits**: `b57f62d`, `bd0abef`, `2d7e67f`.
- **Workspace-archive commit**: `6da458b` (introduced `lat.md/` seed).
- **Related feature**: [`modeltap-tui-v1-evolution.md`](./modeltap-tui-v1-evolution.md) — the parent feature whose US-05b acceptance suite this RCA validated and whose coverage gap this fix closed.
- **Ratatui testing recipe**: https://ratatui.rs/recipes/testing/snapshots/ (now the project's canonical render-layer test pattern).
