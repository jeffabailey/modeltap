# Technology Stack — release-process-homebrew-github

**Wave:** DESIGN (3 of 6)
**Date:** 2026-05-03

Authoritative pinning of every external tool, action, and crate touched by the release pipeline. Every entry is OSS with documented license. CI parity with `.github/workflows/ci.yml` is preserved (C7).

## 1. Toolchain

| Tool | Version pin | License | Source | Rationale |
|---|---|---|---|---|
| Rust toolchain | `stable` (via `dtolnay/rust-toolchain@stable`) | MIT/Apache-2.0 | <https://github.com/dtolnay/rust-toolchain> | Matches `ci.yml` exactly per C7. No nightly; no MSRV pin beyond `Cargo.toml`'s `rust-version = "1.75"`. |
| Cargo | bundled with stable Rust | MIT/Apache-2.0 | <https://github.com/rust-lang/cargo> | Same as ci.yml. |
| `rustfmt` component | bundled with stable | MIT/Apache-2.0 | (Rust standard) | Used by `cargo fmt --all -- --check`. |
| `clippy` component | bundled with stable | MIT/Apache-2.0 | (Rust standard) | Used by `cargo clippy --workspace --all-targets -- -D warnings`. |

## 2. Build/Cross-Compile

| Tool | Version pin | License | Source | Rationale |
|---|---|---|---|---|
| `cross` | `0.2.5` (Cargo.toml dev-dep OR `cross/cross-action@v1` GitHub Action) | MIT/Apache-2.0 | <https://github.com/cross-rs/cross> | Mature Docker-based cross-compile for `aarch64-unknown-linux-gnu`. Maintained by the cross-rs project (10k+ stars, monthly releases). See ADR-012 for alternatives considered. |
| `Swatinem/rust-cache@v2` | `@v2` (major-version pin, latest 2.x) | MIT | <https://github.com/Swatinem/rust-cache> | Already used in `ci.yml`. Caches `~/.cargo/registry`, `~/.cargo/git`, `target/` per (target, Cargo.lock hash). |
| `actions/checkout@v4` | `@v4` | MIT | <https://github.com/actions/checkout> | GitHub-maintained. Used in every job. |
| `actions/upload-artifact@v4` | `@v4` | MIT | <https://github.com/actions/upload-artifact> | Cross-job artifact passing for archives + sha256s. |
| `actions/download-artifact@v4` | `@v4` | MIT | <https://github.com/actions/download-artifact> | Counterpart for upload. |

## 3. Release & Provenance

| Tool | Version pin | License | Source | Rationale |
|---|---|---|---|---|
| `gh` CLI | pre-installed on hosted runners | MIT | <https://github.com/cli/cli> | GitHub-maintained. Used for `gh release create`, `gh pr create`, `gh pr merge --auto`, `gh attestation verify`. |
| `actions/attest-build-provenance@v2` | `@v2` | MIT | <https://github.com/actions/attest-build-provenance> | GitHub-maintained. Produces SLSA Level 3 attestations signed by GitHub's OIDC provider. ~30s per archive. Required by D6 / US-09. |
| `git` | pre-installed on hosted runners | GPL-2.0 | <https://git-scm.com/> | Used by `actions/checkout`, by `git-cliff`, and by `xtask` for tag/branch operations. |

## 4. Changelog Generation

| Tool | Version pin | License | Source | Rationale |
|---|---|---|---|---|
| `git-cliff` | `2.x` (via `orhun/git-cliff-action@v3`) | MIT/Apache-2.0 | <https://github.com/orhun/git-cliff> | OSS, conventional-commit-driven changelog generator. Used both by `cargo xtask release-prep` (locally) and as a fallback in CI if needed. The `orhun/git-cliff-action@v3` action is the GitHub-Actions wrapper. Per D5. |
| `git-cliff.toml` | committed config at repo root | (config, no license) | (this repo) | Defines section grouping for `feat`, `fix`, `chore`, `refactor`, `docs`, `perf`. |

## 5. xtask Crate Dependencies (Rust)

Added under `xtask/Cargo.toml`:

| Crate | Version pin | License | Source | Rationale |
|---|---|---|---|---|
| `clap` | `4` (already in `[workspace.dependencies]`) | MIT/Apache-2.0 | <https://github.com/clap-rs/clap> | CLI subcommand dispatch. Existing workspace dep. |
| `tera` | `1.20` | MIT | <https://github.com/Keats/tera> | Jinja2-style template engine for rendering `Formula/modeltap.rb`. Mature (5k+ stars), actively maintained. See ADR-014. |
| `toml_edit` | `0.22` | MIT/Apache-2.0 | <https://github.com/toml-rs/toml> | Reads + edits `Cargo.toml` while preserving formatting. Required by `release-prep`'s version-bump step. |
| `semver` | `1` | MIT/Apache-2.0 | <https://github.com/dtolnay/semver> | Version parsing and monotonicity checks. Standard Rust. |
| `anyhow` | `1` (already in `[workspace.dependencies]`) | MIT/Apache-2.0 | <https://github.com/dtolnay/anyhow> | Error type at xtask edges. Matches project convention (CLAUDE.md: "anyhow at edges"). |
| `thiserror` | `1` (already in `[workspace.dependencies]`) | MIT/Apache-2.0 | <https://github.com/dtolnay/thiserror> | Error types in xtask pure-functional core. Matches project convention. |
| `serde` + `serde_json` | `1` (already in `[workspace.dependencies]`) | MIT/Apache-2.0 | <https://github.com/serde-rs/serde> | Parses `cargo metadata --format-version 1` output. |
| `cargo_metadata` | `0.18` | MIT | <https://github.com/oli-obk/cargo_metadata> | Typed wrapper around `cargo metadata`. Avoids hand-parsing JSON. |
| `regex` | `1` | MIT/Apache-2.0 | <https://github.com/rust-lang/regex> | Optional; used by `extract-changelog` subcommand to find `## [X.Y.Z]` headings. |

All crates are workspace-owned via `[workspace.dependencies]` where they already exist; the xtask additions (`tera`, `toml_edit`, `semver`, `cargo_metadata`, `regex`) get added to `[workspace.dependencies]` so they participate in `cargo deny` license auditing.

## 6. Tap Repo Tooling

| Tool | Version pin | License | Source | Rationale |
|---|---|---|---|---|
| `Homebrew/actions/setup-homebrew@master` | `@master` (Homebrew's own action) | BSD-2-Clause | <https://github.com/Homebrew/actions> | Sets up Homebrew on the runner. Used by `test-bot.yml` in tap repo. |
| `homebrew/test-bot` | invoked as `brew test-bot` after setup-homebrew | BSD-2-Clause | <https://github.com/Homebrew/homebrew-test-bot> | Audit + install + test for formula PRs. Auto-merge gate (US-11). |
| `actions/checkout@v4` | `@v4` | MIT | (above) | Same as source repo. |

## 7. Workflow Lint

| Tool | Version pin | License | Source | Rationale |
|---|---|---|---|---|
| `cargo xtask lint-workflows` | (own code) | (this repo) | (this repo) | US-14 enforcement: `release.yml` ≤250 lines, every job has `# Purpose:` comment. ~40 lines of Rust in xtask. |
| `actionlint` (optional, future) | `1.7.x` | MIT | <https://github.com/rhysd/actionlint> | Static analysis of GH Actions workflow files. Not required v1; could be added later if drift problems emerge. |

## 8. Cargo-Deny License Audit

`cargo deny check` already runs in `ci.yml`. Adding new workspace deps (`tera`, `toml_edit`, `semver`, `cargo_metadata`, `regex`) requires that `deny.toml` allow their licenses. All proposed crates are MIT/Apache-2.0 — already in the project's allow-list per the existing modeltap-tui DESIGN.

## 9. Version Pinning Discipline

| Layer | Pinning level | Rationale |
|---|---|---|
| Stable Rust toolchain | `stable` (floating) | Matches `ci.yml`. Drift in stable is rare and CI catches it. C7. |
| GitHub Actions (third-party) | major version (`@v4`, `@v3`, `@v2`) | Standard GH Actions convention. Avoids breakage from new majors; gets bug fixes within the major. |
| GitHub Actions (Homebrew) | `@master` | Homebrew's actions don't follow semver tagging. Documented exception. |
| `gh` CLI | runner-bundled | We don't pin; we observe and pin only if breakage occurs. |
| Rust crates in `xtask` | minor-or-major in Cargo.toml; `Cargo.lock` is committed | Standard Rust convention. `--locked` in release builds (US-04) ensures lockfile is honored. |
| `cross` | `0.2.5` (exact pin) | Cross-compile is the highest-risk seam; freeze to a known-good. |
| `git-cliff` | `2.x` via action `@v3` | Major-version pin; recent enough to support modern conventional-commit syntax. |

## 10. Forbidden Tools (explicit list)

These are NOT in the stack; documented to prevent drift:

| Tool | Why excluded |
|---|---|
| `release-please` | Per D2: PR-driven cuts deferred. |
| `cargo-release` | Per D2: deferred. |
| Apple Developer ID signing / notarization (`rcodesign`, `xcrun notarytool`) | Per D3: deferred to a future feature. |
| Argo / Tekton / Jenkins / CircleCI | GitHub Actions is the substrate. |
| Docker Hub / private registries | All artifact hosting via GitHub Releases. |
| Proprietary code-signing services | OSS-only stance. |
| `nightly` Rust | Per CLAUDE.md / C7: stable only. |
| `homebrew-core` formula submission | Per D8: not in v1. |
| External telemetry (Sentry, DataDog, etc.) | Per K-T2T privacy: GitHub-native data only. |

## 11. License Summary

All tools and crates listed above are under permissive OSS licenses (MIT, Apache-2.0, BSD-2-Clause, or `git`'s GPL-2.0 which is a build-tool, not a linked dependency). No copyleft on the build artifact. No proprietary tools.

This passes the `nw-architecture-patterns` OSS-priority gate.
