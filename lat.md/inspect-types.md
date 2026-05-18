# Inspect Domain Types

The `Tool` trait in [[crates/modeltap-core/src/tool.rs]] gained two read-only inspection methods plus the algebraic types they return in [[crates/modeltap-core/src/domain/inspect.rs]]. Default bodies make the extension source-compatible — every existing plugin compiles unchanged.

`ToolDetail` and `ModelDetail` are plain data — no I/O, no async. Both types and the four-variant `InspectError` enum live in `inspect.rs` and are unit-tested via [[crates/modeltap-core/tests/inspect_types.rs]].

## Rationale

Default trait bodies were chosen so plugin authors' work stays optional.

A separate `Inspect` trait would force every plugin to opt in twice and break `&dyn Tool` everywhere. Returning `Option<ToolDetail>` conflates "supported but no data right now" with "this plugin will never support inspection" — the four-variant `InspectError` distinguishes them.

The first override lands on the Ollama plugin in step 02-02. Until then, the detail-view renderer falls back to a "Not supported by `<tool>`" panel for every plugin.

## Tool trait extension

The trait in [[crates/modeltap-core/src/tool.rs]] grew two `async fn` methods alongside the existing seven, both decorated with `#[async_trait::async_trait]`.

The default bodies are literal `Err(InspectError::Unsupported { tool: self.name() })` — no `unimplemented!()` / `todo!()` — so calling the method on a non-overriding plugin returns a typed error, not a panic.

Unit tests in [[crates/modeltap-core/tests/inspect_types.rs]] exercise the default path via a trivial `impl Tool` stub.

## InspectError variants

The `InspectError` enum in [[crates/modeltap-core/src/domain/inspect.rs]] has exactly four variants and no `Other` / `Unknown` fallback:

- `Unsupported { tool }` — the plugin does not implement inspection. Detail view renders a greyed-out panel.
- `PluginPanic { tool, message }` — the plugin's inspect call panicked. The registry's panic-isolation harness (plugin contract test 3.12.S.3) ensures this never crashes modeltap.
- `FileUnreadable { path, source }` — the model file exists but cannot be opened. Detail view shows the path and the OS error.
- `FormatUnreadable { path, detail }` — the file opened but the parser rejected its content (e.g., a corrupt GGUF magic-number).

The closed-set design forces the detail-view renderer's `match` to be exhaustive — a new error category cannot ship without an updated UI.
