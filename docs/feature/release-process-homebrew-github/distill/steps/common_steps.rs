// =============================================================================
// release-process-homebrew-github — Common Step Definitions
//
// Wave: DISTILL (5 of 6)
// Author: Quinn (nw-acceptance-designer)
// Date: 2026-05-03
//
// Shared step definitions used across walking-skeleton, multi-arch, hands-off,
// and integration-checkpoint feature files.
//
// Implementation note for DELIVER (software-crafter):
//   These are skeletons. The actual test-runner choice (cucumber-rs, or
//   integration tests over assert_cmd, or a hybrid) is the crafter's call.
//   The shapes here document the WORLD STATE each step manipulates and the
//   xtask binary calls each step makes. Each function delegates to the xtask
//   CLI binary (driving port) — never to internal xtask modules directly.
//
// Mandate 1 (Hexagonal boundary): every When step invokes the xtask BINARY
// via assert_cmd::Command::cargo_bin("xtask"). No direct calls into
// xtask::version, xtask::formula, etc. from acceptance tests. Those modules
// are unit-tested in the inner TDD loop.
// =============================================================================

use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::*;
use tempfile::TempDir;

/// World state carried across Given/When/Then steps within a single scenario.
///
/// Holds the ephemeral test directories and the captured output of the most
/// recent xtask invocation.
pub struct ReleaseWorld {
    /// Root tempdir for this scenario; auto-cleaned on drop.
    pub tempdir: TempDir,

    /// Path to the fake modeltap-fake repository (seeded `Cargo.toml`,
    /// `CHANGELOG.md`, `release/templates/modeltap.rb.tera`, etc.).
    pub modeltap_fake: PathBuf,

    /// Path to the fake tap repository (init --bare for push semantics).
    /// URL form: `format!("file://{}", world.tap_fake.display())`.
    pub tap_fake: PathBuf,

    /// Captured stdout/stderr/exit_code of the most recent xtask invocation.
    pub last_output: Option<std::process::Output>,
}

impl ReleaseWorld {
    pub fn new() -> Self {
        let _ = (); // SCAFFOLD: true
        unimplemented!("ReleaseWorld::new — DELIVER scaffolds tempdir + ephemeral repos")
    }

    /// Resolves a path inside the tempdir (e.g., "Formula/modeltap.rb").
    pub fn path(&self, _relative: &str) -> PathBuf {
        let _ = (); // SCAFFOLD: true
        unimplemented!("ReleaseWorld::path — DELIVER joins relative onto tempdir root")
    }
}

// -----------------------------------------------------------------------------
// Background steps
// -----------------------------------------------------------------------------

/// `Given a clean tempdir workspace for the scenario`
pub fn given_clean_tempdir(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("given_clean_tempdir — RED scaffold; DELIVER initializes ReleaseWorld")
}

/// `Given a fake modeltap repository seeded at "${TMPDIR}/modeltap-fake"
///  with conventional commit history`
pub fn given_fake_modeltap_repo(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_fake_modeltap_repo — RED scaffold; DELIVER `git init`s a repo with seeded \
         Cargo.toml + CHANGELOG.md + a series of conventional-commit messages spanning \
         feat:, fix:, chore:, refactor: types"
    )
}

/// `Given a fake Homebrew tap repository seeded at "${TMPDIR}/tap-fake"
///  reachable via "file://${TMPDIR}/tap-fake"`
pub fn given_fake_tap_repo(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_fake_tap_repo — RED scaffold; DELIVER `git init --bare`s the tap repo so \
         force-push-with-lease semantics work over file:// URLs"
    )
}

// -----------------------------------------------------------------------------
// Cargo.toml manipulation
// -----------------------------------------------------------------------------

/// `Given the workspace version in Cargo.toml is "<version>"`
pub fn given_workspace_version_is(_world: &mut ReleaseWorld, _version: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_workspace_version_is — RED scaffold; DELIVER writes Cargo.toml with \
         `[workspace.package] version = \"<version>\"` into modeltap_fake"
    )
}

/// `Given the workspace has an uncommitted local change`
pub fn given_dirty_working_tree(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_dirty_working_tree — RED scaffold; DELIVER touches a tracked file or \
         creates an untracked file inside modeltap_fake"
    )
}

// -----------------------------------------------------------------------------
// xtask invocation (the driving port — Mandate 1)
// -----------------------------------------------------------------------------

/// `When the maintainer runs "cargo xtask <subcommand> <args...>"`
///
/// All When-runs-xtask steps delegate here. Captures the Output for Then
/// assertions. Working directory is `world.modeltap_fake` so xtask sees the
/// seeded Cargo.toml + git history.
pub fn when_maintainer_runs_xtask(world: &mut ReleaseWorld, args: &[&str]) {
    // Driving-port invocation: shell out to the xtask BINARY. No direct calls
    // into xtask::version, xtask::formula, etc. — those live in unit tests.
    let mut cmd = Command::cargo_bin("xtask").expect("xtask binary is built");
    cmd.current_dir(&world.modeltap_fake);
    cmd.args(args);
    let output = cmd.output().expect("xtask binary executes");
    world.last_output = Some(output);
}

// -----------------------------------------------------------------------------
// Output assertions (Then steps) — observable outcomes, not internal state
// -----------------------------------------------------------------------------

/// `Then the script exits zero`
pub fn then_script_exits_zero(world: &ReleaseWorld) {
    let output = world.last_output.as_ref().expect("an xtask invocation has run");
    assert!(
        output.status.success(),
        "xtask exited with non-zero status; stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `Then the script exits non-zero`
pub fn then_script_exits_nonzero(world: &ReleaseWorld) {
    let output = world.last_output.as_ref().expect("an xtask invocation has run");
    assert!(
        !output.status.success(),
        "xtask exited with zero status; stdout was:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// `Then the message says "<expected substring>"`
pub fn then_message_contains(world: &ReleaseWorld, expected: &str) {
    let output = world.last_output.as_ref().expect("an xtask invocation has run");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains(expected),
        "expected output to contain {expected:?}; full output was:\n{combined}"
    );
}

// -----------------------------------------------------------------------------
// Filesystem assertions (Then steps)
// -----------------------------------------------------------------------------

/// `Then "<relative path>" is created`
pub fn then_file_is_created(_world: &ReleaseWorld, _relative: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_file_is_created — RED scaffold; DELIVER asserts the file exists at \
         world.path(relative)"
    )
}

/// `Then "<relative path>" content equals <expected>`
pub fn then_file_content_equals(_world: &ReleaseWorld, _relative: &str, _expected: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_file_content_equals — RED scaffold")
}

/// `Then no files in the workspace are modified`
pub fn then_no_files_modified(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_no_files_modified — RED scaffold; DELIVER snapshots file mtimes/hashes \
         in given_fake_modeltap_repo and re-checks here"
    )
}
