# Plugin Inspect Overrides

The Ollama and HF plugins ship `Tool::inspect_tool` overrides in [[plugins/ollama/src/inspect.rs]] and [[plugins/hf/src/inspect.rs]] — the first two production plugins to override the trait's default `Err(InspectError::Unsupported)` body.

llama-cli, lm-studio, atomic-chat, and gpt4all inherit the default until later phases (if at all). The detail screen renders `Version: (not detectable)` for those four, sourced fields-only from the cache.

## Rationale — return Ok with None, not Err on timeout

Ollama's inspect calls `http://localhost:11434/api/version` with a 500 ms timeout (`MODELTAP_OLLAMA_API_URL` override for tests). When Ollama isn't running the timeout fires.

The override returns `Ok(ToolDetail { detected_version: None, ... })` on timeout, not `Err(InspectError::*)`.

The rationale is reconcile-loop stability: an `Err` return signals "I tried and failed", which reconcile then attempts to surface (and would re-attempt next launch). A `None` version with the rest of the ToolDetail populated says "I successfully inspected and the version is unknown" — reconcile records that and moves on. The user sees `(not detectable)` either way; the underlying state machine stays calm.

The same rule applies to HF when the cache dir doesn't exist: return `Ok` with empty `search_paths`, not `Err`.

## Ollama inspect_model

[[plugins/ollama/src/inspect.rs]]'s `inspect_model_impl` reads the manifest JSON at `<MODELTAP_OLLAMA_DIR>/manifests/<.../id>` and projects a small tool-relevant KV subset into `ModelDetail.metadata_kv` per US-22 AC-22-3..AC-22-6.

The locator walks `manifests/` once and matches each candidate path via the same `<repo>:<tag>` projection that `discovery::manifest_id` uses (registry segment dropped, literal `library` segment dropped). First match wins — no full inventory build, no SHA recomputation, no blob touch.

KV selection (≤ 10 keys per AC-22-6; current selection emits 4): `config.architecture`, `parameters` (from `config.parameter_size`), `template` (excerpt — newlines collapsed to spaces, truncated to ≈200 chars), `system` (same excerpt rule). The detail-screen renderer paints each as an aligned `  key : value` line; the source `.feature` substring assertions hit on the literal key names.

Error mapping mirrors the `Tool::inspect_model` trait contract: manifest-file-missing → `Err(InspectError::FileReadable { path, source: NotFound })`; JSON parse failure → `Err(InspectError::FormatUnreadable { path, detail })`. Never panics — the plugin-contract harness in [[crates/modeltap-core/src/tests/inspect.rs]] verifies the panic-isolation invariant for every plugin including this one.

The `Format` top-level field reads `"Ollama manifest v2"` for every Ollama model (the manifest schemaVersion is part of the envelope, not user-visible content). `parameters` is also lifted to the typed `ModelDetail.parameters: Option<f64>` field via a best-effort `<n>B` / `<n>M` suffix parse so the cache row carries a numeric value alongside the raw KV string.

## HF inspect_model

[[plugins/hf/src/inspect.rs]]'s `inspect_model_impl` reads `config.json` at `<HF_HOME>/hub/models--<org>--<repo>/snapshots/<rev>/config.json` and projects a small tool-relevant KV subset into `ModelDetail.metadata_kv` per US-22 AC-22-3..AC-22-6.

The locator parses `<org>/<repo>` out of the model_id (stripping any trailing `/<filename>` segment from the HF discovery projection), reconstructs the `models--<org>--<repo>` directory, then resolves the snapshot via `refs/main` (priority 1) or the lexicographically first snapshot subdirectory (fallback). First match wins — no walkdir, no SHA recomputation, no blob touch.

KV selection (≤ 10 keys per AC-22-6; current selection emits up to 6): `model_type`, `architectures` (comma-joined when multiple), `hidden_size`, `num_attention_heads`, `num_hidden_layers`, `max_position_embeddings`. The detail-screen renderer paints each as an aligned `  key : value` line; the source `.feature` substring assertions hit on the literal key names.

Error mapping mirrors the `Tool::inspect_model` trait contract: missing model dir / snapshot / config.json → `Err(InspectError::FileReadable { path, source: NotFound })`; JSON parse failure → `Err(InspectError::FormatUnreadable { path, detail })`. Never panics — the plugin-contract harness in [[crates/modeltap-core/src/tests/inspect.rs]] verifies the panic-isolation invariant for every plugin including this one.

The `Format` top-level field reads `"safetensors v2"` for every HF model (the canonical on-disk format for HF model artifacts). `architecture` lifts the first entry from `architectures`. `context_length` lifts `max_position_embeddings` clamped to `u32`. `parameters_billions` is a best-effort estimate from `12 * num_hidden_layers * hidden_size^2 / 1e9` — coarse by design, falls back to `(not detectable)` when either input is absent.

## Ollama: env-var short-circuit

`MODELTAP_OLLAMA_VERSION` env var short-circuits the HTTP call. When set, [[plugins/ollama/src/inspect.rs]] returns the env var's value as `detected_version` immediately — no network call, no timeout wait.

The seam exists for D12 (wave-decisions.md) / R5 (acceptance-test-plan.md) risk mitigation: acceptance tests assert AC-21 against deterministic version strings without standing up a real Ollama server. Production builds with `--no-default-features` still compile the env-var read, but production users typically don't set it; CI sets it to a known token to exercise the version-rendering code path under a real `modeltap` binary.

## HF: coexistence with folder_delete.rs

[[plugins/hf/src/inspect.rs]] is a sibling module to [[plugins/hf/src/folder_delete.rs]] from the folder-group-bulk-delete feature.

Per component-boundaries.md the two coexist in the same crate without merge conflict — different sets of `Tool` methods, no shared state, no shared types beyond the trait surface and modeltap-core's `ToolDetail` / `DeleteOutcome`.

Inspect detects the HF cache dir from `$HF_HOME` first, falling back to `~/.cache/huggingface`. The detected `search_paths` list includes the `hub/` sub-directory tagged `Default` plus any user-config entries.

## User-config search paths

[[crates/modeltap-app/src/registry.rs]] reads `[plugins.<id>] search_paths` from `~/.modeltap/config.toml` at plugin-construction time and threads the user-config list into each plugin's constructor.

The plugin's `inspect_tool` then concatenates `Default` (built-in) paths with `UserConfig` (from `~/.modeltap/config.toml`) paths, tagging each entry with the `SearchPathSource` enum so the detail-view render can distinguish them per AC-21-5.

The TOML schema is per-plugin namespaced (`[plugins.ollama]`, `[plugins.hf]`, `[plugins.llama-cli]`, etc.) so future plugin additions don't require a config-file schema migration — each plugin claims its own section by `ToolId`.

## Reconcile error capture

The `cache.last_error` field on `cache_tools` is populated by [[crates/modeltap-app/src/main.rs]]'s `reconcile_writeback` from the per-tool `DiscoverError` that a plugin returned during cold-scan.

Before step 02-02 the function hardcoded `last_error: None` for every tool, throwing away discovery errors. The rewrite consumes the per-tool result carried in `InventorySummary`: when discovery returned `Err`, capture `last_error: Some(format!("{}", err))` plus `last_error_at: Some(SystemTime::now())`; when discovery returned `Ok`, store both as `None` (clearing any prior error).

The detail view in [[crates/modeltap-tui/src/screens/tool_detail.rs]] reads these fields from the cache row via [[crates/modeltap-app/src/orchestration/open_tool_detail.rs]] and renders them per AC-21-4. Acceptance tests drive the path end-to-end by pointing `MODELTAP_OLLAMA_DIR` at a non-existent directory: `Ollama::discover()` returns `DiscoverError::NotInstalled`, reconcile records it, the detail screen displays it on the next Enter.

No `inspect_tool`-into-reconcile wiring is needed for this to work — the existing discover-error pathway already carries the signal, the change was just to stop discarding it.

## Plugin-contract harness

[[crates/modeltap-core/src/tests/inspect.rs]] hosts `run_inspect_tool_contract<T: Tool + ?Sized>` — the layer-B contract test every plugin runs against to prove its `inspect_tool` implementation satisfies the parent's `Tool` invariants.

The harness dispatches on an `InspectCapability::{Unsupported, Supported}` enum. `Unsupported` plugins (lm-studio, atomic-chat, gpt4all) must return `Err(InspectError::Unsupported { tool })` matching `plugin.name()` — that's the test of the trait's default-body inheritance from step 01-01. `Supported` plugins (Ollama, HF) must return `Ok(ToolDetail)` and pass a determinism check (two consecutive calls return equal results modulo `introspected_at` timestamps).

Both capability arms also exercise `run_inspect_with_panic_isolation` — a wrapper that uses `std::panic::catch_unwind` inside a `spawn_blocking` boundary to convert any `inspect_tool()` panic into `Err(InspectError::PluginPanic { tool, message })`. This extends US-18's panic-isolation invariant from the parent feature into the inspect domain: a buggy plugin can never crash modeltap by panicking inside `inspect_tool`.

The plugin-side test files (`plugins/<id>/tests/inspect_tool_contract.rs`) are thin: each is one `#[tokio::test]` that builds the plugin via its public constructor and invokes the harness with the appropriate capability. Ollama uses `MODELTAP_OLLAMA_VERSION=0.6.4` to deterministically hit the env-var short-circuit (no HTTP probe in CI). HF uses a tempdir-based `$HF_HOME` fixture.

The orchestrator-side panic boundary lives in [[crates/modeltap-app/src/orchestration/open_tool_detail.rs]]: a panic in a plugin's `inspect_tool` returns `Err(InspectError::PluginPanic)` to the orchestration, which renders "(inspection failed -- see diagnostics.log)" in the detail view and writes `inspect_panic tool=<id>` to the diagnostics log. That cross-cutting behavior is asserted end-to-end by INT-INFO-8 in `tests/acceptance/integration_checkpoints.rs`.

## Panic isolation at the orchestrator boundary

[[crates/modeltap-app/src/orchestration/open_tool_detail.rs]] wraps each `plugin.inspect_tool()` future in `AssertUnwindSafe(...).catch_unwind()` (from the `futures` crate). A plugin panic in `inspect_tool` becomes `Err(InspectError::PluginPanic { tool, message })` — the orchestrator never unwinds.

`AssertUnwindSafe` is sound at this seam because the plugin owns no mutable state shared with the orchestrator. Any internal partial-mutation a panic leaves behind is irrelevant — the orchestration discards the plugin reference immediately after the catch and never re-calls `inspect_tool` on the same panicked instance in the same run.

When the catch fires, the orchestration emits one line to `<diagnostics_dir>/diagnostics.log` tagged `inspect_panic tool=<id> message=<msg>`. The directory is resolved by the composition root from `MODELTAP_DIAGNOSTICS_DIR` (test override) or `~/.modeltap` (production), threaded into `OpenToolDetailConfig::diagnostics_dir`. Setting it to `None` disables panic-isolation logging entirely (useful for unit tests that don't care about the log artifact).

`INSPECT_PANIC_SENTINEL = "(inspection failed -- see diagnostics.log)"` is the user-visible string the render layer emits for `Err(PluginPanic)`. The convention preserves the rest of the detail-screen's cache-sourced fields — only the version line collapses to the sentinel — so the user still sees model count, disk usage, last scan, etc.

The end-to-end INT-INFO-8 scenario drives this via `MODELTAP_TEST_TOOL_INSPECT_PANIC=1` (see [[test-plugin-seam]]) and asserts: TUI does not crash, sentinel appears in the rendered frame, diagnostics.log gains the tagged line, process stays alive.
