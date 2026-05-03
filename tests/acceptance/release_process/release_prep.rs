// Acceptance tests for `cargo xtask release-prep`.
//
// Step: 01-06 (Walking Skeleton — PREP activity, US-01).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/walking-skeleton.feature, US-01.
//
// Strategy C — real local resources (DWD-01):
//   - Real tempdir (`tempfile::TempDir`) for fixture workspace
//   - Real `git init` + real conventional commits seeded in the temp repo
//   - Real subprocess (`cargo run --package xtask -- release-prep --version <V>`)
//   - Real exit-code observation
//   - Real Cargo.toml mutation observed on disk after the run
//
// Sub-cargo invocations (release-prep shells out to `cargo fmt`/`cargo clippy`/
// `cargo test`) are configured to succeed by NOT including any Rust source in
// the fixture workspace — the gate-failure scenario provides its own malformed
// crate to provoke failure.
//
// PATH note: `~/.pyenv/shims/cc` shadows the real cc on this developer's
// machine and breaks build-script linking. Both the outer `cargo run`
// invocation AND the inner `cargo fmt`/`cargo clippy`/`cargo test` invocations
// (spawned by release-prep) need PATH=/usr/bin:$PATH so a clean cc is found.

use std::path::{Path, PathBuf};
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
/// --quiet -- <args>` with the given working directory and a sanitised PATH so
/// build-script linker invocations find a real cc, not the pyenv shim.
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
    // Sanitise PATH for both the outer cargo invocation AND any sub-cargo
    // invocations release-prep shells out to. The `/usr/bin` prefix wins
    // ahead of `~/.pyenv/shims` so cc resolves to /usr/bin/cc.
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

/// Create a tempdir containing a minimal workspace whose
/// `[workspace.package].version` equals `version`, plus a single trivial
/// member crate `placeholder` (one empty `lib.rs`) so the CI parity gates
/// (`cargo fmt --check` / `cargo clippy` / `cargo test`) all have a target
/// to operate on and succeed trivially. Initialises a git repo and seeds
/// three conventional commits (feat, fix, docs) AFTER tagging
/// `v<version>` so the changelog has something to describe.
///
/// Why the placeholder crate? `cargo fmt --all -- --check` fails with
/// "Failed to find targets" in a workspace with zero members, which would
/// confound the happy-path assertion that the gate succeeds.
fn fixture_repo(version: &str) -> TempDir {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let path = tempdir.path();

    // Workspace Cargo.toml referencing one placeholder member crate.
    let cargo_toml = format!(
        "[workspace]\n\
         resolver = \"2\"\n\
         members = [\"placeholder\"]\n\
         \n\
         [workspace.package]\n\
         version = \"{version}\"\n\
         edition = \"2021\"\n",
    );
    std::fs::write(path.join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");

    // Placeholder member crate: empty lib.rs is well-formatted (rustfmt
    // accepts the empty file as a no-op) and has no clippy lints to fire.
    let placeholder_dir = path.join("placeholder");
    std::fs::create_dir_all(placeholder_dir.join("src")).expect("mkdir placeholder/src");
    std::fs::write(
        placeholder_dir.join("Cargo.toml"),
        "[package]\n\
         name = \"placeholder\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n",
    )
    .expect("write placeholder Cargo.toml");
    // rustfmt requires a single trailing newline even in an empty file —
    // writing zero bytes makes `cargo fmt --check` report a diff and fail
    // the happy-path gate. One '\n' is the canonical empty rust source.
    std::fs::write(placeholder_dir.join("src").join("lib.rs"), "\n")
        .expect("write placeholder/src/lib.rs");

    // git init + initial commit + version tag.
    git(path, &["init", "--quiet", "--initial-branch=main"]);
    git(path, &["add", "."]);
    git(path, &["commit", "--quiet", "-m", "chore: initial commit"]);
    git(path, &["tag", &format!("v{version}")]);

    // Seed three conventional commits AFTER the version tag so the changelog
    // has something to group.
    for (msg, file_name) in [
        ("feat: add release-prep subcommand", "feat.txt"),
        ("fix: correct version parsing edge case", "fix.txt"),
        ("docs: document release runbook", "docs.txt"),
    ] {
        std::fs::write(path.join(file_name), "x\n").expect("write seed file");
        git(path, &["add", file_name]);
        git(path, &["commit", "--quiet", "-m", msg]);
    }

    tempdir
}

// =============================================================================
// Scenario 1 (happy path): Maintainer prepares a release with one command
// walking-skeleton.feature, US-01 primary scenario
// =============================================================================

#[test]
fn release_prep_mutates_cargo_toml_and_writes_changelog_and_exits_zero() {
    // Start at 0.0.1-alpha so 0.0.1-rc1 is a valid strictly-greater bump
    // per semver (0.0.1-alpha < 0.0.1-rc1 < 0.0.1). 0.0.1-rc1 is the version
    // the walking-skeleton happy-path scenario asks for.
    let repo = fixture_repo("0.0.1-alpha");

    let output = xtask_in(repo.path(), &["release-prep", "--version", "0.0.1-rc1"])
        .output()
        .expect("invoke cargo xtask release-prep");

    assert!(
        output.status.success(),
        "release-prep should exit zero on happy path; stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Cargo.toml mutated to the new version.
    let cargo_toml = std::fs::read_to_string(repo.path().join("Cargo.toml"))
        .expect("read Cargo.toml after release-prep");
    assert!(
        cargo_toml.contains("version = \"0.0.1-rc1\""),
        "Cargo.toml should now contain new version, got:\n{cargo_toml}"
    );

    // CHANGELOG.md exists and has a section for the new version that groups
    // commits by conventional-commit type.
    let changelog = std::fs::read_to_string(repo.path().join("CHANGELOG.md"))
        .expect("read CHANGELOG.md after release-prep");
    assert!(
        changelog.contains("## [0.0.1-rc1]"),
        "CHANGELOG should contain section for new version, got:\n{changelog}"
    );
    // Grouping headings — at minimum the three commit types we seeded.
    assert!(
        changelog.contains("### Added") || changelog.contains("### Features"),
        "CHANGELOG should group `feat:` commits under an Added/Features heading, got:\n{changelog}"
    );
    assert!(
        changelog.contains("### Fixed"),
        "CHANGELOG should group `fix:` commits under a Fixed heading, got:\n{changelog}"
    );
    assert!(
        changelog.contains("### Documentation") || changelog.contains("### Docs"),
        "CHANGELOG should group `docs:` commits under a Documentation heading, got:\n{changelog}"
    );

    // Stdout names the next steps.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("commit") && stdout.contains("push") && stdout.contains("PR"),
        "stdout should print commit/push/PR next steps, got:\n{stdout}"
    );
}

// =============================================================================
// Scenario 2: Release-prep refuses on a dirty working tree
// walking-skeleton.feature, US-01
// =============================================================================

#[test]
fn release_prep_refuses_on_dirty_working_tree_and_modifies_no_files() {
    let repo = fixture_repo("0.1.0");

    // Introduce an uncommitted local change.
    std::fs::write(repo.path().join("dirty.txt"), "uncommitted\n").expect("write dirty file");

    // Snapshot Cargo.toml so we can prove it's untouched.
    let cargo_toml_before = std::fs::read_to_string(repo.path().join("Cargo.toml"))
        .expect("snapshot Cargo.toml before");

    let output = xtask_in(repo.path(), &["release-prep", "--version", "0.0.1-rc1"])
        .output()
        .expect("invoke cargo xtask release-prep");

    output
        .assert()
        .failure()
        .stderr(contains("working tree is dirty: commit or stash first"));

    // Cargo.toml is untouched.
    let cargo_toml_after =
        std::fs::read_to_string(repo.path().join("Cargo.toml")).expect("snapshot Cargo.toml after");
    assert_eq!(
        cargo_toml_before, cargo_toml_after,
        "Cargo.toml must NOT be modified when the tree is dirty"
    );

    // No CHANGELOG.md was written.
    assert!(
        !repo.path().join("CHANGELOG.md").exists(),
        "CHANGELOG.md must NOT be written when the tree is dirty"
    );
}

// =============================================================================
// Scenario 3: Release-prep refuses a non-monotonic version bump
// walking-skeleton.feature, US-01
// =============================================================================

#[test]
fn release_prep_refuses_nonmonotonic_bump_with_both_versions_in_message() {
    let repo = fixture_repo("0.2.0");

    let output = xtask_in(repo.path(), &["release-prep", "--version", "0.1.5"])
        .output()
        .expect("invoke cargo xtask release-prep");

    output.assert().failure().stderr(contains(
        "proposed version 0.1.5 is not greater than current 0.2.0",
    ));

    // No CHANGELOG.md was written.
    assert!(
        !repo.path().join("CHANGELOG.md").exists(),
        "CHANGELOG.md must NOT be written when the bump is rejected"
    );
}

// =============================================================================
// Scenario 4: Release-prep halts when a CI parity gate fails
// walking-skeleton.feature, US-01 @infrastructure-failure
// =============================================================================
//
// Provoke a `cargo fmt --check` failure by adding a workspace member crate
// whose source file is intentionally unformatted (trailing whitespace + odd
// indentation that rustfmt will reformat).

#[test]
fn release_prep_halts_when_a_ci_gate_fails_and_names_the_failed_gate() {
    let repo = fixture_repo("0.1.0");
    let path = repo.path();

    // Create a member crate `badfmt` with deliberately unformatted source.
    let badfmt_dir = path.join("badfmt");
    std::fs::create_dir_all(badfmt_dir.join("src")).expect("mkdir badfmt/src");
    std::fs::write(
        badfmt_dir.join("Cargo.toml"),
        "[package]\n\
         name = \"badfmt\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n",
    )
    .expect("write badfmt Cargo.toml");
    // Unformatted source: stray double-spaces, inconsistent indentation.
    std::fs::write(
        badfmt_dir.join("src").join("lib.rs"),
        "pub fn  add ( a:i32 , b:i32 )->i32{a+b}\n",
    )
    .expect("write badfmt/src/lib.rs");

    // Re-write the workspace Cargo.toml to include both placeholder AND
    // badfmt members. The badfmt member is the one that will fail
    // `cargo fmt --check`; placeholder ensures rustfmt has a target even
    // before the gate fires (mirrors the happy-path fixture shape).
    std::fs::write(
        path.join("Cargo.toml"),
        "[workspace]\n\
         resolver = \"2\"\n\
         members = [\"placeholder\", \"badfmt\"]\n\
         \n\
         [workspace.package]\n\
         version = \"0.1.0\"\n\
         edition = \"2021\"\n",
    )
    .expect("rewrite Cargo.toml with badfmt member");

    // Commit the new state so the dirty check passes.
    git(path, &["add", "."]);
    git(
        path,
        &["commit", "--quiet", "-m", "chore: add badfmt member"],
    );

    let output = xtask_in(path, &["release-prep", "--version", "0.2.0"])
        .output()
        .expect("invoke cargo xtask release-prep");

    output.assert().failure().stderr(contains("fmt"));
}
