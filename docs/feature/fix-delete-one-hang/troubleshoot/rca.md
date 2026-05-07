# RCA: "Pressing 'd' (delete-one) hangs the UI and doesn't complete"

- **Author:** Rex (Root Cause Analysis)
- **Date:** 2026-05-07
- **Methodology:** Toyota 5 Whys, multi-causal, evidence-required
- **Scope:** Production interactive loop only (`MODELTAP_HEADLESS != 1`). The headless harness and `us_05b` acceptance suite are unaffected.

---

## 1. Problem Statement and Scope

User reports: pressing `d` while running `modeltap` interactively causes the UI to "hang" and the action never completes. Reproduces on the Main view and on the Detail screen.

The bottom bar advertises `[d] delete-from-one` in BOTH `Main` and `Detail` (`crates/modeltap-tui/src/keymap.rs:127-137`), so the user reasonably expects either screen to work.

The headless acceptance suite for US-05b (`crates/modeltap-app/tests/acceptance/us_05b_delete_one.rs:228-509`) all set `MODELTAP_HEADLESS_DETAIL_REGS` + `MODELTAP_HEADLESS_DELETE_TARGET` and run through `headless::run_scripted`. None of them exercise `interactive::run`, so they cannot have caught the production-only defects below.

---

## 2. Five-Whys Branches

### Branch A — Dialog opens (in state) but is never drawn (Main screen 'd')

```
WHY 1A: User presses 'd' on Main; nothing visible happens; subsequent keystrokes
        appear non-functional.
  Evidence: keymap.rs:127-137 dispatches Msg::DeleteFromOne for both
            Main and Detail.

WHY 2A: On Main, translate_key invokes lift_delete_one_in_main, which
        constructs a DeleteOneConfirmState and returns Msg::OpenDeleteOneDialog.
  Evidence: interactive.rs:325 calls lift_delete_one_in_main;
            interactive.rs:339-364 builds the dialog when state.current_tool()
            and tool_view.model_ids[selected_row] are present.

WHY 3A: update() applies the dialog to state but the render layer never reads
        it. After the post-update terminal.draw at interactive.rs:218, the
        terminal frame contains NO overlay for delete_one_dialog.
  Evidence: update.rs:226-232 sets state.delete_one_dialog = Some(dialog).
            layout.rs:111-181 (view + view_main) renders zap_dialog,
            unify_dialog, running_tool_dialog — but NEVER delete_one_dialog.
            `grep -rn delete_one_dialog crates/modeltap-tui/src/render/
             crates/modeltap-tui/src/layout.rs crates/modeltap-tui/src/screens/`
            returns zero matches.
            crates/modeltap-tui/src/render/mod.rs:8-21 lists submodules:
            unify_dialog, zap_dialog, running_tool_dialog — but no
            delete_one_dialog.

WHY 4A: From the user's perspective the app is now in dialog-modal mode (the
        next keystroke routes through dispatch_in_dialog), but with no visible
        modal. Most non-Esc keys fall into Msg::DialogTextInput / DialogConfirm
        / DialogBackspace — all of which mutate the invisible Unique-mode typed
        buffer or no-op. The bar still suggests `[q] quit`, `[d] delete-from-one`,
        etc., yet pressing `q` produces Msg::DialogTextInput('q') which is
        absorbed silently. The screen is frozen-looking.
  Evidence: interactive.rs:318-323 — when ANY dialog is open, translate_key
            routes through keymap::dispatch_in_dialog.
            keymap.rs:196-202 — Char(_) -> Msg::DialogTextInput,
            Enter -> Msg::DialogConfirm, Backspace -> DialogBackspace.
            update.rs:785-795 — DialogTextInput appends to delete_one_dialog
            (Unique mode).
            Esc IS still bound (keymap.rs:197) and will close the dialog via
            decide_delete_one_unique -> Cancel (update.rs:846,863-869), but
            the user has no on-screen affordance to know that.

WHY 5A: ROOT CAUSE A — the delete-one dialog has no view-layer rendering
        at all. The state plumbing (msg, update, app_state, dialog struct) was
        built for US-05b but the render module was never extended with a
        delete_one_dialog overlay. The headless acceptance tests pass because
        they assert on the OUTCOME (file removed, JSONL event) rather than on
        the modal being visible — a TestBackend frame capture would have shown
        a frame identical with vs. without the dialog.
  Evidence: Compare existing modal renderers
            (render/zap_dialog.rs, render/unify_dialog.rs,
             render/running_tool_dialog.rs) — all are wired into layout.rs
            (lines 169-180 view_main; 121-130 Detail). No equivalent file
            for delete_one. tests/acceptance/us_05b_delete_one.rs:295-340
            asserts on stdout JSONL (`action.zap_one`) and on filesystem
            state — never on a captured frame containing dialog text.
```

→ **ROOT CAUSE A:** The delete-one confirmation dialog (`state.delete_one_dialog`) has no corresponding `render/delete_one_dialog.rs` module and no overlay call from `layout.rs::view_main` / `view`'s Detail branch. Pressing 'd' transitions the app into dialog-modal mode silently; the user perceives "the UI is hung."

→ **SOLUTION A:** Add `crates/modeltap-tui/src/render/delete_one_dialog.rs`, register it in `render/mod.rs`, and overlay it from BOTH `view_main` and the Detail branch of `view` — mirroring the `unify_dialog` and `running_tool_dialog` wiring at `layout.rs:121-130, 172-180`. Sketch:

```rust
// layout.rs (added in BOTH view_main near line 173, and Detail branch near line 122)
if let Some(dialog) = state.delete_one_dialog.as_ref() {
    delete_one_dialog::render(frame, area, dialog);
}
```

---

### Branch B — Detail-screen 'd' is a no-op in production (no detail-lift call)

```
WHY 1B: User presses 'd' on Detail; absolutely nothing happens (state
        does not even gain a delete_one_dialog).
  Evidence: keymap.rs:127-137 dispatches Msg::DeleteFromOne on Detail.

WHY 2B: Production translate_key only lifts on Main. The Detail-screen lift
        function exists but is dead code in production.
  Evidence: interactive.rs:325 — only `lift_delete_one_in_main` is called.
            interactive.rs:343 — `if !matches!(state.current_screen,
            Screen::Main) { return msg; }` returns the unchanged
            Msg::DeleteFromOne when on Detail.
            headless.rs:918 defines lift_delete_one_in_detail, but
            `grep -rn lift_delete_one_in_detail crates/` shows it is called
            ONLY at headless.rs:232 — never from interactive.rs.

WHY 3B: update.rs treats the unlifted Msg::DeleteFromOne as a no-op.
  Evidence: update.rs:224 — `Msg::DeleteFromOne => (state, UpdateEffect::default())`.

WHY 4B: The headless harness (which the US-05b acceptance suite drives) has
        the detail-screen lift and a synthesized detail screen via
        MODELTAP_HEADLESS_DETAIL_REGS, so the production-vs-headless gap is
        invisible to the test suite.
  Evidence: us_05b_delete_one.rs:286-289, 363-365, 408-409, 454-455, 504-506
            all set MODELTAP_HEADLESS_DETAIL_REGS + MODELTAP_HEADLESS_DELETE_TARGET
            and run through assert_cmd against the headless binary.

WHY 5B: ROOT CAUSE B — the Detail-screen lift was implemented only in the
        headless orchestrator. Production's interactive.rs was never
        retrofitted with the equivalent lift. The Detail screen needs a lift
        because `lift_delete_one_in_main` builds the dialog from the Main
        right-pane row (`tool_view.model_ids[selected_row]`), which is
        meaningless on Detail — the Detail screen's "row" is a per-tool
        registration, not a model row.
```

→ **ROOT CAUSE B:** No production-side `lift_delete_one_in_detail` ever runs.

→ **SOLUTION B:** Port the headless `lift_delete_one_in_detail` (`headless.rs:918-961`) into `interactive.rs`, dropping the headless-only env-var seams and instead targeting the FIRST registration (or the registration matching the Detail screen's currently-focused row when that affordance exists; today `DetailScreenState` exposes `registrations` with a single visible row per tool). Wire it after `lift_delete_one_in_main` in `translate_key` at `interactive.rs:325`:

```rust
let raw = keymap::dispatch(key);
let raw = lift_delete_one_in_main(state, raw);
lift_delete_one_in_detail(state, raw)
```

Implementation note: the `MODELTAP_HEADLESS_DELETE_TARGET` and `MODELTAP_HEADLESS_DELETE_ID_IN_TOOL` env-var seams in the headless version are test-only; production should target `detail.registrations.first()` (or, post-fix, the user's currently-selected registration if/when Detail grows row navigation). Use `detail.registrations.len() >= 2` for the `was_shared` flag (mirroring `headless.rs:944`).

---

### Branch C — Main 'd' on the Unified virtual column is a silent no-op

```
WHY 1C: User has cursor on the "Unified" virtual column; presses 'd';
        nothing happens.
  Evidence: keymap.rs:128 — 'd' is dispatched unconditionally on Main
            regardless of which column is selected.

WHY 2C: lift_delete_one_in_main bails out when state.current_tool() is None
        (the Unified virtual column case) — Msg::DeleteFromOne passes
        through unchanged.
  Evidence: interactive.rs:346-348 — `let Some(tool_view) =
            state.current_tool() else { return msg; }`.

WHY 3C: update() drops the unlifted Msg::DeleteFromOne to a no-op.
  Evidence: update.rs:224.

WHY 4C: The bottom bar's `is_available` predicate doesn't gate `[d]` for the
        Unified column on Main (it only checks the Detail screen's
        UnificationStatus::SingleTool — bottom_bar.rs:170-176). So the bar
        advertises `[d] delete-from-one` even when current_tool() is None.

WHY 5C: ROOT CAUSE C — the keymap promises an action that the orchestrator
        cannot fulfil for the Unified virtual-column cursor. There is no
        visible signal (toast, last-action message, dimmed bar entry) telling
        the user "this key is unavailable here."
```

→ **ROOT CAUSE C:** Silent no-op on the Unified virtual column.

→ **SOLUTION C:** EITHER (a) extend `is_available_main` in `render/bottom_bar.rs` to dim `[d]` when the cursor is on Unified (cheap; matches the existing `is_available_detail` pattern at `bottom_bar.rs:164-178`), OR (b) emit an unbound-key brief-highlight/toast effect when `lift_delete_one_in_main` returns the message unchanged. (a) is cheaper and consistent with prior gating; (b) is more user-friendly. Recommend (a) for this fix; track (b) separately.

---

### Branches D and E — refuted

**Branch D (block_on deadlock — H3):** REFUTED for the *initial press* of 'd'. The block_on chain at `interactive.rs:439, 454, 470` only fires when `effect.trigger_delete_one` is set, which requires a Confirm decision (typed-id match in Unique mode, `[y]` in Shared mode) — neither can occur because Branches A/B prevent the dialog from ever being visibly resolved by the user. Also: the runtime is multi-thread (`main.rs:98-107`), the hash pool spawns onto `runtime.handle()` (`interactive.rs:154-161`), so a future `block_on` won't deadlock against pool tasks the way a current_thread runtime would. H3 is a real *secondary* risk to revisit AFTER A/B/C are fixed (a slow `plugin.discover()` on a multi-GB HF cache will still freeze the loop for the duration of all three blocking calls), but it does not explain the reported "press 'd' and the UI hangs" behavior.

**Branch E (Shared-mode Pending trap — H5):** REFUTED. `decide_delete_one_unique` at `update.rs:840-874` returns `Pending` on Enter in Shared mode (dialog stays open), but `[y]`/`[n]` are routed correctly via `translate_key`'s `delete_one_shared_open` branch (`interactive.rs:306-317`). Esc cancels in both modes (`update.rs:846` + `decide_on_esc` at `delete_one_confirm.rs:134-136`). A user *could* hit this if they reached an open-but-invisible dialog (Branch A) and then pressed Enter, but the Pending trap is downstream of A, not a peer cause.

**Branch F (render panic — H4):** REFUTED. There is no `render/delete_one_dialog.rs` to panic in. `view_main` (`layout.rs:153-181`) renders the panes and the OTHER three dialogs without touching `delete_one_dialog`; nothing on the delete-one path calls into ratatui's drawing primitives. The panic-hook teardown path at `interactive.rs:108-109` is a real teardown-correctness mechanism but is not exercised here.

---

## 3. Hypothesis Ranking

| Rank | Hypothesis | Verdict | Evidence Strength |
|------|-----------|---------|-------------------|
| 1 | **H1** (Detail no-op) — partly subsumes A | CONFIRMED — promoted to ROOT CAUSE B | Strong: `grep` proves `lift_delete_one_in_detail` is unreachable from `interactive.rs`. |
| 2 | **(new) Missing render** — Branch A | CONFIRMED — ROOT CAUSE A | Strong: zero `delete_one_dialog` references in `render/` or `layout.rs`. The only renderers are zap, unify, running_tool. |
| 3 | **H2** (Main virtual-tool no-op) | CONFIRMED — ROOT CAUSE C | Strong: `interactive.rs:346-348` early-returns; no bar gating in `bottom_bar.rs:140-162`. |
| 4 | **H5** (Shared-mode Pending trap) | REFUTED standalone; downstream of A | Code path is correct in isolation. |
| 5 | **H3** (block_on deadlock) | REFUTED for primary symptom | Cannot be reached without first resolving the dialog (which A/B prevent). Real secondary concern for slow `discover()`. |
| 6 | **H4** (render panic) | REFUTED | The dialog has no render code; nothing to panic. |

---

## 4. Most-Likely Root Cause and Reasoning

**Primary: Root Cause A (missing render) is the dominant cause of the perceived hang.**

Even when the user is on the Main screen with a real-tool column highlighted (the only path where the dialog state is *constructed* — Branch B disables Detail; Branch C disables Unified column), the dialog is **invisible**. The event loop then flips into dialog-modal mode (`translate_key` at `interactive.rs:318-323`), which means:

- Subsequent letters (including `q`) become `Msg::DialogTextInput` (no-op for the user, just appends to the unique-mode typed buffer, `update.rs:785-795`).
- `Enter` becomes `Msg::DialogConfirm` and runs `decide_delete_one_unique`. In Shared mode it's `Pending` (dialog stays open, `update.rs:872`); in Unique mode it almost certainly cancels (`Cancel`, `update.rs:863`) because the typed-buffer doesn't byte-equal the model id the user can't see.
- Only `Esc` can cleanly exit (`Msg::DialogCancel`).

That's the textbook signature of "the UI hangs" — keystrokes appear to do nothing, the screen never updates beyond the post-press redraw, and the only escape is Esc or `Ctrl+C`.

Why A over B: B explains a subset of the reports (Detail-screen presses) but on Main, B doesn't apply — the dialog IS opened in state. A is required to explain the Main-screen reports. Together A + B + C explain every observable.

---

## 5. Proposed Fix

### Fix 1 — Render the delete-one dialog (addresses Root Cause A)

**New file:** `crates/modeltap-tui/src/render/delete_one_dialog.rs`. Mirror `render/unify_dialog.rs` structure: `pub fn render(frame: &mut Frame<'_>, area: Rect, dialog: &DeleteOneConfirmState)`. Render a centered modal showing:

- Title: `Delete from one tool` (Shared) vs `Delete (UNIQUE — only copy)` (Unique).
- Body: target `tool` + `model_id` + human-readable `size_bytes`.
- Footer in Shared: `[y] confirm   [n] cancel   [Esc] cancel`.
- Footer in Unique: typed-input echo (`> {dialog.typed_input()}_`) + `Type the model id, then [Enter]   [Esc] cancel`.

**Wire-up in `crates/modeltap-tui/src/render/mod.rs`:**

```rust
pub mod delete_one_dialog;
```

**Overlay in `crates/modeltap-tui/src/layout.rs`** — add identical guard in TWO places, mirroring `running_tool_dialog`:

- After `layout.rs:173` (Main view, before the running-tool overlay so running-tool wins layering — see comment at `layout.rs:177-180`).
- After `layout.rs:123` (Detail branch, before the running-tool overlay).

```rust
if let Some(dialog) = state.delete_one_dialog.as_ref() {
    delete_one_dialog::render(frame, area, dialog);
}
```

### Fix 2 — Lift `Msg::DeleteFromOne` on Detail in production (addresses Root Cause B)

In `crates/modeltap-app/src/interactive.rs`:

1. Add `lift_delete_one_in_detail(state: &AppState, msg: Msg) -> Msg` adapted from `headless.rs:918-961`. Drop the env-var seams; target `detail.registrations.first()` and `was_shared = detail.registrations.len() >= 2`. Use the `model.id` field for the dialog's `model_id` (mirror the headless fall-through case at `headless.rs:956-957`). Skip the `check_running_tools` gate for now to keep the diff minimal — that path can be added in a follow-up that also wires `running_tool_dialog` for delete-one (today the Detail render code already overlays `running_tool_dialog`, so the gate would just work).

2. Chain it after `lift_delete_one_in_main` at `interactive.rs:325`:

```rust
let raw = keymap::dispatch(key);
let raw = lift_delete_one_in_main(state, raw);
lift_delete_one_in_detail(state, raw)
```

### Fix 3 — Dim `[d]` on Main when current_tool() is None (addresses Root Cause C)

In `crates/modeltap-tui/src/render/bottom_bar.rs`, extend `is_available_main` (around the function near `bottom_bar.rs:140-162`, which currently dims `[u]` and `[z]` based on `current_tool_has_models`):

```rust
KeyCode::Char('d') => ctx.current_tool_has_models, // Unified column has no concrete tool
```

Plumb `state.current_tool().is_some()` into `BarContext` if `current_tool_has_models` doesn't already encode it (it does — see `bottom_bar.rs:70-73` — `Some(t).map(|t| !t.model_ids.is_empty()).unwrap_or(false)` is already false on Unified).

### Fix 4 — Acceptance test that drives `interactive.rs` end-to-end (regression net)

Add a unit/integration test in `crates/modeltap-tui` (or a new `crates/modeltap-app/tests/render/`) that:

1. Constructs an `AppState` with `delete_one_dialog: Some(...)`.
2. Renders into a `TestBackend` via the public `view()` (`modeltap_tui::view`).
3. Asserts the captured frame buffer contains the dialog's title text.

This catches Root Cause A directly — the existing US-05b suite passes today only because it asserts on filesystem + JSONL outcomes, never on a frame containing the dialog.

---

## 6. Files Affected

| Path | Change | Cause |
|------|--------|-------|
| `crates/modeltap-tui/src/render/delete_one_dialog.rs` | NEW — dialog overlay renderer | A |
| `crates/modeltap-tui/src/render/mod.rs:8-21` | `pub mod delete_one_dialog;` | A |
| `crates/modeltap-tui/src/layout.rs:121-130, 169-180` | Two overlay sites | A |
| `crates/modeltap-app/src/interactive.rs:325, +new fn` | Add `lift_delete_one_in_detail` + chain it | B |
| `crates/modeltap-tui/src/render/bottom_bar.rs:140-162` | Dim `[d]` when `!current_tool_has_models` | C |
| `crates/modeltap-app/tests/...` (new) | Render-snapshot test for delete-one dialog | regression |

---

## 7. Risk Assessment

| Dimension | Assessment |
|-----------|------------|
| **Regression surface — Fix 1 (render)** | Low. New module; only existing callers of `view()` are `interactive.rs:218,222,231` and `headless.rs:243,271`. The new overlay is gated by `Option::Some`, so any existing test where `delete_one_dialog == None` produces an identical frame. |
| **Regression surface — Fix 2 (Detail lift)** | Medium. Affects every `d` keypress on Detail. The headless harness already exercises this lift, so behavior is well-understood; the production version drops env-var seams but otherwise mirrors `headless.rs:918-961`. Existing US-05b acceptance tests will continue passing because they go through `headless::run_scripted`. Risk: if a future Detail screen grows row-cursor navigation, the production lift's "first registration" choice will need to match. |
| **Regression surface — Fix 3 (bar gating)** | Very low. Same pattern as the existing `[u]`/`[z]` gating at `bottom_bar.rs:140-162`. |
| **Blast radius if Fix 1 has a render bug** | Modal-only — a panic here would be caught by the existing panic-hook teardown (`crates/modeltap-app/src/interactive.rs:108-109`). The base panes still render before the overlay (`layout.rs:159-162`, then overlay at line 173+). |
| **Concurrency / async** | None of the fixes touch the `block_on` chain in `apply_effect` (`interactive.rs:430-506`). H3 (slow `discover()` freezing the loop on the post-confirmation path) remains a separate latent issue — recommended follow-up: time-budget the discover on the destructive-keystroke path or do path resolution before showing the dialog. |
| **Headless / acceptance suite** | Unchanged. All US-05b scenarios continue to drive `headless.rs`'s lift + the headless TestBackend. The new render module is exercised by them too (the headless harness calls `terminal.draw(|f| view(&state, f))` at `headless.rs:243`), so US-05b assertions on stdout JSONL stay green and the new dialog frames will newly appear in headless `print_frame` output — verify those acceptance tests don't have negative assertions that an unrelated frame is "empty." `grep -n print_frame crates/modeltap-app/tests/acceptance/us_05b_delete_one.rs` returns no matches; the suite asserts on JSONL + filesystem state, so this risk is nil. |
| **Mutation-testing budget (kill-rate ≥80%)** | New `render/delete_one_dialog.rs` adds new mutation targets. Mirror the test pattern used for `render/unify_dialog.rs` to keep kill-rate. |

---

## 7. Ratatui Integration Testing Recommendation

### Current state in modeltap

- The headless harness (`crates/modeltap-app/src/headless.rs:33`) already uses `ratatui::backend::TestBackend` (sized 100x40 by default per `tests/acceptance/us_03_two_pane_navigation.rs:205`).
- Several acceptance tests already capture frames via the harness's `print_frame`: `us_03`, `us_08`, `us_13`, `us_16`, `us_06_post_action_message:308`. They assert on substrings within stdout-printed frames.
- `insta` is declared as a workspace dependency (`Cargo.toml:85` and `crates/modeltap-app/Cargo.toml:89`) but **NEVER actually invoked** — no `*.snap` files exist, no `insta::assert_*` call sites in `crates/`. It's a dormant tool waiting to be used.

### Gap vs. the user's goal ("verify ALL keyboard handlers work end-to-end")

The headless harness drives `update()` and `view()` through scripted tokens, NOT through the production `interactive.rs` event loop. That means:

- Every defect in this RCA (A, B, C) is invisible to headless tests because the lift functions and render-overlay calls in `interactive.rs` and `layout.rs` are bypassed by the harness.
- "Pressing 'd' opens a visible dialog" is a *production-loop* property; the headless harness's lift functions short-circuit it.

### Recommendation (ranked)

1. **Primary: keep `TestBackend` + add `insta` snapshot assertions, and TEST `view()` DIRECTLY** (not the headless orchestrator). Per ratatui's official recipe ([Testing with insta snapshots](https://ratatui.rs/recipes/testing/snapshots/)), the canonical pattern is:

   ```rust
   use insta::assert_snapshot;
   use ratatui::{backend::TestBackend, Terminal};
   use modeltap_tui::{view, AppState};

   #[test]
   fn delete_one_dialog_renders_in_shared_mode() {
       let state = AppState { /* with delete_one_dialog = Some(shared_dialog) */ };
       let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
       terminal.draw(|f| view(&state, f)).unwrap();
       assert_snapshot!(terminal.backend());
   }
   ```

   This catches Root Cause A directly: with the dialog state set, the snapshot must contain dialog text. The current codebase has the bones for this (insta in Cargo, TestBackend in use) but has never instantiated the pattern.

2. **Secondary: keep the existing scripted-token harness for end-to-end behavior tests**. It's a strong asset; it drives `update()` faithfully, validates effect chains (zap / unify / delete-one orchestrator), and produces real on-disk side effects via `assert_cmd`. It is NOT a substitute for production-loop coverage but it is a substitute for "does pressing this token produce this state transition." Continue using it as-is for outcome assertions.

3. **Optional later: add a small set of `interactive.rs`-level tests** by extracting `translate_key` + `apply_effect` + `event_loop` body into pure functions over a `(state, msg) -> (state, effect)` shape so they can be driven by synthetic `KeyEvent` inputs WITHOUT needing a real terminal. The current `event_loop` is mostly already factored that way; only the I/O wrappers (`enable_raw_mode`, `terminal.draw`) need to be hoisted out.

4. **Do NOT add `ratatui-testlib`** ([crates.io](https://crates.io/crates/ratatui-testlib)) at this time. It targets PTY-level testing (terminal-size negotiation, image protocols, ANSI handling) which modeltap doesn't need — modeltap has no PTY-specific contract, no graphics protocols. The added dependency cost (Bevy ECS, async runtime) is not justified by the gap.

### Minimum-effort suite that would have caught this RCA's defects

| Defect | Catching test |
|--------|---------------|
| Root Cause A (missing render) | `assert_snapshot!` on `view()` with `delete_one_dialog: Some(shared_state)` — must contain "Delete" / model id. |
| Root Cause B (no Detail lift) | Unit test on a future `lift_delete_one_in_detail` in `interactive.rs`: input `(state with Screen::Detail, Msg::DeleteFromOne)` -> output `Msg::OpenDeleteOneDialog(_)`. |
| Root Cause C (unified-column no-op) | Unit test on `bottom_bar::is_available_main` with `current_tool_has_models = false` and KeyCode::Char('d') -> false. |

All three sit at the modeltap-tui or modeltap-app boundary, run in <1 ms each, and require no terminal.

---

## 8. Cross-Validation

- **A + B + C consistent?** Yes. A is a render-layer omission. B is an orchestrator-lift omission. C is a keymap/bar gating omission. Distinct files, distinct call sites, no contradiction. They are independently fixable in any order.
- **Do A + B + C explain every reported symptom?** Yes:
  - "Press 'd' on Main with a real tool selected → UI hangs": A.
  - "Press 'd' on Detail → nothing happens": B (and downstream A would also bite if B were fixed without A).
  - "Press 'd' on Main with Unified column → nothing happens": C.
- **Forward chain check (Root Cause → Symptom):**
  - A: dialog state set → no overlay in frame buffer → user sees no modal → user thinks app is hung. ✓
  - B: `Msg::DeleteFromOne` on Detail → no lift → no-op in update → state unchanged → no dialog ever opens → user sees no modal AND no state change. ✓
  - C: `Msg::DeleteFromOne` on Main with Unified column → lift early-returns → no-op in update → user sees no state change. ✓

---

## 9. References (file:line)

- `crates/modeltap-tui/src/keymap.rs:127-137` — `[d]` shortcut binding.
- `crates/modeltap-tui/src/keymap.rs:183-203` — `dispatch_in_dialog`.
- `crates/modeltap-tui/src/update.rs:224` — `Msg::DeleteFromOne` no-op.
- `crates/modeltap-tui/src/update.rs:226-232` — `Msg::OpenDeleteOneDialog` sets `delete_one_dialog`.
- `crates/modeltap-tui/src/update.rs:785-795` — `mutate_dialogs_text_input` routes to delete-one in Unique mode.
- `crates/modeltap-tui/src/update.rs:840-874` — `decide_delete_one_unique`.
- `crates/modeltap-tui/src/dialogs/delete_one_confirm.rs:120-153` — dialog state machine.
- `crates/modeltap-tui/src/render/mod.rs:8-21` — render submodule list (NO `delete_one_dialog`).
- `crates/modeltap-tui/src/layout.rs:111-181` — `view` / `view_main` — overlays zap, unify, running_tool only.
- `crates/modeltap-tui/src/render/bottom_bar.rs:140-178` — `is_available_main` / `is_available_detail`.
- `crates/modeltap-app/src/interactive.rs:301-364` — `translate_key` and `lift_delete_one_in_main`.
- `crates/modeltap-app/src/interactive.rs:430-506` — `apply_effect` `trigger_delete_one` block_on chain.
- `crates/modeltap-app/src/headless.rs:232, 918-961` — only call site + definition of `lift_delete_one_in_detail`.
- `crates/modeltap-app/src/main.rs:98-107` — multi-thread tokio runtime.
- `crates/modeltap-app/tests/acceptance/us_05b_delete_one.rs:286-289 et al.` — env-var-driven detail-screen seam.
- [Ratatui — Testing with insta snapshots](https://ratatui.rs/recipes/testing/snapshots/)
- [ratatui-testlib (deferred — out of scope here)](https://crates.io/crates/ratatui-testlib)
