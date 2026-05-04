// Acceptance test for the auto-merge wiring on the tap-bump PR.
//
// Step: 03-01 (Phase 3 — hands-off automation, US-11).
// Source scenario: docs/feature/release-process-homebrew-github/distill/
//                  features/hands-off-automation.feature
//                  ("Bump-tap-formula step invokes auto-merge with squash strategy").
// Architectural anchors:
//   - architecture-design.md §6.1 (US-11 auto-merge: gh pr merge --auto --squash
//     once after PR creation; brew test-bot is the gate via tap-repo branch
//     protection).
//   - component-boundaries.md §2.3 (gh_adapter surface includes
//     `gh pr merge --auto --squash`).
//
// What this test asserts (structural — no live GH calls):
//
//   A-1. The shipped `.github/workflows/release.yml` `bump-tap-formula` job
//        contains a step that invokes `gh pr merge --auto --squash --repo
//        jeffabailey/homebrew-modeltap bump/v${VERSION}` AFTER the bump-and-
//        open-PR step. Asserted on the YAML steps[] sequence.
//
//   A-2. The auto-merge step references `--auto`, `--squash`, the
//        `jeffabailey/homebrew-modeltap` repo slug, and a branch ref derived
//        from the version output (`bump/v${{ steps.version.outputs.VERSION }}`
//        or `bump/v${VERSION}`). The first three are exact substring checks;
//        the branch ref check accepts either GH-Actions expression or shell
//        env-var form — both produce the right argv at runtime.
//
//   A-3. Step ordering: the bump-and-open-PR step (the one whose `run:`
//        contains `bump-tap-formula` and `--open-pr`) MUST appear BEFORE the
//        `gh pr merge --auto` step. GitHub auto-merge cannot be enabled on a
//        PR that does not yet exist.
//
//   A-4. (@requires_external #[ignore]d) live smoke: when a tap-bump PR is
//        open and `brew test-bot` reports success on it, the PR auto-merges
//        within 5 minutes. Gated on `MODELTAP_LIVE_SMOKE=1` so the suite stays
//        fast and offline by default.
//
// Strategy C (DWD-01): real local resources. The shipped release.yml is read
// from disk and parsed with serde_yaml — same approach as workflow_structure.rs
// and slsa_attestation.rs. No mocking of file I/O.

use std::path::PathBuf;

use serde_yaml::Value;

/// Workspace root (parent of this `tests/` crate).
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p
}

/// Path to `.github/workflows/release.yml` in the workspace.
fn release_workflow_path() -> PathBuf {
    let mut p = workspace_root();
    p.push(".github");
    p.push("workflows");
    p.push("release.yml");
    p
}

/// Read the release.yml source text from disk. Panics with a clear diagnostic
/// if the file is missing — that IS the RED state for this step.
fn read_release_workflow() -> String {
    let path = release_workflow_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Parse the release.yml as a generic YAML `Value`.
fn parse_workflow(src: &str) -> Value {
    serde_yaml::from_str(src).expect("release.yml must be valid YAML")
}

/// Look up `m[key]` where `m` is expected to be a mapping.
fn get<'a>(m: &'a Value, key: &str) -> &'a Value {
    let mapping = m
        .as_mapping()
        .unwrap_or_else(|| panic!("expected mapping when looking up {key:?}, got {m:?}"));
    mapping
        .get(Value::String(key.to_owned()))
        .unwrap_or_else(|| panic!("expected key {key:?} in mapping {mapping:?}"))
}

/// Optional variant of `get`.
fn get_opt<'a>(m: &'a Value, key: &str) -> Option<&'a Value> {
    m.as_mapping()
        .and_then(|mapping| mapping.get(Value::String(key.to_owned())))
}

/// Concatenate a step's `name:` and `run:` fields into a single searchable
/// string. Matches the pattern used by workflow_structure.rs / build_matrix.rs.
fn step_to_search_string(step: &Value) -> String {
    let name = get_opt(step, "name").and_then(|v| v.as_str()).unwrap_or("");
    let run = get_opt(step, "run").and_then(|v| v.as_str()).unwrap_or("");
    format!("{name} || {run}")
}

/// Pull the `bump-tap-formula` job's steps[] sequence.
fn bump_job_steps(workflow: &Value) -> Vec<Value> {
    let jobs = get(workflow, "jobs");
    let bump = get_opt(jobs, "bump-tap-formula")
        .expect("release.yml must declare a `bump-tap-formula` job (US-06)");
    get(bump, "steps")
        .as_sequence()
        .expect("bump-tap-formula.steps must be a sequence")
        .clone()
}

// =============================================================================
// A-1 + A-2. Auto-merge step exists with the required flags
// =============================================================================

#[test]
fn bump_tap_formula_job_invokes_gh_pr_merge_auto_with_squash() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let steps = bump_job_steps(&workflow);

    let merge_step = steps
        .iter()
        .find(|s| {
            let blob = step_to_search_string(s);
            blob.contains("gh pr merge") && blob.contains("--auto")
        })
        .unwrap_or_else(|| {
            let dump: Vec<String> = steps.iter().map(step_to_search_string).collect();
            panic!(
                "bump-tap-formula job must contain a step invoking `gh pr merge --auto` \
                 (US-11). Steps:\n{}",
                dump.join("\n")
            )
        });

    let blob = step_to_search_string(merge_step);

    assert!(
        blob.contains("--squash"),
        "auto-merge step must use `--squash` strategy (US-11 — single commit on \
         tap main per release). Got step:\n{blob}"
    );
    assert!(
        blob.contains("jeffabailey/homebrew-modeltap"),
        "auto-merge step must target the `jeffabailey/homebrew-modeltap` repo \
         (US-11 cross-repo seam). Got step:\n{blob}"
    );

    // Branch ref MUST identify the bump branch derived from the release
    // version. We accept either the GH-Actions expression form (`steps.version`)
    // or the shell env-var form (`${VERSION}` / `$VERSION`) — both yield the
    // correct argv at runtime.
    let mentions_bump_branch = blob.contains("bump/v${{ steps.version.outputs.VERSION }}")
        || blob.contains("bump/v${VERSION}")
        || blob.contains("bump/v$VERSION");
    assert!(
        mentions_bump_branch,
        "auto-merge step must target `bump/v<version>` (US-11 — auto-merge \
         invocation targets the bump branch for the current version). Got step:\n{blob}"
    );
}

// =============================================================================
// A-3. Auto-merge step runs AFTER the bump-and-open-PR step
//      GitHub rejects `gh pr merge --auto` on a PR that does not yet exist.
// =============================================================================

#[test]
fn auto_merge_step_runs_after_bump_and_open_pr_step() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let steps = bump_job_steps(&workflow);

    let step_strs: Vec<String> = steps.iter().map(step_to_search_string).collect();

    let bump_idx = step_strs
        .iter()
        .position(|s| s.contains("bump-tap-formula") && s.contains("--open-pr"))
        .unwrap_or_else(|| {
            panic!(
                "bump-tap-formula job must contain a step invoking `xtask bump-tap-formula \
                 ... --open-pr` (US-06). Steps:\n{}",
                step_strs.join("\n")
            )
        });

    let merge_idx = step_strs
        .iter()
        .position(|s| s.contains("gh pr merge") && s.contains("--auto"))
        .unwrap_or_else(|| {
            panic!(
                "bump-tap-formula job must contain a `gh pr merge --auto` step (US-11). \
                 Steps:\n{}",
                step_strs.join("\n")
            )
        });

    assert!(
        bump_idx < merge_idx,
        "the `gh pr merge --auto` step (idx {merge_idx}) must run AFTER the \
         `bump-tap-formula --open-pr` step (idx {bump_idx}) — auto-merge cannot \
         be enabled on a PR that has not been created yet."
    );
}

// =============================================================================
// A-4. @requires_external smoke (live tap repo)
//      Gated by env var so the default suite stays offline. Run with:
//          MODELTAP_LIVE_SMOKE=1 cargo test --test \
//              release_process_auto_merge -- --ignored
// =============================================================================

#[test]
#[ignore = "live tap-repo smoke; run with MODELTAP_LIVE_SMOKE=1"]
fn auto_merge_fires_within_five_minutes_when_brew_test_bot_is_green() {
    if std::env::var("MODELTAP_LIVE_SMOKE").ok().as_deref() != Some("1") {
        eprintln!(
            "skipping live smoke: set MODELTAP_LIVE_SMOKE=1 to run against the \
             live jeffabailey/homebrew-modeltap tap repo"
        );
        return;
    }

    // The live smoke is documented in RELEASING.md (step 03-03):
    //   1. Push a release tag (e.g., v0.0.1-rc1) to modeltap.
    //   2. Wait for the bump-tap-formula job to open a PR on the tap repo.
    //   3. Confirm `brew test-bot` runs green on macos-14, macos-13, ubuntu-22.04.
    //   4. Confirm the PR auto-merges within 5 minutes WITHOUT maintainer click.
    //
    // The runner-only assertion below is a sentinel that the smoke fixture
    // wiring is in place. The full live observation lives outside the test
    // harness because `brew test-bot` runs on the tap repo's GH-Actions, not
    // here. This test exists to satisfy `auto-merge fires when brew test-bot
    // is green` from adapter-coverage.md and to give the maintainer a single
    // command to invoke before declaring US-11 done.
    panic!(
        "TODO: live smoke not yet runnable — requires a freshly-pushed release \
         tag and is observed via gh PR API. See RELEASING.md (step 03-03) for \
         the manual procedure. This panic is the sentinel: the test runs only \
         under MODELTAP_LIVE_SMOKE=1 and is currently expected to fail until a \
         maintainer wires the gh-PR-poll loop in step 03-05 (recovery + smoke)."
    );
}
