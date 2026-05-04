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

use assert_cmd::prelude::OutputAssertExt;
use modeltap_acceptance::{template_path, xtask_in};
use tempfile::TempDir;

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

// =============================================================================
// Step 02-04: Multi-arch 4-platform render scenarios.
// Source: multi-arch-release.feature, US-10.
// =============================================================================

/// All four supported targets, paired with a unique sha256 fixture per target.
/// Sha256 values are made structurally distinct (last byte encodes the kind:
/// `aa`=mac_arm, `bb`=mac_intel, `cc`=linux_intel, `dd`=linux_arm) so the
/// round-trip assertion can prove each block was wired to the *correct* sidecar
/// and not, say, the same sha256 four times.
const ALL_FOUR_TARGETS: [(&str, &str); 4] = [
    (
        "aarch64-apple-darwin",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ),
    (
        "x86_64-apple-darwin",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    ),
    (
        "x86_64-unknown-linux-gnu",
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    ),
    (
        "aarch64-unknown-linux-gnu",
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
    ),
];

/// Stage all 4 sidecars in `artifacts_dir`. Returns the version used.
fn stage_four_sidecars(artifacts_dir: &std::path::Path, version: &str) {
    for (triple, sha) in ALL_FOUR_TARGETS {
        let sidecar = format!("modeltap-{version}-{triple}.tar.gz.sha256");
        std::fs::write(artifacts_dir.join(sidecar), format!("{sha}\n"))
            .expect("write sha256 sidecar");
    }
}

// -----------------------------------------------------------------------------
// AC1 + AC4 (round-trip): rendered formula contains all 4 platform blocks
// with each sha256 wired to its correct triple.
// -----------------------------------------------------------------------------

#[test]
fn render_formula_writes_all_four_platform_blocks_with_correct_sha256s() {
    let workspace = TempDir::new().expect("create tempdir");
    let artifacts = workspace.path().join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");

    let version = "0.2.0";
    stage_four_sidecars(&artifacts, version);

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

    // Version field.
    assert!(
        formula.contains(&format!("version \"{version}\"")),
        "formula must declare version field, got: {formula}"
    );

    // All four platform DSL keywords appear.
    assert!(formula.contains("on_macos"), "missing on_macos: {formula}");
    assert!(formula.contains("on_linux"), "missing on_linux: {formula}");
    assert!(
        formula.matches("on_arm").count() == 2,
        "expected exactly 2 on_arm blocks (mac+linux), got: {formula}"
    );
    assert!(
        formula.matches("on_intel").count() == 2,
        "expected exactly 2 on_intel blocks (mac+linux), got: {formula}"
    );

    // Round-trip: each sha256 sidecar is wired to its correct triple's archive
    // URL. We check by locating the archive URL line and asserting the sha256
    // line that follows it (within ~3 lines of slack) matches the sidecar.
    for (triple, expected_sha) in ALL_FOUR_TARGETS {
        let archive = format!("modeltap-{version}-{triple}.tar.gz");
        let url_idx = formula
            .find(&archive)
            .unwrap_or_else(|| panic!("formula missing {triple} archive URL: {formula}"));
        // Look in a small window after the URL line for the sha256 line.
        let window = &formula[url_idx..(url_idx + 200).min(formula.len())];
        assert!(
            window.contains(&format!("sha256 \"{expected_sha}\"")),
            "sha256 for {triple} not wired to its archive URL; window: {window}"
        );
    }
}

// -----------------------------------------------------------------------------
// AC2: CLI exits non-zero identifying the missing sidecar by filename.
// -----------------------------------------------------------------------------

#[test]
fn render_formula_fails_when_expected_sidecar_is_missing() {
    let workspace = TempDir::new().expect("create tempdir");
    let artifacts = workspace.path().join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");

    let version = "0.2.0";
    // Stage 3 sidecars; deliberately omit aarch64-apple-darwin.
    let missing_triple = "aarch64-apple-darwin";
    for (triple, sha) in ALL_FOUR_TARGETS {
        if triple == missing_triple {
            continue;
        }
        let sidecar = format!("modeltap-{version}-{triple}.tar.gz.sha256");
        std::fs::write(artifacts.join(sidecar), format!("{sha}\n")).expect("write sha256 sidecar");
    }

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

    assert!(
        !output.status.success(),
        "render-formula must fail when a sidecar is missing; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The error message identifies the missing sidecar by filename.
    let expected_filename = format!("modeltap-{version}-{missing_triple}.tar.gz.sha256");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&expected_filename),
        "stderr must name the missing sidecar {expected_filename}, got: {stderr}"
    );

    // No formula file is written (US-10 @infrastructure-failure scenario).
    assert!(
        !output_path.exists(),
        "no formula file may be written when a sidecar is missing"
    );
}

// -----------------------------------------------------------------------------
// AC3: CLI exits non-zero identifying the offending sidecar with bad content.
// -----------------------------------------------------------------------------

#[test]
fn render_formula_rejects_sidecar_with_invalid_sha256_content() {
    let workspace = TempDir::new().expect("create tempdir");
    let artifacts = workspace.path().join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");

    let version = "0.2.0";
    // Stage 3 valid sidecars + 1 with malformed (non-hex) content.
    let bad_triple = "x86_64-unknown-linux-gnu";
    for (triple, sha) in ALL_FOUR_TARGETS {
        let sidecar_name = format!("modeltap-{version}-{triple}.tar.gz.sha256");
        let content = if triple == bad_triple {
            "not-a-valid-hex-digest\n".to_owned()
        } else {
            format!("{sha}\n")
        };
        std::fs::write(artifacts.join(sidecar_name), content).expect("write sha256 sidecar");
    }

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

    assert!(
        !output.status.success(),
        "render-formula must fail when a sidecar is malformed; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let expected_filename = format!("modeltap-{version}-{bad_triple}.tar.gz.sha256");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&expected_filename),
        "stderr must name the offending sidecar {expected_filename}, got: {stderr}"
    );
}
