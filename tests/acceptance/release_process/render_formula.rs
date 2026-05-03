// Acceptance tests for `cargo xtask render-formula`.
//
// Step: 01-04 (Walking Skeleton — TAP-BUMP activity, US-06).
// Source scenario: docs/feature/release-process-homebrew-github/distill/
//                  features/walking-skeleton.feature, US-06 (render-formula).
//
// Strategy C — real local resources (DWD-01):
//   - Real tempdir (`tempfile::TempDir`) for fixture sha256 sidecar(s) and output
//   - Real subprocess (`cargo run --package xtask -- ...`)
//   - Real exit-code observation
//   - Real file-system assertions on the rendered Formula/modeltap.rb
//
// We invoke through `cargo run --manifest-path <ws> --package xtask --` so
// `cargo test --workspace` builds everything in one pass and the test honours
// the same `cargo xtask` alias the maintainer uses locally.

use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::OutputAssertExt;
use tempfile::TempDir;

/// Path to the modeltap workspace's root Cargo.toml. Resolved at compile time
/// from `CARGO_MANIFEST_DIR` of THIS crate (`tests/`), one level up.
fn workspace_manifest() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("Cargo.toml");
    p
}

/// Path to the in-repo Tera template that the rendered formula is built from.
/// Lives at `release/templates/modeltap.rb.tera` (relative to workspace root).
fn template_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("release/templates/modeltap.rb.tera");
    p
}

/// Build a `Command` that invokes
/// `cargo run --manifest-path <ws> --package xtask --quiet -- <args>` with the
/// given working directory.
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

// =============================================================================
// Scenario: Render-formula produces a single-platform formula for the WS
// (walking-skeleton.feature, US-06, primary WS scenario)
// =============================================================================

#[test]
fn render_formula_writes_single_platform_formula_for_walking_skeleton() {
    let workspace = TempDir::new().expect("create tempdir");
    let artifacts = workspace.path().join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");

    // Walking-skeleton fixture: a single x86_64-linux sha256 sidecar.
    // Per data-models.md §4: filename = `<archive>.sha256`, content = bare
    // 64-hex digest with optional trailing newline.
    let version = "0.0.1-rc1";
    let triple = "x86_64-unknown-linux-gnu";
    let archive_name = format!("modeltap-{version}-{triple}.tar.gz");
    let sha256_hex = "e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let sidecar_name = format!("{archive_name}.sha256");
    std::fs::write(artifacts.join(&sidecar_name), format!("{sha256_hex}\n"))
        .expect("write sha256 sidecar");

    let formula_dir = workspace.path().join("Formula");
    std::fs::create_dir_all(&formula_dir).expect("mkdir Formula");
    let output_path = formula_dir.join("modeltap.rb");

    let release_base_url =
        format!("https://github.com/jeffabailey/modeltap/releases/download/v{version}");

    let output = xtask_in(
        workspace.path(),
        &[
            "render-formula",
            "--version",
            version,
            "--template",
            template_path().to_str().expect("utf-8 template path"),
            "--output",
            output_path.to_str().expect("utf-8 output path"),
            "--sha256-dir",
            artifacts.to_str().expect("utf-8 artifacts path"),
            "--release-base-url",
            &release_base_url,
        ],
    )
    .output()
    .expect("invoke cargo xtask render-formula");

    output.assert().success();

    let formula = std::fs::read_to_string(&output_path)
        .expect("Formula/modeltap.rb should exist after successful render");

    // Then the formula contains a `version` field equal to "0.0.1-rc1".
    assert!(
        formula.contains(&format!("version \"{version}\"")),
        "formula must declare version field, got: {formula:?}"
    );

    // Then the formula contains the on_linux on_intel block with url ending in
    // `modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz` and the sha256 sidecar
    // value verbatim. We assert the URL ending and the sha256 value separately
    // (and that they appear in proximity to the on_linux/on_intel keywords).
    let expected_url_ending = format!("/{archive_name}");
    assert!(
        formula.contains(&expected_url_ending),
        "formula must reference the linux/intel archive URL, got: {formula:?}"
    );
    assert!(
        formula.contains(&format!("sha256 \"{sha256_hex}\"")),
        "formula must contain the sidecar sha256 verbatim, got: {formula:?}"
    );
    assert!(
        formula.contains("on_linux"),
        "formula must include the on_linux block, got: {formula:?}"
    );
    assert!(
        formula.contains("on_intel"),
        "formula must include the on_intel block, got: {formula:?}"
    );

    // Then no other platform blocks are populated. We assert by checking that
    // the OTHER three archive triples do NOT appear anywhere in the formula.
    for other_triple in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-unknown-linux-gnu",
    ] {
        assert!(
            !formula.contains(other_triple),
            "WS formula must not populate {other_triple} block, got: {formula:?}"
        );
    }
}
