# modeltap — Project Guide for Claude

This project is `modeltap`, a Rust TUI for managing local AI models across multiple tools (Ollama, llama-cli, Hugging Face cache, LM Studio) with UMR-style cross-tool unification via hardlinks.

## Status

Currently in DELIVER wave (wave 6 of 6). Prior waves DISCUSS / DESIGN / DEVOPS / DISTILL all complete and peer-reviewed (APPROVED). Architecture under `docs/feature/modeltap-tui/design/` and `docs/adrs/`. Acceptance scenarios under `docs/feature/modeltap-tui/distill/features/master-acceptance.feature` (93 scenarios across US-01..US-20 + US-05b).

## Mutation Testing Strategy

`per-feature` — kill-rate gate of ≥80% must pass before finalize. Run via `cargo-mutants` against `modeltap-core` (pure logic) and per-plugin contract tests after walking-skeleton green.

## Development Paradigm

**Rust-idiomatic, multi-paradigm.** This project is OOP-with-traits at the plugin boundary, pure-functional in the domain core, and async I/O at the edges. Specifically:

- **Plugin extensibility (F5/US-18) → trait objects.** The `Tool` trait is the public extension contract. `Box<dyn Tool>` registered into a `Vec` at startup. This is what the user explicitly asked for.
- **Domain logic (compatibility computation, dedup grouping, plan-building) → pure functions over algebraic types.** No interior state, no I/O. Easy to unit-test.
- **Discovery and link/delete operations → async `tokio` I/O at the edges.** Parallel per-tool discovery is required to meet K3 (< 1 s first paint) given the stateless-rediscovery decision.
- **TUI → ratatui with an Elm-like update loop.** `App` state, `Msg` enum, `update(state, msg) -> (state, cmd)` — see ADR-006.

Internal structure is composition over inheritance (Rust has no inheritance). Lecture about "OOP vs FP" is unwelcome — Rust is Rust. Prefer the simplest tool that fits the seam.

## Recommended Agent for DELIVER Wave

`nw-software-crafter` — Outside-In TDD aligns with the dependency-inversion seams the architecture already provides:
- Acceptance tests through `modeltap-app` (composition root).
- Unit tests against pure functions in `modeltap-core`.
- Plugin-contract tests against the `Tool` trait, with in-memory test plugins.

## Constraints DESIGN Has Already Closed (do not relitigate)

1. **No central modeltap-owned model store.** Per intake-brief Q1 override: modeltap reads each tool's directory directly. The "unify" action installs a model file from one tool's store into another tool's store via hardlink/config — there is no `~/.modeltap/store/` staging area.
2. **Stateless rediscovery on every launch.** Per Q7. No persistent index file. Each launch walks every tool's directory.
3. **Dedup key = SHA256 of file content (primary), HF repo+quant (display fallback).** Per Q6. SHA256 is computed lazily on first inspect/dedup operation, cached in process memory only.
4. **Concurrency: detect-and-prompt-then-retry.** Per Q5. No file locking, no PID-detection complexity beyond a soft pre-action check.
5. **Single-model delete (`delete_one`) is in-scope** in addition to whole-tool zap. Per F4 update. The `Tool` trait MUST expose both.
6. **WSL-only on Windows.** Architecturally identical to Linux paths.

## Layout

```
modeltap/
├── Cargo.toml                 # workspace
├── crates/
│   ├── modeltap-core/         # algebraic types + pure logic, no I/O
│   ├── modeltap-tui/          # ratatui rendering + event loop
│   ├── modeltap-app/          # composition root: wires plugins → core → TUI
│   └── modeltap-cli/          # optional non-TUI entrypoint (future)
├── plugins/
│   ├── ollama/
│   ├── llama-cli/
│   ├── hf/
│   └── lm-studio/
└── docs/
    ├── feature/modeltap-tui/  # wave artifacts (intake, discuss, design, ...)
    └── adrs/                  # architectural decision records
```
