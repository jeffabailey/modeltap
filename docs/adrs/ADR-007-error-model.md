# ADR-007: Error Model — `thiserror` in Domain, `anyhow` at Edges

## Status

Accepted (2026-04-28).

## Context

Rust has two dominant error patterns:

- **`thiserror`** — derive macro for `std::error::Error` on user-defined enums. Each variant is a known case the caller can match on.
- **`anyhow`** — `anyhow::Error` is an opaque error type with a backtrace; carries any `Box<dyn Error>` plus a context chain.

Common pattern: use `thiserror` in libraries (where callers need to match), use `anyhow` in binaries / edges (where you just want to log and exit).

## Decision

**`thiserror` for error enums in `modeltap-core` and in each `plugins/<name>/`. `anyhow` only at the `modeltap-app::main()` boundary and in `modeltap-app::orchestration` glue where heterogeneous errors converge.**

Rule: if a function lives in `modeltap-core` or a plugin crate, its errors are typed with `thiserror`. If it lives in `modeltap-app/src/main.rs` or in an orchestration module that fan-ins many error sources, it may use `anyhow`.

## Alternatives considered

### A — anyhow everywhere

**Pros:** terse.
**Cons:**
- The `Tool` trait's signature returns `anyhow::Error` ⇒ callers can't pattern-match on `LinkError::CrossFilesystem` to trigger US-19's fallback flow. They'd be string-matching, which is fragile.
- Plugin contract tests can't assert on specific error variants.

**Rejected.**

### B — thiserror everywhere

**Pros:** typed all the way up.
**Cons:**
- `main()` would need a master enum that wraps every domain enum and every glue error. Tedious.
- `anyhow`'s context chain (`.context("while loading config")`) is very useful at the edge for diagnostic logging.

**Rejected as exclusive.**

### C (CHOSEN) — thiserror + anyhow split

Standard Rust idiom. Lets domain code be precise where precision matters; lets edges be terse where they don't.

## Domain error enums

(see `data-models.md` for full definitions)

```rust
// modeltap-core::errors

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError { NotInstalled, PermissionDenied { path, source }, ... }

#[derive(Debug, thiserror::Error)]
pub enum LinkError { CrossFilesystem { canonical, target }, InUse { tool }, ... }

#[derive(Debug, thiserror::Error)]
pub enum DeleteError { NotFound(String), InUse { tool }, ... }
```

These are returned by `Tool` trait methods. The orchestration layer in `modeltap-app` matches on them:

```rust
match plugin.link(canonical, &model, &ctx).await {
    Ok(outcome) => /* update plan */,
    Err(LinkError::CrossFilesystem { .. }) => /* US-19 fallback */,
    Err(LinkError::InUse { tool }) => /* US-17/Q5: prompt user to close tool */,
    Err(other) => return Err(anyhow::Error::from(other).context("during unify")),
}
```

## Anyhow usage

```rust
// modeltap-app::main

fn main() -> anyhow::Result<()> {
    let config = load_config().context("loading ~/.modeltap/config.toml")?;
    let plugins = assemble_plugins(&config);
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(run_app(plugins, config)).context("modeltap exiting")
}
```

The context chain ends up in `~/.modeltap/diagnostics.log` (per US-02 unreadable-dir AC).

## Consequences

### Positive

- Plugin error types are part of the public plugin contract. Easy to test, easy to handle specifically.
- Edges stay terse. `?` propagation works cleanly from `LinkError` to `anyhow::Error` via `From` (thiserror auto-derives).
- Diagnostic log entries have full context chains.

### Negative

- Two error-handling styles in the codebase. Convention enforced via code review, not tooling. Acceptable for a small project.

## Enforcement

A small clippy lint config in `.cargo/config.toml`: `clippy::missing_errors_doc` warning level for `pub fn` in `modeltap-core` to nudge towards documenting error variants. No automated check that `anyhow` doesn't leak into domain code; manual review at PR.
