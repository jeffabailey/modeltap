# ADR-005: Async Runtime — Tokio

## Status

Accepted (2026-04-28).

## Context

Stateless-rediscovery (ADR-003) plus the K3 first-paint budget (< 1 s) requires parallel per-plugin discovery. We need an async runtime.

Rust async runtimes (2026):

- **tokio** — dominant, multi-threaded by default, large ecosystem.
- **async-std** — second tier; ecosystem support has slowed.
- **smol** — small, single-binary friendly, less ecosystem.
- **pollster** / synchronous — works but defeats parallelism.

## Decision

**tokio (1.x current, multi-threaded runtime, default features).**

## Alternatives considered

### A — async-std

**Pros:** stdlib-like API.
**Cons:** much smaller ecosystem; ratatui examples and async tooling lean tokio.
**Rejected** on ecosystem alignment.

### B — smol

**Pros:** small dep footprint.
**Cons:** ecosystem mismatch; would need `smol-tokio` adapters for `inventory` and other tokio-flavored deps.
**Rejected.**

### C — Synchronous + threads

**Pros:** zero async complexity.
**Cons:** parallel discovery would require manual `std::thread::spawn` plus a sync channel; works but loses cancellation, structured concurrency, and the natural plugin-trait `async fn` shape. Trait-object support for `async fn` is exactly what we need from `async_trait`.
**Rejected** for ergonomics; we'd reinvent half of tokio.

### D — tokio current-thread runtime

**Pros:** smaller scheduler.
**Cons:** prevents true parallel discovery on multi-core. Single-thread runtime serializes I/O — no first-paint benefit.
**Rejected** for `multi_thread` runtime.

## Consequences

### Positive

- Parallel `discover()` per plugin via `tokio::task::spawn`.
- Plugin panic isolation via `JoinError` (panics caught at task boundary).
- Cancellation primitives (`CancellationToken`) for "user pressed Esc; abort discovery" scenarios.
- Standard ecosystem alignment — most Rust crates we need (`reqwest`, `sqlx`, etc., should we ever need them) are tokio-first.

### Negative

- Heavier dep tree (~30 transitive deps for tokio + async-trait + futures). Acceptable for a CLI tool whose binary already includes ratatui.
- Runtime startup adds ~10 ms to process start. Within the K3 budget allocation in §7.

## Configuration

```toml
[dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "fs", "macros", "sync", "time", "process", "signal"] }
```

Avoid `tokio = { features = ["full"] }` — pulls in unused subsystems and bloats compile time.

## Migration trigger

If binary size becomes a serious user complaint or if WASM-host targets become a goal, revisit smol or a synchronous-with-threadpool design. Neither is plausible for v1.
