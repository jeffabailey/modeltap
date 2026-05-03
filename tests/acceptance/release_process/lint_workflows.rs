// Acceptance tests for `cargo xtask lint-workflows`.
//
// Step: 01-05 (Walking Skeleton — lint pure function + CLI wiring, US-14).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/hands-off-automation.feature, US-14.
//
// Strategy C — real local resources (DWD-01):
//   - Real tempdir (`tempfile::TempDir`) for fixture workflow YAML files
//   - Real subprocess (`cargo run --package xtask -- ...`)
//   - Real exit-code observation
//
// Three scenarios cover the AC matrix:
//   1. Valid workflow within budget AND every job has a `# Purpose:` comment    -> exit 0
//   2. Workflow exceeds the line budget                                          -> exit non-zero
//   3. A job is missing its `# Purpose:` comment                                 -> exit non-zero
//      and the message identifies the offending job by name.

use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::OutputAssertExt;
use predicates::str::contains;
use tempfile::TempDir;

/// Path to the modeltap workspace's root Cargo.toml. Resolved at compile time
/// from `CARGO_MANIFEST_DIR` of THIS crate (`tests/`), one level up.
fn workspace_manifest() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("Cargo.toml");
    p
}

/// Build a `Command` that invokes `cargo run --manifest-path <ws> --package xtask
/// --quiet -- <args>` with the given working directory.
fn xtask_in(workdir: &std::path::Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.arg("run")
        .arg("--manifest-path")
        .arg(workspace_manifest())
        .arg("--package")
        .arg("xtask")
        .arg("--quiet")
        .arg("--");
    for a in args {
        cmd.arg(a);
    }
    cmd.current_dir(workdir);
    cmd
}

/// Build a synthetic release.yml that:
///   - is `total_lines` lines long (counting blanks + comments + content),
///   - declares two top-level jobs `validate-tag` and `build`,
///   - precedes each job with a `# Purpose: ...` comment line.
///
/// Lines are padded with `# filler` comments after the jobs block so the
/// total line count is exactly `total_lines`.
fn synthetic_workflow_with_two_jobs(total_lines: usize) -> String {
    let header = "\
name: release
on:
  push:
    tags: ['v*']

jobs:
  # Purpose: Refuse a tag that does not match the workspace version.
  validate-tag:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo run -p xtask -- validate-tag --tag ${GITHUB_REF_NAME}

  # Purpose: Cross-compile the modeltap binaries for every supported triple.
  build:
    needs: validate-tag
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
";
    pad_to_line_count(header, total_lines)
}

/// Build a synthetic release.yml where the `build` job lacks its `# Purpose:`
/// comment. `validate-tag` retains its purpose comment so the linter has to
/// pick out the offending job specifically.
fn synthetic_workflow_with_build_missing_purpose(total_lines: usize) -> String {
    let header = "\
name: release
on:
  push:
    tags: ['v*']

jobs:
  # Purpose: Refuse a tag that does not match the workspace version.
  validate-tag:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo run -p xtask -- validate-tag --tag ${GITHUB_REF_NAME}

  build:
    needs: validate-tag
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
";
    pad_to_line_count(header, total_lines)
}

/// Append `# filler` comment lines until the total line count equals exactly
/// `target`. If the seed already has more than `target` lines, returns the
/// seed unchanged (caller is responsible for not requesting impossible sizes).
fn pad_to_line_count(seed: &str, target: usize) -> String {
    let current = seed.lines().count();
    if current >= target {
        return seed.to_owned();
    }
    let mut out = String::with_capacity(seed.len() + (target - current) * 12);
    out.push_str(seed);
    if !seed.ends_with('\n') {
        out.push('\n');
    }
    for _ in current..target {
        out.push_str("# filler\n");
    }
    out
}

// =============================================================================
// Scenario: Lint-workflows accepts a release.yml within the line budget
// (hands-off-automation.feature, US-14, primary scenario)
// =============================================================================

#[test]
fn lint_workflows_exits_zero_when_under_budget_and_every_job_has_purpose_comment() {
    let workspace = TempDir::new().expect("create tempdir");
    let workflow = synthetic_workflow_with_two_jobs(180);
    std::fs::write(workspace.path().join("release.yml"), workflow).expect("write release.yml");

    let output = xtask_in(
        workspace.path(),
        &[
            "lint-workflows",
            "--workflow",
            "release.yml",
            "--max-lines",
            "250",
        ],
    )
    .output()
    .expect("invoke cargo xtask lint-workflows");

    output.assert().success();
}

// =============================================================================
// Scenario: Lint-workflows rejects a release.yml exceeding the line budget
// (hands-off-automation.feature, US-14)
// =============================================================================

#[test]
fn lint_workflows_exits_nonzero_with_line_count_message_when_over_budget() {
    let workspace = TempDir::new().expect("create tempdir");
    let workflow = synthetic_workflow_with_two_jobs(270);
    std::fs::write(workspace.path().join("release.yml"), workflow).expect("write release.yml");

    let output = xtask_in(
        workspace.path(),
        &[
            "lint-workflows",
            "--workflow",
            "release.yml",
            "--max-lines",
            "250",
        ],
    )
    .output()
    .expect("invoke cargo xtask lint-workflows");

    output
        .assert()
        .failure()
        .stderr(contains("250"))
        .stderr(contains("270"));
}

// =============================================================================
// Scenario: Lint-workflows rejects a job missing the purpose comment
// (hands-off-automation.feature, US-14)
// =============================================================================

#[test]
fn lint_workflows_exits_nonzero_and_identifies_job_missing_purpose_comment() {
    let workspace = TempDir::new().expect("create tempdir");
    let workflow = synthetic_workflow_with_build_missing_purpose(200);
    std::fs::write(workspace.path().join("release.yml"), workflow).expect("write release.yml");

    let output = xtask_in(
        workspace.path(),
        &[
            "lint-workflows",
            "--workflow",
            "release.yml",
            "--max-lines",
            "250",
        ],
    )
    .output()
    .expect("invoke cargo xtask lint-workflows");

    output
        .assert()
        .failure()
        .stderr(contains("build"))
        .stderr(contains("Purpose"));
}
