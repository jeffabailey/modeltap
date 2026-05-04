// =============================================================================
// release-process-homebrew-github — Walking Skeleton Step Definitions
//
// Wave: DISTILL (5 of 6)
// Author: Quinn (nw-acceptance-designer)
// Date: 2026-05-03
//
// Step definitions specific to walking-skeleton.feature scenarios.
// Generic steps (xtask invocation, exit-code assertions) live in common_steps.rs.
//
// Mandate 1 (Hexagonal boundary): All When steps invoke the xtask CLI binary.
// No internal-component imports.
// =============================================================================

use super::common_steps::ReleaseWorld;

// -----------------------------------------------------------------------------
// PREP — release-prep scenario steps (US-01)
// -----------------------------------------------------------------------------

/// `Given there are <N> conventional commits since the v<version> tag`
pub fn given_conventional_commits_since_tag(
    _world: &mut ReleaseWorld,
    _count: u32,
    _previous_tag: &str,
) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_conventional_commits_since_tag — RED scaffold; DELIVER seeds a series of \
         commits with `feat:`, `fix:`, `chore:`, `refactor:` prefixes between the \
         previous_tag and HEAD in modeltap_fake. Use a deterministic mix to make the \
         git-cliff output reproducible."
    )
}

/// `Then Cargo.toml workspace.package.version becomes "<version>"`
pub fn then_cargo_toml_version_becomes(_world: &ReleaseWorld, _version: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_cargo_toml_version_becomes — RED scaffold; DELIVER reads modeltap_fake/Cargo.toml \
         and asserts the workspace.package.version field"
    )
}

/// `Then CHANGELOG.md gains a new "## [<version>]" section grouping commits by type`
pub fn then_changelog_section_added(_world: &ReleaseWorld, _version: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_changelog_section_added — RED scaffold; DELIVER reads CHANGELOG.md and \
         asserts the [<version>] section exists AND contains type-grouped subsections \
         (e.g., '### Features', '### Fixes')"
    )
}

// -----------------------------------------------------------------------------
// TAG — validate-tag scenario steps (US-02)
// -----------------------------------------------------------------------------
// (covered by common_steps::when_maintainer_runs_xtask + then_message_contains)

// -----------------------------------------------------------------------------
// BUILD — CI parity gates + single-target archive (US-03, US-04)
// -----------------------------------------------------------------------------

/// `Given the workspace passes formatting, linting, and tests`
pub fn given_workspace_passes_gates(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_workspace_passes_gates — RED scaffold; DELIVER seeds modeltap_fake with a \
         minimal-but-clean Rust workspace that genuinely passes cargo fmt + clippy + test"
    )
}

/// `Given the workspace contains a linting warning`
pub fn given_workspace_has_clippy_warning(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_workspace_has_clippy_warning — RED scaffold; DELIVER seeds modeltap_fake with \
         a known clippy-trippable pattern (e.g., a needless_borrow) so the gate fails"
    )
}

/// `Given the validate-tag step has passed for tag "<tag>"`
pub fn given_validate_tag_passed(_world: &mut ReleaseWorld, _tag: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_validate_tag_passed — RED scaffold; DELIVER simulates the workflow having \
         already validated the tag (i.e., set up the world state as if `validate-tag` \
         job succeeded) — concretely: ensure Cargo.toml version matches tag-without-v"
    )
}

/// `When the build orchestration runs for target "<target>"`
pub fn when_build_orchestration_runs(_world: &mut ReleaseWorld, _target: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "when_build_orchestration_runs — RED scaffold; DELIVER invokes the xtask command \
         (or a shell helper) that mirrors release.yml build steps in order: fmt, clippy, \
         test, then build, strip, package, sha256"
    )
}

/// `Then formatting, linting, and tests all pass before any release artifact is built`
pub fn then_gates_pass_before_artifact(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_gates_pass_before_artifact — RED scaffold; DELIVER asserts on captured \
         stdout that fmt-line precedes clippy-line precedes test-line precedes \
         'cargo build --release' line; or asserts via a step-ordering hook"
    )
}

/// `Then an archive named "<name>" is created`
pub fn then_archive_named(_world: &ReleaseWorld, _name: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_archive_named — RED scaffold; DELIVER asserts world.path(name).exists()")
}

/// `Then the archive contains exactly one file named "<name>"`
pub fn then_archive_contains_single_file(_world: &ReleaseWorld, _archive: &str, _name: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_archive_contains_single_file — RED scaffold; DELIVER tar-extracts and \
         asserts the entry list is exactly [name]"
    )
}

/// `Then a sidecar file "<name>" contains the bare-hex sha256 of the archive`
pub fn then_sidecar_contains_bare_hex_sha256(_world: &ReleaseWorld, _sidecar: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_sidecar_contains_bare_hex_sha256 — RED scaffold; DELIVER reads sidecar, \
         asserts it matches /^[a-f0-9]{64}$/, and recomputes the archive's sha256 to \
         confirm equality (this is the EXPECTED-OUTPUT check, not a fixture-theater)"
    )
}

// -----------------------------------------------------------------------------
// PUBLISH — extract-changelog scenario steps (US-05)
// -----------------------------------------------------------------------------

/// `Given a CHANGELOG.md file containing sections "## [<v1>]" and "## [<v2>]"`
pub fn given_changelog_with_sections(_world: &mut ReleaseWorld, _v1: &str, _v2: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_changelog_with_sections — RED scaffold; DELIVER writes a CHANGELOG.md \
         into modeltap_fake with the two named sections in standard keep-a-changelog \
         format (## [X.Y.Z] - YYYY-MM-DD)"
    )
}

/// `Given the "## [<version>]" section says "<text>"`
pub fn given_changelog_section_body(_world: &mut ReleaseWorld, _version: &str, _body: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("given_changelog_section_body — RED scaffold")
}

/// `Then RELEASE_NOTES.md exists`
pub fn then_release_notes_exists(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_release_notes_exists — RED scaffold")
}

/// `Then its content equals the body of the "## [<version>]" section`
pub fn then_release_notes_equals_section(_world: &ReleaseWorld, _version: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_release_notes_equals_section — RED scaffold; DELIVER reads RELEASE_NOTES.md \
         and reads the same section from CHANGELOG.md, asserts equal after trim"
    )
}

// -----------------------------------------------------------------------------
// TAP-BUMP — render-formula + push to ephemeral tap (US-06)
// -----------------------------------------------------------------------------

/// `Given the formula template at "<path>"`
pub fn given_formula_template(_world: &mut ReleaseWorld, _path: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_formula_template — RED scaffold; DELIVER ensures the template file \
         exists at modeltap_fake/<path>; if the production template hasn't been \
         authored yet, copy a fixture template from tests/fixtures/"
    )
}

/// `Given a sha256 sidecar file for target "<triple>" with content "<hex>"`
pub fn given_sha256_sidecar(_world: &mut ReleaseWorld, _triple: &str, _hex: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_sha256_sidecar — RED scaffold; DELIVER writes \
         ${TMPDIR}/artifacts/modeltap-<version>-<triple>.tar.gz.sha256 with bare-hex content"
    )
}

/// `Then the formula contains a "version" field equal to "<version>"`
pub fn then_formula_version_equals(_world: &ReleaseWorld, _version: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_formula_version_equals — RED scaffold; DELIVER reads the rendered formula \
         and asserts the `version \"X.Y.Z\"` line matches"
    )
}

/// `Then the formula contains the on_linux on_intel block with url ending in "<archive>"`
pub fn then_formula_block_url_ends_with(
    _world: &ReleaseWorld,
    _block: &str,
    _archive_suffix: &str,
) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_formula_block_url_ends_with — RED scaffold")
}

/// `Then the formula contains the sha256 "<hex>"`
pub fn then_formula_contains_sha256(_world: &ReleaseWorld, _hex: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_formula_contains_sha256 — RED scaffold")
}

/// `Then no other platform blocks are populated`
pub fn then_no_other_platform_blocks(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_no_other_platform_blocks — RED scaffold; DELIVER counts populated \
         (non-templated, non-{% if %}-elided) platform blocks and asserts the count"
    )
}

/// `Given the GitHub Release for "<tag>" exists with one archive`
pub fn given_github_release_exists(_world: &mut ReleaseWorld, _tag: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_github_release_exists — RED scaffold; for non-@requires_external scenarios \
         this is a logical precondition (no live GH); for @requires_external it implies \
         a real published release exists. Implement as a no-op for the local-only flow."
    )
}

/// `Given the rendered formula has been written into the tap-repo working tree`
pub fn given_rendered_formula_in_tap(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_rendered_formula_in_tap — RED scaffold; DELIVER copies a rendered formula \
         into ${TMPDIR}/tap-fake/Formula/modeltap.rb"
    )
}

/// `When the bump-tap-formula step commits and pushes the bump branch to "<url>"`
pub fn when_bump_step_pushes_to(_world: &mut ReleaseWorld, _url: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "when_bump_step_pushes_to — RED scaffold; DELIVER invokes the bump orchestration \
         (xtask helper or shell) that runs git checkout -B + git add + git commit + \
         git push --force-with-lease against the file:// URL"
    )
}

/// `Then a branch "<name>" exists in the tap repository`
pub fn then_tap_branch_exists(_world: &ReleaseWorld, _branch: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_tap_branch_exists — RED scaffold; DELIVER queries the bare tap repo's \
         refs (git --git-dir=${TMPDIR}/tap-fake show-ref) and asserts presence"
    )
}

/// `Then the branch's HEAD commit contains the rendered "Formula/modeltap.rb"`
pub fn then_tap_branch_has_formula(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_tap_branch_has_formula — RED scaffold; DELIVER `git show <branch>:Formula/modeltap.rb` \
         and asserts non-empty + contains expected version string"
    )
}

/// `Then the commit message is "<msg>"`
pub fn then_commit_message_equals(_world: &ReleaseWorld, _msg: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_commit_message_equals — RED scaffold")
}

/// `Given the bump-tap-formula step has been configured with an invalid tap-bump-token`
pub fn given_invalid_tap_token(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_invalid_tap_token — RED scaffold; DELIVER configures the bump step to \
         attempt push against a remote URL that requires auth (e.g., a non-existent \
         file:// path the user can't write to, or an HTTPS URL with a bogus token); \
         the goal is to provoke a visible failure"
    )
}

/// `Then the error output identifies an authentication failure`
pub fn then_output_identifies_auth_failure(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_output_identifies_auth_failure — RED scaffold; DELIVER asserts stderr \
         contains 'permission denied', 'authentication', '401', or similar"
    )
}
