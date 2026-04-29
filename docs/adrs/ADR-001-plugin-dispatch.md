# ADR-001: Plugin Dispatch — Dynamic In-Process via `Box<dyn Tool>`

## Status

Accepted (2026-04-28).

## Context

modeltap's central architectural constraint (C1, US-18) is that adding a 5th tool requires zero changes to `modeltap-core`. The intake-brief's F5 explicitly says "implementing a small trait." We need to choose how plugins are dispatched at runtime.

Three candidate mechanisms:

1. **Static dispatch via generics.** `App<T1, T2, T3, T4>` parameterized over plugin types. Adding a 5th tool changes the type signature of `App`. Type-erased only at the trait-bound level.
2. **Dynamic dispatch via `Box<dyn Tool>`.** Plugins are trait objects in a `Vec<Box<dyn Tool>>` registered at `main()`. Trait must be object-safe.
3. **Out-of-process / cdylib loading.** Plugins are dynamic libraries loaded via `libloading`. ABI must be stable. New plugin = new `.so` / `.dylib` dropped in a directory.

Constraints to weigh:

- US-18 AC: "adding a 5th plugin requires zero changes to modeltap-core source files."
- US-18 AC: "plugin panics caught — one bad plugin does not crash the TUI."
- K3: < 1 s first paint — dispatch overhead negligible at this granularity, not a discriminator.
- Maintainability: 6-method trait should be cheap to implement.
- Ecosystem: Rust async-trait gymnastics around object safety.

## Decision

**Dynamic in-process dispatch via `Box<dyn Tool>`. Plugins are Rust crates in `plugins/<name>/` linked into the single binary at compile time. Registration via the `inventory` crate so adding a plugin requires zero changes outside its own crate.**

## Alternatives considered

### Alternative A — Static dispatch via generics

```rust
struct App<P: PluginSet> { plugins: P, ... }
```

**Pros:** zero dispatch overhead (negligible benefit here); compile-time-known plugin set.

**Cons:** `App`'s type signature changes when adding a plugin (violates US-18 strictly read); generic explosion makes orchestration code (which iterates over heterogeneous plugins) require macro tricks or a tuple-based PluginSet trait. Cumbersome.

**Rejected** because the type-signature change conflicts with US-18 AC-3 ("zero changes to modeltap-core"). Also, the genuine benefits (perf, monomorphization) do not apply at this granularity — discovery is I/O-bound, not dispatch-bound.

### Alternative B — Out-of-process / cdylib

Plugins compile to `.so` / `.dylib` files; the binary loads them at runtime via `libloading` against a stable C ABI.

**Pros:** truly hot-reloadable plugins; users can install a plugin without rebuilding; plugin panics genuinely cannot crash the host.

**Cons:**
- Rust's async story across cdylib boundaries is hostile (no stable async ABI).
- Trait objects across cdylib are even worse (no stable Rust ABI).
- Forces plugins to a C-ish ABI surface, defeating the "small Rust trait" promise.
- Distribution complexity: per-platform `.so`/`.dylib`/`.dll` files; signature verification; load-path conventions.
- Setup cost is months, not days. Walking-skeleton-in-2-3-days (C4) is unattainable.

**Rejected** as massive overkill for a single-user local CLI. Out-of-process plugins make sense for IDEs, browsers, kernels — not for a tool a user installs once and uses for cleanup.

### Alternative C (CHOSEN) — Dynamic in-process via `Box<dyn Tool>`

Plugins compile into the binary. The `Tool` trait is object-safe (with `async_trait`). Registration uses `inventory` for compile-time discovery without `modeltap-core` knowing the plugin list.

**Pros:**
- Object-safety achievable: `async_trait` macro produces poll-pinned methods that work in trait objects.
- Adding a plugin: create a crate under `plugins/<name>/`, add it to the workspace in `Cargo.toml`, add `path = "plugins/<name>"` to `modeltap-app`'s deps. **The only code touched outside the new plugin crate is `modeltap-app/Cargo.toml`** — `modeltap-core` source files are untouched.
- With `inventory::submit!`, even the registry list update can be eliminated — the plugin self-registers via a `static` linker section. modeltap-app then does `inventory::iter::<PluginFactory>` to assemble the plugin list. Zero changes outside `plugins/<new>/` and `Cargo.toml`.
- Plugin panics caught via `tokio::task::spawn` returning `JoinError` — orchestration shows `(error)` for that tool, app continues.

**Cons:**
- All plugins compile into the same binary — binary grows with N plugins. For the 4 v1 plugins, negligible. If the plugin count balloons to 50, revisit.
- Trait objects have a small dispatch overhead. Orders of magnitude smaller than the I/O it wraps.

## Consequences

### Positive

- US-18 AC-3 satisfied.
- Object-safe trait keeps the orchestration code simple: `for plugin in &self.plugins { plugin.discover().await }`.
- DELIVER can write contract tests against `Box<dyn Tool>` directly.
- Substituting test plugins for acceptance tests is a one-line change at `assemble_plugins()`.

### Negative

- All 4 plugins linked statically; user cannot install just a subset. Acceptable trade-off.
- `async_trait` adds compile-time overhead and a small runtime allocation per call. Acceptable.

## Enforcement

The architecture-lint test (`tests/architecture.rs` per `component-boundaries.md`) enforces that:

- Only `modeltap-app` and `modeltap-cli` declare path-deps on `plugins/*`.
- No `plugins/*` crate depends on another `plugins/*` crate or on `modeltap-tui`.

If a future plugin author tries to add a sibling-plugin dep, CI fails before merge.

## Migration trigger

If plugin count exceeds ~15 OR a serious user demand for hot-installable plugins emerges, revisit Alternative B (cdylib) or Alternative D (subprocess + JSON-RPC, modeled on Language Server Protocol). v1 does not need this.
