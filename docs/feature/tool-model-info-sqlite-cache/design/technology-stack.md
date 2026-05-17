# Technology Stack — tool-model-info-sqlite-cache

**Wave:** DESIGN (3 of 6)
**Author:** Morgan (nw-solution-architect)
**Date:** 2026-05-17

This document records every new external dependency this feature adds, the rationale for choosing it, the OSS license, and the rejected alternatives. Existing workspace dependencies (tokio, ratatui, sha2, etc.) are unchanged — see parent `docs/feature/modeltap-tui/design/technology-stack.md` for those.

## New crate dependencies

### 1. `rusqlite` — SQLite bindings

| Field | Value |
|---|---|
| Crate | `rusqlite` |
| Version pin | `0.31` (workspace dep; latest stable as of 2026-05) |
| License | MIT |
| Repo | https://github.com/rusqlite/rusqlite |
| Stars / maintenance | 2.9K+ stars; active monthly releases; 100+ contributors |
| Features enabled | `bundled` (ships a known SQLite C library; no system-SQLite version skew between dev and CI) |
| Used by | `modeltap-store` only |

**Why:** the only first-class Rust SQLite client. Sync API matches our edges-async, core-sync style. The `bundled` feature is non-negotiable for reproducible CI (macOS vs Ubuntu ship different system SQLite versions).

**Alternatives considered:**

| Alternative | Verdict | Rationale |
|---|---|---|
| `sqlx` with the SQLite driver | REJECTED | Async-first (we want sync at this layer); compile-time SQL checking requires a `DATABASE_URL` at build time, hostile to CI; pulls a runtime dep tree 3× the size of rusqlite; offers nothing the workspace needs (no Postgres/MySQL future) |
| `sea-orm` / `diesel` | REJECTED | ORM machinery is disproportionate to 4 tables; ADR-017 alternatives analysis discusses |
| `sqlite` (libsqlite3-sys) crate raw | REJECTED | Unsafe FFI surface; rusqlite is the established wrapper |
| Hand-rolled FFI | REJECTED | Reinvented wheel; opens security surface for SQL injection via string formatting |

### 2. `rusqlite_migration` — forward-only migration runner

| Field | Value |
|---|---|
| Crate | `rusqlite_migration` |
| Version pin | `1.2` |
| License | MIT OR Apache-2.0 |
| Repo | https://github.com/cljoly/rusqlite_migration |
| Stars / maintenance | 200+ stars; quarterly releases; single primary maintainer (small surface; risk acceptable) |
| Features enabled | default (`from-directory` for embedding `migrations/*.sql` at compile time) |
| Used by | `modeltap-store` only |

**Why:** minimal wrapper (~500 LoC) that handles `PRAGMA user_version` bumping, ordered migration application, and idempotency. Replaces ~200 LoC of hand-rolled migration logic with a vetted dep.

**Alternatives considered:** see ADR-017.

**Risk note:** single primary maintainer. Mitigation: if the crate is abandoned, the API surface is small enough (`Migrations::new(...).to_latest(conn)`) to fork or inline the relevant logic in `modeltap-store/src/migrate.rs` in <100 LoC. Migration-trigger documented in ADR-017.

### 3. `dirs` — cross-platform standard paths

| Field | Value |
|---|---|
| Crate | `dirs` |
| Version pin | `5` |
| License | MIT OR Apache-2.0 |
| Repo | https://github.com/dirs-dev/dirs-rs |
| Stars / maintenance | 700+ stars; semver-stable since 2020 |
| Features enabled | default |
| Used by | `modeltap-app` (for cache path resolution); future `modeltap-cli` config commands |

**Why:** resolves `$XDG_DATA_HOME` on Linux, `~/Library/Application Support` on macOS, the appropriate `%APPDATA%` path on Windows (WSL inherits Linux). Tested across all three. Already considered by the parent design (its absence in parent stack is a coincidence; this feature is the first to need it).

**Alternatives considered:**

| Alternative | Verdict | Rationale |
|---|---|---|
| `directories` crate (similar API) | REJECTED on net | Both are well-maintained; `dirs` is simpler (no struct hierarchy) and matches our use case (single path lookup) |
| Hand-rolled `std::env::var("XDG_DATA_HOME")` + fallback | REJECTED | macOS does not set XDG vars by default; reinventing the platform fallback table is error-prone |
| Hardcoded `~/.local/share/modeltap/` | REJECTED | Wrong on macOS (should be `~/Library/Application Support/`); violates C-INFO-5 |

## Workspace `Cargo.toml` changes

```toml
# Cargo.toml (workspace root) — additions under [workspace.dependencies]

# Cache layer (per ADR-015, ADR-017)
rusqlite = { version = "0.31", features = ["bundled"] }
rusqlite_migration = "1.2"

# Cross-platform standard paths (per ADR-015 C-INFO-5)
dirs = "5"
```

## New crate Cargo manifest (skeleton)

```toml
# crates/modeltap-store/Cargo.toml

[package]
name = "modeltap-store"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "SQLite-backed cache layer for modeltap (per ADR-015, ADR-016, ADR-017)."

[dependencies]
rusqlite.workspace = true
rusqlite_migration.workspace = true
modeltap-core = { path = "../modeltap-core" }
thiserror.workspace = true
serde.workspace = true
serde_json.workspace = true
time.workspace = true

[dev-dependencies]
tempfile.workspace = true
insta.workspace = true
```

Note: **no `tokio` dependency.** The store is sync; the app calls it via `spawn_blocking`. Enforced by architecture-lint R8.

## CI lint discipline (per CLAUDE.md)

This feature introduces:

- 1 new workspace dependency family (rusqlite + migration)
- 1 new crate (`modeltap-store`)
- 1 new directory tree (`crates/modeltap-store/migrations/*.sql`)

Required CI runs before merge to main:

```sh
cargo fmt --all && \
cargo clippy --workspace --all-targets -- -D warnings && \
cargo test --workspace
```

The `cargo test --workspace` run includes:
- `modeltap-store` unit tests (against `:memory:` SQLite)
- `modeltap-store` integration tests with `tempfile` (corruption recovery, migration matrix)
- `tests/architecture.rs` R7-R9 lint extensions
- Existing parent acceptance suite (must continue to pass)

## Maintenance & audit notes

- **rusqlite bundled SQLite:** ships SQLite 3.45+ (rusqlite tracks upstream within ~1 minor release). Security patches to upstream SQLite require a rusqlite version bump; CI Dependabot is configured to surface these (parent feature owns the Dependabot config).
- **migration files are append-only:** every `migrations/NNNN_*.sql` file is immutable after merge. Schema changes are made via new migration files, not by editing existing ones. ADR-017 documents this discipline.
- **No `unsafe` blocks** are required in `modeltap-store` source. The crate-level lint `#![forbid(unsafe_code)]` is set.
