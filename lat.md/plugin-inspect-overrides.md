# Plugin Inspect Overrides

The Ollama and HF plugins ship `Tool::inspect_tool` overrides in [[plugins/ollama/src/inspect.rs]] and [[plugins/hf/src/inspect.rs]] — the first two production plugins to override the trait's default `Err(InspectError::Unsupported)` body.

llama-cli, lm-studio, atomic-chat, and gpt4all inherit the default until later phases (if at all). The detail screen renders `Version: (not detectable)` for those four, sourced fields-only from the cache.

## Rationale — return Ok with None, not Err on timeout

Ollama's inspect calls `http://localhost:11434/api/version` with a 500 ms timeout (`MODELTAP_OLLAMA_API_URL` override for tests). When Ollama isn't running the timeout fires.

The override returns `Ok(ToolDetail { detected_version: None, ... })` on timeout, not `Err(InspectError::*)`.

The rationale is reconcile-loop stability: an `Err` return signals "I tried and failed", which reconcile then attempts to surface (and would re-attempt next launch). A `None` version with the rest of the ToolDetail populated says "I successfully inspected and the version is unknown" — reconcile records that and moves on. The user sees `(not detectable)` either way; the underlying state machine stays calm.

The same rule applies to HF when the cache dir doesn't exist: return `Ok` with empty `search_paths`, not `Err`.

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
