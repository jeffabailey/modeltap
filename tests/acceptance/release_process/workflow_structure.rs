// Acceptance test for the structural integrity of `.github/workflows/release.yml`.
//
// Step: 01-07 (Walking Skeleton — first workflow YAML file).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/walking-skeleton.feature
//                   ("Build orchestration runs formatting, linting, and tests
//                   before packaging" — US-03/US-04;
//                   "Single-target archive is produced and named correctly"
//                   — US-04).
// Architectural anchors:
//   - ADR-010 (single-workflow multi-job DAG)
//   - architecture-design.md §8.1 (atomicity model)
//   - hands-off-automation.feature US-14 (≤250 lines + per-job `# Purpose:`).
//
// This test parses the SHIPPED `.github/workflows/release.yml` file (NOT a
// synthetic fixture) and asserts the architectural invariants the workflow
// MUST satisfy for the walking skeleton to be valid:
//
//   I-1. `on.push.tags` includes the pattern `v*.*.*`.
//   I-2. The `validate-tag` job exists and runs on `ubuntu-latest`.
//   I-3. The `build` job declares `needs: validate-tag` (string or list).
//   I-4. The `publish-github-release` job declares `needs:` containing BOTH
//        `validate-tag` and `build`.
//   I-5. The CI parity gates inside `build` run in this order BEFORE any
//        `cargo build --release` step:
//             cargo fmt --check  <  cargo clippy  <  cargo test  <  cargo build --release
//        (per C3 / US-03: production builds NEVER run on unverified code).
//   I-6. Every top-level job has a `# Purpose:` comment IMMEDIATELY above its
//        declaration line (per US-14, enforced by `xtask lint-workflows`).
//   I-7. The file passes `cargo xtask lint-workflows --workflow ... --max-lines 300`.
//        (Budget bumped 250 → 300 in step 02-03 to accommodate the SLSA L3
//        permissions block + per-cell attest-build-provenance step. The next
//        bump requires a refactor — the workflow is already lean: matrix
//        `if:` gating collapses cross-only steps and per-target packaging is
//        a single multiline `run:`.)
//
// Strategy C — real local resources (DWD-01): the test reads the real
// release.yml from the workspace, parses it with serde_yaml, and shells out
// to the real `xtask lint-workflows` subcommand for invariant 7.
//
// The end-to-end live `gh release create` step is NOT tested here — that
// requires an authenticated GitHub runner and is deferred to a separate
// `@requires_external` smoke test (per the walking-skeleton.feature scenario
// tagged `@requires_external @cross-repo`).

use std::path::PathBuf;
use std::process::Command;

use assert_cmd::prelude::OutputAssertExt;
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

/// Parse the release.yml as a generic YAML `Value` so the test can navigate
/// arbitrary mapping keys without committing to a typed schema.
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

/// Coerce a YAML scalar/sequence into a `Vec<String>`. A bare string becomes
/// a one-element vector; a sequence becomes its scalar contents. Other shapes
/// panic with a diagnostic.
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
// I-1. Trigger: on.push.tags includes 'v*.*.*'
// =============================================================================

#[test]
fn release_workflow_triggers_on_semver_tag_push() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);

    // YAML quirk: `on:` is parsed as the boolean `true` by some YAML 1.1
    // implementations. `serde_yaml` (YAML 1.2) keeps it as the string `"on"`,
    // so a direct `get("on")` works. We assert defensively below.
    let on = get_opt(&workflow, "on")
        .or_else(|| get_opt(&workflow, "true"))
        .expect("workflow must have an `on:` trigger block");
    let push = get(on, "push");
    let tags = get(push, "tags");
    let patterns = as_string_list(tags);

    assert!(
        patterns.iter().any(|p| p == "v*.*.*"),
        "release.yml must trigger on tag pattern 'v*.*.*' \
         (per ADR-010 + walking-skeleton.feature). Got: {patterns:?}"
    );
}

// =============================================================================
// I-2 + I-3. validate-tag job exists; build job needs validate-tag
// =============================================================================

#[test]
fn build_job_depends_on_validate_tag() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");

    let validate_tag = get_opt(jobs, "validate-tag")
        .expect("release.yml must declare a `validate-tag` job (ADR-010)");
    let runs_on = get(validate_tag, "runs-on")
        .as_str()
        .expect("validate-tag.runs-on must be a string");
    assert_eq!(
        runs_on, "ubuntu-latest",
        "validate-tag must run on ubuntu-latest (cheapest fail-fast gate)"
    );

    let build = get_opt(jobs, "build").expect("release.yml must declare a `build` job");
    let needs = get(build, "needs");
    let needs_list = as_string_list(needs);
    assert!(
        needs_list.iter().any(|n| n == "validate-tag"),
        "build.needs must include 'validate-tag' (atomic-publish DAG, ADR-010). \
         Got: {needs_list:?}"
    );
}

// =============================================================================
// I-4. publish-github-release job needs both validate-tag AND build
// =============================================================================

#[test]
fn publish_job_depends_on_validate_tag_and_build() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");

    let publish = get_opt(jobs, "publish-github-release")
        .expect("release.yml must declare a `publish-github-release` job (ADR-010 atomic publish)");
    let needs = get(publish, "needs");
    let needs_list = as_string_list(needs);

    assert!(
        needs_list.iter().any(|n| n == "validate-tag"),
        "publish-github-release.needs must include 'validate-tag'. Got: {needs_list:?}"
    );
    assert!(
        needs_list.iter().any(|n| n == "build"),
        "publish-github-release.needs must include 'build'. Got: {needs_list:?}"
    );
}

// =============================================================================
// I-5. CI parity gates run BEFORE cargo build --release
//      Order: fmt --check < clippy < test < build --release
// =============================================================================

#[test]
fn build_job_runs_ci_parity_gates_before_release_build() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");
    let build = get_opt(jobs, "build").expect("`build` job must exist");
    let steps = get(build, "steps")
        .as_sequence()
        .expect("build.steps must be a sequence");

    // For each step, derive a single string we can substring-search. Steps
    // can carry their command in either `run:` (inline) or in a composite
    // `name:` field; we concatenate both when present so the assertion is
    // robust to either style.
    let step_strs: Vec<String> = steps
        .iter()
        .map(|s| {
            let name = get_opt(s, "name").and_then(|v| v.as_str()).unwrap_or("");
            let run = get_opt(s, "run").and_then(|v| v.as_str()).unwrap_or("");
            format!("{name} || {run}")
        })
        .collect();

    let idx_of = |needle: &str| -> usize {
        step_strs
            .iter()
            .position(|s| s.contains(needle))
            .unwrap_or_else(|| {
                panic!(
                    "expected a build step containing {needle:?}; build steps were:\n{}",
                    step_strs.join("\n")
                )
            })
    };

    let fmt_idx = idx_of("cargo fmt");
    let clippy_idx = idx_of("cargo clippy");
    let test_idx = idx_of("cargo test");
    let release_build_idx = idx_of("cargo build --release");

    assert!(
        fmt_idx < clippy_idx,
        "cargo fmt --check (idx {fmt_idx}) must precede cargo clippy (idx {clippy_idx}) \
         (C3 CI parity)"
    );
    assert!(
        clippy_idx < test_idx,
        "cargo clippy (idx {clippy_idx}) must precede cargo test (idx {test_idx}) \
         (C3 CI parity)"
    );
    assert!(
        test_idx < release_build_idx,
        "cargo test (idx {test_idx}) must precede cargo build --release \
         (idx {release_build_idx}) — production builds NEVER run on unverified code (C3, US-03)"
    );
}

// =============================================================================
// I-6. Every job has a `# Purpose:` comment IMMEDIATELY above its declaration
//      (US-14 maintainer-legibility convention; same rule xtask lint-workflows
//      enforces, asserted here as well so this test catches the violation
//      directly without needing to interpret the linter's stderr).
// =============================================================================

#[test]
fn every_job_has_purpose_comment_immediately_above_declaration() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");
    let job_names: Vec<String> = jobs
        .as_mapping()
        .expect("jobs must be a mapping")
        .keys()
        .map(|k| k.as_str().expect("job keys must be strings").to_owned())
        .collect();

    let lines: Vec<&str> = src.lines().collect();
    let mut missing = Vec::new();

    for name in &job_names {
        // Same locator rule xtask::lint uses: `^  <name>:` (two-space indent).
        let needle = format!("  {name}:");
        let decl_idx = lines
            .iter()
            .position(|l| {
                l.starts_with(&needle)
                    && l[needle.len()..]
                        .chars()
                        .all(|c| c.is_whitespace() || c == '\r')
            })
            .unwrap_or_else(|| panic!("job {name:?} not found in source at `^  {name}:`"));

        if decl_idx == 0 {
            missing.push(name.clone());
            continue;
        }
        let prev = lines[decl_idx - 1].trim_start();
        if !(prev.starts_with("# Purpose:") || prev.starts_with("#Purpose:")) {
            missing.push(name.clone());
        }
    }

    assert!(
        missing.is_empty(),
        "every top-level job must be preceded by a `# Purpose:` comment line \
         (US-14). Missing: {missing:?}"
    );
}

// =============================================================================
// I-6b. Matrix expansion (step 02-01) MUST NOT regress the build job's
//       atomic-publish wiring: build still depends on validate-tag, runs-on
//       still resolves correctly under matrix.runs-on, and the job-level
//       `runs-on:` is not hardcoded to a single platform.
//       This is a regression guard — the matrix-shape assertions live in
//       build_matrix.rs; this test catches a multi-arch-aware DAG break.
// =============================================================================

#[test]
fn build_job_runs_on_resolves_through_matrix() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");
    let build = get_opt(jobs, "build").expect("`build` job must exist");

    let runs_on = get(build, "runs-on")
        .as_str()
        .expect("build.runs-on must be a string");

    // After multi-arch expansion (step 02-01), build.runs-on MUST template
    // through the matrix; any hardcoded `ubuntu-latest` would silently force
    // every cell onto a single runner and defeat the matrix.
    assert!(
        runs_on.contains("matrix.runs-on"),
        "build.runs-on must reference `matrix.runs-on` after step 02-01 \
         (US-07 multi-arch matrix). Hardcoded runner found: {runs_on:?}"
    );
}

// =============================================================================
// I-7. The shipped release.yml passes xtask lint-workflows --max-lines 300.
//      Treats lint-workflows as the canonical enforcement for both line budget
//      and per-job purpose comments (defence in depth alongside I-6).
//      Budget bumped 250 → 300 in step 02-03 (SLSA L3 wiring per ADR-013).
// =============================================================================

#[test]
fn release_workflow_passes_xtask_lint_workflows() {
    // Skip if the file does not exist yet — the I-1..I-6 tests will fail with
    // clearer diagnostics. We use the read helper here so a missing file still
    // panics with the standard message.
    let _ = read_release_workflow();

    let mut workspace_manifest = workspace_root();
    workspace_manifest.push("Cargo.toml");

    let mut cmd = Command::new(env!("CARGO"));
    cmd.arg("run")
        .arg("--manifest-path")
        .arg(&workspace_manifest)
        .arg("--package")
        .arg("xtask")
        .arg("--quiet")
        .arg("--")
        .arg("lint-workflows")
        .arg("--workflow")
        .arg(".github/workflows/release.yml")
        .arg("--max-lines")
        .arg("300")
        .current_dir(workspace_root());

    let output = cmd.output().expect("invoke cargo xtask lint-workflows");
    output.assert().success();
}
