// modeltap-acceptance — black-box acceptance test crate.
//
// Most acceptance test code lives under `tests/acceptance/**`. This `lib.rs`
// hosts shared helpers (workspace path resolution, xtask invocation with the
// PATH workaround for `~/.pyenv/shims/cc`, git wrappers, ephemeral repo
// seeding) so multi-file e2e suites (walking_skeleton_e2e, multi_arch_e2e,
// version_consistency) can share fixture wiring without duplicating it.
//
// Step 02-05 extracted these from walking_skeleton_e2e.rs (commit 06c33027)
// after multi_arch_e2e + version_consistency needed the same primitives.
// See `nw-tdd-methodology` Mandate 5 + Mandate 6 for fixture provenance.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Path to the modeltap workspace's root Cargo.toml. Resolved at compile time
/// from `CARGO_MANIFEST_DIR` of THIS crate (`tests/`), one level up.
pub fn workspace_manifest() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("Cargo.toml");
    p
}

/// Path to the in-repo Tera template that the rendered formula is built from.
pub fn template_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("release/templates/modeltap.rb.tera");
    p
}

/// Build a Command that invokes `cargo run --manifest-path <ws> --package
/// xtask --quiet -- <args>` with the given working directory and a sanitised
/// PATH so build-script linker invocations find a real cc.
///
/// The PATH workaround is mandatory on developer machines where
/// `~/.pyenv/shims/cc` shadows `/usr/bin/cc` — without it, build-script linker
/// invocations fail with cryptic linker errors. Documented in CLAUDE.md and
/// the original walking_skeleton_e2e.rs.
pub fn xtask_in(workdir: &Path, args: &[&str]) -> Command {
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

/// Invoke `git <args>` in `repo`, asserting success. Sets a deterministic
/// author/committer identity so tests are reproducible across machines.
pub fn git(repo: &Path, args: &[&str]) {
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

/// Invoke `git <args>` in `repo` and return its stdout, asserting success.
pub fn git_capture(repo: &Path, args: &[&str]) -> String {
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

/// Initialise a bare git repo at `path` to act as the ephemeral tap remote.
/// Returns the `file://` URL bump-tap-formula consumes for `--tap-repo-url`.
pub fn init_bare_tap_remote(path: &Path) -> String {
    let status = Command::new("git")
        .args(["init", "--bare", "--quiet", "--initial-branch=main"])
        .arg(path)
        .status()
        .expect("invoke git init --bare");
    assert!(status.success(), "git init --bare {:?} failed", path);
    format!("file://{}", path.display())
}

/// Seed the bare tap remote with one initial commit on `main` so bump branches
/// have a base. Clones into a working copy, drops a Formula/.keep + README,
/// commits, pushes, then drops the working copy.
pub fn seed_tap_remote_with_initial_commit(bare_path: &Path) {
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

/// Write a workspace `Cargo.toml` at `repo` with the given `version` plus a
/// CHANGELOG.md containing one section for that version. Used by both the WS
/// e2e and the multi-arch e2e to seed the modeltap-fake working repo.
pub fn seed_modeltap_fake_workspace(repo: &Path, version: &str) {
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

    let changelog = format!(
        "# Changelog\n\n\
         All notable changes are documented here.\n\n\
         ## [{version}]\n\n\
         ### Added\n\n\
         - Multi-arch release-candidate.\n",
    );
    std::fs::write(repo.join("CHANGELOG.md"), changelog).expect("write CHANGELOG.md");
}

/// The four supported release targets, paired with structurally distinct
/// sha256 fixtures (last byte encodes the kind: `aa`=mac_arm, `bb`=mac_intel,
/// `cc`=linux_intel, `dd`=linux_arm) so round-trip assertions can prove each
/// formula block was wired to the *correct* sidecar.
pub const ALL_FOUR_TARGETS: [(&str, &str); 4] = [
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

/// Stage a sha256 sidecar in `artifacts_dir` for `(triple, sha)` at the given
/// `version`. Filename = `modeltap-<version>-<triple>.tar.gz.sha256`,
/// content = bare hex + trailing newline (per data-models.md §4).
pub fn write_sidecar(artifacts_dir: &Path, version: &str, triple: &str, sha: &str) {
    let sidecar = format!("modeltap-{version}-{triple}.tar.gz.sha256");
    std::fs::write(artifacts_dir.join(sidecar), format!("{sha}\n")).expect("write sha256 sidecar");
}

/// Stage a placeholder archive file (NOT a real tarball — render-formula does
/// not read archive bytes; only sidecars feed into the formula). The presence
/// of an archive on disk corresponds to the upload-artifact step in the build
/// matrix; a missing archive simulates a failed cell.
pub fn write_archive_placeholder(artifacts_dir: &Path, version: &str, triple: &str) {
    let archive = format!("modeltap-{version}-{triple}.tar.gz");
    std::fs::write(
        artifacts_dir.join(archive),
        b"placeholder archive bytes\n" as &[u8],
    )
    .expect("write archive placeholder");
}
