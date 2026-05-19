# Test Plugin Seam

The acceptance crate ships an in-process `TestTool` in [[tests/src/test_tool.rs]] that implements the full nine-method `Tool` trait.

The [[crates/modeltap-app/src/registry.rs]] composition root registers it when `MODELTAP_TEST_PLUGINS=test-tool` is set — but only in builds that enable the `test-harness` Cargo feature.

The seam is what lets the walking-skeleton acceptance scenario drive end-to-end without a real Ollama/HF tool installed. Real plugins do the hard work; TestTool is the test double for the trait surface itself.

## Rationale

A real plugin would make the walking skeleton non-hermetic — its result would depend on whichever tool is installed on the dev machine.

A `#[cfg(test)]`-only TestTool would not flow through the composition root's `collect_plugins()` function — it would have to live in a parallel registration path that does not get exercised under release builds.

The `test-harness` feature gate is the middle ground: TestTool lives in `tests/` so it never compiles into `cargo build --release`, the env-var seam in `registry.rs` is `cfg(any(test, feature = "test-harness"))`, and the cfg-false branch is a no-op stub. Step 06-02 lands a `strings target/release/modeltap | grep MODELTAP_TEST_PLUGINS` absence assertion that fails the build if the env-var string leaks into a release binary.

The feature is **default-on** in `crates/modeltap-app/Cargo.toml` so `cargo test --workspace` exercises the seam without per-crate feature flags. Release pipelines pass `--no-default-features` to strip it.

The `test-harness` feature also pulls in `async-trait` as an optional dep (`async-trait = { version = "0.1", optional = true }` gated behind `test-harness = ["dep:async-trait"]`). Release builds compiled with `--no-default-features` link neither the env-var seam nor the `async-trait` proc-macro into `modeltap-app` itself; the only path by which `async-trait` reaches the release binary is transitively through `modeltap-core` (which needs it to declare the `Tool` trait).

## Two-arm cfg pattern

[[crates/modeltap-app/src/registry.rs]] declares `maybe_register_test_plugins` twice — once decorated with `#[cfg(any(test, feature = "test-harness"))]` containing the env-var-reading logic, and once decorated with `#[cfg(not(...))]` returning a no-op.

The compiler picks exactly one based on build flags. The two-arm pattern is preferred over a single function with an `if cfg!(...)` runtime check because the no-op arm has zero code references to `"MODELTAP_TEST_PLUGINS"` (string literal absent) or to `tests::test_tool::TestTool` (path unreachable). Release builds therefore cannot accidentally execute the seam, and the post-build `strings` check has nothing to match against.

`collect_plugins()` calls `maybe_register_test_plugins` unconditionally; the cfg switch happens at the function definition, not at the call site. This keeps the call-site clean and the dispatch decision in one place.

## TestTool surface

[[tests/src/test_tool.rs]] defines `pub struct TestTool { root: PathBuf, ... }` and `impl Tool for TestTool` with all nine methods.

`discover()` returns one model at a path the harness writes ahead of time. `inspect_tool()` returns `Ok(ToolDetail { detected_version: Some("test-1.0.0"), ... })`. `inspect_model()` returns `Ok(ModelDetail { metadata_kv: { "test.kind" -> "synthetic" }, ... })`.

Methods the walking skeleton does not exercise (`link`, `delete_one`, `delete_all`, `delete_folder`) return sensible stub `Ok` values rather than panicking. The future Phase 05 scenarios (US-26 mutation-path coverage) will need richer behavior here, but the Phase 01 contract is "does not panic and returns the type the trait promises".

[[crates/modeltap-app/tests/plugin_registry_test_harness.rs]] asserts the registration: setting `MODELTAP_TEST_PLUGINS=test-tool` produces a `Vec<Box<dyn Tool>>` containing a TestTool entry. The test runs under the `test-harness` feature (Cargo enables it automatically for `[[test]]` targets in the same crate).

## Per-method behavior-override env vars

Beyond registration, individual TestTool methods read additional env vars to flip behavior at scenario boundaries without modifying production plugin code.

`MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED=1` is the first of these. When set, both [[tests/src/test_tool.rs]]'s canonical `TestTool::inspect_tool` and [[crates/modeltap-app/src/registry.rs]]'s inline `TestToolRegistration::inspect_tool` return `Err(InspectError::Unsupported { tool })` instead of their normal `Ok(ToolDetail)`.

The env var exists because step 02-01's tool-detail acceptance scenarios need to assert AC-21-3 ("(not detectable)" version) and AC-21-4 (cache-sourced `last_error` rendering) under the default-Unsupported path that every production plugin will exhibit until step 02-02 lands the real Ollama override. Without the seam the acceptance suite would either (a) modify production plugin code just to test the default path, or (b) wait for step 02-02 before any tool-detail scenario can ship.

The two impls (acceptance crate + in-binary registration) match each other so a fixture that sets the env var sees the same `Unsupported` behavior whether the test invokes TestTool directly or through the modeltap binary's in-binary `TestToolRegistration`. Both impls are inside the same `cfg(any(test, feature = "test-harness"))` regions that gate the rest of the seam — release builds compiled with `--no-default-features` link neither read of the env var.
