// Acceptance tests for `cargo xtask extract-changelog`.
//
// Step: 01-03 (Walking Skeleton — PUBLISH activity, US-05).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/walking-skeleton.feature, US-05.
//
// Strategy C — real local resources (DWD-01):
//   - Real tempdir (`tempfile::TempDir`) for fixture CHANGELOG.md and output
//   - Real subprocess (`cargo run --package xtask -- ...`)
//   - Real exit-code observation
//   - Real file-system assertions on the produced RELEASE_NOTES.md
//
// We invoke through `cargo run --manifest-path <ws> --package xtask --` so
// `cargo test --workspace` builds everything in one pass and the test honours
// the same `cargo xtask` alias the maintainer uses locally.

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
/// --quiet -- <args>` with the given working directory. Using `--manifest-path`
/// rather than relying on cwd lets the test set its own `current_dir` to the
/// fixture tempdir without confusing cargo about which workspace to compile.
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

/// Write a fixture CHANGELOG.md to the tempdir containing two sections, with
/// distinguishable bodies so we can assert "no leakage from adjacent sections".
fn write_two_section_changelog(dir: &std::path::Path) {
    let body = "\
# Changelog

## [0.1.0] - 2026-04-01

Initial public release.

- feat: first thing
- feat: second thing

## [0.0.1-rc1] - 2026-05-03

Walking-skeleton release-candidate.

- chore: bootstrap pipeline
";
    std::fs::write(dir.join("CHANGELOG.md"), body).expect("write CHANGELOG.md");
}

// =============================================================================
// Scenario: Release notes are extracted from the matching changelog section
// (walking-skeleton.feature, US-05, primary WS scenario)
// =============================================================================

#[test]
fn extract_changelog_writes_matching_section_body_to_output_file() {
    let workspace = TempDir::new().expect("create tempdir");
    write_two_section_changelog(workspace.path());

    let output = xtask_in(
        workspace.path(),
        &[
            "extract-changelog",
            "--version",
            "0.0.1-rc1",
            "--input",
            "CHANGELOG.md",
            "--output",
            "RELEASE_NOTES.md",
        ],
    )
    .output()
    .expect("invoke cargo xtask extract-changelog");

    output.assert().success();

    let release_notes = std::fs::read_to_string(workspace.path().join("RELEASE_NOTES.md"))
        .expect("RELEASE_NOTES.md should exist after successful extraction");

    // Body of the requested section must be present.
    assert!(
        release_notes.contains("Walking-skeleton release-candidate."),
        "release notes should contain the matching section body, got: {release_notes:?}"
    );
    assert!(
        release_notes.contains("chore: bootstrap pipeline"),
        "release notes should contain the matching section bullet, got: {release_notes:?}"
    );

    // No leakage from the adjacent "## [0.1.0]" section.
    assert!(
        !release_notes.contains("Initial public release."),
        "release notes must not leak the [0.1.0] body, got: {release_notes:?}"
    );
    assert!(
        !release_notes.contains("first thing"),
        "release notes must not leak [0.1.0] bullets, got: {release_notes:?}"
    );

    // The section heading itself must NOT be in the output — only the body.
    assert!(
        !release_notes.contains("## [0.0.1-rc1]"),
        "release notes should contain the body only (no heading), got: {release_notes:?}"
    );
}

// =============================================================================
// Scenario: Missing changelog section fails with a clear message
// (walking-skeleton.feature, US-05 @infrastructure-failure)
// =============================================================================

#[test]
fn extract_changelog_exits_nonzero_and_writes_no_output_when_section_missing() {
    let workspace = TempDir::new().expect("create tempdir");
    // Single-section changelog: only [0.1.0] exists.
    let body = "\
# Changelog

## [0.1.0] - 2026-04-01

Initial public release.
";
    std::fs::write(workspace.path().join("CHANGELOG.md"), body).expect("write CHANGELOG.md");

    let output = xtask_in(
        workspace.path(),
        &[
            "extract-changelog",
            "--version",
            "0.2.0",
            "--input",
            "CHANGELOG.md",
            "--output",
            "RELEASE_NOTES.md",
        ],
    )
    .output()
    .expect("invoke cargo xtask extract-changelog");

    output
        .assert()
        .failure()
        .stderr(contains("CHANGELOG.md has no [0.2.0] section"));

    // No partial output file must be left behind on failure.
    assert!(
        !workspace.path().join("RELEASE_NOTES.md").exists(),
        "no RELEASE_NOTES.md must be written on SectionNotFound"
    );
}
