# modeltap — Project Guide for Claude

This project is `modeltap`, a Rust TUI for managing local AI models across multiple tools (Ollama, Hugging Face cache, LM Studio, Atomic Chat) with UMR-style cross-tool unification via hardlinks.

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

## CI Lint Discipline (MANDATORY before `git push`)

CI runs `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` on stable Rust. Per-step crafters use `cargo fmt -p <crate>` to keep diffs surgical, but this leaves cross-crate formatting drift.

**Before any `git push` to main**, run:

```sh
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

If `cargo fmt --all` produces a diff, commit it as a single `chore: cargo fmt --all` commit before pushing. CI's clippy may also surface lints from a newer Rust stable than the local toolchain — fix forward (do not `#[allow(...)]` blanket-suppress).

## Running Tests Fast on macOS

`cargo test --workspace` produces ~75 integration test binaries. Two factors made fresh-build test runs take 30–90 minutes (or hang indefinitely):

1. **Cargo flock deadlock (FIXED)** — release_process acceptance tests under `tests/acceptance/release_process/` used to invoke `cargo run --package xtask` from inside `cargo test --workspace`. Both contend for the same exclusive `target/.cargo-lock`, and the child cargo's `flock(LOCK_EX)` blocked behind the parent's compile-time lock. The longest of these tests (walking_skeleton_e2e, version_consistency, idempotent_retry, follow_up_workflows) chained 4–10 such invocations per scenario and could stall for 30+ minutes per test before any progress. As of the optimization in `tests/src/lib.rs::xtask_in()`, the helper invokes the prebuilt binary at `<workspace>/target/debug/xtask` directly — no cargo, no flock. The four duplicated `xtask_in` definitions in `recovery.rs`, `release_prep.rs`, `walking_skeleton_e2e.rs`, and `bump_tap_formula.rs` were collapsed to import the lib version.

2. **macOS Gatekeeper first-run scan** — on Sonoma+ each freshly-linked Mach-O binary triggers a synchronous syspolicyd / XProtect yara-rules scan on first execution (10–100 s per binary). Cargo runs test binaries serially, so a clean-build test run pays this cost ~75 times.

   The actual fix is system-level: add your terminal emulator (Terminal.app, iTerm2, Ghostty, etc.) to *System Settings → Privacy & Security → Developer Tools* — binaries spawned by an allowed dev tool skip the first-run scan entirely. After enabling, restart the terminal.

   `scripts/test.sh` invokes every current test binary with `--list` (a sub-millisecond no-op) in parallel batches before handing off to `cargo test`. Partial relief only — kernel scan parallelism is weak — but better than serial.

If your test loop is regularly stuck for >10 minutes with near-zero CPU usage, suspect a stale `cargo test` or `cargo run` process holding the build lock; check `ps -ef | grep cargo` and kill orphans before retrying.

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
│   ├── hf/
│   ├── lm-studio/
│   └── atomic-chat/
└── docs/
    ├── feature/modeltap-tui/  # wave artifacts (intake, discuss, design, ...)
    └── adrs/                  # architectural decision records
```

<!-- dgc-policy-v11 -->
# Dual-Graph Context Policy

This project uses a local dual-graph MCP server for efficient context retrieval.

## MANDATORY: Always follow this order

1. **Call `graph_continue` first** — before any file exploration, grep, or code reading.

2. **If `graph_continue` returns `needs_project=true`**: call `graph_scan` with the
   current project directory (`pwd`). Do NOT ask the user.

3. **If `graph_continue` returns `skip=true`**: project has fewer than 5 files.
   Do NOT do broad or recursive exploration. Read only specific files if their names
   are mentioned, or ask the user what to work on.

4. **Read `recommended_files`** using `graph_read` — **one call per file**.
   - `graph_read` accepts a single `file` parameter (string). Call it separately for each
     recommended file. Do NOT pass an array or batch multiple files into one call.
   - `recommended_files` may contain `file::symbol` entries (e.g. `src/auth.ts::handleLogin`).
     Pass them verbatim to `graph_read(file: "src/auth.ts::handleLogin")` — it reads only
     that symbol's lines, not the full file.
   - Example: if `recommended_files` is `["src/auth.ts::handleLogin", "src/db.ts"]`,
     call `graph_read(file: "src/auth.ts::handleLogin")` and `graph_read(file: "src/db.ts")`
     as two separate calls (they can be parallel).

5. **Check `confidence` and obey the caps strictly:**
   - `confidence=high` -> Stop. Do NOT grep or explore further.
   - `confidence=medium` -> If recommended files are insufficient, call `fallback_rg`
     at most `max_supplementary_greps` time(s) with specific terms, then `graph_read`
     at most `max_supplementary_files` additional file(s). Then stop.
   - `confidence=low` -> Call `fallback_rg` at most `max_supplementary_greps` time(s),
     then `graph_read` at most `max_supplementary_files` file(s). Then stop.

## Token Usage

A `token-counter` MCP is available for tracking live token usage.

- To check how many tokens a large file or text will cost **before** reading it:
  `count_tokens({text: "<content>"})`
- To log actual usage after a task completes (if the user asks):
  `log_usage({input_tokens: <est>, output_tokens: <est>, description: "<task>"})`
- To show the user their running session cost:
  `get_session_stats()`

Live dashboard URL is printed at startup next to "Token usage".

## Rules

- Do NOT use `rg`, `grep`, or bash file exploration before calling `graph_continue`.
- Do NOT do broad/recursive exploration at any confidence level.
- `max_supplementary_greps` and `max_supplementary_files` are hard caps - never exceed them.
- Do NOT dump full chat history.
- Do NOT call `graph_retrieve` more than once per turn.
- After edits, call `graph_register_edit` with the changed files. Use `file::symbol` notation (e.g. `src/auth.ts::handleLogin`) when the edit targets a specific function, class, or hook.

## Context Store

Whenever you make a decision, identify a task, note a next step, fact, or blocker during a conversation, call `graph_add_memory`.

**To add an entry:**
```
graph_add_memory(type="decision|task|next|fact|blocker", content="one sentence max 15 words", tags=["topic"], files=["relevant/file.ts"])
```

**Do NOT write context-store.json directly** — always use `graph_add_memory`. It applies pruning and keeps the store healthy.

**Rules:**
- Only log things worth remembering across sessions (not every minor detail)
- `content` must be under 15 words
- `files` lists the files this decision/task relates to (can be empty)
- Log immediately when the item arises — not at session end

## Session End

When the user signals they are done (e.g. "bye", "done", "wrap up", "end session"), proactively update `CONTEXT.md` in the project root with:
- **Current Task**: one sentence on what was being worked on
- **Key Decisions**: bullet list, max 3 items
- **Next Steps**: bullet list, max 3 items

Keep `CONTEXT.md` under 20 lines total. Do NOT summarize the full conversation — only what's needed to resume next session.
