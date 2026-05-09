# Mutation Report — arrow-keys-navigate-tools

**Run date**: 2026-05-09 (UTC)
**Tool**: cargo-mutants 27.0.0
**Scope**: `crates/modeltap-tui/src/keymap.rs` + `crates/modeltap-tui/src/render/bottom_bar.rs` (feature-scoped)
**Test scope**: `--test-package modeltap-tui` (workspace-level acceptance test
`release_process_version_consistency` excluded — runs ~9 min and would make
mutation testing impractical; the focus-aware dispatch logic is fully
exercised by `modeltap-tui` unit tests).

## Summary

| Bucket | Count |
| --- | --- |
| Total mutants | 55 |
| Caught (killed by tests) | 49 |
| Missed (survived) | 2 |
| Unviable (compile errors — not counted) | 4 |
| Timeout | 0 |
| **Kill rate** | **49 / (49 + 2) = 96.08%** |
| **Threshold** | **≥80% — PASS** |

### Initial vs final run

| Run | Caught | Missed | Kill rate |
| --- | --- | --- | --- |
| Initial (pre-fix) | 33 | 18 | 64.71% — FAIL |
| Final (after adding `unit_mutation_kill_keymap_bottom_bar.rs`) | 49 | 2 | 96.08% — PASS |

Sixteen previously-surviving mutants are now killed by the new test file
(see `unit_mutation_kill_keymap_bottom_bar.rs`).

## New-feature-logic coverage

The new logic introduced by the `arrow-keys-navigate-tools` feature is
fully covered:

- `keymap::dispatch_focus_aware` — match arms for `(KeyCode::Up,
  KeyModifiers::NONE)` (line 187) and `(KeyCode::Down, KeyModifiers::NONE)`
  (line 188): **caught**.
- `keymap::up_down_bar_label` — return-value mutations to `""` and
  `"xyzzy"` (line 208): **caught**.
- `render::bottom_bar::render_bottom_bar` — focus-aware Up/Down label
  substitution branch (lines 131–132 `&&`/`==` mutations): **caught**.
- `BarContext::focus` field plumbing in `for_state` (line 84): **caught**
  via the `Screen::Detail(_)` and `current_tool().model_ids` mutations.

Zero mutants in the new-logic surface area survived.

## Surviving mutants (equivalent — cannot be killed via unit tests)

Both survivors are in the file-private helper `no_color_active`:

| File:line | Mutation | Why it cannot be killed |
| --- | --- | --- |
| `bottom_bar.rs:204:5` | `replace no_color_active -> bool with true` | Equivalent mutant. |
| `bottom_bar.rs:204:5` | `replace no_color_active -> bool with false` | Equivalent mutant. |

### Equivalence proof

`no_color_active()` is called once, on line 92, by the frame entry-point
`render(...)`:

```rust
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let ctx = BarContext::for_state(state);
    let line = render_bottom_bar(&ctx, no_color_active());   // <-- here
    frame.render_widget(Paragraph::new(line), area);
}
```

The receiving function `render_bottom_bar(ctx, _no_color: bool)` declares
its second parameter with a leading underscore — it is currently unused
(the bar's styling is governed by `Modifier::DIM` / `Modifier::CROSSED_OUT`
which ratatui automatically downgrades when the terminal advertises
`NO_COLOR`). No assertion in the unit-test path can observe whether
`no_color_active()` returns `true` or `false`, so neither mutation can
flip a test from green to red.

This is a textbook **equivalent mutant** — the helper is a thin wrapper
that exists for future call-sites; until a caller actually consumes the
boolean, the function's body is observationally indistinguishable from
either constant. Killing this mutant would require either adding I/O to
the unit tests (violates the test pyramid) or removing the unused
parameter (production-code change, out of scope for Phase 5).

## Recommendation

**PASS — no further action required.** Kill rate of 96.08% comfortably
exceeds the 80% per-feature threshold. The two equivalents are documented
above. Future cleanup option: when a future feature wires the `_no_color`
parameter through to actual style decisions inside `render_bottom_bar`,
these two mutants will become non-equivalent and a corresponding test will
naturally cover them.

## Reproduction

```sh
cargo mutants \
  --file crates/modeltap-tui/src/keymap.rs \
  --file crates/modeltap-tui/src/render/bottom_bar.rs \
  --test-package modeltap-tui \
  --no-shuffle \
  --output docs/feature/arrow-keys-navigate-tools/deliver/mutation/
```

Runtime: ~4 minutes on Apple Silicon (M-series).
