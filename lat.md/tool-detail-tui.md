# Tool Detail TUI Surface

Pressing **`i`** on a left-pane row opens a per-tool detail screen rendered by [[crates/modeltap-tui/src/screens/tool_detail.rs]].

Pre-fix, the production keymap had no path to either detail screen — `Msg::OpenToolDetail` and `Msg::OpenDetail` were only ever constructed behind `MODELTAP_HEADLESS_TOOL_DETAIL` / `MODELTAP_HEADLESS_DETAIL_REGS` env-var seams in `headless.rs`, so users had no way to reach the rendered screens even though both shipped. The keymap now dispatches `[i]` to a payload-free `Msg::OpenInfo` that the composition root translates into the focus-appropriate detail-screen open Msg — see "Msg dispatch" below.

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

## Msg dispatch — interactive and headless event loops

`Msg::OpenToolDetail(tool_id)` is dispatched into the async runtime by both [[crates/modeltap-app/src/interactive.rs]] (the production event loop) and [[crates/modeltap-app/src/headless.rs]] (the acceptance-test harness).

Both paths follow the same shape: resolve the `&dyn Tool` from the live plugin registry by `tool_id`, locate the open `&Cache` (held in the app's runtime state since the warm-start path opened it), then `tokio::spawn` the async [[crates/modeltap-app/src/orchestration/open_tool_detail.rs]] orchestration. The spawned task posts `Msg::ToolDetailReady(Box<ToolDetail>)` back through the existing msg channel once `inspect_tool()` returns and the cache merge completes.

The headless variant differs in one detail: the keymap binds Enter to `Msg::OpenToolDetail` directly, while the interactive variant goes through `ContextFilter::LeftPaneFocus`. Both end up at the same dispatch site so the orchestration only knows one caller pattern.

## Production `[i]` dispatch — `Msg::OpenInfo` translation (bugfix, 2026-05-26)

The production `[i]` hotkey is mapped in [[crates/modeltap-tui/src/keymap.rs]] to a payload-free `Msg::OpenInfo`.

[[crates/modeltap-app/src/interactive.rs]]'s `lift_open_info_in_main` inspects `state.focus` at peek-then-dispatch time and rewrites the Msg before the pure update runs. `FocusPane::Left` produces `Msg::OpenToolDetail(state.current_tool().tool)`. `FocusPane::Right` produces `Msg::OpenDetail(detail)` with a `DetailScreenState` synthesised from live AppState (target model id from `model_ids[selected_row]`, cross-tool registrations walked from `real_tools_iter`).

This mirrors the `RefreshScope` peek-translate precedent that step 05-03 established (see [[crates/modeltap-tui/src/msg.rs]] for the `RequestRefresh(RefreshScope)` doc-comment). The keymap stays layer-pure — it knows nothing about AppState focus or model lists.

The `[i]` Shortcut carries `sections: &[]` so the bar text stays within the 100-col headless terminal budget — discovery is via the `[?]` help overlay's Concepts glossary, same precedent as the `[Enter] expand/collapse` entry.

The MODELTAP_HEADLESS_TOOL_DETAIL env-var lift in [[crates/modeltap-app/src/headless.rs]]'s `lift_enter_in_left_pane_to_tool_detail` continues to serve the acceptance-test seam (Enter + env-var → OpenToolDetail) — it is orthogonal to the production `[i]` path.
