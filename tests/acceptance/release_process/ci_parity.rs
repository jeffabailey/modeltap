// Acceptance tests for cross-workflow CI parity (INT.AC-6).
//
// Step: 03-05 (DELIVER wave, FINAL Phase-2 step).
// Source scenario: docs/feature/release-process-homebrew-github/distill/
//                  features/integration-checkpoints.feature INT.AC-6:
//   "release.yml CI parity gates use the exact same flags as ci.yml"
//
// Plus the step's own acceptance criterion that ci.yml runs
// `cargo run -p xtask -- lint-workflows` against release.yml AND the two
// follow-up workflows so workflow drift is caught at PR time (not at the
// next tagged release).
//
// Strategy C — real local resources (DWD-01):
//   - Real workspace files (`.github/workflows/{ci,release}.yml`)
//   - Real `serde_yaml` parser asserts the structural invariants
//
// Two scenarios:
//   1. Flag-parity: the three CI gates (fmt, clippy, test) appear in BOTH
//      ci.yml and release.yml with byte-for-byte identical command lines.
//      This catches the silent-drift case where someone tweaks one file
//      and forgets the other (e.g., adds `--all-targets` to clippy in
//      ci.yml but not release.yml — now PRs go green but releases fail).
//   2. ci.yml runs `xtask lint-workflows` against release.yml +
//      release-pipeline-alert.yml + token-expiry-warning.yml so a workflow
//      that overshoots its line budget OR drops a `# Purpose:` comment is
//      caught at PR time, not at the next tag push.

use std::path::PathBuf;

use serde_yaml::Value;

/// Path to the modeltap workspace root (parent of this `tests/` crate).
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p
}

/// Path to a workflow file under `.github/workflows/`.
fn workflow_path(name: &str) -> PathBuf {
    let mut p = workspace_root();
    p.push(".github");
    p.push("workflows");
    p.push(name);
    p
}

/// Read a workflow file from disk. Panics with a clear diagnostic if missing
/// — that IS the RED state for this step.
fn read_workflow(name: &str) -> String {
    let path = workflow_path(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Parse a workflow YAML source as a generic `Value`.
fn parse_workflow(src: &str) -> Value {
    serde_yaml::from_str(src).expect("workflow YAML must parse")
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

/// Walk every step's `run:` field in every job in the workflow and return
/// the `run` strings concatenated by newline. Multi-line `run:` blocks are
/// preserved verbatim so substring searches over this haystack catch the
/// command line wherever it appears.
fn all_run_blocks(workflow: &Value) -> String {
    let jobs = get(workflow, "jobs");
    let mut out = String::new();
    for (_name, body) in jobs.as_mapping().expect("jobs must be mapping") {
        let steps = match get_opt(body, "steps").and_then(|v| v.as_sequence()) {
            Some(s) => s,
            None => continue,
        };
        for s in steps {
            if let Some(run) = get_opt(s, "run").and_then(|v| v.as_str()) {
                out.push_str(run);
                out.push('\n');
            }
        }
    }
    out
}

// =============================================================================
// INT.AC-6 scenario 1: Flag-parity for the three CI gates.
//
// The ci.yml is the canonical source of truth (per CLAUDE.md "CI Lint
// Discipline" — `cargo fmt --all -- --check` and
// `cargo clippy --workspace --all-targets -- -D warnings` run on every PR).
// release.yml MUST run those same commands BEFORE producing release artifacts
// (per `release.yml`'s "C3 / US-03" comment block: production builds NEVER
// run on unverified code).
//
// Flag parity is byte-for-byte: a single missing `--all-targets` would let a
// release ship clippy lints that PR builds would reject.
// =============================================================================

/// Each parity-gate substring we expect to find in BOTH workflows.
/// These are the exact command lines (modulo surrounding whitespace) used by
/// ci.yml today; release.yml MUST match them verbatim.
const PARITY_GATES: &[&str] = &[
    "cargo fmt --all -- --check",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo test --workspace --locked",
];

#[test]
fn ci_parity_gates_appear_in_both_ci_and_release_with_identical_flags() {
    let ci_src = read_workflow("ci.yml");
    let release_src = read_workflow("release.yml");

    let ci = parse_workflow(&ci_src);
    let release = parse_workflow(&release_src);

    let ci_run_blocks = all_run_blocks(&ci);
    let release_run_blocks = all_run_blocks(&release);

    for gate in PARITY_GATES {
        assert!(
            ci_run_blocks.contains(gate),
            "ci.yml must invoke `{gate}` somewhere in its job steps; \
             got run blocks:\n{ci_run_blocks}"
        );
        assert!(
            release_run_blocks.contains(gate),
            "release.yml must invoke `{gate}` somewhere in its job steps \
             (CI parity per INT.AC-6 / US-03); got run blocks:\n{release_run_blocks}"
        );
    }
}

#[test]
fn no_parity_gate_command_differs_between_ci_and_release() {
    // Defensive negative assertion: catch the silent-drift case where someone
    // tweaks a flag in ONE file. We extract every line from both workflows'
    // `run:` blocks that LOOKS like a parity-gate command (starts with
    // `cargo fmt`, `cargo clippy`, or `cargo test`) and assert each prefix
    // appears in BOTH files with the same text.
    let ci_src = read_workflow("ci.yml");
    let release_src = read_workflow("release.yml");
    let ci = parse_workflow(&ci_src);
    let release = parse_workflow(&release_src);

    let ci_runs = all_run_blocks(&ci);
    let release_runs = all_run_blocks(&release);

    let extract_lines = |runs: &str, prefix: &str| -> Vec<String> {
        runs.lines()
            .map(str::trim)
            .filter(|l| l.starts_with(prefix))
            .map(str::to_owned)
            .collect()
    };

    for prefix in ["cargo fmt", "cargo clippy", "cargo test"] {
        let ci_lines = extract_lines(&ci_runs, prefix);
        let release_lines = extract_lines(&release_runs, prefix);

        assert!(
            !ci_lines.is_empty(),
            "ci.yml must invoke `{prefix}` at least once"
        );
        assert!(
            !release_lines.is_empty(),
            "release.yml must invoke `{prefix}` at least once (US-03 CI parity)"
        );

        // Every ci.yml `prefix` invocation must have a byte-equal counterpart
        // in release.yml (and vice versa). This catches silent flag drift.
        for line in &ci_lines {
            assert!(
                release_lines.iter().any(|r| r == line),
                "ci.yml invokes `{line}` but release.yml has no matching \
                 invocation. release.yml `{prefix}` invocations were: \
                 {release_lines:?}. INT.AC-6 (flag-parity) violated."
            );
        }
        for line in &release_lines {
            assert!(
                ci_lines.iter().any(|c| c == line),
                "release.yml invokes `{line}` but ci.yml has no matching \
                 invocation. ci.yml `{prefix}` invocations were: {ci_lines:?}. \
                 INT.AC-6 (flag-parity) violated."
            );
        }
    }
}

// =============================================================================
// Step 03-05 acceptance criterion: ci.yml runs `xtask lint-workflows` against
// release.yml + the two follow-up workflows so workflow drift (line-budget
// overshoot, missing `# Purpose:` comment) is caught at PR time, not at the
// next tagged release.
// =============================================================================

#[test]
fn ci_yml_runs_xtask_lint_workflows_against_release_yml() {
    let ci_src = read_workflow("ci.yml");
    assert!(
        ci_src.contains("lint-workflows"),
        "ci.yml must run `cargo run -p xtask -- lint-workflows` against the \
         release workflows so workflow drift is caught at PR time. ci.yml \
         did not contain the substring `lint-workflows`."
    );
    assert!(
        ci_src.contains(".github/workflows/release.yml"),
        "ci.yml must lint `.github/workflows/release.yml` specifically (the \
         primary release workflow). ci.yml did not reference release.yml \
         in its lint-workflows step."
    );
}

#[test]
fn ci_yml_lints_both_follow_up_workflows() {
    let ci_src = read_workflow("ci.yml");

    assert!(
        ci_src.contains(".github/workflows/release-pipeline-alert.yml"),
        "ci.yml must also lint `.github/workflows/release-pipeline-alert.yml` \
         (DEVOPS handoff #1; tested in follow_up_workflows.rs against the \
         deployed workflow but ci.yml is the gate that catches drift before \
         a tag push). ci.yml did not reference release-pipeline-alert.yml."
    );
    assert!(
        ci_src.contains(".github/workflows/token-expiry-warning.yml"),
        "ci.yml must also lint `.github/workflows/token-expiry-warning.yml` \
         (DEVOPS handoff #3). ci.yml did not reference token-expiry-warning.yml."
    );
}

#[test]
fn ci_yml_lint_workflows_step_uses_explicit_max_lines_flag() {
    // The lint-workflows subcommand requires an explicit --max-lines flag
    // (no default — see xtask/src/main.rs). ci.yml MUST pass it for every
    // workflow it lints; otherwise the cargo invocation fails with a clap
    // "required" error and CI goes red for the wrong reason.
    let ci_src = read_workflow("ci.yml");
    assert!(
        ci_src.contains("--max-lines"),
        "ci.yml's lint-workflows step must pass `--max-lines <N>` for every \
         workflow it lints (the xtask CLI has no default for this flag). \
         ci.yml did not contain `--max-lines`."
    );
}
