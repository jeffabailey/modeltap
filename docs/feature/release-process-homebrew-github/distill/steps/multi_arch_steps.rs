// =============================================================================
// release-process-homebrew-github — Multi-Arch Release Step Definitions
//
// Wave: DISTILL (5 of 6)
// Author: Quinn (nw-acceptance-designer)
// Date: 2026-05-03
//
// Step definitions specific to multi-arch-release.feature scenarios.
// Covers US-07 (4-target matrix), US-08 (atomic publish), US-09 (SLSA), US-10 (4 platform blocks).
// =============================================================================

use super::common_steps::ReleaseWorld;

// -----------------------------------------------------------------------------
// US-07 — 4-target build matrix (workflow YAML inspection)
// -----------------------------------------------------------------------------

/// `Given the release workflow file at ".github/workflows/release.yml"`
pub fn given_release_workflow_file(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_release_workflow_file — RED scaffold; DELIVER reads the on-disk \
         .github/workflows/release.yml from modeltap_fake (or from a fixture under \
         tests/fixtures/workflows/) — both for parsing and for line-count assertions"
    )
}

/// `When the workflow definition is parsed`
pub fn when_workflow_parsed(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "when_workflow_parsed — RED scaffold; DELIVER parses YAML via serde_yaml and \
         stores the parsed structure in world (or invokes `xtask lint-workflows --json` \
         and stores the report)"
    )
}

/// `Then the build job declares a matrix of exactly <N> targets`
pub fn then_build_matrix_has_n_targets(_world: &ReleaseWorld, _n: usize) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_build_matrix_has_n_targets — RED scaffold; DELIVER navigates parsed YAML \
         to jobs.build.strategy.matrix.target and asserts length"
    )
}

/// `Then the targets include <list>`
pub fn then_build_matrix_includes_targets(_world: &ReleaseWorld, _targets: &[&str]) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_build_matrix_includes_targets — RED scaffold")
}

/// `Then "<target>" runs on "<runner>"`
pub fn then_target_runs_on_runner(_world: &ReleaseWorld, _target: &str, _runner: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_target_runs_on_runner — RED scaffold; DELIVER asserts the matrix-include \
         entry pairs target → runs-on as expected"
    )
}

/// `Then the matrix uses "fail-fast: false"`
pub fn then_matrix_fail_fast_false(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_matrix_fail_fast_false — RED scaffold")
}

// -----------------------------------------------------------------------------
// US-08 — Atomic publish
// -----------------------------------------------------------------------------

/// `Then the publish-github-release job has needs equal to <list>`
pub fn then_job_needs_equal(_world: &ReleaseWorld, _job: &str, _needs: &[&str]) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_job_needs_equal — RED scaffold; DELIVER asserts jobs.<job>.needs equals \
         the expected list (handles both string-form and array-form YAML)"
    )
}

/// `Then no job uses "if: always()" or "if: failure()" to bypass the guard`
pub fn then_no_bypass_overrides(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_no_bypass_overrides — RED scaffold; DELIVER inspects every job for an \
         `if:` condition containing always()/failure() and asserts none of these \
         appear on publish-github-release or bump-tap-formula"
    )
}

/// `Given the build matrix has run with <N1> cells succeeding and <N2> cells failing`
pub fn given_matrix_outcomes(
    _world: &mut ReleaseWorld,
    _succeeded: u32,
    _failed: u32,
) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_matrix_outcomes — RED scaffold; for the local-only flow this simulates \
         the workflow's needs-DAG outcome by setting up world state. DELIVER may use a \
         workflow simulator (small Rust helper that walks a `needs:` graph and applies \
         pass/fail per cell) — preserves the integration semantics without a live runner."
    )
}

/// `Then the <job> job is skipped`
pub fn then_job_is_skipped(_world: &ReleaseWorld, _job: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_job_is_skipped — RED scaffold; asserts simulator output for the job is 'skipped'")
}

/// `Then no GitHub Release for the tag is created`
pub fn then_no_github_release_created(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_no_github_release_created — RED scaffold; for local flow, asserts the \
         simulator never invoked the gh-release-create stub"
    )
}

/// `Then no PR is opened against the tap repository`
pub fn then_no_tap_pr_opened(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_no_tap_pr_opened — RED scaffold; asserts no branch exists in tap-fake")
}

// -----------------------------------------------------------------------------
// US-09 — SLSA build provenance attestation
// -----------------------------------------------------------------------------

/// `Then the build job permissions include <list>`
pub fn then_job_permissions_include(_world: &ReleaseWorld, _permissions: &[&str]) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_job_permissions_include — RED scaffold; DELIVER asserts jobs.build.permissions \
         contains the expected key:value pairs"
    )
}

/// `Then the attest-build-provenance step is invoked with the archive as the subject`
pub fn then_attest_step_invoked(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_attest_step_invoked — RED scaffold; DELIVER asserts a step in jobs.build.steps \
         uses 'actions/attest-build-provenance@v2' with subject-path referencing the archive"
    )
}

/// `Then the action version is pinned to "@<ver>"`
pub fn then_action_version_pinned(_world: &ReleaseWorld, _version: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_action_version_pinned — RED scaffold")
}

// -----------------------------------------------------------------------------
// US-10 — Formula renders 4 platform blocks
// -----------------------------------------------------------------------------

/// `Given a fixture artifact directory containing <N> sha256 sidecar files for the <N> supported targets`
pub fn given_artifact_dir_with_sidecars(_world: &mut ReleaseWorld, _n: u32) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_artifact_dir_with_sidecars — RED scaffold; DELIVER writes N sidecar files \
         under ${TMPDIR}/artifacts/ with deterministic-but-distinct hex digests per triple"
    )
}

/// `Given a fixture artifact directory missing the sidecar for "<triple>"`
pub fn given_artifact_dir_missing_sidecar(_world: &mut ReleaseWorld, _triple: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("given_artifact_dir_missing_sidecar — RED scaffold")
}

/// `Given a fixture artifact directory where one sidecar contains "<malformed>"`
pub fn given_artifact_dir_malformed_sidecar(_world: &mut ReleaseWorld, _malformed: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("given_artifact_dir_malformed_sidecar — RED scaffold")
}

/// `Then the rendered formula contains an "<block-path>" block with sha256 from the <triple> sidecar`
pub fn then_formula_block_sha256_matches_sidecar(
    _world: &ReleaseWorld,
    _block_path: &str,
    _triple: &str,
) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_formula_block_sha256_matches_sidecar — RED scaffold; DELIVER reads the rendered \
         formula, extracts the sha256 line under the named on_macos.on_arm / etc. block, and \
         asserts it equals the bare-hex content of artifacts/modeltap-<v>-<triple>.tar.gz.sha256"
    )
}

/// `Then the version field equals "<version>"`
pub fn then_version_field_equals(_world: &ReleaseWorld, _version: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_version_field_equals — RED scaffold")
}
