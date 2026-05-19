# Tool Detail TUI Surface

Pressing Enter on a left-pane row opens a per-tool detail screen rendered by [[crates/modeltap-tui/src/screens/tool_detail.rs]].

The screen is the user-facing surface of US-21 — discovery root, detected version, search paths, model count, disk usage, last scan timestamp, last error, plugin version. The view layer is pure: it reads `&ToolDetailScreenState` and writes ratatui widgets with no I/O.

The orchestration that composes the cached `CachedTool` with `Tool::inspect_tool()` into a `ToolDetail` lives in modeltap-app — that piece lands in a follow-up step and gets its own section.

## Rationale

Tool detail is rendered as a full-screen replacement, not an overlay, because the right pane already carries the model list and there is no readable real estate for a 10-field detail grid alongside it.

The render is pure (per ADR-006's Elm-like discipline) so a snapshot test can drive it without spinning up a real terminal or stubbing async I/O.

Default-`Unsupported` plugins (everything except Ollama and HF until step 02-02) still get a useful screen: cache-sourced fields render normally and Version reads `"(not detectable)"` (AC-21-3).

## Msg variants and the dropped Eq derive

The Msg enum in [[crates/modeltap-tui/src/msg.rs]] gained three variants: `Msg::OpenToolDetail(ToolId)`, `Msg::ToolDetailReady(Box<ToolDetail>)`, `Msg::CloseToolDetail`.

The `Box<ToolDetail>` is deliberate. `ToolDetail` contains `Option<SystemTime>` fields (`last_scan_at`, `last_error_at`, `introspected_at`); `SystemTime` does not implement `Eq` because wall-clock time is not reflexively equal across system clock adjustments.

This is why Step 02-01 drops `Eq` from the `Msg` enum's derive (and from `Screen` / `AppState`, which transitively contain `Msg`). `PartialEq` is retained — `assert_eq!` and the existing update-loop comparisons all work without `Eq`.

## Screen state and cursor preservation

`Screen::ToolDetail` in [[crates/modeltap-tui/src/app_state.rs]] carries three fields: `tool_id`, `detail: Option<Box<ToolDetailScreenState>>`, and `left_pane_cursor: usize`.

The `detail` is `Option` because the screen opens immediately on Enter (loading state) and the actual `ToolDetail` arrives later via `Msg::ToolDetailReady` — the render layer paints a placeholder skeleton until it lands.

`left_pane_cursor` snapshots `selected_tool` at the moment Enter was pressed, so `Msg::CloseToolDetail` (Esc) restores the cursor even when intervening async refreshes have changed the underlying tool list. The cursor preservation rule is AC-21-7.

## Bottom bar context

When `Screen::ToolDetail` is active, [[crates/modeltap-tui/src/render/bottom_bar.rs]] shows `[Esc] back / [r] refresh this tool / [?] help` per AC-21-8.

The keymap dispatches Esc → `Msg::CloseToolDetail`, `r` → a refresh Msg (full refresh wiring lands in step 02-02), and `?` → the existing help overlay. The two-arm pattern from `test-plugin-seam` style is not reused here — keymap context is a runtime `ContextFilter` enum value, not a cfg gate, because Screen state is dynamic.
