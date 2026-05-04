// Acceptance tests for `cargo xtask bump-tap-formula` and the cross-repo
// seam (modeltap-fake ↔ tap-fake) per DWD-02.
//
// Step: 01-08 (Walking Skeleton — TAP-BUMP activity, US-06).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/walking-skeleton.feature, US-06:
//   - "Bump-tap-formula opens a PR against the ephemeral tap repository"
//   - "Tap-bump step surfaces token failure visibly" (@infrastructure-failure)
//
// Strategy C (DWD-01) + DWD-02 cross-repo seam:
//   - Real tempdir for the tap-fake bare repo (file://${TMPDIR}/tap-fake.git)
//   - Real `git init --bare` for the tap-fake remote
//   - Real subprocess for `cargo run --package xtask -- bump-tap-formula ...`
//   - Real `git push --force-with-lease` against the file:// URL
//   - `gh pr create` is gated behind `--open-pr`; the local-only flow runs
//     WITHOUT that flag so no live GitHub API call is made.
//
// PATH note: `~/.pyenv/shims/cc` shadows the real cc on this developer's
// machine and breaks build-script linking. Sub-cargo invocations need
// PATH=/usr/bin:$PATH.

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::prelude::OutputAssertExt;
use predicates::prelude::PredicateBooleanExt;
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

/// Build a `Command` that invokes `cargo run --manifest-path <ws> --package
/// xtask --quiet -- <args>` with the given working directory.
fn xtask_in(workdir: &Path, args: &[&str]) -> Command {
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
    let original_path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("/usr/bin:{original_path}"));
    cmd
}

/// Run a git command in `repo`, panicking on failure. Used by fixture setup
/// only; the production code uses xtask::git_adapter, not this helper.
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

/// Capture stdout of a git command in `repo`, panicking on failure.
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

/// Initialise a bare git repo at `path` to act as the ephemeral tap "remote".
/// Per DWD-02 the bare-repo + file:// URL combination is what makes the
/// cross-repo push faithful (real refs land in real packed-refs, not just a
/// working-tree commit).
///
/// Returns the file:// URL the bump step will push to.
fn init_bare_tap_remote(path: &Path) -> String {
    let status = Command::new("git")
        .args(["init", "--bare", "--quiet", "--initial-branch=main"])
        .arg(path)
        .status()
        .expect("invoke git init --bare");
    assert!(status.success(), "git init --bare {:?} failed", path);
    format!("file://{}", path.display())
}

/// Seed a fresh tap-fake bare repo with one commit on `main` so `bump/v*`
/// branches have a sensible base. We do this by creating a working clone,
/// committing a placeholder Formula, and pushing back to the bare remote.
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

/// Render a fixture formula file so the bump step has something to commit.
/// We do NOT shell out to `xtask render-formula` here because the rendering
/// path is already covered by `render_formula.rs`; this test isolates the
/// bump-tap-formula behaviour (push + branch + commit message).
fn write_fixture_formula(path: &Path, version: &str) {
    let formula = format!(
        "class Modeltap < Formula\n  \
         desc \"TUI for managing local AI models\"\n  \
         homepage \"https://github.com/jeffabailey/modeltap\"\n  \
         version \"{version}\"\n  \
         license \"MIT OR Apache-2.0\"\n\n  \
         on_linux do\n    \
         on_intel do\n      \
         url \"https://example.invalid/modeltap-{version}-x86_64-unknown-linux-gnu.tar.gz\"\n      \
         sha256 \"e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n    \
         end\n  \
         end\n\n  \
         def install\n    \
         bin.install \"modeltap\"\n  \
         end\n\
         end\n"
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir formula parent");
    }
    std::fs::write(path, formula).expect("write fixture formula");
}

// =============================================================================
// Scenario 1 (primary WS): Bump-tap-formula opens a PR against the ephemeral
// tap repository (walking-skeleton.feature, US-06)
// =============================================================================
//
// Without `--open-pr` the bump step performs the local-only portion of the
// workflow: clone the tap-repo, write the rendered formula, commit, and push
// the bump branch back to the bare remote. The acceptance asserts:
//   - branch `bump/v<version>` exists in the bare remote
//   - the branch's HEAD commit message is exactly `modeltap <version>`
//   - the file `Formula/modeltap.rb` at that commit equals the rendered input

#[test]
fn bump_tap_formula_pushes_branch_with_formula_and_commit_message() {
    let workdir = TempDir::new().expect("create tempdir");
    let bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&bare).expect("mkdir tap-fake.git");
    let tap_url = init_bare_tap_remote(&bare);
    seed_tap_remote_with_initial_commit(&bare);

    // Render a fixture formula in the workdir; the bump step will read it.
    let version = "0.0.1-rc1";
    let formula_src = workdir.path().join("modeltap.rb");
    write_fixture_formula(&formula_src, version);

    let output = xtask_in(
        workdir.path(),
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula");

    assert!(
        output.status.success(),
        "bump-tap-formula should exit zero on local-only flow; stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Inspect the bare remote: the bump branch must exist.
    let refs = git_capture(&bare, &["show-ref"]);
    let expected_branch = format!("refs/heads/bump/v{version}");
    assert!(
        refs.contains(&expected_branch),
        "bare tap repo must contain branch {expected_branch}; show-ref output:\n{refs}"
    );

    // The branch's HEAD commit message is `modeltap <version>`.
    let msg = git_capture(
        &bare,
        &["log", "-1", "--format=%s", &format!("bump/v{version}")],
    );
    assert_eq!(
        msg.trim(),
        format!("modeltap {version}"),
        "branch HEAD commit message must equal `modeltap <version>`"
    );

    // The file Formula/modeltap.rb at that commit equals the rendered input.
    let committed_formula = git_capture(
        &bare,
        &["show", &format!("bump/v{version}:Formula/modeltap.rb")],
    );
    let expected_formula =
        std::fs::read_to_string(&formula_src).expect("read fixture formula source");
    assert_eq!(
        committed_formula, expected_formula,
        "committed Formula/modeltap.rb must equal the rendered input verbatim"
    );
}

// =============================================================================
// Scenario 2 (idempotent re-run): A second invocation against the same tap
// repo with the same version succeeds (force-push-with-lease overwrites the
// bump branch). This proves the bump step is safe to retry per US-12 (full
// retry semantics ship in step 03-02; here we just need the force-push-with-
// lease primitive to be wired so retries don't hard-fail).
// =============================================================================

#[test]
fn bump_tap_formula_is_idempotent_under_repeated_invocation() {
    let workdir = TempDir::new().expect("create tempdir");
    let bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&bare).expect("mkdir tap-fake.git");
    let tap_url = init_bare_tap_remote(&bare);
    seed_tap_remote_with_initial_commit(&bare);

    let version = "0.0.1-rc1";
    let formula_src = workdir.path().join("modeltap.rb");
    write_fixture_formula(&formula_src, version);

    // First run.
    let first = xtask_in(
        workdir.path(),
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula (1st)");
    assert!(
        first.status.success(),
        "first bump invocation should exit zero; stderr=\n{}",
        String::from_utf8_lossy(&first.stderr)
    );

    // Second run with the SAME inputs — must succeed (force-with-lease).
    let second = xtask_in(
        workdir.path(),
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula (2nd)");
    assert!(
        second.status.success(),
        "second bump invocation must also exit zero (idempotent retry); stderr=\n{}",
        String::from_utf8_lossy(&second.stderr)
    );

    // The bump branch still exists with the same commit message.
    let msg = git_capture(
        &bare,
        &["log", "-1", "--format=%s", &format!("bump/v{version}")],
    );
    assert_eq!(msg.trim(), format!("modeltap {version}"));
}

// =============================================================================
// Scenario 3 (token failure): pushing to a non-existent bare-repo path
// surfaces a non-zero exit and a recognisable failure message. This stands in
// for the "invalid tap-bump-token" workflow scenario — locally we provoke the
// same failure shape (git push fails) by pointing at a bogus URL. The xtask
// CLI MUST NOT silently succeed.
// =============================================================================

#[test]
fn bump_tap_formula_fails_visibly_when_remote_is_unreachable() {
    let workdir = TempDir::new().expect("create tempdir");

    let version = "0.0.1-rc1";
    let formula_src = workdir.path().join("modeltap.rb");
    write_fixture_formula(&formula_src, version);

    // A file:// URL pointing at a path that does not exist.
    let bogus_url = format!(
        "file://{}/does-not-exist-{}.git",
        workdir.path().display(),
        version
    );

    let output = xtask_in(
        workdir.path(),
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &bogus_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula");

    assert!(
        !output.status.success(),
        "bump-tap-formula must exit non-zero when the remote is unreachable; stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // The diagnostic on stderr must identify what went wrong (git or the
    // remote URL). This is the "no silent success" guarantee.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("git") || combined.contains("repository") || combined.contains("clone"),
        "stderr/stdout should identify a git/clone/remote failure; got stderr=\n{stderr}\nstdout=\n{stdout}"
    );
}

// =============================================================================
// Scenario 4 (formula content fidelity): the formula committed to the tap
// repo equals the input formula byte-for-byte — proves the bump step does not
// mutate the rendered Formula content (no whitespace stripping, no reformat).
// This is the "no fixture theater" check for the cross-repo seam: the test
// supplies a formula with a recognisable marker and asserts the marker
// survives the round-trip through clone → write → commit → push → show.
// =============================================================================

#[test]
fn bump_tap_formula_preserves_formula_content_verbatim() {
    let workdir = TempDir::new().expect("create tempdir");
    let bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&bare).expect("mkdir tap-fake.git");
    let tap_url = init_bare_tap_remote(&bare);
    seed_tap_remote_with_initial_commit(&bare);

    let version = "0.0.1-rc1";
    let formula_src = workdir.path().join("modeltap.rb");

    // A formula with a unique marker we can search for.
    let marker = "MARKER-2db9a07f-bump-tap-formula-fidelity-check";
    let formula_text =
        format!("class Modeltap < Formula\n  # {marker}\n  version \"{version}\"\nend\n");
    std::fs::write(&formula_src, &formula_text).expect("write fixture formula");

    let output = xtask_in(
        workdir.path(),
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula");
    output.assert().success();

    let committed = git_capture(
        &bare,
        &["show", &format!("bump/v{version}:Formula/modeltap.rb")],
    );
    assert!(
        committed.contains(marker),
        "round-tripped formula must contain marker {marker:?}; got:\n{committed}"
    );
    assert_eq!(
        committed, formula_text,
        "round-tripped formula must equal input byte-for-byte"
    );
}

// =============================================================================
// Scenario 5 (input validation): an absent --formula file fails with a clear
// diagnostic and exits non-zero (no partial state in the tap repo).
// =============================================================================

#[test]
fn bump_tap_formula_fails_when_formula_file_does_not_exist() {
    let workdir = TempDir::new().expect("create tempdir");
    let bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&bare).expect("mkdir tap-fake.git");
    let tap_url = init_bare_tap_remote(&bare);
    seed_tap_remote_with_initial_commit(&bare);

    let version = "0.0.1-rc1";
    let absent_path = workdir.path().join("does-not-exist.rb");

    let output = xtask_in(
        workdir.path(),
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &tap_url,
            "--formula",
            absent_path.to_str().expect("utf-8 absent path"),
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula");

    output
        .assert()
        .failure()
        .stderr(contains("formula").or(contains("does-not-exist")));

    // No bump branch was created.
    let refs = git_capture(&bare, &["show-ref"]);
    assert!(
        !refs.contains(&format!("refs/heads/bump/v{version}")),
        "no bump branch must be created when formula input is absent"
    );
}
