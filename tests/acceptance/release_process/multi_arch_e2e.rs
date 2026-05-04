// Multi-arch end-to-end smoke against ephemeral local repos.
//
// Step: 02-05 (closes Phase 02 — multi-arch + integrity).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/integration-checkpoints.feature, INT.AC-5:
//   - "All four build cells succeeding produces all visible effects"
//   - "Any build cell failing produces no visible effects"
//
// Strategy C (DWD-01) + DWD-02 cross-repo seam, multi-arch lift of the
// walking-skeleton exit gate (01-08) to all 4 build matrix cells:
//   - Real tempdir for both modeltap-fake (working) and tap-fake (bare)
//   - Real git init/commit/push/clone for the cross-repo operation
//   - Real `cargo run --package xtask -- ...` subprocess invocations
//   - 4 sha256 sidecars + 4 archive placeholders staged in modeltap-fake/
//     artifacts to simulate the upload-artifact downloads from all 4 cells
//
// What this test PROVES (INT.AC-5 happy path):
//   - When all 4 cells succeed (sidecars present), render-formula renders a
//     4-platform formula and bump-tap-formula opens the bump branch in the
//     ephemeral tap repo with all 4 sha256s round-tripped verbatim.
//
// What this test PROVES (INT.AC-5 failure path):
//   - When any cell "fails" (sidecar missing), render-formula refuses with
//     a non-zero exit identifying the missing sidecar (the gate added in
//     02-04). Therefore no formula is written, bump-tap-formula is never
//     invoked, no bump branch is created in the tap repo, and no PR is
//     opened.
//
// Note: the actual `gh release create` and `gh pr create` commands are gated
// by `@requires_external` and not executed here. The test asserts the
// LOCAL effect that gates them — render-formula refusing means the workflow
// step that calls `gh pr create` would never run, and the publish-github-release
// `needs:` predicate (covered by 02-02) means GH-Release creation is gated
// on all 4 cells succeeding upstream of this point.
//
// PATH note: `~/.pyenv/shims/cc` shadows the real cc on this developer's
// machine and breaks build-script linking. Sub-cargo invocations need
// PATH=/usr/bin:$PATH (handled by the shared `xtask_in` helper in
// modeltap_acceptance::*).

use modeltap_acceptance::{
    git_capture, init_bare_tap_remote, seed_modeltap_fake_workspace,
    seed_tap_remote_with_initial_commit, template_path, write_archive_placeholder, write_sidecar,
    xtask_in, ALL_FOUR_TARGETS,
};
use tempfile::TempDir;

// =============================================================================
// INT.AC-5 happy path: all 4 cells succeeding produces release-able artifacts
// =============================================================================

#[test]
fn multi_arch_e2e_all_four_cells_passing_renders_formula_and_opens_bump_branch() {
    let workdir = TempDir::new().expect("create tempdir");
    let modeltap_fake = workdir.path().join("modeltap-fake");
    std::fs::create_dir(&modeltap_fake).expect("mkdir modeltap-fake");
    let tap_fake_bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&tap_fake_bare).expect("mkdir tap-fake.git");

    let version = "0.2.0";
    let tag = format!("v{version}");

    // Set up the two ephemeral repos.
    seed_modeltap_fake_workspace(&modeltap_fake, version);
    let tap_url = init_bare_tap_remote(&tap_fake_bare);
    seed_tap_remote_with_initial_commit(&tap_fake_bare);

    // Stage all 4 cells' artifacts (sidecar + archive placeholder per cell).
    let artifacts = modeltap_fake.join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");
    for (triple, sha) in ALL_FOUR_TARGETS {
        write_sidecar(&artifacts, version, triple, sha);
        write_archive_placeholder(&artifacts, version, triple);
    }

    // -------------------------------------------------------------------------
    // RENDER: the 4-platform formula renders successfully.
    // -------------------------------------------------------------------------
    let formula_dir = workdir.path().join("Formula");
    std::fs::create_dir_all(&formula_dir).expect("mkdir Formula");
    let formula_path = formula_dir.join("modeltap.rb");
    let release_base_url =
        format!("https://github.com/jeffabailey/modeltap/releases/download/{tag}");

    let render = xtask_in(
        &modeltap_fake,
        &[
            "render-formula",
            "--version",
            version,
            "--template",
            template_path().to_str().expect("utf-8 template path"),
            "--output",
            formula_path.to_str().expect("utf-8 output path"),
            "--sha256-dir",
            artifacts.to_str().expect("utf-8 sha256-dir path"),
            "--release-base-url",
            &release_base_url,
        ],
    )
    .output()
    .expect("invoke xtask render-formula");
    assert!(
        render.status.success(),
        "render-formula must exit zero when all 4 sidecars are present; \
         stderr={}",
        String::from_utf8_lossy(&render.stderr)
    );

    let rendered_formula =
        std::fs::read_to_string(&formula_path).expect("read rendered Formula/modeltap.rb");

    // All 4 platform blocks present, each wired to its own sha256.
    for (triple, expected_sha) in ALL_FOUR_TARGETS {
        let archive = format!("modeltap-{version}-{triple}.tar.gz");
        let url_idx = rendered_formula
            .find(&archive)
            .unwrap_or_else(|| panic!("formula missing {triple} archive URL: {rendered_formula}"));
        let window = &rendered_formula[url_idx..(url_idx + 200).min(rendered_formula.len())];
        assert!(
            window.contains(&format!("sha256 \"{expected_sha}\"")),
            "sha256 for {triple} not wired to its archive URL; window: {window}"
        );
    }

    // -------------------------------------------------------------------------
    // BUMP: bump-tap-formula opens a bump/v<version> branch carrying the
    // 4-platform formula.
    // -------------------------------------------------------------------------
    let bump = xtask_in(
        &modeltap_fake,
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &tap_url,
            "--formula",
            formula_path.to_str().expect("utf-8 formula path"),
        ],
    )
    .output()
    .expect("invoke xtask bump-tap-formula");
    assert!(
        bump.status.success(),
        "bump-tap-formula must exit zero on the multi-arch happy path; stderr=\n{}",
        String::from_utf8_lossy(&bump.stderr)
    );

    // -------------------------------------------------------------------------
    // INT.AC-5 visible-effects assertions: bump branch + 4-platform formula
    // round-tripped into the tap repo. (gh release/PR creation gated by
    // @requires_external; covered structurally by 02-02 atomic-publish.)
    // -------------------------------------------------------------------------
    let refs = git_capture(&tap_fake_bare, &["show-ref"]);
    assert!(
        refs.contains(&format!("refs/heads/bump/{tag}")),
        "tap-fake.git must contain branch bump/{tag} (proves bump PR opened); \
         show-ref:\n{refs}"
    );

    let committed_formula = git_capture(
        &tap_fake_bare,
        &["show", &format!("bump/{tag}:Formula/modeltap.rb")],
    );
    for (triple, expected_sha) in ALL_FOUR_TARGETS {
        let archive = format!("modeltap-{version}-{triple}.tar.gz");
        assert!(
            committed_formula.contains(&archive),
            "committed Formula/modeltap.rb must reference {triple} archive {archive}; \
             got:\n{committed_formula}"
        );
        assert!(
            committed_formula.contains(&format!("sha256 \"{expected_sha}\"")),
            "committed Formula/modeltap.rb must contain {triple}'s sidecar sha256 \
             {expected_sha} verbatim; got:\n{committed_formula}"
        );
    }

    // The committed formula equals the rendered formula byte-for-byte.
    assert_eq!(
        committed_formula, rendered_formula,
        "committed formula must equal rendered formula verbatim — no mutation \
         between render-formula and tap-repo commit"
    );
}

// =============================================================================
// INT.AC-5 failure path: any cell failing produces no visible effects
// =============================================================================

#[test]
fn multi_arch_e2e_one_cell_failing_produces_no_formula_and_no_bump_branch() {
    let workdir = TempDir::new().expect("create tempdir");
    let modeltap_fake = workdir.path().join("modeltap-fake");
    std::fs::create_dir(&modeltap_fake).expect("mkdir modeltap-fake");
    let tap_fake_bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&tap_fake_bare).expect("mkdir tap-fake.git");

    let version = "0.2.0";
    let tag = format!("v{version}");

    seed_modeltap_fake_workspace(&modeltap_fake, version);
    init_bare_tap_remote(&tap_fake_bare);
    seed_tap_remote_with_initial_commit(&tap_fake_bare);

    // Stage only 3 of 4 cells' artifacts. Omitting the aarch64-apple-darwin
    // sidecar simulates that cell failing to produce its archive (the
    // upload-artifact step never ran for that triple).
    let artifacts = modeltap_fake.join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");
    let failing_triple = "aarch64-apple-darwin";
    for (triple, sha) in ALL_FOUR_TARGETS {
        if triple == failing_triple {
            continue;
        }
        write_sidecar(&artifacts, version, triple, sha);
        write_archive_placeholder(&artifacts, version, triple);
    }

    // -------------------------------------------------------------------------
    // RENDER: must REFUSE because the failing cell's sidecar is missing.
    // (This is the gate added in 02-04. INT.AC-5 failure-path correctness
    // depends on this gate firing BEFORE bump-tap-formula is ever invoked.)
    // -------------------------------------------------------------------------
    let formula_dir = workdir.path().join("Formula");
    std::fs::create_dir_all(&formula_dir).expect("mkdir Formula");
    let formula_path = formula_dir.join("modeltap.rb");
    let release_base_url =
        format!("https://github.com/jeffabailey/modeltap/releases/download/{tag}");

    let render = xtask_in(
        &modeltap_fake,
        &[
            "render-formula",
            "--version",
            version,
            "--template",
            template_path().to_str().expect("utf-8 template path"),
            "--output",
            formula_path.to_str().expect("utf-8 output path"),
            "--sha256-dir",
            artifacts.to_str().expect("utf-8 sha256-dir path"),
            "--release-base-url",
            &release_base_url,
        ],
    )
    .output()
    .expect("invoke xtask render-formula");
    assert!(
        !render.status.success(),
        "render-formula MUST refuse when a cell's sidecar is missing — this \
         is the local gate that prevents bump-tap-formula from ever running, \
         which in turn means no PR is opened. stdout={} stderr={}",
        String::from_utf8_lossy(&render.stdout),
        String::from_utf8_lossy(&render.stderr)
    );

    let expected_filename = format!("modeltap-{version}-{failing_triple}.tar.gz.sha256");
    let stderr = String::from_utf8_lossy(&render.stderr);
    assert!(
        stderr.contains(&expected_filename),
        "stderr must name the missing sidecar {expected_filename} so the \
         maintainer can identify the failed cell; got: {stderr}"
    );

    // No formula file is written.
    assert!(
        !formula_path.exists(),
        "no formula file may be written when a sidecar is missing — INT.AC-5 \
         failure path requires zero visible effects"
    );

    // -------------------------------------------------------------------------
    // INT.AC-5 zero-visible-effects assertions: the tap repo has NO bump
    // branch. Because render-formula refused, bump-tap-formula was never
    // invoked, and `gh pr create` was therefore never invoked either.
    // -------------------------------------------------------------------------
    let refs = git_capture(&tap_fake_bare, &["show-ref"]);
    assert!(
        !refs.contains(&format!("refs/heads/bump/{tag}")),
        "tap-fake.git must NOT contain branch bump/{tag} when any cell \
         failed — INT.AC-5 failure path is all-or-nothing. show-ref:\n{refs}"
    );
}
