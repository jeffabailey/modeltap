# Model Detail TUI Surface

Pressing **`i`** on a right-pane model row opens a per-model detail screen rendered by [[crates/modeltap-tui/src/screens/detail.rs]] — the same `Screen::Detail` from US-13 that step 03-01 extends with a Metadata section.

Pre-fix the production keymap had no path to this screen (no production code path ever constructed `Msg::OpenDetail`); the bugfix wires `[i]` through `Msg::OpenInfo` and `lift_open_info_in_main`. See the "Production `[i]` dispatch" section in [[tool-detail-tui]] for the full peek-translate pattern — the right-pane branch of the same lift builds a `DetailScreenState` from live AppState (`current_tool().model_ids[selected_row]` as the target id, cross-tool registrations walked from `real_tools_iter`).

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

## Msg dispatch — interactive and headless event loops

[[crates/modeltap-app/src/interactive.rs]] and [[crates/modeltap-app/src/headless.rs]] both wire `Msg::OpenDetail` and `Msg::ReintrospectModel` into the model-detail orchestrator via a peek-then-dispatch block that mirrors the Phase 02 `Msg::OpenToolDetail` pattern in [[tool-detail-tui]].

Before `update(state, msg)` consumes the Msg, each event loop captures the intent into a local `open_model_detail: Option<(ToolId, ModelId, RunMode)>`. `Msg::OpenDetail(detail)` captures the (tool_id, model_id) from the carried `DetailScreenState` with `RunMode::WarmIfCached`. `Msg::ReintrospectModel` re-reads the same fields from the active `Screen::Detail(state)` with `RunMode::ForceReintrospect` — the pure update is a state-noop for that Msg, so the orchestrator dispatch is the entire effect.

After the pure update + `apply_effect` complete (and the screen has transitioned into `Screen::Detail(state)`), `dispatch_open_model_detail` resolves the plugin from the registry by `tool_id`, runs `open_model_detail::run(...)` under `runtime.block_on`, and on success dispatches `Msg::ModelDetailReady(Box::new(MetadataSection { kv, source, introspected_at }))` back through `update()`. On orchestrator error (unknown tool, cache I/O failure) the dispatch is skipped with a `tracing::warn!` — the detail screen renders WITHOUT the Metadata section, matching the legacy US-13 path.

`extract_model_detail_dispatch` is a pure helper colocated with the dispatcher that pulls (tool_id, model_id) out of a `DetailScreenState`. It reads tool_id from `registrations.first()?.tool` — the orchestrator only needs ONE plugin to consult for `inspect_model()` and the cache writeback is keyed on (tool_id, model_id). When `registrations` is empty (synthetic / empty detail row) the helper returns `None` and the dispatch is skipped — graceful degradation per AC-22-4's metadata-absent fallback.

The headless dispatch additionally drives a frame-capture seam: after `dispatch_open_model_detail` returns, the loop forces a redraw and `print_frame(&terminal)` so US-22 acceptance assertions see the rendered Metadata section BEFORE the next iteration's `<esc>` closes the screen. Same pattern as the tool-detail / dry-run / running-tool capture seams above it in the loop.

The interactive path defaults `diagnostics_dir` to `None` for now — wiring `MODELTAP_DIAGNOSTICS_DIR` / `~/.modeltap` through `interactive::run` is deferred, identical to the tool-detail dispatch's deferral. The in-TUI `INSPECT_PANIC_SENTINEL` still renders without on-disk panic logging on the production path.

## Cucumber acceptance

[[tests/acceptance/model_detail.rs]] is the end-to-end driver for the five `model-detail.feature` scenarios. It spawns the production `modeltap` binary headless against fixture-populated tempdirs (Strategy B per `wave-decisions.md`) and substring-matches the captured stdout frame.

Step 03-01 part 3/3 ships one active scenario (AC-22-7 — un-introspectable model file renders partial info gracefully) and four `#[ignore]`d scenarios deferred to step 03-02. The deferred set (GGUF header metadata, Ollama manifest fields, HF config.json fields, re-introspect cache writeback) all require plugin overrides of `inspect_model` that no production plugin ships in step 03-01.

The active AC-22-7 scenario routes through the Ollama plugin's trait-default `inspect_model` (which returns `Err(InspectError::Unsupported)`); the orchestrator's merge maps that to the public `METADATA_UNSUPPORTED_SENTINEL` rendered in the Metadata section. The .feature line wording (`(introspection failed -- see diagnostics.log)`) tightens to that literal when step 03-02 lands the plugin overrides that emit `FormatUnreadable`; AC-22-7's intent ("partial info gracefully" + "screen does not crash" + "other panels still render") is fully exercised either way.

[[tests/src/fixtures/inspect_fixtures.rs]] gains `devon_model_unintrospectable_fixture`, which seeds a tempdir with a non-GGUF binary file under `<temp>/ollama-root/unintrospectable-model.bin` plus the standard cache / log / diagnostics tree. The model file is reachable via `devon_unintrospectable_model_path(&fixture)`.

The headless lift that triggers `Msg::OpenDetail` on `<enter>` is the existing `MODELTAP_HEADLESS_DETAIL_REGS` JSON-payload seam in [[crates/modeltap-app/src/headless.rs]]'s `synthesize_detail_from_env` helper. The synthesiser's `tool` whitelist accepts `{ollama, hf, lm-studio}` only — the AC-22-7 driver uses `"ollama"` for its registration entry because the production Ollama plugin's trait-default `inspect_model` is the merge branch under test.
