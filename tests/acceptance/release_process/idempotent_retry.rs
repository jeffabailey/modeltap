// Acceptance tests for the bump-tap-formula idempotent-retry semantics.
//
// Step: 03-02 (Phase 3 — hands-off automation, US-12).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/hands-off-automation.feature, US-12:
//   - "First-run creates the bump branch and opens a new PR"
//   - "Re-run after token rotation force-pushes to the existing branch"
//   - "One PR per version invariant holds across any number of retries"
//   - "Manual edits to the bump branch are clobbered by the next render"
//
// Strategy C (DWD-01) extended with a stubbed `gh` binary on PATH so the
// `--open-pr` code path is exercised end-to-end WITHOUT live GitHub:
//
//   - Real tempdir for the tap-fake bare repo (file://${TMPDIR}/tap-fake.git)
//   - Real `git init --bare` + `git push --force-with-lease` against file://
//   - Stubbed `gh` shell script on PATH: parses `pr list --head ...` /
//     `pr create ...` and records each invocation to ${GH_STATE_FILE}.
//     The stub returns `[]` for `pr list` UNTIL `pr create` is invoked at
//     least once, then it returns a one-element JSON array. This faithfully
//     models the live-GitHub state machine for an idempotent re-run.
//
// Why a shell stub rather than a Rust mock: the xtask binary launches a real
// subprocess (`std::process::Command::new("gh")`); a Rust-level mock cannot
// intercept that. The stub is the smallest unit that gives us a faithful
// observation of the argv xtask hands to gh.
//
// PATH note: the `xtask_in` helper already prepends `/usr/bin:` to PATH (for
// the cc-shim workaround). We further prepend the stub-bin tempdir so the
// stub `gh` shadows any system `gh`. Order: <stub-bin>:/usr/bin:<original>.

use std::path::{Path, PathBuf};
use std::process::Command;

use modeltap_acceptance::{
    git_capture, init_bare_tap_remote, seed_tap_remote_with_initial_commit, workspace_manifest,
};
use tempfile::TempDir;

// =============================================================================
// Fake `gh` stub: a POSIX shell script that records argv to ${GH_STATE_FILE}
// and emits the JSON shape `xtask::gh_adapter::pr_list_for_head` expects.
// =============================================================================

/// Write a fake `gh` script into `bin_dir` and chmod +x it. The script:
///   * appends every invocation's argv (one line, space-joined) to
///     ${GH_STATE_FILE}
///   * for `pr list --head ... --json ...`: emits `[]` if the state file
///     has no `pr_create` line yet, otherwise emits a one-element array
///   * for `pr create ...`: emits a fake PR URL on stdout (mirrors live gh)
///   * for any other subcommand: exits 0 with empty stdout
fn install_fake_gh(bin_dir: &Path) {
    let script = r#"#!/usr/bin/env bash
# Record this invocation (one line per call) so the test can assert later.
echo "$@" >> "${GH_STATE_FILE}"

case "$1" in
  pr)
    case "$2" in
      list)
        # If pr_create has been recorded already, surface a one-element list.
        if grep -q '^pr create ' "${GH_STATE_FILE}" 2>/dev/null; then
          # Extract the head ref from --head <ref> in argv (positional arg
          # after --head). We only need one PR's worth of metadata.
          head_ref=""
          while [ $# -gt 0 ]; do
            if [ "$1" = "--head" ]; then
              shift
              head_ref="$1"
              break
            fi
            shift
          done
          printf '[{"number": 42, "title": "modeltap stub", "state": "OPEN", "headRefName": "%s"}]\n' "${head_ref}"
        else
          echo "[]"
        fi
        exit 0
        ;;
      create)
        # Mimic gh's success line: print a fake PR URL.
        echo "https://github.com/jeffabailey/homebrew-modeltap/pull/42"
        exit 0
        ;;
      merge)
        # Auto-merge stub: ack and exit clean.
        exit 0
        ;;
    esac
    ;;
esac

# Default: succeed silently (any other gh subcommand the workflow happens to
# invoke during a test run).
exit 0
"#;
    let path = bin_dir.join("gh");
    std::fs::write(&path, script).expect("write fake gh script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .expect("stat fake gh")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod fake gh");
    }
}

/// Build a `Command` that invokes `cargo run --manifest-path <ws> --package
/// xtask --quiet -- <args>` with PATH set to `<stub_bin_dir>:/usr/bin:<orig>`
/// and `GH_STATE_FILE` exported so the stub records argv there.
fn xtask_with_fake_gh(
    workdir: &Path,
    stub_bin_dir: &Path,
    state_file: &Path,
    args: &[&str],
) -> Command {
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
    cmd.env(
        "PATH",
        format!("{}:/usr/bin:{}", stub_bin_dir.display(), original_path),
    );
    cmd.env("GH_STATE_FILE", state_file);
    cmd
}

/// Render a fixture formula file so the bump step has something to commit.
/// We intentionally do not re-render via `xtask render-formula` here — that
/// path is covered elsewhere; this test isolates the retry / no-second-PR
/// behaviour of bump-tap-formula.
fn write_fixture_formula(path: &Path, version: &str, marker: &str) {
    let formula = format!(
        "class Modeltap < Formula\n  \
         # {marker}\n  \
         desc \"TUI for managing local AI models\"\n  \
         version \"{version}\"\n  \
         license \"MIT OR Apache-2.0\"\nend\n"
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir formula parent");
    }
    std::fs::write(path, formula).expect("write fixture formula");
}

/// Count how many `pr create ...` lines appear in the gh state file. Each
/// line records one `gh pr create` invocation (the stub records the full
/// argv space-joined; the leading two tokens are always `pr <subcommand>`).
fn count_pr_create_invocations(state_file: &Path) -> usize {
    let text = std::fs::read_to_string(state_file).unwrap_or_default();
    text.lines()
        .filter(|line| line.starts_with("pr create "))
        .count()
}

/// Count how many `pr list ...` lines appear in the gh state file.
fn count_pr_list_invocations(state_file: &Path) -> usize {
    let text = std::fs::read_to_string(state_file).unwrap_or_default();
    text.lines()
        .filter(|line| line.starts_with("pr list "))
        .count()
}

/// Build a fully-wired tap fixture: bare repo + seeded main + stub-bin dir +
/// state file path. Returns (workdir, bare_path, tap_url, stub_bin, state_file).
struct Fixture {
    _workdir: TempDir,
    bare: PathBuf,
    tap_url: String,
    stub_bin: PathBuf,
    state_file: PathBuf,
    workdir_path: PathBuf,
}

fn setup_fixture() -> Fixture {
    let workdir = TempDir::new().expect("create tempdir");
    let workdir_path = workdir.path().to_path_buf();
    let bare = workdir_path.join("tap-fake.git");
    std::fs::create_dir(&bare).expect("mkdir tap-fake.git");
    let tap_url = init_bare_tap_remote(&bare);
    seed_tap_remote_with_initial_commit(&bare);

    let stub_bin = workdir_path.join("stub-bin");
    std::fs::create_dir(&stub_bin).expect("mkdir stub-bin");
    install_fake_gh(&stub_bin);

    let state_file = workdir_path.join("gh-state.log");
    // Ensure the file exists so `grep -q` does not error on first call.
    std::fs::write(&state_file, "").expect("seed gh-state file");

    Fixture {
        _workdir: workdir,
        bare,
        tap_url,
        stub_bin,
        state_file,
        workdir_path,
    }
}

// =============================================================================
// Scenario 1 — First-run creates the bump branch AND opens a new PR.
//   AC: no pre-existing branch ⇒ branch `bump/v<version>` is created
//       AND `gh pr create` is invoked exactly once.
// =============================================================================

#[test]
fn first_run_creates_bump_branch_and_opens_new_pr() {
    let fx = setup_fixture();
    let version = "0.2.0";
    let formula_src = fx.workdir_path.join("modeltap.rb");
    write_fixture_formula(&formula_src, version, "first-run-marker");

    let output = xtask_with_fake_gh(
        &fx.workdir_path,
        &fx.stub_bin,
        &fx.state_file,
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &fx.tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
            "--open-pr",
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula");

    assert!(
        output.status.success(),
        "first-run with --open-pr must exit zero; stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Branch exists in the bare remote.
    let refs = git_capture(&fx.bare, &["show-ref"]);
    assert!(
        refs.contains(&format!("refs/heads/bump/v{version}")),
        "bump branch must exist after first run; show-ref=\n{refs}"
    );

    // Exactly ONE `gh pr create` invocation.
    assert_eq!(
        count_pr_create_invocations(&fx.state_file),
        1,
        "first run must call `gh pr create` exactly once; gh-state=\n{}",
        std::fs::read_to_string(&fx.state_file).unwrap_or_default()
    );

    // The state file must show a `list` BEFORE the `create` (we look for an
    // open PR before deciding to create one).
    let log = std::fs::read_to_string(&fx.state_file).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    let list_idx = lines.iter().position(|l| l.starts_with("pr list "));
    let create_idx = lines.iter().position(|l| l.starts_with("pr create "));
    assert!(
        matches!((list_idx, create_idx), (Some(li), Some(ci)) if li < ci),
        "first run must invoke `gh pr list` BEFORE `gh pr create`; gh-state=\n{log}"
    );
}

// =============================================================================
// Scenario 2 — Re-run with existing branch + open PR force-pushes the latest
// formula to the existing branch AND does NOT open a second PR.
//   AC: After two consecutive runs with the same version, `gh pr create` was
//       invoked exactly ONCE (during the first run), and the bump branch's
//       HEAD commit reflects the SECOND invocation's formula content.
// =============================================================================

#[test]
fn re_run_force_pushes_and_does_not_open_second_pr() {
    let fx = setup_fixture();
    let version = "0.2.0";
    let formula_src = fx.workdir_path.join("modeltap.rb");

    // First run: marker A.
    write_fixture_formula(&formula_src, version, "marker-FIRST-RUN");
    let first = xtask_with_fake_gh(
        &fx.workdir_path,
        &fx.stub_bin,
        &fx.state_file,
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &fx.tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
            "--open-pr",
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula (1st)");
    assert!(
        first.status.success(),
        "first invocation must exit zero; stderr=\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        count_pr_create_invocations(&fx.state_file),
        1,
        "first run must call `gh pr create` exactly once"
    );

    // Second run: marker B (different content), same version.
    write_fixture_formula(&formula_src, version, "marker-SECOND-RUN");
    let second = xtask_with_fake_gh(
        &fx.workdir_path,
        &fx.stub_bin,
        &fx.state_file,
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &fx.tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
            "--open-pr",
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula (2nd)");
    assert!(
        second.status.success(),
        "second invocation must exit zero (idempotent retry); stderr=\n{}",
        String::from_utf8_lossy(&second.stderr)
    );

    // STILL exactly ONE `gh pr create` invocation (no second PR opened).
    assert_eq!(
        count_pr_create_invocations(&fx.state_file),
        1,
        "re-run with existing PR must NOT open a second PR; gh-state=\n{}",
        std::fs::read_to_string(&fx.state_file).unwrap_or_default()
    );

    // Both runs queried `gh pr list` (the gating call).
    assert_eq!(
        count_pr_list_invocations(&fx.state_file),
        2,
        "each run must call `gh pr list` to check for an existing PR; gh-state=\n{}",
        std::fs::read_to_string(&fx.state_file).unwrap_or_default()
    );

    // The bump branch's HEAD commit reflects the SECOND run's formula content.
    let committed = git_capture(
        &fx.bare,
        &["show", &format!("bump/v{version}:Formula/modeltap.rb")],
    );
    assert!(
        committed.contains("marker-SECOND-RUN"),
        "force-push must overwrite the branch with the latest formula; got:\n{committed}"
    );
    assert!(
        !committed.contains("marker-FIRST-RUN"),
        "first run's formula must be replaced by the second run's; got:\n{committed}"
    );
}

// =============================================================================
// Scenario 3 — Property: for any N retries on the same version, exactly ONE
// PR + ONE bump branch exist. We exercise N ∈ 1..=5 to stay under CI budget
// (each invocation is a real cargo run + git push); the property holds for
// all N ≥ 1 by induction on the loop body.
// =============================================================================

#[test]
fn n_retries_yield_exactly_one_pr_and_one_bump_branch() {
    for n in 1..=5usize {
        let fx = setup_fixture();
        let version = "0.2.0";
        let formula_src = fx.workdir_path.join("modeltap.rb");

        for run_idx in 1..=n {
            // Each run uses a slightly-different marker so we can confirm
            // the LAST run's content wins.
            let marker = format!("retry-iteration-{run_idx}-of-{n}");
            write_fixture_formula(&formula_src, version, &marker);

            let output = xtask_with_fake_gh(
                &fx.workdir_path,
                &fx.stub_bin,
                &fx.state_file,
                &[
                    "bump-tap-formula",
                    "--version",
                    version,
                    "--tap-repo-url",
                    &fx.tap_url,
                    "--formula",
                    formula_src.to_str().expect("utf-8 formula path"),
                    "--open-pr",
                ],
            )
            .output()
            .expect("invoke cargo xtask bump-tap-formula");

            assert!(
                output.status.success(),
                "retry {run_idx}/{n} must exit zero; stderr=\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Invariant 1: exactly ONE `gh pr create` across all N runs.
        assert_eq!(
            count_pr_create_invocations(&fx.state_file),
            1,
            "for N={n} retries, `gh pr create` must be invoked exactly once; gh-state=\n{}",
            std::fs::read_to_string(&fx.state_file).unwrap_or_default()
        );

        // Invariant 2: exactly ONE bump branch exists in the bare remote.
        let refs = git_capture(&fx.bare, &["show-ref"]);
        let bump_branches: Vec<&str> = refs
            .lines()
            .filter(|line| line.contains("refs/heads/bump/"))
            .collect();
        assert_eq!(
            bump_branches.len(),
            1,
            "for N={n} retries, exactly one bump branch must exist; got={bump_branches:?}"
        );
        assert!(
            bump_branches[0].contains(&format!("refs/heads/bump/v{version}")),
            "the single bump branch must be `bump/v{version}`; got={bump_branches:?}"
        );

        // Invariant 3: the LAST run's formula content wins on the branch.
        let committed = git_capture(
            &fx.bare,
            &["show", &format!("bump/v{version}:Formula/modeltap.rb")],
        );
        let expected_marker = format!("retry-iteration-{n}-of-{n}");
        assert!(
            committed.contains(&expected_marker),
            "the LAST run's formula must win for N={n}; expected marker {expected_marker:?}, got:\n{committed}"
        );
    }
}

// =============================================================================
// Scenario 4 — Manual edits to `Formula/modeltap.rb` on the bump branch are
// overwritten on the next render.
//   AC: A maintainer manually edits the formula on `bump/v<version>` (e.g.,
//       adding a stray comment). The next bump-tap-formula run REMOVES the
//       manual edit because it force-pushes the freshly-rendered formula.
// =============================================================================

#[test]
fn manual_edits_to_bump_branch_are_clobbered_on_re_run() {
    let fx = setup_fixture();
    let version = "0.2.0";
    let formula_src = fx.workdir_path.join("modeltap.rb");
    write_fixture_formula(&formula_src, version, "rendered-content-marker");

    // First run: produces a clean bump branch.
    let first = xtask_with_fake_gh(
        &fx.workdir_path,
        &fx.stub_bin,
        &fx.state_file,
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &fx.tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
            "--open-pr",
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula (1st)");
    assert!(
        first.status.success(),
        "first invocation must exit zero; stderr=\n{}",
        String::from_utf8_lossy(&first.stderr)
    );

    // Maintainer manually edits Formula/modeltap.rb on the bump branch in a
    // throwaway clone of the bare remote and pushes the edit back.
    let manual_edit_marker = "MANUAL-EDIT-malicious-comment-do-not-ship";
    {
        let manual_clone = TempDir::new().expect("manual clone tempdir");
        let manual_path = manual_clone.path();
        let bare_url = format!("file://{}", fx.bare.display());
        let status = Command::new("git")
            .args(["clone", "--quiet", &bare_url, "."])
            .current_dir(manual_path)
            .status()
            .expect("git clone");
        assert!(status.success(), "manual clone failed");
        let status = Command::new("git")
            .args(["checkout", "--quiet", &format!("bump/v{version}")])
            .current_dir(manual_path)
            .status()
            .expect("git checkout bump branch");
        assert!(status.success(), "checkout bump branch failed");

        // Append a manual-edit comment to the formula.
        let formula_path = manual_path.join("Formula").join("modeltap.rb");
        let mut text = std::fs::read_to_string(&formula_path).expect("read formula");
        text.push_str(&format!("\n# {manual_edit_marker}\n"));
        std::fs::write(&formula_path, &text).expect("write tampered formula");

        let status = Command::new("git")
            .args(["add", "Formula/modeltap.rb"])
            .current_dir(manual_path)
            .status()
            .expect("git add");
        assert!(status.success(), "git add failed");
        let status = Command::new("git")
            .args(["commit", "--quiet", "-m", "manual: edit by maintainer"])
            .current_dir(manual_path)
            .env("GIT_AUTHOR_NAME", "Maintainer")
            .env("GIT_AUTHOR_EMAIL", "maint@example.com")
            .env("GIT_COMMITTER_NAME", "Maintainer")
            .env("GIT_COMMITTER_EMAIL", "maint@example.com")
            .status()
            .expect("git commit");
        assert!(status.success(), "git commit (manual edit) failed");
        let status = Command::new("git")
            .args(["push", "--quiet", "origin", &format!("bump/v{version}")])
            .current_dir(manual_path)
            .status()
            .expect("git push manual edit");
        assert!(status.success(), "push manual edit failed");
    }

    // Sanity: the manual edit IS now on the bump branch.
    let pre_rerun = git_capture(
        &fx.bare,
        &["show", &format!("bump/v{version}:Formula/modeltap.rb")],
    );
    assert!(
        pre_rerun.contains(manual_edit_marker),
        "fixture sanity: manual edit must be visible on the branch BEFORE the re-run; got:\n{pre_rerun}"
    );

    // Second run: re-render (same formula source — no upstream change) and
    // bump again. The manual edit MUST be gone afterwards.
    let second = xtask_with_fake_gh(
        &fx.workdir_path,
        &fx.stub_bin,
        &fx.state_file,
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            &fx.tap_url,
            "--formula",
            formula_src.to_str().expect("utf-8 formula path"),
            "--open-pr",
        ],
    )
    .output()
    .expect("invoke cargo xtask bump-tap-formula (2nd)");
    assert!(
        second.status.success(),
        "second invocation must exit zero; stderr=\n{}",
        String::from_utf8_lossy(&second.stderr)
    );

    let post_rerun = git_capture(
        &fx.bare,
        &["show", &format!("bump/v{version}:Formula/modeltap.rb")],
    );
    assert!(
        !post_rerun.contains(manual_edit_marker),
        "manual edit must be CLOBBERED by the re-render; got:\n{post_rerun}"
    );
    assert!(
        post_rerun.contains("rendered-content-marker"),
        "post-rerun formula must contain the rendered content; got:\n{post_rerun}"
    );

    // And no second PR was opened.
    assert_eq!(
        count_pr_create_invocations(&fx.state_file),
        1,
        "re-run must NOT open a second PR; gh-state=\n{}",
        std::fs::read_to_string(&fx.state_file).unwrap_or_default()
    );
}
