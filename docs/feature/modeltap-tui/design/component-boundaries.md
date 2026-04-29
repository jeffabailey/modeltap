# Component Boundaries — modeltap-tui

This document defines the dependency rules between crates in the workspace and the responsibilities each owns. These rules are enforced by the architecture-lint test (see `technology-stack.md` and ADR-001).

## Workspace Layout

```
modeltap/
├── Cargo.toml                       # workspace root
├── crates/
│   ├── modeltap-core/               # inner layer — pure logic, no I/O
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── domain/
│   │       │   ├── types.rs         # Model, ModelMeta, Format, Capability, ...
│   │       │   ├── tool.rs          # trait Tool (the plugin port)
│   │       │   └── plan.rs          # UnifyPlan, ZapPlan
│   │       ├── logic/
│   │       │   ├── compatibility.rs # compute_indicator (pure fn)
│   │       │   ├── dedup.rs         # group_by_dedup_key (pure fn)
│   │       │   └── plan.rs          # build_unify_plan, build_zap_plan
│   │       ├── ports/               # secondary ports (driven)
│   │       │   ├── hasher.rs        # trait Hasher
│   │       │   ├── fs_probe.rs      # trait FsProbe (same_filesystem?, lsof)
│   │       │   └── clock.rs         # trait Clock
│   │       └── errors.rs
│   │
│   ├── modeltap-tui/                # ratatui rendering + event loop
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── app_state.rs         # the view-model
│   │       ├── update.rs            # Elm-style update fn
│   │       ├── view/
│   │       │   ├── two_pane.rs
│   │       │   ├── detail.rs
│   │       │   ├── confirm_dialog.rs
│   │       │   └── unify_dialog.rs
│   │       └── input/
│   │           ├── keymap.rs        # SHORTCUT_TABLE single source of truth
│   │           └── events.rs        # crossterm → Msg
│   │
│   ├── modeltap-app/                # composition root
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs              # ENTRY POINT
│   │       ├── registry.rs          # plugin assembly
│   │       ├── adapters/            # secondary-port adapters (driven)
│   │       │   ├── sha256_hasher.rs # impl Hasher using sha2
│   │       │   ├── unix_fs_probe.rs # impl FsProbe using nix + lsof
│   │       │   └── system_clock.rs  # impl Clock
│   │       ├── orchestration/
│   │       │   ├── discover_all.rs  # parallel-discovery driver
│   │       │   ├── execute_zap.rs
│   │       │   └── execute_unify.rs
│   │       └── config.rs            # ~/.modeltap/config.toml loader
│   │
│   └── modeltap-cli/                # OPTIONAL v1 / v2 — non-TUI entrypoint
│       ├── Cargo.toml
│       └── src/main.rs              # `modeltap-cli list-models`, etc.
│
└── plugins/
    ├── ollama/
    │   ├── Cargo.toml
    │   ├── src/lib.rs               # impl Tool for OllamaPlugin
    │   └── tests/
    │       ├── contract.rs          # runs the modeltap-core plugin contract test
    │       └── fixtures/            # sample ~/.ollama trees
    ├── llama-cli/
    │   ├── Cargo.toml
    │   ├── src/lib.rs
    │   └── tests/...
    ├── hf/
    │   ├── Cargo.toml
    │   ├── src/lib.rs
    │   └── tests/...
    └── lm-studio/
        ├── Cargo.toml
        ├── src/lib.rs
        └── tests/...
```

## Crate Responsibilities

### `modeltap-core` (inner layer)

**Owns:** algebraic types, the `Tool` trait, the secondary ports, all pure logic (compatibility computation, dedup grouping, plan building), domain error enums.

**Does not:** perform I/O. Open files. Spawn tasks. Touch ratatui. Touch tokio. Know about specific tools or specific filesystem paths.

**Allowed dependencies:** `std`, `serde`, `serde_derive` (for type derives), `thiserror`, `async-trait` (the trait surface needs it for the port definitions).

**Test layer:** pure unit tests + property tests using `proptest`. Plugin contract test fixtures live here for plugins to consume.

### `modeltap-tui`

**Owns:** the visual layout, the event loop, the keymap, the rendering of `Inventory` / `Plan` / `LastAction` to a terminal buffer.

**Does not:** perform discovery, perform mutation, talk to the filesystem, or know about specific plugins by name.

**Allowed dependencies:** `modeltap-core`, `ratatui`, `crossterm`, `tracing` (for in-TUI diagnostic display).

**Communication with the rest of the app:** dispatches `Msg` (e.g., `Msg::RequestZap { tool: ToolId }`, `Msg::RequestUnifyDryRun { dedup_group_id }`) on a tokio channel. The composition root subscribes and routes. The TUI receives back `Update` events (e.g., `Update::DiscoveryProgress`, `Update::PlanReady`, `Update::ActionComplete`).

### `modeltap-app` (composition root)

**Owns:** `main()`, the tokio runtime, the plugin registry assembly, the secondary-port adapter implementations, the orchestration of multi-step actions (zap, unify), config loading.

**Allowed dependencies:** everything in the workspace. This is the only place that imports concrete plugin crates.

**Pattern:**

```rust
// crates/modeltap-app/src/registry.rs
pub fn assemble_plugins(config: &Config) -> Vec<Box<dyn Tool>> {
    let mut v: Vec<Box<dyn Tool>> = Vec::new();
    v.push(Box::new(modeltap_plugin_ollama::OllamaPlugin::new(config)));
    v.push(Box::new(modeltap_plugin_llama_cli::LlamaCliPlugin::new(config)));
    v.push(Box::new(modeltap_plugin_hf::HfPlugin::new(config)));
    v.push(Box::new(modeltap_plugin_lm_studio::LmStudioPlugin::new(config)));
    v
}
```

If `inventory` is used (ADR-001), the registry function instead calls `inventory::iter::<PluginFactory>` and the explicit `push` lines disappear.

### `plugins/<name>/`

**Owns:** the per-tool on-disk-layout knowledge: where the tool stores models, what counts as a model entry, how to register an external file with the tool, how to delete cleanly.

**Does not:** know about other plugins, about the TUI, or about the composition root. Knows only about `modeltap-core` types.

**Allowed dependencies:** `modeltap-core`, `tokio`, plugin-specific crates the plugin author needs (e.g., `gguf-rs` for the llama-cli plugin's GGUF header parsing, `walkdir` for the HF plugin's tree walk). Each plugin manages its own dep tree.

**Required test:** every plugin must include `tests/contract.rs` that runs the parameterized plugin contract test from `modeltap-core` against itself with fixture data under `tests/fixtures/`.

### `modeltap-cli` (optional)

**Owns:** non-TUI entrypoints for scripting (e.g., `modeltap-cli list-models --tool ollama --json`, `modeltap-cli zap --tool llama-cli --confirm llama-cli`). Useful for testing and for CI/CD users.

**Status in v1:** **seam left open, code not shipped.** The composition root is structured so that adding a CLI binary is purely additive — it imports the same registry assembly and orchestration modules from `modeltap-app`.

## Dependency Rules (Enforced by CI)

The architecture lint test (`tests/architecture.rs` at workspace root) parses `cargo metadata --format-version 1` and asserts:

1. **R1: core has no plugin deps.** `modeltap-core`'s direct deps must not include any `plugins/*` crate.
2. **R2: plugins do not depend on each other.** For all i ≠ j, `plugins/<i>` does not declare `plugins/<j>` in its `[dependencies]`.
3. **R3: tui has no plugin deps.** `modeltap-tui`'s direct deps must not include any `plugins/*` crate.
4. **R4: tui has no I/O deps.** `modeltap-tui` must not depend on `tokio::fs`, `nix`, or other filesystem crates. (Allowed: `tokio` for channels, `crossterm` for terminal I/O — those are *terminal* I/O, not *model* I/O.)
5. **R5: app is the only assembler.** Only `modeltap-app` (and `modeltap-cli`) may declare path-deps on `plugins/*`.
6. **R6: core is leaf-y.** `modeltap-core` may not depend on `tokio`, `ratatui`, `crossterm`, `reqwest`, `nix`. (Allowed: `async-trait` because the trait definitions need it.)

A failing rule fails CI. New plugins do not need to update this test (it walks the workspace dynamically).

## Sequence Diagram — Discovery (Cold Start)

```
User                modeltap-tui          modeltap-app           plugins/*               filesystem
  |                       |                     |                       |                       |
  |  $ modeltap           |                     |                       |                       |
  |---------------------->|                     |                       |                       |
  |                       |  spawn TUI loop     |                       |                       |
  |                       |  draw skeleton      |                       |                       |
  |<--------(< 150 ms first paint)              |                       |                       |
  |                       |  Msg::StartDiscover |                       |                       |
  |                       |-------------------->|                       |                       |
  |                       |                     |  for each plugin:     |                       |
  |                       |                     |  spawn discover()     |                       |
  |                       |                     |---------------------->|                       |
  |                       |                     |                       |  read manifests       |
  |                       |                     |                       |---------------------->|
  |                       |                     |                       |<----------------------|
  |                       |                     |  per-plugin result    |                       |
  |                       |                     |<----------------------|                       |
  |                       |  Update::ProgressN  |                       |                       |
  |                       |<--------------------|                       |                       |
  |                       |  fill row N         |                       |                       |
  |<--------(stream of updates as plugins finish)                       |                       |
  |                       |                     |  all done →           |                       |
  |                       |                     |  compute_indicator    |                       |
  |                       |                     |  (pure fn in core)    |                       |
  |                       |  Update::Indicators |                       |                       |
  |                       |<--------------------|                       |                       |
  |<--------(< 1.15 s total per §7 budget)                              |                       |
```

## Why this layout

- **Hexagonal separation** lets DELIVER's Outside-In TDD drive from outside (acceptance test boots `modeltap-app` with mock plugins) and inside (unit tests on `modeltap-core` pure fns).
- **Plugins-as-crates** rather than plugins-as-modules ensures the dependency rule "plugins don't depend on each other" is mechanical, not convention. A contributor adding a plugin physically cannot add a sibling plugin as a dependency without it being visible in `Cargo.toml` and failing the lint.
- **Composition root is the only impure assembly point** — easy to read, easy to swap, easy to test by substitution.
- **Single binary** keeps deployment trivial and respects the user's "one statically-linked Rust binary" expectation.
