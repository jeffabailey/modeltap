# Technology Stack — modeltap-tui

All dependencies are OSS (MIT, Apache-2.0, or dual). No proprietary dependencies.

## Languages and toolchains

| Item | Choice | License | Why |
|---|---|---|---|
| Language | **Rust 2021 edition, MSRV 1.80** | n/a | Mandated by user. Memory-safe, no GC, single static binary, mature TUI ecosystem. |
| Build system | **Cargo workspace** | n/a | Standard. Multi-crate layout (core / tui / app / cli / plugins/*). |
| MSRV pin | 1.80 | — | Pinned in workspace Cargo.toml. Allows async-in-trait fallback via async_trait without forcing 1.75+ poll-pinning gymnastics. |

## Runtime / async

| Crate | Version line | License | Role | Alternatives considered |
|---|---|---|---|---|
| `tokio` | 1.x (current LTS line) | MIT | Async runtime; parallel discovery; spawn for plugin panic isolation | `async-std` (smaller community, 2024 maintenance slowed); `smol` (lighter but less ecosystem); `std` only (would force serial discovery; fails K3 budget) |
| `async-trait` | 0.1.x | MIT/Apache-2.0 | `async fn` in trait objects (Rust async-in-trait stable but trait objects still need help) | Native `async fn in trait` (Rust 1.75+) — chose async_trait because we use `Box<dyn Tool>` which native syntax doesn't yet support cleanly |
| `futures` | 0.3.x | MIT/Apache-2.0 | Stream/join utilities | n/a — paired with tokio |

## TUI

| Crate | Version line | License | Role | Alternatives considered |
|---|---|---|---|---|
| `ratatui` | 0.x current | MIT | TUI rendering | `cursive` (more widget-y, less ratatui-style declarative); `termion` (low-level, deprecated for new projects); `tui-rs` (predecessor of ratatui, archived). Ratatui is the active fork and the user explicitly named it. |
| `crossterm` | 0.x current | MIT | Cross-platform terminal backend used by ratatui | `termion` (Unix-only — fails cross-platform); `pancurses` (heavier, requires curses dev libs) |

## Filesystem and platform

| Crate | License | Role |
|---|---|---|
| `dirs` | MIT/Apache-2.0 | Cross-platform home/cache/config dirs. `dirs::cache_dir()`, `dirs::home_dir()`. |
| `walkdir` | MIT/Apache-2.0 | Recursive directory walking with sane defaults (follows symlinks under control). Used by HF + LM Studio plugins. |
| `nix` | MIT | POSIX `stat` for `dev_t` cross-fs check; `posix_fadvise` to hint sequential SHA256 read. Unix-only; gated `cfg!(unix)`. |
| `std::fs` | n/a | `hard_link`, `metadata`, `rename`, `remove_file` — direct stdlib usage. |

## Hashing

| Crate | License | Role |
|---|---|---|
| `sha2` | MIT/Apache-2.0 | SHA-256 implementation (RustCrypto). Pure-Rust, audited, fast. |
| `digest` | MIT/Apache-2.0 | RustCrypto trait shared with `sha2`. Lets the `Hasher` port mock cleanly in tests. |

For a 50 GB GGUF, SHA256 at ~500 MB/s on a modern SSD ≈ 100 s. Mitigation: lazy compute, in-process cache, optional `--prefetch-hashes` flag (deferred). See ADR-002.

## Configuration

| Crate | License | Role |
|---|---|---|
| `toml` | MIT/Apache-2.0 | `~/.modeltap/config.toml` parsing |
| `serde` + `serde_derive` | MIT/Apache-2.0 | Type derives for config + diagnostic log entries |

Config schema (preview; full schema in DELIVER):

```toml
# ~/.modeltap/config.toml — all keys optional

[plugins.llama-cli]
search_paths = ["~/llms", "~/models", "/data/models"]

[plugins.lm-studio]
search_paths = ["~/.cache/lm-studio/models", "~/.lmstudio/models"]

[plugins.hf]
# Honors HF_HOME env var by default; this only overrides
hub_dir = "/data/hf-cache/hub"

[telemetry]
enabled = false  # opt-in, default false (C5)
```

## Logging and diagnostics

| Crate | License | Role |
|---|---|---|
| `tracing` | MIT | Structured logging; per-plugin spans for diagnostics. |
| `tracing-subscriber` | MIT | Writes to `~/.modeltap/diagnostics.log`. Suppresses to stderr only on TUI-disabled mode. |

## Error handling

| Crate | License | Role |
|---|---|---|
| `thiserror` | MIT/Apache-2.0 | Domain error enums in `modeltap-core` and per-plugin |
| `anyhow` | MIT/Apache-2.0 | Edge errors at `modeltap-app::main()` and CLI boundaries |

ADR-007 details the split.

## Plugin registration

| Crate | License | Role |
|---|---|---|
| `inventory` | MIT/Apache-2.0 | Static plugin registration without modifying `modeltap-core` (`inventory::collect!` + `inventory::submit!`). |

Alternative: a hand-rolled `static PLUGINS: &[fn() -> Box<dyn Tool>]` slice in `modeltap-app/src/registry.rs` — adding a plugin then adds one line. ADR-001 picks `inventory` for v1 to satisfy the strictest reading of "zero changes outside `plugins/<new>/`".

## TUI testing

| Crate | License | Role |
|---|---|---|
| `insta` | Apache-2.0 | Snapshot testing for ratatui buffer outputs (text snapshots) |
| `tokio-test` | MIT | Test utilities for async code |
| `tempfile` | MIT/Apache-2.0 | Temporary fixture directories for plugin contract tests |
| `proptest` | MIT/Apache-2.0 | Property-based tests for `compute_indicator`, `group_by_dedup_key`, `build_unify_plan` (pure functions) |

## CI and licensing

| Tool | License | Role |
|---|---|---|
| `cargo-deny` | Apache-2.0 | License allow-list (MIT, Apache-2.0, BSD, MPL-2.0); banned-crates list; advisory check |
| `cargo-machete` | MIT/Apache-2.0 | Detect unused workspace dependencies |
| `cargo-audit` | MIT/Apache-2.0 | RustSec advisory check |
| `cargo-modules` | MIT/Apache-2.0 | Dependency graph visualization (manual / on-demand) |

## Architecture enforcement

A workspace-level integration test (`tests/architecture.rs`) parses `cargo metadata --format-version 1` JSON and asserts:

1. `modeltap-core` declares no path-dependency on any `plugins/*` crate.
2. No `plugins/*` crate depends on another `plugins/*` crate.
3. `modeltap-tui` declares no dependency on a concrete plugin crate.
4. `modeltap-app` is the only crate depending on every plugin crate.

This is the Rust analog of ArchUnit / dependency-cruiser. Failing this test fails CI. See ADR-001.

## What's intentionally NOT here

- **No SQLite, no rocksdb, no sled.** State is on tool directories; no embedded DB needed.
- **No serde-json for state files.** No state files.
- **No reqwest, no hyper, no any HTTP client.** No remote integrations in v1.
- **No clap.** v1 has no CLI subcommands beyond the binary itself; if a `--list-models` flag is added in v1.x, then add `clap` later. Don't pay the dep cost now.
- **No env_logger, log4rs.** `tracing` covers it.
- **No proprietary anything.** Verified by `cargo-deny` license allow-list.

## Total dependency footprint (estimate)

About 18 direct deps in workspace `Cargo.toml`s. Transitive closure ~150-200 (typical for tokio + ratatui projects). All MIT / Apache / BSD / MPL. No GPL / LGPL / AGPL / proprietary in tree.
