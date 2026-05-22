# Model Detail TUI Surface

Pressing Enter on a right-pane model row opens a per-model detail screen rendered by [[crates/modeltap-tui/src/screens/detail.rs]] — the same `Screen::Detail` from US-13 that step 03-01 extends with a Metadata section.

The Metadata section renders `BTreeMap<String, String>` from `ModelDetail.metadata_kv` as aligned key-value pairs with a dim header reading `"Metadata (from <source>, introspected <N> ago)"` per AC-22-4.

The existing US-13 panels (Registered with, Size on disk, Dedup key, Status, Reclaim estimate) render unchanged per AC-22-3.

## Rationale

The detail screen is a full-screen replacement (mirrors [[tool-detail-tui]]'s rationale).

The Metadata section is optional in the render path. When `MetadataSection` is `None` the screen omits it entirely — useful for the legacy US-13 path that doesn't carry inspect_model data. When the section is `Some(_)` the renderer paints the header plus aligned KV rows.

## Msg variants and screen state

[[crates/modeltap-tui/src/msg.rs]] adds three new variants: `Msg::OpenModelDetail`, `Msg::ModelDetailReady(Box<MetadataSection>)`, `Msg::ReintrospectModel`.

`Box` on the ready payload mirrors `Msg::ToolDetailReady` for the same reason — `ModelDetail` contains `Option<SystemTime>` which doesn't implement `Eq`, and large payloads in enum variants bloat the discriminant.

`Msg::OpenModelDetail` triggers `open_model_detail::run` with `RunMode::Warm` (use cached metadata if present, fall back to `inspect_model()`). `Msg::ReintrospectModel` triggers the same orchestrator with `RunMode::ForceReintrospect`, which skips the cache-hit path and always calls `inspect_model()` — used by `[r]` on the detail screen (AC-22-2 + AC-22-8).

The pure `update` produces a `Msg::ModelDetailReady` once the orchestrator returns, which attaches the `MetadataSection` payload to `Screen::Detail.metadata`.

## Orchestration

[[crates/modeltap-app/src/orchestration/open_model_detail.rs]] mirrors the `open_tool_detail` composition pattern from Phase 02. Read cache + call inspect via `spawn_blocking` + `AssertUnwindSafe(...).catch_unwind()` panic boundary + JSONL timing emission.

The merge logic distinguishes four cases. `Ok(ModelDetail)` updates cache via `cache.write_models(...)` with new `metadata_kv_json` + `metadata_introspected_at` columns atomically per model_id — the cache writeback that AC-22-2 mandates. `Err(InspectError::Unsupported)` renders the `METADATA_UNSUPPORTED_SENTINEL = "(metadata unsupported for this tool)"` constant. `Err(InspectError::FormatUnreadable)` or `Err(InspectError::PluginPanic)` reuse [[plugin-inspect-overrides]]'s `INSPECT_PANIC_SENTINEL` and emit the `inspect_panic tool=<id>` line to `diagnostics.log`.

## RunMode enum

[[crates/modeltap-app/src/orchestration/open_model_detail.rs]] exposes `pub enum RunMode { Warm, ForceReintrospect }`.

The keymap distinguishes user intent — Enter wants the fastest paint (cache hit OK), `[r]` wants fresh data regardless of cache age. Both modes feed the same merge + writeback pipeline; only the cache-hit short-circuit differs.

The `RunMode` lives in the orchestrator (not the TUI) because the TUI emits Msg-level intent and the composition root translates that intent into orchestrator-level mode. This keeps the TUI free of cache-policy decisions.
