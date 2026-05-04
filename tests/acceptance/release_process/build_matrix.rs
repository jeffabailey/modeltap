// Acceptance tests for the multi-arch build matrix in
// `.github/workflows/release.yml`.
//
// Step: 02-01 (Phase 2 — multi-arch + integrity, first step).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/multi-arch-release.feature
//                     ("Build matrix declares all four supported targets with
//                     correct runners" — US-07)
//                     ("Each build cell uploads a workflow artifact named by
//                     target" — US-07).
// Architectural anchors:
//   - ADR-012 (cross-compile strategy: cross v0.2.5 for aarch64-linux)
//   - architecture-design.md §8.5 (cross-platform coverage)
//   - requirements.md K-PIPE ≥95% reliability
//
// This test parses the SHIPPED `.github/workflows/release.yml` (NOT a synthetic
// fixture) and asserts the matrix-shape invariants the multi-arch build job
// MUST satisfy:
//
//   M-1. The `build` job declares `strategy.matrix.include` with exactly 4
//        entries whose `target` values are the 4 supported triples:
//          aarch64-apple-darwin, x86_64-apple-darwin,
//          x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu
//   M-2. Per-target runner assignment matches ADR-012:
//          aarch64-apple-darwin    → macos-14
//          x86_64-apple-darwin     → macos-13
//          x86_64-unknown-linux-gnu → ubuntu-22.04
//          aarch64-unknown-linux-gnu → ubuntu-22.04
//   M-3. The matrix declares `fail-fast: false` (one cell crash must not abort
//        the others — observed-failure cascade is a publish-blocker, not a
//        build-aborter).
//   M-4. The aarch64-unknown-linux-gnu cell installs `cross` pinned at version
//        0.2.5 with `--locked`, and the build step uses `cross build` rather
//        than `cargo build` for that target only (per ADR-012).
//   M-5. The upload-artifact step names artifacts exactly `release-<target>`
//        (templated via `${{ matrix.target }}`), so downstream jobs can fan-in
//        all 4 archives + sidecars by `pattern: release-*`.
//
// Strategy C — real local resources (DWD-01): the test reads the real
// release.yml from the workspace and parses it with serde_yaml. The actual
// cross-compile run (Docker-required) is deferred to a `@requires_docker`
// scenario in multi-arch-release.feature; THIS test is structural.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_yaml::Value;

/// Path to the modeltap workspace root (parent of this `tests/` crate).
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

/// Look up `m[key]` where `m` is expected to be a mapping. Returns a clear
/// diagnostic if the key is missing or `m` is not a mapping.
fn get<'a>(m: &'a Value, key: &str) -> &'a Value {
    let mapping = m
        .as_mapping()
        .unwrap_or_else(|| panic!("expected mapping when looking up {key:?}, got {m:?}"));
    mapping
        .get(Value::String(key.to_owned()))
        .unwrap_or_else(|| panic!("expected key {key:?} in mapping {mapping:?}"))
}

/// Optional variant of `get` — returns `None` if the key is absent.
fn get_opt<'a>(m: &'a Value, key: &str) -> Option<&'a Value> {
    m.as_mapping()
        .and_then(|mapping| mapping.get(Value::String(key.to_owned())))
}

/// Fetch the `build` job mapping from the parsed workflow.
fn build_job(workflow: &Value) -> &Value {
    let jobs = get(workflow, "jobs");
    get_opt(jobs, "build").expect("release.yml must declare a `build` job")
}

/// Fetch the `strategy.matrix.include` sequence from the build job.
/// Diagnostic-rich panic if any layer is missing.
fn matrix_include(build: &Value) -> &Vec<Value> {
    let strategy = get_opt(build, "strategy")
        .expect("build job must declare `strategy:` (required for multi-arch matrix, US-07)");
    let matrix = get_opt(strategy, "matrix").expect("build.strategy must declare `matrix:`");
    let include = get_opt(matrix, "include")
        .expect("build.strategy.matrix must declare `include:` for per-target attributes");
    include
        .as_sequence()
        .expect("build.strategy.matrix.include must be a sequence")
}

// =============================================================================
// M-1. Matrix declares exactly 4 targets — the supported set.
// =============================================================================

#[test]
fn build_matrix_declares_four_supported_targets() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);
    let include = matrix_include(build);

    let targets: BTreeSet<String> = include
        .iter()
        .map(|entry| {
            get_opt(entry, "target")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    panic!("matrix.include entry missing `target:` field: {entry:?}")
                })
        })
        .collect();

    let expected: BTreeSet<String> = [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();

    assert_eq!(
        targets, expected,
        "build matrix targets must be exactly the 4 supported triples \
         (ADR-012 + multi-arch-release.feature US-07). Got: {targets:?}"
    );
    assert_eq!(
        include.len(),
        4,
        "matrix.include must have exactly 4 entries (no duplicates, no extras). \
         Got {} entries.",
        include.len()
    );
}

// =============================================================================
// M-2. Per-target runner assignment matches ADR-012.
// =============================================================================

#[test]
fn build_matrix_assigns_correct_runner_per_target() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);
    let include = matrix_include(build);

    // Build a map: target -> runs-on, asserting both keys are present per entry.
    let mut runner_for: std::collections::BTreeMap<String, String> = Default::default();
    for entry in include {
        let target = get_opt(entry, "target")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("matrix.include entry missing `target:`: {entry:?}"))
            .to_owned();
        let runs_on = get_opt(entry, "runs-on")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "matrix.include entry for target {target:?} missing `runs-on:` \
                     (per-target runner is required so the job-level `runs-on: \
                     ${{{{ matrix.runs-on }}}}` resolves correctly)"
                )
            })
            .to_owned();
        runner_for.insert(target, runs_on);
    }

    let expected = [
        ("aarch64-apple-darwin", "macos-14"),
        ("x86_64-apple-darwin", "macos-13"),
        ("x86_64-unknown-linux-gnu", "ubuntu-22.04"),
        ("aarch64-unknown-linux-gnu", "ubuntu-22.04"),
    ];

    for (target, expected_runner) in expected {
        let actual = runner_for
            .get(target)
            .unwrap_or_else(|| panic!("matrix has no entry for target {target:?}"));
        assert_eq!(
            actual, expected_runner,
            "target {target:?} must run on {expected_runner:?} (per ADR-012); \
             got {actual:?}"
        );
    }

    // Defensive: the job-level `runs-on:` must template through the matrix
    // attribute, otherwise the per-target runner table is dead config.
    let runs_on = get_opt(build, "runs-on")
        .and_then(|v| v.as_str())
        .expect("build.runs-on must be a string templated from matrix.runs-on");
    assert!(
        runs_on.contains("matrix.runs-on"),
        "build.runs-on must reference `matrix.runs-on` so per-target runners apply. \
         Got: {runs_on:?}"
    );
}

// =============================================================================
// M-3. Matrix declares `fail-fast: false`.
// =============================================================================

#[test]
fn build_matrix_disables_fail_fast() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);
    let strategy = get_opt(build, "strategy").expect("build.strategy must exist");
    let fail_fast = get_opt(strategy, "fail-fast")
        .expect("build.strategy must declare `fail-fast:` (multi-arch matrix policy)");
    let value = fail_fast
        .as_bool()
        .expect("build.strategy.fail-fast must be a boolean");
    assert!(
        !value,
        "build.strategy.fail-fast must be false — one failing cell must not \
         abort the other 3 (the atomic-publish guard at the publish job is what \
         enforces all-or-nothing release, NOT fail-fast at build time, US-08)"
    );
}

// =============================================================================
// M-4. aarch64-unknown-linux-gnu cell installs `cross` v0.2.5 (--locked) and
//      uses `cross build` rather than `cargo build` (per ADR-012). The other
//      3 cells must NOT install cross (cost discipline).
// =============================================================================

#[test]
fn aarch64_linux_cell_uses_cross_pinned_at_0_2_5() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);

    // Concatenate `name` + `run` + `if` of every step into a single searchable
    // string so the assertion is robust to step-style variations.
    let steps = get(build, "steps")
        .as_sequence()
        .expect("build.steps must be a sequence");
    let step_strs: Vec<String> = steps
        .iter()
        .map(|s| {
            let name = get_opt(s, "name").and_then(|v| v.as_str()).unwrap_or("");
            let run = get_opt(s, "run").and_then(|v| v.as_str()).unwrap_or("");
            let cond = get_opt(s, "if").and_then(|v| v.as_str()).unwrap_or("");
            format!("name={name} || if={cond} || run={run}")
        })
        .collect();
    let all_steps = step_strs.join("\n");

    // Cross install step: must be pinned at exactly 0.2.5 with --locked, and
    // gated on `matrix.use-cross` or `matrix.target == 'aarch64-unknown-linux-gnu'`.
    assert!(
        all_steps.contains("cargo install")
            && all_steps.contains("cross")
            && all_steps.contains("0.2.5")
            && all_steps.contains("--locked"),
        "build job must install `cross` pinned at version 0.2.5 with --locked \
         (ADR-012 reproducibility). Steps:\n{all_steps}"
    );

    // The cross install must be conditional — we MUST NOT install cross on the
    // 3 native targets (cost + complexity discipline).
    let install_step = steps
        .iter()
        .find(|s| {
            let run = get_opt(s, "run").and_then(|v| v.as_str()).unwrap_or("");
            run.contains("cargo install") && run.contains("cross") && run.contains("0.2.5")
        })
        .expect("expected a step running `cargo install --locked --version 0.2.5 cross`");
    let install_cond = get_opt(install_step, "if").and_then(|v| v.as_str()).expect(
        "the cross-install step MUST be conditional (`if:`) so it only runs \
             on the aarch64-unknown-linux-gnu cell; otherwise the 3 native cells \
             needlessly compile cross every release (~30s waste each)",
    );
    assert!(
        install_cond.contains("use-cross") || install_cond.contains("aarch64-unknown-linux-gnu"),
        "cross-install step `if:` must gate on `matrix.use-cross` (preferred) \
         or `matrix.target == 'aarch64-unknown-linux-gnu'`. Got: {install_cond:?}"
    );

    // The actual cross-build invocation must use `cross build` for the
    // aarch64-linux cell. We accept either a dedicated `cross build` step or
    // a single `cargo`-OR-`cross` step that switches on the matrix attribute,
    // so we look for the literal command `cross build`.
    assert!(
        all_steps.contains("cross build"),
        "build job must invoke `cross build` for the aarch64-unknown-linux-gnu \
         target (ADR-012). Steps:\n{all_steps}"
    );
}

// =============================================================================
// M-5. Each cell uploads a workflow artifact named exactly `release-<target>`.
// =============================================================================

#[test]
fn upload_artifact_step_uses_per_target_naming() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);

    let steps = get(build, "steps")
        .as_sequence()
        .expect("build.steps must be a sequence");

    let upload = steps
        .iter()
        .find(|s| {
            get_opt(s, "uses")
                .and_then(|v| v.as_str())
                .is_some_and(|u| u.starts_with("actions/upload-artifact@"))
        })
        .expect("build job must include an actions/upload-artifact step");

    let with = get_opt(upload, "with").expect("upload-artifact step must declare `with:`");
    let name = get_opt(with, "name")
        .and_then(|v| v.as_str())
        .expect("upload-artifact must declare `with.name`");

    // Templated name MUST resolve to `release-<target>` per matrix cell.
    // Accept either `release-${{ matrix.target }}` or
    // `release-${{ matrix.target }}` with surrounding whitespace tolerance.
    let normalised = name.replace(' ', "");
    assert!(
        normalised.contains("release-${{matrix.target}}"),
        "upload-artifact name must template per matrix.target as \
         `release-${{{{ matrix.target }}}}` so downstream jobs fan-in via \
         `pattern: release-*`. Got: {name:?}"
    );

    // Defensive: artifact must contain BOTH the archive AND the sidecar (per
    // multi-arch-release.feature: "each artifact contains an archive plus a
    // sha256 sidecar").
    let path = get_opt(with, "path").expect("upload-artifact must declare `with.path`");
    let path_str = match path {
        Value::String(s) => s.clone(),
        Value::Sequence(seq) => seq
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("upload-artifact path must be string or sequence, got {other:?}"),
    };
    assert!(
        path_str.contains("tar.gz"),
        "upload-artifact path must include the .tar.gz archive. Got: {path_str:?}"
    );
    assert!(
        path_str.contains("sha256"),
        "upload-artifact path must include the .sha256 sidecar. Got: {path_str:?}"
    );
}
