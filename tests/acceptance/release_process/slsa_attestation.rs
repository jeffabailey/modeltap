// Acceptance test for SLSA L3 build-provenance attestation in
// `.github/workflows/release.yml`.
//
// Step: 02-03 (Phase 2 — multi-arch + integrity, third step).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/multi-arch-release.feature
//                     - "Build job declares the OIDC permissions required for
//                        attestation"                              (@us-09)
//                     - "Each build cell invokes the attest-build-provenance
//                        action against its archive"               (@us-09)
//                     - "Devon verifies a published archive's attestation with
//                        one command"                  (@us-09 @requires_external)
// Architectural anchors:
//   - ADR-013 (SLSA L3 supply-chain integrity)
//   - US-09 (signed provenance per archive, verifiable with `gh attestation
//     verify`); KPI hook K-PROV in DEVOPS kpi-instrumentation.md.
//
// This test parses the SHIPPED `.github/workflows/release.yml` (NOT a synthetic
// fixture) and asserts the SLSA invariants the multi-arch build job MUST
// satisfy:
//
//   S-1. The `build` job declares a job-level `permissions:` block (least
//        privilege — workflow-level token minted only for `contents: write`
//        on the publish job) that includes BOTH:
//          id-token: write       (OIDC token minting for Sigstore)
//          attestations: write   (write provenance to repo's attestations API)
//          contents: read        (checkout the source for the action)
//   S-2. The build job declares a step using `actions/attest-build-provenance`
//        pinned to the major-version tag `@v2` (per US-09 accepted trade-off
//        for a single-maintainer OSS project — NOT a SHA pin).
//   S-3. The attest-build-provenance step's `subject-path:` resolves to the
//        per-cell archive (`modeltap-*.tar.gz`), so each of the 4 matrix cells
//        produces its own attestation tied to ITS archive.
//   S-4. Step ordering inside the build job:
//          Package archive and sha256 sidecar  <  attest-build-provenance
//                                              <  upload-artifact
//        Attesting BEFORE the archive exists fails; uploading BEFORE attesting
//        leaves an attestation race against artifact retention.
//
// Strategy C — real local resources (DWD-01): the test reads the real
// release.yml from the workspace and parses it with serde_yaml. The actual
// `gh attestation verify` smoke (which requires a published release on GitHub)
// is gated behind `#[ignore]` and explicit comment — it is the @requires_external
// scenario from the feature file.

use std::path::PathBuf;

use serde_yaml::Value;

// =============================================================================
// Helpers — same shape as workflow_structure.rs / build_matrix.rs so YAML
// navigation stays consistent across the release_process acceptance suite.
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

fn build_job(workflow: &Value) -> &Value {
    let jobs = get(workflow, "jobs");
    get_opt(jobs, "build").expect("release.yml must declare a `build` job")
}

fn build_steps(build: &Value) -> &Vec<Value> {
    get(build, "steps")
        .as_sequence()
        .expect("build.steps must be a sequence")
}

/// Find the index of the first step whose `name`, `uses`, or `run` substring
/// matches `needle`. Panics with a diagnostic listing all steps if not found.
fn step_index(steps: &[Value], needle: &str) -> usize {
    steps
        .iter()
        .position(|s| {
            let name = get_opt(s, "name").and_then(|v| v.as_str()).unwrap_or("");
            let uses = get_opt(s, "uses").and_then(|v| v.as_str()).unwrap_or("");
            let run = get_opt(s, "run").and_then(|v| v.as_str()).unwrap_or("");
            name.contains(needle) || uses.contains(needle) || run.contains(needle)
        })
        .unwrap_or_else(|| {
            let dump: Vec<String> = steps
                .iter()
                .map(|s| {
                    let name = get_opt(s, "name").and_then(|v| v.as_str()).unwrap_or("");
                    let uses = get_opt(s, "uses").and_then(|v| v.as_str()).unwrap_or("");
                    format!("name={name:?} uses={uses:?}")
                })
                .collect();
            panic!(
                "expected a build step matching {needle:?}; build steps were:\n{}",
                dump.join("\n")
            )
        })
}

// =============================================================================
// S-1. build job declares job-level permissions block with id-token: write,
//      attestations: write, contents: read (least privilege; OIDC + provenance).
// =============================================================================

#[test]
fn build_job_declares_oidc_permissions_for_attestation() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);

    let permissions = get_opt(build, "permissions").expect(
        "build job MUST declare a job-level `permissions:` block — \
         attest-build-provenance@v2 needs `id-token: write` to mint an OIDC \
         token for Sigstore and `attestations: write` to publish the provenance \
         (US-09 + ADR-013). Workflow-level permissions stay minimal so the \
         publish-github-release job's `GITHUB_TOKEN` doesn't carry attestation \
         scope it doesn't need.",
    );

    let id_token = get_opt(permissions, "id-token")
        .and_then(|v| v.as_str())
        .expect("build.permissions must declare `id-token:` (OIDC token minting)");
    assert_eq!(
        id_token, "write",
        "build.permissions.id-token must be `write` (OIDC token minting for \
         Sigstore in attest-build-provenance@v2). Got: {id_token:?}"
    );

    let attestations = get_opt(permissions, "attestations")
        .and_then(|v| v.as_str())
        .expect("build.permissions must declare `attestations:` (provenance write API)");
    assert_eq!(
        attestations, "write",
        "build.permissions.attestations must be `write` (publish provenance to \
         the repo's attestations API). Got: {attestations:?}"
    );

    let contents = get_opt(permissions, "contents")
        .and_then(|v| v.as_str())
        .expect("build.permissions must declare `contents:` (checkout requires read)");
    assert_eq!(
        contents, "read",
        "build.permissions.contents must be `read` (least privilege — only the \
         publish job needs `contents: write` to create the release). Got: {contents:?}"
    );
}

// =============================================================================
// S-2. attest-build-provenance step exists and is pinned to @v2 (the major-
//      version tag, NOT a SHA — single-maintainer OSS trade-off per US-09).
// =============================================================================

#[test]
fn build_job_invokes_attest_build_provenance_pinned_to_v2() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);
    let steps = build_steps(build);

    let attest_step = steps
        .iter()
        .find(|s| {
            get_opt(s, "uses")
                .and_then(|v| v.as_str())
                .is_some_and(|u| u.starts_with("actions/attest-build-provenance@"))
        })
        .expect(
            "build job MUST include a step using `actions/attest-build-provenance` \
             (US-09 + ADR-013 — every release archive ships with SLSA L3 \
             provenance verifiable by `gh attestation verify`)",
        );

    let uses = get_opt(attest_step, "uses")
        .and_then(|v| v.as_str())
        .expect("attest-build-provenance step must declare `uses:`");
    assert_eq!(
        uses, "actions/attest-build-provenance@v2",
        "attest-build-provenance MUST be pinned to `@v2` (major-version tag — \
         per US-09 accepted trade-off: a single-maintainer OSS project gets \
         security patches via the floating major tag and accepts the supply- \
         chain risk in exchange for not having to chase SHA bumps every minor \
         release). Got: {uses:?}"
    );
}

// =============================================================================
// S-3. attest-build-provenance.with.subject-path resolves to the per-cell
//      archive (`modeltap-*.tar.gz`). Each matrix cell attests ITS own archive
//      so end-users can verify the specific binary they downloaded.
// =============================================================================

#[test]
fn attest_step_subject_path_targets_per_cell_archive() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);
    let steps = build_steps(build);

    let attest_step = steps
        .iter()
        .find(|s| {
            get_opt(s, "uses")
                .and_then(|v| v.as_str())
                .is_some_and(|u| u.starts_with("actions/attest-build-provenance@"))
        })
        .expect("build job must include actions/attest-build-provenance step");

    let with =
        get_opt(attest_step, "with").expect("attest-build-provenance step must declare `with:`");
    let subject_path = get_opt(with, "subject-path")
        .and_then(|v| v.as_str())
        .expect(
            "attest-build-provenance.with MUST declare `subject-path:` — without \
             it the action has no archive to hash and attest",
        );

    // The subject-path must reference the per-cell archive. We accept either a
    // literal `modeltap-*.tar.gz` glob (resolves to ONE file per cell because
    // each cell only produces its own target's archive) or an explicit template
    // referencing matrix.target.
    let mentions_archive = subject_path.contains("modeltap-") && subject_path.contains(".tar.gz");
    assert!(
        mentions_archive,
        "attest-build-provenance.with.subject-path must reference the per-cell \
         archive `modeltap-*.tar.gz` (or a matrix.target-templated equivalent) \
         so each of the 4 matrix cells attests its own archive. Got: {subject_path:?}"
    );
}

// =============================================================================
// S-4. Step ordering: package step (produces the archive) MUST precede the
//      attest step, which MUST precede the upload-artifact step. Attesting
//      before the archive exists fails; uploading before attesting risks
//      a race where the artifact retention drops the archive before
//      attestation completes.
// =============================================================================

#[test]
fn attest_step_runs_after_package_and_before_upload() {
    let src = read_release_workflow();
    let workflow = parse_workflow(&src);
    let build = build_job(&workflow);
    let steps = build_steps(build);

    // Package step is identified by its name "Package archive and sha256 sidecar"
    // (see release.yml). The attest step is identified by its `uses:` prefix.
    // The upload step is identified by `actions/upload-artifact@`.
    let package_idx = step_index(steps, "Package archive");
    let attest_idx = steps
        .iter()
        .position(|s| {
            get_opt(s, "uses")
                .and_then(|v| v.as_str())
                .is_some_and(|u| u.starts_with("actions/attest-build-provenance@"))
        })
        .expect("attest-build-provenance step must exist (S-2)");
    let upload_idx = steps
        .iter()
        .position(|s| {
            get_opt(s, "uses")
                .and_then(|v| v.as_str())
                .is_some_and(|u| u.starts_with("actions/upload-artifact@"))
        })
        .expect("upload-artifact step must exist (M-5 in build_matrix.rs)");

    assert!(
        package_idx < attest_idx,
        "Package step (idx {package_idx}) MUST precede attest-build-provenance \
         (idx {attest_idx}) — the action hashes the archive on disk; without \
         the archive present the step fails."
    );
    assert!(
        attest_idx < upload_idx,
        "attest-build-provenance (idx {attest_idx}) MUST precede upload-artifact \
         (idx {upload_idx}) — uploading before attesting risks the artifact \
         retention dropping the archive before attestation completes, and \
         publishing an un-attested archive defeats the SLSA guarantee."
    );
}

// =============================================================================
// S-5. @requires_external smoke — Devon verifies a published archive's
//      attestation with `gh attestation verify --owner jeffabailey`. This is
//      the K-PROV KPI hook (DEVOPS kpi-instrumentation.md). It requires a
//      live GitHub release with a published archive, so it is `#[ignore]`d
//      by default. Run on demand:
//          PATH=/usr/bin:$PATH MODELTAP_VERIFY_ARCHIVE=/path/to/archive.tar.gz \
//              cargo test -p modeltap-acceptance \
//              --test release_process_slsa_attestation \
//              -- --ignored devon_can_verify_published_archive_attestation
// =============================================================================

#[test]
#[ignore = "@requires_external — needs a published GitHub Release archive + \
            an authenticated `gh` CLI on PATH; see MODELTAP_VERIFY_ARCHIVE env"]
fn devon_can_verify_published_archive_attestation() {
    use std::process::Command;

    let archive = std::env::var("MODELTAP_VERIFY_ARCHIVE").expect(
        "set MODELTAP_VERIFY_ARCHIVE to the path of a published modeltap \
         archive (downloaded from a real GitHub Release) before running this \
         @requires_external smoke",
    );

    // K-PROV smoke: the production verification command Devon would run. The
    // `--owner` flag scopes verification to the modeltap repo's attestation
    // signing identity, defending against a malicious upload from an unrelated
    // repo with a colliding archive name.
    let output = Command::new("gh")
        .arg("attestation")
        .arg("verify")
        .arg(&archive)
        .arg("--owner")
        .arg("jeffabailey")
        .output()
        .expect("failed to invoke `gh attestation verify` — is the gh CLI installed?");

    assert!(
        output.status.success(),
        "gh attestation verify {archive} --owner jeffabailey FAILED. \
         stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // The verification output must identify the modeltap workflow as the
    // builder — otherwise verification succeeded against an unrelated
    // workflow's attestation (the --owner scope alone is insufficient if
    // the same owner has multiple repos producing colliding archive names).
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("modeltap"),
        "gh attestation verify output must identify the modeltap workflow as \
         the builder. Got:\n{combined}"
    );
}
