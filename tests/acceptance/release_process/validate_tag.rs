// Acceptance tests for `cargo xtask validate-tag`.
//
// Step: 01-02 (Walking Skeleton — TAG activity).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/walking-skeleton.feature, US-02.
//
// Strategy C — real local resources (DWD-01):
//   - Real tempdir (`tempfile::TempDir`) for fixture Cargo.toml
//   - Real subprocess (`cargo run --package xtask -- ...`)
//   - Real exit-code observation
//
// We deliberately invoke through `cargo run --package xtask --` rather than
// shelling out to a pre-built binary. This (a) lets `cargo test --workspace`
// build everything in one pass, and (b) honours the `cargo xtask` alias the
// human maintainer would use locally.
//
// The xtask binary reads `./Cargo.toml` from its current working directory,
// so each test sets `current_dir` to its own tempdir containing a fixture
// Cargo.toml whose `[workspace.package].version` is the value under test.

use assert_cmd::prelude::OutputAssertExt;
use modeltap_acceptance::{fixture_workspace, xtask_in};
use predicates::str::contains;

// =============================================================================
// Scenario: Validate-tag accepts a tag that matches the workspace version
// (walking-skeleton.feature, US-02, primary WS scenario)
// =============================================================================

#[test]
fn validate_tag_exits_zero_when_tag_matches_workspace_version() {
    let workspace = fixture_workspace("0.1.0");

    let output = xtask_in(workspace.path(), &["validate-tag", "--tag", "v0.1.0"])
        .output()
        .expect("invoke cargo xtask validate-tag");

    output
        .assert()
        .success()
        .stderr(predicates::str::is_empty());
}

// =============================================================================
// Scenario: Validate-tag rejects a tag that does not match the workspace version
// =============================================================================

#[test]
fn validate_tag_exits_nonzero_with_message_when_tag_does_not_match() {
    let workspace = fixture_workspace("0.1.0");

    let output = xtask_in(workspace.path(), &["validate-tag", "--tag", "v0.2.0"])
        .output()
        .expect("invoke cargo xtask validate-tag");

    output.assert().failure().stderr(contains(
        "tag v0.2.0 does not match workspace version 0.1.0",
    ));
}
