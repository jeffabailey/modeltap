// Walking-Skeleton EXIT GATE — end-to-end smoke wiring PREP → TAG → BUILD-
// surrogate → PUBLISH (extract-changelog) → TAP-BUMP against ephemeral local
// repos (DWD-02).
//
// Step: 01-08 (final WS step).
// Source scenario: docs/feature/release-process-homebrew-github/distill/
//                  features/walking-skeleton.feature, US-06:
//   - "Bump-tap-formula opens a PR against the ephemeral tap repository"
//
// This is THE walking-skeleton exit gate — when this test passes green the
// maintainer can (in principle) cut a real x86_64-linux release end-to-end.
// The test exercises the cross-repo seam (modeltap-fake ↔ tap-fake) and the
// xtask subcommand chain that production release.yml jobs invoke.
//
// Strategy C (DWD-01) + DWD-02 cross-repo seam:
//   - Real tempdir for both modeltap-fake (working) and tap-fake (bare)
//   - Real git init/commit/push/clone for cross-repo operations
//   - Real `cargo run --package xtask -- ...` subprocess invocations for
//     each phase: validate-tag, extract-changelog, render-formula,
//     bump-tap-formula
//   - Real Tera template render against the in-repo
//     `release/templates/modeltap.rb.tera`
//
// What we DELIBERATELY do NOT exercise here:
//   - `release-prep` (covered by release_prep.rs; running it again here would
//     require a fully-buildable Rust workspace inside the fixture and would
//     dominate the test cost without exercising new wiring).
//   - `cargo build --release` + `tar` + `sha256sum` (covered by the
//     workflow_structure.rs invariants + the @requires_external smoke).
//   - The `gh release create` step (gated by `@requires_external`).
//   - The `gh pr create` step (gated by `@requires_external`).
//
// What this test PROVES:
//   - The xtask subcommand chain is wired end-to-end with no missing seams.
//   - validate-tag → render-formula → bump-tap-formula round-trips a real
//     sha256 sidecar through the pipeline into a real bump branch in a real
//     bare tap repo.
//   - The committed Formula/modeltap.rb in the tap repo contains the version
//     and sha256 that flowed through from the original sidecar.
//
// PATH note: `~/.pyenv/shims/cc` shadows the real cc on this developer's
// machine and breaks build-script linking. Sub-cargo invocations need
// PATH=/usr/bin:$PATH.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// Path to the in-repo Tera template the rendered formula is built from.
fn template_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("release/templates/modeltap.rb.tera");
    p
}

use modeltap_acceptance::xtask_in;

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("invoke git");
    assert!(status.success(), "git {:?} failed", args);
}

fn git_capture(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("invoke git");
    assert!(
        output.status.success(),
        "git {:?} failed: stderr={}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Seed the modeltap-fake working repo with a Cargo.toml whose
/// `[workspace.package].version` matches `version`, plus a CHANGELOG.md
/// containing one section for that version, plus a sha256 sidecar in
/// `artifacts/`.
fn seed_modeltap_fake(repo: &Path, version: &str, sha256_hex: &str) {
    // Cargo.toml — workspace shape with the version under test.
    let cargo_toml = format!(
        "[workspace]\n\
         resolver = \"2\"\n\
         members = []\n\
         \n\
         [workspace.package]\n\
         version = \"{version}\"\n\
         edition = \"2021\"\n",
    );
    std::fs::write(repo.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");

    // CHANGELOG.md — keep-a-changelog format with one section for `version`.
    let changelog = format!(
        "# Changelog\n\n\
         All notable changes are documented here.\n\n\
         ## [{version}]\n\n\
         ### Added\n\n\
         - Walking-skeleton release-candidate.\n",
    );
    std::fs::write(repo.join("CHANGELOG.md"), changelog).expect("write CHANGELOG.md");

    // sha256 sidecar — bare-hex content, walking-skeleton single triple.
    let triple = "x86_64-unknown-linux-gnu";
    let archive_name = format!("modeltap-{version}-{triple}.tar.gz");
    let sidecar_name = format!("{archive_name}.sha256");
    let artifacts = repo.join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");
    std::fs::write(artifacts.join(&sidecar_name), format!("{sha256_hex}\n"))
        .expect("write sha256 sidecar");
}

/// Initialise a bare git repo at `path` to act as the ephemeral tap remote.
fn init_bare_tap_remote(path: &Path) -> String {
    let status = Command::new("git")
        .args(["init", "--bare", "--quiet", "--initial-branch=main"])
        .arg(path)
        .status()
        .expect("invoke git init --bare");
    assert!(status.success(), "git init --bare {:?} failed", path);
    format!("file://{}", path.display())
}

/// Seed the bare tap remote with one initial commit on `main` so bump
/// branches have a base.
fn seed_tap_remote_with_initial_commit(bare_path: &Path) {
    let working = tempfile::tempdir().expect("create working clone tempdir");
    let working_path = working.path();
    git(
        working_path,
        &["clone", "--quiet", &bare_path.display().to_string(), "."],
    );
    std::fs::create_dir_all(working_path.join("Formula")).expect("mkdir Formula");
    std::fs::write(working_path.join("Formula").join(".keep"), "").expect("write .keep");
    std::fs::write(working_path.join("README.md"), "# homebrew-modeltap\n")
        .expect("write README.md");
    git(working_path, &["add", "."]);
    git(
        working_path,
        &["commit", "--quiet", "-m", "chore: seed tap repo"],
    );
    git(working_path, &["push", "--quiet", "origin", "main"]);
}

// =============================================================================
// THE Walking-Skeleton EXIT GATE TEST
// =============================================================================
//
// Wires the WS pipeline end-to-end against ephemeral local repos:
//
//   1. TAG       — `xtask validate-tag --tag v<V>` against modeltap-fake
//                  Cargo.toml (must exit zero).
//   2. PUBLISH   — `xtask extract-changelog --version <V>` against the
//                  modeltap-fake CHANGELOG.md (writes RELEASE_NOTES.md).
//   3. TAP-BUMP  — `xtask render-formula --version <V> --sha256-dir
//                  modeltap-fake/artifacts` (renders Formula/modeltap.rb), then
//                  `xtask bump-tap-formula --version <V> --tap-repo-url
//                  file://${TMPDIR}/tap-fake.git --formula <rendered>` (pushes
//                  bump/v<V> branch with the rendered formula committed).
//
// Then the test asserts THE WALKING-SKELETON EXIT GATE: the bump branch in
// the bare tap remote contains a Formula/modeltap.rb whose version matches
// `<V>` and whose sha256 matches the sidecar value seeded into modeltap-fake/
// artifacts. This proves the data flowed through the entire pipeline without
// loss.
//
// Note on PREP: `release-prep` is exercised by its own acceptance test
// (release_prep.rs); re-exercising it here would require a fully-buildable
// Rust workspace in the fixture (clippy + test) and would dominate the test
// cost without exercising new wiring. The WS exit gate is about CROSS-REPO
// data flow; release-prep correctness lives in its own test.

#[test]
fn walking_skeleton_exit_gate_validate_tag_extract_changelog_render_and_bump_round_trip() {
    let workdir = TempDir::new().expect("create tempdir");
    let modeltap_fake = workdir.path().join("modeltap-fake");
    std::fs::create_dir(&modeltap_fake).expect("mkdir modeltap-fake");
    let tap_fake_bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&tap_fake_bare).expect("mkdir tap-fake.git");

    let version = "0.0.1-rc1";
    let tag = format!("v{version}");
    let sha256_hex = "e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    // Set up the two ephemeral repos.
    seed_modeltap_fake(&modeltap_fake, version, sha256_hex);
    let tap_url = init_bare_tap_remote(&tap_fake_bare);
    seed_tap_remote_with_initial_commit(&tap_fake_bare);

    // -------------------------------------------------------------------------
    // Phase 1 — TAG: validate-tag accepts the matching tag.
    // -------------------------------------------------------------------------
    let validate = xtask_in(&modeltap_fake, &["validate-tag", "--tag", &tag])
        .output()
        .expect("invoke xtask validate-tag");
    assert!(
        validate.status.success(),
        "validate-tag {tag} must exit zero against the seeded Cargo.toml; stderr=\n{}",
        String::from_utf8_lossy(&validate.stderr)
    );

    // -------------------------------------------------------------------------
    // Phase 2 — PUBLISH: extract-changelog writes RELEASE_NOTES.md.
    // -------------------------------------------------------------------------
    let release_notes = workdir.path().join("RELEASE_NOTES.md");
    let extract = xtask_in(
        &modeltap_fake,
        &[
            "extract-changelog",
            "--version",
            version,
            "--input",
            "CHANGELOG.md",
            "--output",
            release_notes.to_str().expect("utf-8 release notes path"),
        ],
    )
    .output()
    .expect("invoke xtask extract-changelog");
    assert!(
        extract.status.success(),
        "extract-changelog must exit zero; stderr=\n{}",
        String::from_utf8_lossy(&extract.stderr)
    );
    let release_notes_body =
        std::fs::read_to_string(&release_notes).expect("read RELEASE_NOTES.md");
    assert!(
        release_notes_body.contains("Walking-skeleton release-candidate."),
        "RELEASE_NOTES.md must contain the section body; got:\n{release_notes_body}"
    );

    // -------------------------------------------------------------------------
    // Phase 3a — TAP-BUMP (render): render the formula from the sidecar.
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
            modeltap_fake
                .join("artifacts")
                .to_str()
                .expect("utf-8 sha256-dir path"),
            "--release-base-url",
            &release_base_url,
        ],
    )
    .output()
    .expect("invoke xtask render-formula");
    assert!(
        render.status.success(),
        "render-formula must exit zero; stderr=\n{}",
        String::from_utf8_lossy(&render.stderr)
    );
    let rendered_formula =
        std::fs::read_to_string(&formula_path).expect("read rendered Formula/modeltap.rb");
    assert!(
        rendered_formula.contains(&format!("version \"{version}\"")),
        "rendered formula must declare version field"
    );
    assert!(
        rendered_formula.contains(sha256_hex),
        "rendered formula must include the sidecar sha256 verbatim"
    );

    // -------------------------------------------------------------------------
    // Phase 3b — TAP-BUMP (push): bump-tap-formula pushes the rendered
    //            formula to the bare tap remote on a bump/v<version> branch.
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
        "bump-tap-formula must exit zero on the WS happy path; stderr=\n{}",
        String::from_utf8_lossy(&bump.stderr)
    );

    // -------------------------------------------------------------------------
    // EXIT-GATE assertions: the bump branch in the bare tap remote contains
    // the rendered formula end-to-end (version + sha256 round-tripped).
    // -------------------------------------------------------------------------
    let refs = git_capture(&tap_fake_bare, &["show-ref"]);
    assert!(
        refs.contains(&format!("refs/heads/bump/{tag}")),
        "tap-fake.git must contain branch bump/{tag}; show-ref:\n{refs}"
    );

    let commit_msg = git_capture(
        &tap_fake_bare,
        &["log", "-1", "--format=%s", &format!("bump/{tag}")],
    );
    assert_eq!(
        commit_msg.trim(),
        format!("modeltap {version}"),
        "bump branch HEAD commit message must equal `modeltap <version>`"
    );

    let committed_formula = git_capture(
        &tap_fake_bare,
        &["show", &format!("bump/{tag}:Formula/modeltap.rb")],
    );
    assert!(
        committed_formula.contains(&format!("version \"{version}\"")),
        "committed Formula/modeltap.rb must declare version {version}; got:\n{committed_formula}"
    );
    assert!(
        committed_formula.contains(sha256_hex),
        "committed Formula/modeltap.rb must contain the sidecar sha256 \
         {sha256_hex} verbatim — proves the data flowed through render-formula \
         into the tap repo without loss; got:\n{committed_formula}"
    );

    // The committed formula equals the rendered formula byte-for-byte.
    assert_eq!(
        committed_formula, rendered_formula,
        "committed formula must equal rendered formula verbatim — no mutation \
         between render-formula and tap-repo commit"
    );
}

// =============================================================================
// Workflow-structure assertion: the WS exit gate also requires that the
// production release.yml has the bump-tap-formula job wired correctly. This
// secondary assertion is here (not in workflow_structure.rs) because it's
// the WS-completion check: bump-tap-formula must declare
// `needs: publish-github-release` so the atomic-publish guarantee extends to
// the tap-bump step (US-08).
// =============================================================================

#[test]
fn release_workflow_declares_bump_tap_formula_job_with_correct_needs() {
    let mut workflow_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    workflow_path.pop(); // tests/ -> workspace root
    workflow_path.push(".github");
    workflow_path.push("workflows");
    workflow_path.push("release.yml");
    let src = std::fs::read_to_string(&workflow_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", workflow_path.display()));
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(&src).expect("release.yml must be valid YAML");

    let jobs = workflow
        .get("jobs")
        .expect("release.yml must declare jobs:");
    let bump = jobs
        .get("bump-tap-formula")
        .expect("release.yml must declare a `bump-tap-formula` job (WS exit gate, US-06)");

    let needs = bump
        .get("needs")
        .expect("bump-tap-formula must declare `needs:` so atomic publish guarantee holds");
    let needs_list: Vec<String> = match needs {
        serde_yaml::Value::String(s) => vec![s.clone()],
        serde_yaml::Value::Sequence(seq) => seq
            .iter()
            .map(|v| v.as_str().expect("needs item must be string").to_owned())
            .collect(),
        other => panic!("needs must be string or sequence, got {other:?}"),
    };
    assert!(
        needs_list.iter().any(|n| n == "publish-github-release"),
        "bump-tap-formula.needs must include `publish-github-release` so the \
         atomic-publish guarantee (US-08) extends to the tap-bump step. \
         Got: {needs_list:?}"
    );

    let runs_on = bump
        .get("runs-on")
        .and_then(|v| v.as_str())
        .expect("bump-tap-formula.runs-on must be a string");
    assert_eq!(
        runs_on, "ubuntu-latest",
        "bump-tap-formula must run on ubuntu-latest (cheapest runner for the \
         single git-push + gh-pr-create operation)"
    );
}
