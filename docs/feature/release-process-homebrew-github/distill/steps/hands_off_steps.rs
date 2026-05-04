// =============================================================================
// release-process-homebrew-github — Hands-Off Automation Step Definitions
//
// Wave: DISTILL (5 of 6)
// Author: Quinn (nw-acceptance-designer)
// Date: 2026-05-03
//
// Step definitions specific to hands-off-automation.feature scenarios.
// Covers US-11 (auto-merge), US-12 (idempotent retry), US-13 (runbook), US-14 (lint).
// =============================================================================

use super::common_steps::ReleaseWorld;

// -----------------------------------------------------------------------------
// US-11 — Auto-merge
// -----------------------------------------------------------------------------

/// `Given the bump-tap-formula step has opened a PR against the tap repository`
pub fn given_bump_pr_opened(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_bump_pr_opened — RED scaffold; for the local flow, sets up world state \
         indicating a PR exists (e.g., a marker file in tap-fake) — the actual gh-pr-create \
         is not invoked since it requires live GH"
    )
}

/// `Then the step invokes the gh command to enable auto-merge with squash strategy`
pub fn then_gh_auto_merge_invoked(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_gh_auto_merge_invoked — RED scaffold; DELIVER asserts the bump step's \
         captured command log contains 'gh pr merge --auto --squash' (use a gh-shim \
         that records invocations under @real-io but skips actual GH calls)"
    )
}

/// `Then the auto-merge invocation targets the bump branch for the current version`
pub fn then_auto_merge_targets_bump_branch(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_auto_merge_targets_bump_branch — RED scaffold")
}

// -----------------------------------------------------------------------------
// US-12 — Idempotent retry
// -----------------------------------------------------------------------------

/// `Given no "<branch>" branch exists in the tap repository`
pub fn given_no_bump_branch(_world: &mut ReleaseWorld, _branch: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_no_bump_branch — RED scaffold; DELIVER ensures tap-fake has no ref \
         matching refs/heads/<branch>"
    )
}

/// `Given a "<branch>" branch already exists in the tap repository with a previous commit`
pub fn given_existing_bump_branch(_world: &mut ReleaseWorld, _branch: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_existing_bump_branch — RED scaffold; DELIVER pushes a synthetic commit \
         on the named branch in tap-fake (using a separate tempdir clone)"
    )
}

/// `Given exactly one PR for the bump branch is already open`
pub fn given_one_pr_open(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("given_one_pr_open — RED scaffold; for local flow, marker-file based")
}

/// `When the bump-tap-formula step runs for tag "<tag>"`
pub fn when_bump_step_runs_for_tag(_world: &mut ReleaseWorld, _tag: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "when_bump_step_runs_for_tag — RED scaffold; DELIVER invokes the bump orchestration \
         (xtask helper or shell sequence) with VERSION derived from the tag and \
         GH_TAP_TOKEN set to a stub"
    )
}

/// `When the bump-tap-formula step is re-run for tag "<tag>"`
pub fn when_bump_step_rerun(_world: &mut ReleaseWorld, _tag: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "when_bump_step_rerun — RED scaffold; semantically the same as a first run, \
         but world state has the existing branch/PR markers set"
    )
}

/// `Then a "<branch>" branch is created in the tap repository`
pub fn then_branch_created(_world: &ReleaseWorld, _branch: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_branch_created — RED scaffold")
}

/// `Then exactly one PR titled "<title>" is open against the tap repository`
pub fn then_one_pr_open(_world: &ReleaseWorld, _title: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_one_pr_open — RED scaffold; for local flow asserts the marker-file count is 1 \
         and the title field is correct"
    )
}

/// `Then the existing branch is force-pushed with the latest rendered formula`
pub fn then_branch_force_pushed(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_branch_force_pushed — RED scaffold; DELIVER asserts the tap-fake branch HEAD \
         differs from the previous synthetic commit (force-push happened) and contains \
         the freshly-rendered formula"
    )
}

/// `Then no second PR for the same version is created`
pub fn then_no_duplicate_pr(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_no_duplicate_pr — RED scaffold; PR-marker count is 1")
}

/// `Then the existing PR remains the only PR for the version`
pub fn then_pr_id_unchanged(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_pr_id_unchanged — RED scaffold")
}

/// `Given the bump-tap-formula step has been re-run any number of times for the same version`
pub fn given_bump_rerun_n_times(_world: &mut ReleaseWorld, _n: u32) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("given_bump_rerun_n_times — RED scaffold; loop n times")
}

/// `Then exactly one PR exists for the version`
/// `Then exactly one bump branch exists for the version`
pub fn then_count_prs_and_branches(_world: &ReleaseWorld, _expected: u32) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_count_prs_and_branches — RED scaffold")
}

/// `Given the maintainer has manually edited "Formula/modeltap.rb" on the "<branch>" branch`
pub fn given_manual_edit_on_branch(_world: &mut ReleaseWorld, _branch: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_manual_edit_on_branch — RED scaffold; DELIVER pushes a manually-edited \
         formula to the bump branch via a tempdir clone"
    )
}

/// `Then the manual edits are overwritten by the rendered formula`
pub fn then_manual_edits_clobbered(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_manual_edits_clobbered — RED scaffold; DELIVER asserts the post-bump \
         formula content is the rendered version (recognizable marker), not the manual edit"
    )
}

/// `Then the runbook documents this trade-off`
pub fn then_runbook_documents_clobber(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_runbook_documents_clobber — RED scaffold; DELIVER greps RELEASING.md for \
         a section on the manual-edit trade-off"
    )
}

// -----------------------------------------------------------------------------
// US-13 — RELEASING.md runbook
// -----------------------------------------------------------------------------

/// `Given the source repository`
/// `When "RELEASING.md" is opened`
pub fn when_releasing_md_opened(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("when_releasing_md_opened — RED scaffold; reads modeltap_fake/RELEASING.md")
}

/// `Then the file exists at the repository root`
pub fn then_releasing_md_exists(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_releasing_md_exists — RED scaffold")
}

/// `Then the file has at most <N> lines`
pub fn then_file_at_most_n_lines(_world: &ReleaseWorld, _max: usize) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_file_at_most_n_lines — RED scaffold; line-count assertion")
}

/// `Then the file contains at most <N> numbered steps`
pub fn then_at_most_n_numbered_steps(_world: &ReleaseWorld, _max: usize) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_at_most_n_numbered_steps — RED scaffold; DELIVER counts lines matching \
         /^\\d+\\. / and asserts ≤ max"
    )
}

/// `Then a markdown table with columns for ... is present`
pub fn then_release_log_table_present(_world: &ReleaseWorld, _columns: &[&str]) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_release_log_table_present — RED scaffold; DELIVER asserts the markdown \
         contains a table whose header row includes every named column"
    )
}

/// `Then the file documents <topic>`
pub fn then_file_documents(_world: &ReleaseWorld, _topic: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_file_documents — RED scaffold; DELIVER greps RELEASING.md for a section \
         heading matching the topic (e.g., 'GH_TAP_TOKEN rotation', 'manual-edit', 'xattr')"
    )
}

// -----------------------------------------------------------------------------
// US-14 — Lint-workflows
// -----------------------------------------------------------------------------

/// `Given a workflow file at "<path>" with <N> lines`
pub fn given_workflow_file_with_lines(_world: &mut ReleaseWorld, _path: &str, _lines: usize) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_workflow_file_with_lines — RED scaffold; DELIVER writes a synthetic \
         workflow file with the requested line count (jobs + comments + blanks)"
    )
}

/// `Given every job in the workflow has a "# Purpose:" comment immediately above its declaration`
pub fn given_all_jobs_have_purpose(_world: &mut ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "given_all_jobs_have_purpose — RED scaffold; DELIVER ensures the synthetic \
         workflow has '# Purpose: ...' on the line preceding each `<job>:` block"
    )
}

/// `Given the "<job>" job does not have a "# Purpose:" comment immediately above its declaration`
pub fn given_job_missing_purpose(_world: &mut ReleaseWorld, _job: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("given_job_missing_purpose — RED scaffold")
}

/// `Then the message says the workflow exceeds the 250-line limit`
pub fn then_message_says_exceeds_limit(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_message_says_exceeds_limit — RED scaffold; asserts captured stderr \
         contains 'exceeds 250 lines' or similar"
    )
}

/// `Then the message reports the actual line count`
pub fn then_message_reports_line_count(_world: &ReleaseWorld) {
    let _ = (); // SCAFFOLD: true
    unimplemented!(
        "then_message_reports_line_count — RED scaffold; DELIVER asserts the actual \
         numeric count appears in the diagnostic output"
    )
}

/// `Then the message identifies "<job>" as the job missing its purpose comment`
pub fn then_message_identifies_missing_purpose_job(_world: &ReleaseWorld, _job: &str) {
    let _ = (); // SCAFFOLD: true
    unimplemented!("then_message_identifies_missing_purpose_job — RED scaffold")
}
