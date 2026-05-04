// Acceptance test for the atomic-publish guard (US-08, ADR-010).
//
// Step: 02-02 (DELIVER wave, slice "Multi-arch real release").
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/multi-arch-release.feature
//                     - "Publish job declares dependency on validate-tag and
//                        build matrix"            (@release-1 @us-08)
//                     - "Single failing build cell prevents publish and
//                        tap-bump from running"  (@release-1 @us-08)
//                     - "Publish atomicity holds for any combination of build
//                        cell outcomes"          (@release-1 @us-08 @property)
// Architectural anchors:
//   - ADR-010 §Decision: `publish-github-release: needs: [validate-tag, build]`
//   - ADR-010 §Decision: NO `if: always()` or `if: failure()` overrides on
//     publish-github-release or bump-tap-formula (US-08.AC-3).
//
// This test layer asserts the atomic-publish invariant in two complementary
// ways:
//
//   (1) STRUCTURAL — parse the SHIPPED `.github/workflows/release.yml` and
//       assert the `needs:` DAG plus the absence of any bypass `if:` on ANY
//       job. The bypass-clause check walks every job (not just the two
//       gated ones) because a future contributor could accidentally add
//       `if: always()` to `bump-tap-formula` and that would silently defeat
//       the guard. Defence in depth.
//
//   (2) PROPERTY — proptest the GH-Actions matrix-job semantics across all
//       2^4 = 16 outcome combinations of pass/fail across the 4 build cells.
//       The simulator mirrors the GH semantics in a tiny DSL:
//          job.runs ⇔ every job in `needs` resolved to Success
//       and asserts the invariant for the 3-job DAG (build-matrix,
//       publish-github-release, bump-tap-formula):
//          publish_runs == all_cells_passed
//          bump_runs    == publish_runs
//
// Note on test scope: the `bump-tap-formula needs: publish-github-release`
// assertion is ALSO checked in `walking_skeleton_e2e.rs` at the WS exit gate.
// We re-assert it here because step 02-02's acceptance criterion explicitly
// covers BOTH `needs:` declarations together with the bypass-`if:` rule, and
// regression diagnostics are clearer when both edges of the DAG fail in the
// same test. (Per Mandate 5: variations of the same behavior — different
// edges of the same DAG — are parametrized via a table; see I-2.)
//
// Per DWD-03 the live workflow execution (actually triggering a tag push and
// observing the skipped status) is verified manually on the first real
// release; the simulator stands in for the GH runtime.

use std::path::PathBuf;

use proptest::prelude::*;
use serde_yaml::Value;

// =============================================================================
// Shared helpers — mirror the style used in workflow_structure.rs so the two
// files cooperate without diverging on YAML-navigation quirks.
// =============================================================================

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p
}

fn release_workflow_path() -> PathBuf {
    let mut p = workspace_root();
    p.push(".github");
    p.push("workflows");
    p.push("release.yml");
    p
}

fn read_release_workflow() -> String {
    let path = release_workflow_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn parse_workflow(src: &str) -> Value {
    serde_yaml::from_str(src).expect("release.yml must be valid YAML")
}

fn get<'a>(m: &'a Value, key: &str) -> &'a Value {
    let mapping = m
        .as_mapping()
        .unwrap_or_else(|| panic!("expected mapping when looking up {key:?}, got {m:?}"));
    mapping
        .get(Value::String(key.to_owned()))
        .unwrap_or_else(|| panic!("expected key {key:?} in mapping {mapping:?}"))
}

fn get_opt<'a>(m: &'a Value, key: &str) -> Option<&'a Value> {
    m.as_mapping()
        .and_then(|mapping| mapping.get(Value::String(key.to_owned())))
}

/// Coerce a YAML scalar/sequence into a `Vec<String>`.
fn as_string_list(v: &Value) -> Vec<String> {
    if let Some(s) = v.as_str() {
        return vec![s.to_owned()];
    }
    if let Some(seq) = v.as_sequence() {
        return seq
            .iter()
            .map(|el| {
                el.as_str()
                    .unwrap_or_else(|| panic!("expected string in sequence, got {el:?}"))
                    .to_owned()
            })
            .collect();
    }
    panic!("expected string or sequence-of-strings, got {v:?}");
}

// =============================================================================
// I-1. publish-github-release.needs == [validate-tag, build]
//      bump-tap-formula.needs       == [publish-github-release]
//      Parametrised single-source-of-truth table so a regression on EITHER
//      edge fails with a clear, edge-specific diagnostic.
// =============================================================================

#[test]
fn atomic_publish_needs_dag_is_intact() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");

    // (downstream-job, must-need-each-of)
    let edges: &[(&str, &[&str])] = &[
        ("publish-github-release", &["validate-tag", "build"]),
        ("bump-tap-formula", &["publish-github-release"]),
    ];

    for (job_name, required_needs) in edges {
        let job = get_opt(jobs, job_name)
            .unwrap_or_else(|| panic!("release.yml must declare job `{job_name}` (ADR-010)"));
        let needs = get_opt(job, "needs").unwrap_or_else(|| {
            panic!(
                "{job_name}.needs must be declared so the atomic-publish guarantee holds \
                 (ADR-010 §Decision)"
            )
        });
        let needs_list = as_string_list(needs);

        for required in *required_needs {
            assert!(
                needs_list.iter().any(|n| n == required),
                "{job_name}.needs must include `{required}` (ADR-010 atomic-publish DAG). \
                 Got: {needs_list:?}"
            );
        }
    }
}

// =============================================================================
// I-2. NO job in release.yml uses `if: always()` or `if: failure()` to
//      bypass the `needs:` guard. Either expression would cause the job to
//      run even when a needed job failed — which is exactly the silent-skip
//      pattern C8 forbids and US-08.AC-3 prohibits.
//
//      We walk EVERY job (not just publish + bump) because the only way to
//      bypass the guard is via an `if:` that evaluates to true on failure;
//      preventing it on any future-added job is cheap defence in depth.
// =============================================================================

#[test]
fn no_job_uses_if_always_or_if_failure_to_bypass_needs_guard() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");

    let jobs_map = jobs
        .as_mapping()
        .expect("`jobs:` must be a mapping in release.yml");

    let mut violations: Vec<String> = Vec::new();

    for (key, job) in jobs_map {
        let job_name = key.as_str().expect("job keys must be strings").to_owned();

        let Some(if_value) = get_opt(job, "if") else {
            continue;
        };
        let Some(if_str) = if_value.as_str() else {
            // Non-string `if:` is unusual but not necessarily a bypass;
            // record it so a reviewer can audit by hand.
            violations.push(format!(
                "{job_name}: `if:` is not a string — got {if_value:?}; manual audit required"
            ));
            continue;
        };

        // Normalise whitespace + lowercase for substring search; the GH
        // expression syntax is case-insensitive for function identifiers.
        let needle = if_str.to_ascii_lowercase();
        if needle.contains("always()") || needle.contains("failure()") {
            violations.push(format!(
                "{job_name}: `if: {if_str}` — uses `always()` or `failure()` which would \
                 bypass the `needs:` guard (US-08.AC-3, ADR-010)"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "release.yml must NOT bypass the atomic-publish `needs:` guard via `if: always()` \
         or `if: failure()` on ANY job. Violations:\n  - {}",
        violations.join("\n  - ")
    );
}

// =============================================================================
// I-3. PROPERTY — GH-Actions `needs:` semantics simulator across all 2^4 = 16
//      outcome combinations for the 4 build matrix cells.
//
// GH semantics under test (per ADR-010 + GH Actions docs):
//   - A matrix job's overall result is Success iff every cell succeeded.
//     (`fail-fast: false` does not change this — it only controls cancellation
//     of in-flight cells.)
//   - A downstream job declared with `needs: <upstream>` runs iff every
//     upstream resolved to Success. Otherwise it is Skipped.
//
// Invariants to assert for the 3-edge DAG:
//   build-matrix → publish-github-release → bump-tap-formula
//
//   publish_runs == every build cell succeeded
//   bump_runs    == publish_runs
//
// We use proptest with an explicit 16-case strategy. The strategy is a
// vector of length 4 of booleans (true=pass, false=fail). With four 2-state
// cells the state space is exactly 2^4 = 16; proptest's default 256 cases
// gives saturation and the shrinker is well-defined on bool vectors.
// =============================================================================

/// Outcome of a single GH Actions job-or-cell. Mirrors the subset of the
/// `result` field GH writes when a job finishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Success,
    Failure,
    Skipped,
}

/// Compute a job's outcome given the outcomes of every job it `needs:`.
///
/// GH rule: the job runs iff every needed job resolved to Success. If ANY
/// dependency failed (or was itself skipped), the job is Skipped — we map
/// "did not run" to `Skipped`; "ran and would have its own outcome" maps to
/// the outcome it would observably produce (always Success in our simulator
/// because publish/bump have no failure mode in the steady state — the
/// matrix-cell failure scenarios are the entire concern).
fn downstream_outcome(needs: &[Outcome]) -> Outcome {
    if needs.iter().all(|o| *o == Outcome::Success) {
        Outcome::Success
    } else {
        Outcome::Skipped
    }
}

/// Reduce a matrix-job's per-cell outcomes to the single outcome the matrix
/// job exposes to its downstream consumers. GH rule: the matrix succeeds
/// iff every cell succeeded.
fn matrix_outcome(cells: &[Outcome]) -> Outcome {
    if cells.iter().all(|o| *o == Outcome::Success) {
        Outcome::Success
    } else {
        Outcome::Failure
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Exhaustive over 2^4 = 16 with comfortable headroom; deterministic
        // failures dominate, no I/O, no flake.
        cases: 256,
        .. ProptestConfig::default()
    })]

    /// Property: across ALL combinations of pass/fail outcomes for the 4
    /// build matrix cells, the publish-github-release job runs iff every
    /// cell succeeded, and the bump-tap-formula job runs iff publish ran.
    ///
    /// Source scenario:
    ///   "Publish atomicity holds for any combination of build cell
    ///    outcomes" (@release-1 @us-08 @property).
    #[test]
    fn publish_atomicity_holds_for_every_matrix_outcome_combination(
        cells in proptest::collection::vec(any::<bool>(), 4..=4)
    ) {
        // Map booleans → Outcomes so the simulator inputs read naturally.
        let cell_outcomes: Vec<Outcome> = cells
            .iter()
            .map(|pass| if *pass { Outcome::Success } else { Outcome::Failure })
            .collect();

        // validate-tag is a precondition of build; in this scenario we hold
        // it at Success because the scenario isolates matrix-cell behaviour.
        let validate_tag_outcome = Outcome::Success;
        let build_job_outcome = matrix_outcome(&cell_outcomes);

        // publish-github-release: needs [validate-tag, build]
        let publish_outcome = downstream_outcome(&[validate_tag_outcome, build_job_outcome]);

        // bump-tap-formula: needs publish-github-release
        let bump_outcome = downstream_outcome(&[publish_outcome]);

        let all_cells_passed = cells.iter().all(|p| *p);
        let publish_ran = publish_outcome == Outcome::Success;
        let bump_ran = bump_outcome == Outcome::Success;

        // Invariant 1: publish runs iff every cell succeeded.
        prop_assert_eq!(
            publish_ran,
            all_cells_passed,
            "publish-github-release ran ({publish_ran}) but all_cells_passed={all_cells_passed} \
             — atomic-publish guarantee violated for cells={cells:?}",
            publish_ran = publish_ran,
            all_cells_passed = all_cells_passed,
            cells = cells
        );

        // Invariant 2: bump runs iff publish ran.
        prop_assert_eq!(
            bump_ran,
            publish_ran,
            "bump-tap-formula ran ({bump_ran}) but publish_ran={publish_ran} — bump must NEVER \
             run when publish was skipped (cells={cells:?})",
            bump_ran = bump_ran,
            publish_ran = publish_ran,
            cells = cells
        );
    }
}

// =============================================================================
// I-4. Concrete witness for the headline scenario: "Single failing build
//      cell prevents publish and tap-bump from running". This is the
//      smoke-level companion to the proptest above — fixed inputs, fixed
//      expected outputs, easy to read in a stack trace, and it pins the
//      semantics of `Outcome::Skipped` so a future refactor of the
//      simulator cannot quietly invert the meaning.
// =============================================================================

#[test]
fn single_failing_cell_skips_publish_and_bump() {
    // 3 cells pass, the 4th (e.g. aarch64-unknown-linux-gnu) fails.
    let cells = [
        Outcome::Success,
        Outcome::Success,
        Outcome::Success,
        Outcome::Failure,
    ];

    let build = matrix_outcome(&cells);
    assert_eq!(
        build,
        Outcome::Failure,
        "build job must surface as Failure when any cell fails (GH matrix semantics)"
    );

    let publish = downstream_outcome(&[Outcome::Success, build]);
    assert_eq!(
        publish,
        Outcome::Skipped,
        "publish-github-release MUST be Skipped when build failed (US-08 atomic publish)"
    );

    let bump = downstream_outcome(&[publish]);
    assert_eq!(
        bump,
        Outcome::Skipped,
        "bump-tap-formula MUST be Skipped when publish was Skipped (US-08 atomic publish)"
    );
}
