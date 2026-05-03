// xtask — build-time tooling for modeltap release pipeline.
//
// SCAFFOLD: true
//
// Wave: DISTILL (5 of 6) — RED scaffolds per Mandate 7.
// Date: 2026-05-03
// Authors: Quinn (nw-acceptance-designer)
//
// All public functions in this crate currently `panic!("Not yet implemented —
// RED scaffold")`. DELIVER (software-crafter, Outside-In TDD) replaces each
// panic with a tested implementation as the matching acceptance scenario is
// enabled.
//
// Module layout matches DESIGN component-boundaries.md §2.2:
//   - cargo_toml: parse_workspace_version, assert_monotonic
//   - tag:        assert_tag_matches
//   - formula:    render (Tera-driven)
//   - changelog:  extract_section
//   - lint:       lint (workflow YAML, line count + purpose comments)
//
// CLI dispatcher lives in `main.rs` and translates argv into calls into these
// modules. Adapter shell-outs (git, cargo, gh, git-cliff, fs) are NOT in this
// scaffold — they are introduced in DELIVER as each subcommand's adapter layer
// is needed.

pub const SCAFFOLD_MARKER: bool = true;

pub mod cargo_toml;
pub mod changelog;
pub mod formula;
// Single seam for filesystem reads. Added in DELIVER step 01-02 (validate-tag);
// every later subcommand's CLI dispatcher pulls Cargo.toml / CHANGELOG.md /
// .sha256 sidecars through this module.
pub mod fs_adapter;
pub mod lint;
pub mod tag;

// Adapter modules introduced in DELIVER step 01-06 (release-prep, US-01).
// Each is a thin shell-out wrapper per component-boundaries.md §2.3:
//   - git_adapter   wraps `git status --porcelain` (clean-tree check)
//   - cargo_adapter wraps `cargo fmt|clippy|test` (CI parity gates) and a
//                   toml_edit-based `[workspace.package].version` mutator
//   - cliff_adapter generates CHANGELOG.md sections from `git log` (pure-Rust
//                   stand-in for git-cliff; see component-boundaries.md §8 for
//                   the future swap to the real `git-cliff` binary)
pub mod cargo_adapter;
pub mod cliff_adapter;
pub mod git_adapter;
