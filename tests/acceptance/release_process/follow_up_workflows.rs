// Acceptance tests for the two follow-up workflows that close DEVOPS handoff
// items #1 (release-pipeline-alert) and #3 (token-expiry-warning).
//
// Step: 03-04 (DELIVER wave, US-14 follow-up workflows).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/integration-checkpoints.feature
//                   (the 5 scenarios tagged `@follow-up-workflow`, added
//                   2026-05-03 to close roadmap reviewer blocker D1).
// Design specs: docs/feature/release-process-homebrew-github/devops/
//               monitoring-alerting.md §1 + §2,
//               ci-cd-pipeline.md §4 + §5.
//
// Strategy C — real local resources (DWD-01):
//   - Real workspace files (`.github/workflows/release-pipeline-alert.yml`
//     and `.github/workflows/token-expiry-warning.yml`)
//   - Real `serde_yaml` parser asserts the structural invariants
//   - Real `cargo xtask lint-workflows` subprocess for the line-budget +
//     per-job purpose-comment check
//
// The five Gherkin scenarios in integration-checkpoints.feature map to test
// functions below 1:1:
//
//   Gherkin                                                     Test fn
//   ──────────────────────────────────────────────────────────  ────────────
//   Release-pipeline-alert opens issue on workflow failure   →  alert_*
//   Release-pipeline-alert stays silent on workflow success  →  alert_silent_*
//   Token-expiry-warning opens issue when GH_TAP_TOKEN ...    →  token_*
//   Token-expiry-warning stays silent when token is healthy   →  token_silent_*
//   Both follow-up workflows pass xtask lint-workflows        →  lint_*
//
// We do NOT spin up GitHub Actions — that would require a live runner. Instead
// we assert the structural shape of each workflow file (trigger, permissions,
// job filter, the issue-creation step's title/labels) so a future edit that
// breaks the alerting path fails this test deterministically. The end-to-end
// "deliberate failure produces an issue" self-test is a manual procedure
// documented in monitoring-alerting.md §1.6 (RELEASING.md First-time setup).

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

/// Parse a workflow YAML source as a generic `Value` so the assertions can
/// navigate arbitrary mapping keys without committing to a typed schema.
fn parse_workflow(src: &str) -> Value {
    serde_yaml::from_str(src).expect("workflow YAML must parse")
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

/// Optional variant of `get`. Returns `None` if the key is absent OR if the
/// underlying value is not a mapping.
fn get_opt<'a>(m: &'a Value, key: &str) -> Option<&'a Value> {
    m.as_mapping()
        .and_then(|mapping| mapping.get(Value::String(key.to_owned())))
}

/// Resolve a workflow's `on:` block. Tolerates the YAML 1.1 quirk where bare
/// `on:` parses as the boolean `true`.
fn on_block(workflow: &Value) -> &Value {
    get_opt(workflow, "on")
        .or_else(|| get_opt(workflow, "true"))
        .expect("workflow must have an `on:` trigger block")
}

/// Coerce a YAML scalar/sequence into `Vec<String>`.
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

/// Concatenate every step's `name:` and `run:` field into one searchable
/// string per step. Matches the technique used in workflow_structure.rs so
/// substring searches survive either inline-`run` or composite-`name` styles.
fn step_strings(job: &Value) -> Vec<String> {
    let steps = get(job, "steps")
        .as_sequence()
        .expect("job.steps must be a sequence");
    steps
        .iter()
        .map(|s| {
            let name = get_opt(s, "name").and_then(|v| v.as_str()).unwrap_or("");
            let run = get_opt(s, "run").and_then(|v| v.as_str()).unwrap_or("");
            format!("{name} || {run}")
        })
        .collect()
}

const ALERT_WF: &str = "release-pipeline-alert.yml";
const TOKEN_WF: &str = "token-expiry-warning.yml";

// =============================================================================
// Scenario 1: Release-pipeline-alert opens issue on workflow failure
//   - Triggers on workflow_run completion of "Release"
//   - Has a job gated to non-success conclusions
//   - Issue title contains "release-pipeline-failure" trigger phrase / label
//   - Body links the failed run; label is "release-pipeline-failure"
// =============================================================================

#[test]
fn alert_workflow_triggers_on_release_workflow_run_completed() {
    let src = read_workflow(ALERT_WF);
    let workflow = parse_workflow(&src);
    let on = on_block(&workflow);
    let workflow_run = get(on, "workflow_run");

    let workflows = as_string_list(get(workflow_run, "workflows"));
    assert!(
        workflows.iter().any(|w| w.eq_ignore_ascii_case("release")),
        "release-pipeline-alert.yml must trigger on workflow_run for the \
         `release` workflow (DEVOPS monitoring-alerting.md §1.2). Got: {workflows:?}"
    );

    let types = as_string_list(get(workflow_run, "types"));
    assert!(
        types.iter().any(|t| t == "completed"),
        "release-pipeline-alert.yml must subscribe to `completed` workflow_run \
         events. Got: {types:?}"
    );
}

#[test]
fn alert_workflow_grants_least_privilege_for_issue_creation() {
    let src = read_workflow(ALERT_WF);
    let workflow = parse_workflow(&src);
    let perms = get_opt(&workflow, "permissions")
        .expect("release-pipeline-alert.yml must declare a top-level `permissions:` block");

    let issues = get(perms, "issues")
        .as_str()
        .expect("permissions.issues must be a string");
    assert_eq!(
        issues, "write",
        "release-pipeline-alert needs `issues: write` to file the failure issue \
         (monitoring-alerting.md §1.2)"
    );

    let contents = get(perms, "contents")
        .as_str()
        .expect("permissions.contents must be a string");
    assert_eq!(
        contents, "read",
        "release-pipeline-alert.yml should request only `contents: read` \
         (least privilege; monitoring-alerting.md §1.2)"
    );
}

#[test]
fn alert_workflow_filters_to_non_success_conclusions() {
    let src = read_workflow(ALERT_WF);
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");

    let mut filter_seen = false;
    for (_name, body) in jobs.as_mapping().expect("jobs must be mapping") {
        let if_clause = get_opt(body, "if").and_then(|v| v.as_str()).unwrap_or("");
        if if_clause.contains("workflow_run.conclusion") && if_clause.contains("success") {
            filter_seen = true;
            break;
        }
    }
    assert!(
        filter_seen,
        "release-pipeline-alert.yml must gate at least one job on \
         `github.event.workflow_run.conclusion != 'success'` so successful \
         runs do NOT open an issue (monitoring-alerting.md §1.2)"
    );
}

#[test]
fn alert_workflow_creates_issue_with_failure_label_and_run_link() {
    let src = read_workflow(ALERT_WF);
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");

    // Concatenate every step's searchable text across every job, then assert
    // the issue-creation step's contract: label is `release-pipeline-failure`
    // and the body or env carries a link to the failed run.
    let mut all_step_text = String::new();
    for (_name, body) in jobs.as_mapping().expect("jobs must be mapping") {
        for s in step_strings(body) {
            all_step_text.push_str(&s);
            all_step_text.push('\n');
        }
    }

    assert!(
        all_step_text.contains("release-pipeline-failure"),
        "the issue-creation step must apply the `release-pipeline-failure` \
         label (monitoring-alerting.md §1.2 / ci-cd-pipeline.md §4.2). \
         Step text was:\n{all_step_text}"
    );

    // The run link arrives via `workflow_run.html_url` (env var or templated
    // into the body). The exact YAML shape can vary — env block vs in-body
    // template — so assert on the source text instead.
    assert!(
        src.contains("workflow_run.html_url") || src.contains("RUN_URL"),
        "release-pipeline-alert.yml must surface the failed run's URL \
         (workflow_run.html_url / RUN_URL env var) in the issue. Source did \
         not reference either."
    );
}

// =============================================================================
// Scenario 2: Release-pipeline-alert stays silent on workflow success
//   - Same `if:` filter, asserted positively above. We add a defensive
//     assertion here that NO job runs unconditionally (no `if: always()`
//     and no missing-`if` job in the workflow).
// =============================================================================

#[test]
fn alert_workflow_has_no_unconditional_job_that_would_fire_on_success() {
    let src = read_workflow(ALERT_WF);
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");

    for (name, body) in jobs.as_mapping().expect("jobs must be mapping") {
        let job_name = name.as_str().expect("job name must be string");
        let if_clause = get_opt(body, "if").and_then(|v| v.as_str()).unwrap_or("");
        // Either the job's `if:` filters non-success, or it does NOT run an
        // issue-creation step (no path to opening an issue on success).
        let filters_failure =
            if_clause.contains("workflow_run.conclusion") && if_clause.contains("success");
        if filters_failure {
            continue;
        }
        // If the job lacks the failure filter, it must not contain
        // `gh issue create` / `release-pipeline-failure` — otherwise it would
        // fire on every workflow_run, including success.
        let body_text = step_strings(body).join("\n");
        assert!(
            !body_text.contains("release-pipeline-failure"),
            "job {job_name:?} would open a failure issue on every workflow_run \
             (including success) because it lacks an `if:` filter on \
             workflow_run.conclusion. monitoring-alerting.md §1.2 requires the \
             gate."
        );
    }
}

// =============================================================================
// Scenario 3: Token-expiry-warning opens issue when GH_TAP_TOKEN is expired
//   - Triggers on schedule (weekly cron) + workflow_dispatch
//   - Uses `gh api` against the tap repo
//   - Authenticates via GH_TAP_TOKEN
//   - Opens an issue when the probe fails
// =============================================================================

#[test]
fn token_workflow_runs_on_weekly_schedule_and_workflow_dispatch() {
    let src = read_workflow(TOKEN_WF);
    let workflow = parse_workflow(&src);
    let on = on_block(&workflow);

    let schedule = get(on, "schedule")
        .as_sequence()
        .expect("on.schedule must be a sequence");
    assert!(
        !schedule.is_empty(),
        "token-expiry-warning.yml must declare at least one cron schedule \
         (monitoring-alerting.md §2.2)"
    );
    let mut weekly_seen = false;
    for entry in schedule {
        let cron = get(entry, "cron")
            .as_str()
            .expect("schedule[].cron must be a string");
        // Weekly cadence: 5-field cron ending in a single weekday (Mon=1).
        // We accept any cron whose 5th field is `1` (Monday) — the spec is
        // explicit Mondays-13:00-UTC but we don't lock the hour here.
        let fields: Vec<&str> = cron.split_whitespace().collect();
        if fields.len() == 5 && fields[4] == "1" {
            weekly_seen = true;
            break;
        }
    }
    assert!(
        weekly_seen,
        "token-expiry-warning.yml must run weekly on Mondays (cron field 5 = 1; \
         monitoring-alerting.md §2.2)"
    );

    // workflow_dispatch is the manual ad-hoc trigger.
    assert!(
        get_opt(on, "workflow_dispatch").is_some(),
        "token-expiry-warning.yml must also expose a workflow_dispatch trigger \
         for ad-hoc rotation testing (monitoring-alerting.md §2.4 step 3)"
    );
}

#[test]
fn token_workflow_probes_tap_repo_via_gh_api_with_tap_token() {
    let src = read_workflow(TOKEN_WF);

    assert!(
        src.contains("gh api") && src.contains("/repos/jeffabailey/homebrew-modeltap"),
        "token-expiry-warning.yml must probe the tap repo via \
         `gh api /repos/jeffabailey/homebrew-modeltap` (monitoring-alerting.md §2.2). \
         Source did not contain that probe."
    );
    assert!(
        src.contains("GH_TAP_TOKEN"),
        "token-expiry-warning.yml must authenticate via the GH_TAP_TOKEN secret \
         (monitoring-alerting.md §2.2)"
    );
}

#[test]
fn token_workflow_opens_issue_with_tap_token_expiry_label_on_probe_failure() {
    let src = read_workflow(TOKEN_WF);

    assert!(
        src.contains("tap-token-expiry"),
        "token-expiry-warning.yml must apply the `tap-token-expiry` label so \
         the alert is searchable + idempotent (monitoring-alerting.md §2.2 / \
         ci-cd-pipeline.md §5.4)"
    );
    assert!(
        src.contains("RELEASING.md") || src.contains("rotation"),
        "token-expiry-warning.yml issue body must reference the rotation \
         procedure (RELEASING.md / monitoring-alerting.md §2.4)"
    );
    // The integration scenario expects the issue title to contain "expired or
    // invalid"; the spec wording is "GH_TAP_TOKEN appears invalid or expired".
    // Both substrings (`invalid` + `expired`) prove the title is the rotation
    // alert and not some unrelated issue title.
    assert!(
        src.contains("expired") && src.contains("invalid"),
        "token-expiry-warning.yml issue title must indicate the token is \
         `expired or invalid` (per integration-checkpoints.feature scenario 3 + \
         monitoring-alerting.md §2.2)"
    );
}

#[test]
fn token_workflow_grants_least_privilege_for_issue_creation() {
    let src = read_workflow(TOKEN_WF);
    let workflow = parse_workflow(&src);
    let perms = get_opt(&workflow, "permissions")
        .expect("token-expiry-warning.yml must declare a top-level `permissions:` block");

    let issues = get(perms, "issues")
        .as_str()
        .expect("permissions.issues must be a string");
    assert_eq!(issues, "write");

    let contents = get(perms, "contents")
        .as_str()
        .expect("permissions.contents must be a string");
    assert_eq!(contents, "read");
}

// =============================================================================
// Scenario 4: Token-expiry-warning stays silent when token is healthy
//   - Probe step writes a status output; issue step gates on that output.
//   - We assert the issue-creation step has an `if:` filter that depends on
//     the probe's output (so a 200 response yields zero issue creations).
// =============================================================================

#[test]
fn token_workflow_gates_issue_creation_on_probe_status_output() {
    let src = read_workflow(TOKEN_WF);
    let workflow = parse_workflow(&src);
    let jobs = get(&workflow, "jobs");

    // Find the job that contains the probe + issue steps. There is only one
    // job in the spec (`check-tap-token`); we iterate defensively.
    let mut gated_seen = false;
    for (_name, body) in jobs.as_mapping().expect("jobs must be mapping") {
        let steps = get(body, "steps")
            .as_sequence()
            .expect("job.steps must be a sequence");
        for s in steps {
            let if_clause = get_opt(s, "if").and_then(|v| v.as_str()).unwrap_or("");
            // The spec writes `if: steps.probe.outputs.status == 'invalid'`.
            // Accept any reference to the probe step's outputs as the gate.
            if if_clause.contains("steps.probe.outputs") {
                gated_seen = true;
                break;
            }
        }
    }
    assert!(
        gated_seen,
        "token-expiry-warning.yml must gate the issue-creation step on the \
         probe step's outputs (e.g. `if: steps.probe.outputs.status == 'invalid'`) \
         so a healthy 200 response opens NO issue. Per integration-checkpoints \
         scenario `Token-expiry-warning stays silent when token is healthy`."
    );
}

// =============================================================================
// Scenario 5: Both follow-up workflows pass `xtask lint-workflows`
//   - Per-job `# Purpose:` comments on every top-level job
//   - Within the per-workflow line budget
// =============================================================================

#[test]
fn alert_workflow_passes_xtask_lint_workflows() {
    // Force the workflow file to be read first so a missing file panics with
    // the standard diagnostic, NOT the linter's parse error.
    let _ = read_workflow(ALERT_WF);

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
        .arg(format!(".github/workflows/{ALERT_WF}"))
        .arg("--max-lines")
        .arg("80")
        .current_dir(workspace_root());

    let output = cmd.output().expect("invoke cargo xtask lint-workflows");
    output.assert().success();
}

#[test]
fn token_workflow_passes_xtask_lint_workflows() {
    let _ = read_workflow(TOKEN_WF);

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
        .arg(format!(".github/workflows/{TOKEN_WF}"))
        .arg("--max-lines")
        .arg("100")
        .current_dir(workspace_root());

    let output = cmd.output().expect("invoke cargo xtask lint-workflows");
    output.assert().success();
}
