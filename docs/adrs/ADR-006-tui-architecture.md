# ADR-006: TUI Architecture — Ratatui with Elm-style Update Loop

## Status

Accepted (2026-04-28).

## Context

Ratatui is mandated. Within ratatui, several patterns are idiomatic:

- **Immediate-mode rendering with mutable state** — write a render function that reads `&App`; modify `App` directly in the event handler. Common in small examples.
- **Elm Architecture (TEA)** — `App` state, `Msg` enum, `update(state, msg) -> (state, cmd)`, `view(state) -> Frame`. Used by libraries like `tui-realm`.
- **Component-based** — each pane is a struct with its own state; parents own children. Used by `cursive` and `tui-realm`.

We need a pattern that:

- Cleanly separates input from rendering (testability — TUI snapshot tests should not need to drive event handling).
- Plays well with async commands (discovery, hashing, link operations) that return Updates over time.
- Matches DELIVER's Outside-In TDD approach: the App should be testable from outside by sending `Msg`s and asserting on rendered buffers.

## Decision

**Elm-style update loop. App state, Msg enum, `update: (State, Msg) -> (State, Vec<Cmd>)`, `view: &State -> Frame`. Async commands are dispatched via tokio channels. The view function is pure; the update function is pure (commands are described, not executed).**

```rust
// crates/modeltap-tui/src/lib.rs

pub struct AppState { /* the entire view-model */ }

pub enum Msg {
    KeyPressed(KeyEvent),
    DiscoveryProgress(ToolId, ToolInventory),
    DiscoveryComplete(Inventory),
    PlanReady(UnifyPlan),
    ActionComplete(LastAction),
    Tick,
    Quit,
}

pub enum Cmd {
    StartDiscovery,
    BuildUnifyPlan { group_id: DedupGroupId, dry_run: bool },
    ExecuteUnify { plan: UnifyPlan },
    ExecuteZap { tool: ToolId, confirmation: String },
    ExecuteZapOne { tool: ToolId, model: ModelMeta },
    Exit,
}

pub fn update(state: AppState, msg: Msg) -> (AppState, Vec<Cmd>) {
    // pure function — no I/O, no side effects
    // testable with `assert_eq!(update(state, msg), (expected_state, expected_cmds))`
}

pub fn view(state: &AppState, frame: &mut Frame) {
    // pure function over &AppState
    // snapshot-testable with TestBackend
}
```

The `modeltap-app` orchestrator runs the executor loop:

```rust
loop {
    tokio::select! {
        Some(event) = event_rx.recv() => {
            let msg = key_to_msg(event);
            let (new_state, cmds) = update(state.clone(), msg);
            state = new_state;
            for cmd in cmds {
                spawn_command(cmd, msg_tx.clone());
            }
            terminal.draw(|f| view(&state, f))?;
        }
        Some(msg) = msg_rx.recv() => {
            let (new_state, cmds) = update(state.clone(), msg);
            state = new_state;
            for cmd in cmds { spawn_command(cmd, msg_tx.clone()); }
            terminal.draw(|f| view(&state, f))?;
        }
        else => break,
    }
}
```

## Alternatives considered

### A — Immediate-mode with mutable App

```rust
struct App { state, ... }
impl App {
    fn handle_event(&mut self, evt: Event) { ... }
    fn render(&mut self, f: &mut Frame) { ... }
}
```

**Pros:** less ceremony.
**Cons:**
- `update()` becomes a giant `&mut self` method intermixing state changes with side effects (spawning tasks). Difficult to unit-test.
- Async commands need separate channels anyway; ends up reinventing `Cmd`.
- Testing requires constructing a real `App` with stubbed I/O, vs. testing a pure `update` function with a synthesized state.

**Rejected** on testability.

### B — Component-based (one struct per pane)

**Pros:** familiar from web frontend.
**Cons:**
- Adds depth: parent has to route messages to children, pass back commands.
- For a 2-pane app with a few dialogs, the depth pays no dividend.
- `tui-realm` library exists for this but adds a dep with its own opinions.

**Rejected** for v1; revisit if the TUI grows beyond ~6 distinct screens.

### C — Pure ratatui with no architecture

**Rejected.** No long-lived TUI app survives without an architecture.

## Consequences

### Positive

- `update()` and `view()` are pure functions. Both unit-testable in `modeltap-tui/tests/update_tests.rs` and `view_tests.rs`.
- Snapshot testing via `insta` + `ratatui::backend::TestBackend`: render `view(&state)` to a `TestBackend`, snapshot the buffer text. Diffs land in PRs.
- Clear seam for DELIVER's acceptance tests: drive `Msg` sequences, assert on rendered buffers.
- Async I/O lives in `Cmd` executors (in `modeltap-app/src/orchestration/`), not in the TUI.

### Negative

- More boilerplate up front than immediate-mode.
- `AppState` clones per update (Rust's borrow-checker friendlier this way). For a state of a few KB this is negligible.

## Keymap (single source of truth)

`modeltap-tui::input::keymap::SHORTCUT_TABLE` is a `&'static [(KeyEvent, ContextFilter, Msg)]` list. Both:

- The bottom-bar renderer (US-08) reads it to display shortcuts.
- The event handler reads it to dispatch keys to `Msg`s.

This satisfies US-08 AC-5: "Shortcuts shown in the bar match the actual key handler dispatch table (single source of truth)."

## Panic-safety

`crossterm`'s raw mode and the alternate screen need explicit teardown. `modeltap-app` installs a panic hook (via `std::panic::set_hook`) that disables raw mode and exits the alternate screen BEFORE printing the panic message. This guarantees no escape-sequence garbage on panic (US-01 AC-5).
