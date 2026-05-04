# ADR-011: xtask placement — repo-root `xtask/` excluded from default workspace

## Status

Proposed (2026-05-03 — DESIGN wave for `release-process-homebrew-github`).

## Context

US-01 introduces a `cargo xtask release-prep` subcommand for maintainer release prep. US-02 / US-06 / US-10 / US-14 reuse the same xtask pattern for `validate-tag`, `render-formula`, `extract-changelog`, and `lint-workflows`. The xtask is non-trivial Rust code: it parses `Cargo.toml`, validates versions, renders Tera templates, and lints YAML.

Three placement options exist for xtask Rust code in a Cargo workspace:

1. **Repo-root `xtask/` directory, declared as a workspace member but excluded from `default-members`**. Invoked via `cargo xtask <subcommand>` (an alias defined in `.cargo/config.toml`).
2. **`crates/xtask/` directory** as a normal workspace member alongside `modeltap-core`, `modeltap-tui`, etc.
3. **Binary target inside an existing crate** (e.g., `modeltap-app/src/bin/xtask.rs`).

Constraints:

- Tera and toml_edit dependencies must NOT pollute production crates (they have no business in `modeltap-app`).
- `cargo build` / `cargo test` (no `--workspace` flag) should NOT compile the xtask — it's a build-time tool, not a runtime artifact.
- The pattern should be recognizable to any Rust contributor; project conventions matter (Riley reads the codebase per K-CONTRIB).

## Decision

**Add `xtask/` at the repo root, declared as a workspace member with `default-members` set so plain `cargo build` / `cargo test` skip it.**

`Cargo.toml` modifications:

```toml
[workspace]
resolver = "2"
members = [
    "crates/modeltap-core",
    "crates/modeltap-tui",
    "crates/modeltap-app",
    "plugins/ollama",
    # ... existing members ...
    "xtask",                # NEW
]
default-members = [          # NEW — explicitly list non-xtask members
    "crates/modeltap-core",
    "crates/modeltap-tui",
    "crates/modeltap-app",
    "plugins/ollama",
    # ... etc, every existing member EXCEPT xtask ...
]
```

`.cargo/config.toml` (new file, or extend if exists):

```toml
[alias]
xtask = "run --package xtask --quiet --"
```

This makes `cargo xtask release-prep --version 0.2.0` invoke `cargo run --package xtask --quiet -- release-prep --version 0.2.0`.

The `xtask/Cargo.toml` declares its own `[dependencies]` for Tera, toml_edit, semver, cargo_metadata — these stay out of the `modeltap-app` dep tree.

## Alternatives Considered

### Alternative 1: `crates/xtask/` as a normal workspace member

- **Pros**: locates xtask alongside other crates; consistent directory pattern.
- **Cons**:
  - **`cargo build` / `cargo test` would compile xtask** unless we also use `default-members`, which negates the "consistent" appeal.
  - **`crates/` is the project's convention for runtime crates** (per CLAUDE.md layout). Putting a build-time tool there sends a misleading signal.
  - **The Rust community convention is `xtask/` at repo root** — see <https://github.com/matklad/cargo-xtask> (the canonical reference, by the matklad of `rust-analyzer`). Tools like `rustc`, `wasmtime`, and many others follow this pattern.
- **Rejection rationale**: clashes with both project layout conventions (runtime vs build-time crates) and the broader Rust ecosystem convention.

### Alternative 2: Binary target inside `modeltap-app`

- **Pros**: zero new directories; trivial to add.
- **Cons**:
  - **Pollutes `modeltap-app` dep tree** with Tera, toml_edit, etc. — these would compile every time `modeltap-app` is built, slowing CI.
  - **Pollutes the production binary's transitive license tree** — every release archive's SBOM would list Tera even though it's never linked.
  - **Conflates concerns**: `modeltap-app` is the composition root of the runtime application; build tooling has no business there.
  - **No convention support**: search Rust ecosystem for "release tooling as a binary inside a runtime crate" — virtually no examples.
- **Rejection rationale**: violates separation between runtime and build-time code; fattens the production crate's dep tree for no benefit.

### Alternative 3: Standalone shell scripts (no Rust xtask)

- **Pros**: zero Rust compile time for tooling.
- **Cons**:
  - **Cross-platform fragility**: bash works on Linux/macOS runners but breaks down for any Windows-touch even via WSL. The project is Linux/macOS today but shell scripts age badly.
  - **No type checking on the rendering logic**: the formula template has 4 platform blocks with sha256s that must be threaded through correctly; getting this wrong via `sed`/`awk` is error-prone.
  - **Hard to unit-test**: Bash has no `cargo test` equivalent.
  - **Mutation testing has no surface**: the project's mutation-testing strategy (CLAUDE.md: ≥80% kill-rate gate) cannot apply to bash.
- **Rejection rationale**: the rendering logic is non-trivial enough to belong in tested Rust code (US-10 explicitly mandates "sha256 read from artifact, not recomputed" — easy to bug-check in Rust, hard to bug-check in shell).

## Consequences

### Positive

- **Standard Rust convention**: any contributor familiar with `rust-analyzer`, `wasmtime`, etc. recognizes the pattern instantly.
- **Clean dep separation**: Tera + toml_edit live only in `xtask/Cargo.toml`. `cargo deny` audits them but they never enter the production binary.
- **`cargo build` / `cargo test` are unaffected**: existing CI runs are unchanged in behavior.
- **`cargo xtask` is the maintainer's single-entry-point**: subcommand catalog evolves in one binary.
- **Mutation testing surface**: the pure-functional core (`xtask::version`, `xtask::formula`, `xtask::changelog`, `xtask::workflow_lint`) gets the same ≥80% kill-rate treatment as `modeltap-core`.

### Negative

- **One new top-level directory**: minor; matches Rust convention.
- **`cargo build --workspace` does compile xtask**: acceptable; the `--workspace` flag is the explicit "build everything" gesture.
- **`.cargo/config.toml` is a new file**: minor maintenance overhead; standard Rust pattern.
- **`default-members` must be kept up-to-date**: when a new runtime crate is added, the maintainer must add it to `default-members` too. Drift between `members` and `default-members` is an easy mistake. Mitigation: `cargo xtask lint-workspace` (out of scope for this feature; can be added later).

### Quality attribute impact

| Attribute | Impact |
|---|---|
| Maintainability | **Positive** — clean separation, standard convention |
| Build performance | **Positive** — xtask deps don't slow runtime crate builds |
| Testability | **Positive** — full Rust testing infrastructure available |
| Contributor onboarding | **Positive** — recognizable pattern |

## References

- matklad/cargo-xtask (canonical reference): <https://github.com/matklad/cargo-xtask>
- DISCUSS `user-stories.md` US-01 (release-prep), US-14 (workflow lint)
- DESIGN `component-boundaries.md` §2
- Project `CLAUDE.md` (Rust paradigm: pure-functional in domain core, async I/O at edges)
