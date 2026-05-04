// Acceptance tests for the two `@recovery` scenarios in
// integration-checkpoints.feature.
//
// Step: 03-05 (DELIVER wave, FINAL Phase-2 step).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/integration-checkpoints.feature
//                   (the 2 scenarios tagged `@recovery`).
// Design specs: docs/feature/release-process-homebrew-github/devops/
//               monitoring-alerting.md §4.2 (Scenarios A-D).
//
// Strategy C — real local resources (DWD-01):
//   - Real tempdir for ephemeral modeltap-fake (working) + tap-fake (bare) repos
//   - Real git init/commit/push/clone for cross-repo operations
//   - Real `cargo run --package xtask -- ...` subprocess invocations
//   - The "GitHub Release" surface is modelled as a directory of artifact files
//     (the bare-hex sha256 sidecars + a release-notes file). Per DWD-02 the
//     `gh release` API itself is gated by `@requires_external` smoke tests; the
//     recovery contract under test is about LOCAL state surviving partial
//     failure, not about the GitHub Releases API itself.
//
// Two recovery scenarios:
//
//   R-1. "GitHub Release succeeds but tap-bump fails leaves an intact release"
//        (monitoring-alerting.md §4.2 Scenario B, US-12 idempotent retry).
//        Acceptance criterion: when bump-tap-formula fails (e.g. because the
//        tap remote URL is unreachable, modelling an expired token), the
//        previously-published "GitHub Release" artifact directory remains
//        unchanged AND a successful re-run reaches the same end state with
//        no duplicate bump branch (idempotency contract).
//
//   R-2. "Maintainer yanks a release after a critical defect is found"
//        (monitoring-alerting.md §4.2 Scenario C). The maintainer deletes the
//        "GitHub Release" artifact directory AND reverts the tap-bump branch.
//        End state: the prior release artifacts are still installable from
//        their own snapshot (the system is back to the v(N-1) state).
//
// PATH note: `~/.pyenv/shims/cc` shadows the real cc on this developer's
// machine and breaks build-script linking. Sub-cargo invocations need
// PATH=/usr/bin:$PATH.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// Path to the modeltap workspace's root Cargo.toml.
fn workspace_manifest() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("Cargo.toml");
    p
}

/// Path to the in-repo Tera template the rendered formula is built from.
fn template_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("release/templates/modeltap.rb.tera");
    p
}

/// Build a Command that invokes `cargo run --manifest-path <ws> --package
/// xtask --quiet -- <args>` with the given working directory and a sanitised
/// PATH so build-script linker invocations find a real cc.
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
    assert!(status.success(), "git {args:?} failed");
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
         - Walking-skeleton release-candidate.\n",
    );
    std::fs::write(repo.join("CHANGELOG.md"), changelog).expect("write CHANGELOG.md");

    let triple = "x86_64-unknown-linux-gnu";
    let archive_name = format!("modeltap-{version}-{triple}.tar.gz");
    let sidecar_name = format!("{archive_name}.sha256");
    let artifacts = repo.join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");
    std::fs::write(artifacts.join(&sidecar_name), format!("{sha256_hex}\n"))
        .expect("write sha256 sidecar");
}

/// Initialise a bare git repo to act as the ephemeral tap remote.
fn init_bare_tap_remote(path: &Path) -> String {
    let status = Command::new("git")
        .args(["init", "--bare", "--quiet", "--initial-branch=main"])
        .arg(path)
        .status()
        .expect("invoke git init --bare");
    assert!(status.success(), "git init --bare {path:?} failed");
    format!("file://{}", path.display())
}

/// Seed the bare tap remote with one initial commit on `main`.
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

/// Render the formula for `version` into `formula_path` using the in-repo Tera
/// template + the modeltap-fake's sidecar directory.
fn render_formula(modeltap_fake: &Path, version: &str, formula_path: &Path) {
    std::fs::create_dir_all(formula_path.parent().expect("formula path has parent"))
        .expect("mkdir formula parent");
    let tag = format!("v{version}");
    let release_base_url =
        format!("https://github.com/jeffabailey/modeltap/releases/download/{tag}");
    let render = xtask_in(
        modeltap_fake,
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
}

/// Invoke `xtask bump-tap-formula` and return the captured `Output`. The
/// caller asserts on success/failure.
fn bump_tap_formula(
    modeltap_fake: &Path,
    version: &str,
    tap_url: &str,
    formula_path: &Path,
) -> std::process::Output {
    xtask_in(
        modeltap_fake,
        &[
            "bump-tap-formula",
            "--version",
            version,
            "--tap-repo-url",
            tap_url,
            "--formula",
            formula_path.to_str().expect("utf-8 formula path"),
        ],
    )
    .output()
    .expect("invoke xtask bump-tap-formula")
}

/// Compute a digest of the artifact directory contents so we can prove "the
/// GitHub Release remained intact" — every file's bytes unchanged after the
/// failed tap-bump. We intentionally include filenames + bytes (not mtimes)
/// to stay deterministic across reruns.
fn artifact_directory_fingerprint(artifacts_dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries: Vec<(String, Vec<u8>)> = std::fs::read_dir(artifacts_dir)
        .expect("read artifacts dir")
        .map(|entry| {
            let entry = entry.expect("dir entry");
            let name = entry.file_name().into_string().expect("utf-8 filename");
            let bytes = std::fs::read(entry.path()).expect("read artifact bytes");
            (name, bytes)
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

// =============================================================================
// R-1 — GH Release stays intact when bump-tap-formula fails; the maintainer
//       can re-run the bump step after fixing the underlying problem (token
//       rotation in the production pipeline; here we model it as repointing
//       the tap-repo-url at a reachable remote) and reaches the same end
//       state with no duplicate PR.
// =============================================================================

#[test]
fn gh_release_remains_intact_when_bump_tap_formula_fails_and_retry_reaches_same_end_state() {
    let workdir = TempDir::new().expect("create tempdir");
    let modeltap_fake = workdir.path().join("modeltap-fake");
    std::fs::create_dir(&modeltap_fake).expect("mkdir modeltap-fake");
    let tap_fake_bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&tap_fake_bare).expect("mkdir tap-fake.git");
    // The "GitHub Release" surface — modelled as a local artifact directory
    // populated by an upstream `publish-github-release` job that succeeded.
    let gh_release_dir = workdir.path().join("gh-release-v0.2.0");
    std::fs::create_dir(&gh_release_dir).expect("mkdir gh-release dir");

    let version = "0.2.0";
    let tag = format!("v{version}");
    let sha256_hex = "a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90";

    // Set up modeltap-fake (Cargo.toml + CHANGELOG + sidecar) and the tap remote.
    seed_modeltap_fake(&modeltap_fake, version, sha256_hex);
    let tap_url = init_bare_tap_remote(&tap_fake_bare);
    seed_tap_remote_with_initial_commit(&tap_fake_bare);

    // Simulate the upstream "publish-github-release" succeeding: copy the
    // archive sidecar + RELEASE_NOTES into the GH-Release artifact dir.
    let triple = "x86_64-unknown-linux-gnu";
    let archive_name = format!("modeltap-{version}-{triple}.tar.gz");
    let sidecar_name = format!("{archive_name}.sha256");
    std::fs::copy(
        modeltap_fake.join("artifacts").join(&sidecar_name),
        gh_release_dir.join(&sidecar_name),
    )
    .expect("copy sidecar into gh-release dir");
    std::fs::write(
        gh_release_dir.join("RELEASE_NOTES.md"),
        "## v0.2.0\n\nWalking-skeleton release-candidate.\n",
    )
    .expect("write release notes");

    // Snapshot the GH-Release dir BEFORE the failed bump — used to prove the
    // failed bump did NOT touch the release artifacts.
    let pre_failure_fingerprint = artifact_directory_fingerprint(&gh_release_dir);

    // Render the formula from the sidecar.
    let formula_path = workdir.path().join("Formula").join("modeltap.rb");
    render_formula(&modeltap_fake, version, &formula_path);

    // -------------------------------------------------------------------------
    // PHASE 1 — bump-tap-formula FAILS because the tap remote URL is wrong.
    //           In production this models GH_TAP_TOKEN expiry: the cross-repo
    //           push fails, but the GitHub Release was already published in
    //           the upstream job and remains intact (atomic-publish per US-08).
    // -------------------------------------------------------------------------
    let unreachable_url = format!("file://{}/does-not-exist.git", workdir.path().display());
    let failed_bump = bump_tap_formula(&modeltap_fake, version, &unreachable_url, &formula_path);
    assert!(
        !failed_bump.status.success(),
        "bump-tap-formula MUST fail when the tap remote URL is unreachable \
         (modelling GH_TAP_TOKEN expiry); stderr=\n{}\nstdout=\n{}",
        String::from_utf8_lossy(&failed_bump.stderr),
        String::from_utf8_lossy(&failed_bump.stdout),
    );

    // ASSERT R-1.a: the GH-Release artifact directory is byte-for-byte
    // unchanged. The atomic-publish guarantee (US-08) means a tap-bump
    // failure does NOT corrupt the published release.
    let post_failure_fingerprint = artifact_directory_fingerprint(&gh_release_dir);
    assert_eq!(
        pre_failure_fingerprint, post_failure_fingerprint,
        "GH-Release artifact directory must be byte-for-byte unchanged after \
         a tap-bump failure (R-1.a, monitoring-alerting.md §4.2 Scenario B)"
    );

    // -------------------------------------------------------------------------
    // PHASE 2 — Maintainer "rotates the token" (here: repoints the tap URL
    //           at the reachable file:// remote) and re-runs bump-tap-formula.
    //           The retry MUST succeed AND reach the same end state.
    // -------------------------------------------------------------------------
    let retried_bump = bump_tap_formula(&modeltap_fake, version, &tap_url, &formula_path);
    assert!(
        retried_bump.status.success(),
        "bump-tap-formula retry MUST succeed once the tap URL is reachable; \
         stderr=\n{}",
        String::from_utf8_lossy(&retried_bump.stderr)
    );

    // ASSERT R-1.b: exactly ONE bump branch exists for this tag (no duplicate
    // from the retry). The idempotent-retry contract (US-12, force-with-lease)
    // overwrites the branch if it already exists; here the branch did not
    // exist after the failed first attempt (the failure happened before push)
    // so we verify there is exactly one.
    let refs = git_capture(&tap_fake_bare, &["show-ref"]);
    let bump_branches: Vec<&str> = refs
        .lines()
        .filter(|line| line.contains(&format!("refs/heads/bump/{tag}")))
        .collect();
    assert_eq!(
        bump_branches.len(),
        1,
        "exactly one bump/{tag} branch must exist after retry (no duplicates) \
         (R-1.b, US-12 idempotency); refs were:\n{refs}"
    );

    // ASSERT R-1.c: the retry's committed formula contains the same version +
    // sha256 that flowed through the pipeline before the failure (proves the
    // retry reaches the SAME end state, not a divergent one).
    let committed_formula = git_capture(
        &tap_fake_bare,
        &["show", &format!("bump/{tag}:Formula/modeltap.rb")],
    );
    assert!(
        committed_formula.contains(&format!("version \"{version}\"")),
        "retry's committed formula must declare version {version}"
    );
    assert!(
        committed_formula.contains(sha256_hex),
        "retry's committed formula must contain the sidecar sha256 verbatim — \
         proves the data flowed through unchanged after the recovery"
    );

    // ASSERT R-1.d: the GH-Release artifact directory is STILL byte-for-byte
    // unchanged after the successful retry. The retry only writes to the tap
    // repo, never to the GH-Release surface.
    let final_fingerprint = artifact_directory_fingerprint(&gh_release_dir);
    assert_eq!(
        pre_failure_fingerprint, final_fingerprint,
        "GH-Release artifact directory must remain unchanged across the \
         tap-bump retry (R-1.d, monitoring-alerting.md §4.2 Scenario B \
         option 1: re-run only the bump-tap-formula step)"
    );
}

// =============================================================================
// R-2 — Maintainer yanks a release after a critical defect is found. The
//       prior version's installable state is preserved on a separate
//       artifact snapshot, so end users on `brew install` resolve to that
//       prior version after the yank.
// =============================================================================

#[test]
fn yank_and_revert_leaves_prior_version_installable() {
    let workdir = TempDir::new().expect("create tempdir");
    let modeltap_fake_v1 = workdir.path().join("modeltap-fake-v1");
    std::fs::create_dir(&modeltap_fake_v1).expect("mkdir modeltap-fake-v1");
    let modeltap_fake_v2 = workdir.path().join("modeltap-fake-v2");
    std::fs::create_dir(&modeltap_fake_v2).expect("mkdir modeltap-fake-v2");
    let tap_fake_bare = workdir.path().join("tap-fake.git");
    std::fs::create_dir(&tap_fake_bare).expect("mkdir tap-fake.git");

    // PRIOR version (v0.1.0) — already shipped end-to-end.
    let v1 = "0.1.0";
    let tag1 = format!("v{v1}");
    let sha1 = "1111111111111111111111111111111111111111111111111111111111111111";
    seed_modeltap_fake(&modeltap_fake_v1, v1, sha1);

    // BUGGY version (v0.2.0) — to be yanked.
    let v2 = "0.2.0";
    let tag2 = format!("v{v2}");
    let sha2 = "2222222222222222222222222222222222222222222222222222222222222222";
    seed_modeltap_fake(&modeltap_fake_v2, v2, sha2);

    let tap_url = init_bare_tap_remote(&tap_fake_bare);
    seed_tap_remote_with_initial_commit(&tap_fake_bare);

    // Snapshot the prior version's GH-Release artifacts (proves the
    // pre-yank installable state survives — `brew install` reads from this).
    let prior_release_snapshot = workdir.path().join(format!("gh-release-{tag1}"));
    std::fs::create_dir(&prior_release_snapshot).expect("mkdir prior release snapshot");
    let triple = "x86_64-unknown-linux-gnu";
    let prior_sidecar_name = format!("modeltap-{v1}-{triple}.tar.gz.sha256");
    std::fs::copy(
        modeltap_fake_v1.join("artifacts").join(&prior_sidecar_name),
        prior_release_snapshot.join(&prior_sidecar_name),
    )
    .expect("copy prior sidecar");

    // -------------------------------------------------------------------------
    // PHASE 1 — Ship v0.1.0 end-to-end: render formula, bump tap.
    // -------------------------------------------------------------------------
    let formula_v1 = workdir.path().join("Formula-v1").join("modeltap.rb");
    render_formula(&modeltap_fake_v1, v1, &formula_v1);
    let bump_v1 = bump_tap_formula(&modeltap_fake_v1, v1, &tap_url, &formula_v1);
    assert!(
        bump_v1.status.success(),
        "v0.1.0 bump-tap-formula must succeed; stderr=\n{}",
        String::from_utf8_lossy(&bump_v1.stderr)
    );

    // Merge the v1 bump branch into main so `brew install` resolves to v1.
    let working = tempfile::tempdir().expect("create working clone tempdir");
    git(
        working.path(),
        &[
            "clone",
            "--quiet",
            &tap_fake_bare.display().to_string(),
            ".",
        ],
    );
    git(
        working.path(),
        &["fetch", "--quiet", "origin", &format!("bump/{tag1}")],
    );
    git(
        working.path(),
        &[
            "merge",
            "--quiet",
            "--no-ff",
            "-m",
            &format!("Merge bump/{tag1}"),
            &format!("origin/bump/{tag1}"),
        ],
    );
    git(working.path(), &["push", "--quiet", "origin", "main"]);

    // Snapshot the post-v1-merge tap state for the post-yank assertion.
    let main_after_v1 = git_capture(&tap_fake_bare, &["rev-parse", "main"])
        .trim()
        .to_owned();
    let formula_after_v1 = git_capture(&tap_fake_bare, &["show", "main:Formula/modeltap.rb"]);
    assert!(
        formula_after_v1.contains(&format!("version \"{v1}\"")),
        "tap main must point at v{v1} after the v1 merge"
    );

    // -------------------------------------------------------------------------
    // PHASE 2 — Ship v0.2.0 (the buggy one) end-to-end.
    // -------------------------------------------------------------------------
    let formula_v2 = workdir.path().join("Formula-v2").join("modeltap.rb");
    render_formula(&modeltap_fake_v2, v2, &formula_v2);
    let bump_v2 = bump_tap_formula(&modeltap_fake_v2, v2, &tap_url, &formula_v2);
    assert!(
        bump_v2.status.success(),
        "v0.2.0 bump-tap-formula must succeed; stderr=\n{}",
        String::from_utf8_lossy(&bump_v2.stderr)
    );

    // Merge v2 into tap main.
    let working_v2 = tempfile::tempdir().expect("create working clone tempdir v2");
    git(
        working_v2.path(),
        &[
            "clone",
            "--quiet",
            &tap_fake_bare.display().to_string(),
            ".",
        ],
    );
    git(
        working_v2.path(),
        &["fetch", "--quiet", "origin", &format!("bump/{tag2}")],
    );
    git(
        working_v2.path(),
        &[
            "merge",
            "--quiet",
            "--no-ff",
            "-m",
            &format!("Merge bump/{tag2}"),
            &format!("origin/bump/{tag2}"),
        ],
    );
    git(working_v2.path(), &["push", "--quiet", "origin", "main"]);

    // Sanity: tap main now points at v2.
    let formula_after_v2 = git_capture(&tap_fake_bare, &["show", "main:Formula/modeltap.rb"]);
    assert!(
        formula_after_v2.contains(&format!("version \"{v2}\"")),
        "tap main must point at v{v2} after the v2 merge"
    );

    // -------------------------------------------------------------------------
    // PHASE 3 — YANK: maintainer reverts the v2 merge in the tap repo. In
    //           production this is a `gh pr` revert; here it is a
    //           `git revert` against the bare remote via a working clone.
    //           We also "delete the GH Release" by removing the v2 snapshot
    //           directory from the workdir (it was never created — we model
    //           this by simply NOT creating a v2 snapshot).
    // -------------------------------------------------------------------------
    let working_yank = tempfile::tempdir().expect("create working clone tempdir yank");
    git(
        working_yank.path(),
        &[
            "clone",
            "--quiet",
            &tap_fake_bare.display().to_string(),
            ".",
        ],
    );
    // Revert the merge commit (HEAD on main is the v2 merge).
    git(
        working_yank.path(),
        &["revert", "--no-edit", "-m", "1", "HEAD"],
    );
    git(working_yank.path(), &["push", "--quiet", "origin", "main"]);

    // -------------------------------------------------------------------------
    // ASSERT R-2: after the yank, tap main resolves to v0.1.0 — `brew install`
    // would read this Formula and install the prior version.
    // -------------------------------------------------------------------------
    let formula_after_yank = git_capture(&tap_fake_bare, &["show", "main:Formula/modeltap.rb"]);
    assert!(
        formula_after_yank.contains(&format!("version \"{v1}\"")),
        "after the yank, tap main's Formula/modeltap.rb must declare version \
         {v1} so `brew install` resolves to the prior release; got:\n{formula_after_yank}"
    );
    assert!(
        formula_after_yank.contains(sha1),
        "after the yank, tap main's formula must contain the v{v1} sha256 \
         {sha1} (proves the prior version's archive hash is what `brew install` \
         would verify against)"
    );
    assert!(
        !formula_after_yank.contains(&format!("version \"{v2}\"")),
        "after the yank, tap main's formula must NOT declare the buggy version \
         {v2} (R-2: revert removed v{v2} from main)"
    );
    assert!(
        !formula_after_yank.contains(sha2),
        "after the yank, tap main's formula must NOT contain the v{v2} sha256 \
         (R-2: yank removed the buggy archive's hash)"
    );

    // ASSERT R-2.b: the prior version's GH-Release snapshot is still intact —
    // `brew install` downloads from this surface. The yank only touched the
    // tap repo (Formula points at v1 again); the v1 release archives were
    // never deleted.
    let prior_snapshot_after_yank = artifact_directory_fingerprint(&prior_release_snapshot);
    assert_eq!(
        prior_snapshot_after_yank.len(),
        1,
        "prior release snapshot must still contain exactly the v{v1} sidecar \
         after the yank — the yank only operates on the buggy v{v2} surface"
    );
    assert_eq!(
        prior_snapshot_after_yank[0].0, prior_sidecar_name,
        "prior release snapshot must still contain the v{v1} archive sidecar"
    );

    // ASSERT R-2.c: the revert produced a NEW commit on main (we did not
    // rewrite history) — this is the "next release v0.2.1 follows the
    // standard process" precondition: main is moving forward, not backward.
    let main_after_yank = git_capture(&tap_fake_bare, &["rev-parse", "main"])
        .trim()
        .to_owned();
    assert_ne!(
        main_after_v1, main_after_yank,
        "the yank revert must produce a NEW commit on main (not rewrite \
         history) so the next release v0.2.1 builds on top of it"
    );
}
