// xtask::gh_adapter — thin shell-out wrapper around the `gh` CLI.
//
// Step: 01-08 (Walking Skeleton — TAP-BUMP activity, US-06).
//
// Per DESIGN component-boundaries.md §2.3, adapters are kept THIN: one
// function per shell-out, no business logic. They translate a typed input to
// a CLI invocation and a CLI exit-code/stdout to a typed Result.
//
// Currently exposes:
//   - pr_list_for_head(head_ref, base_repo) -> Vec<PrSummary> :
//         shells out to `gh pr list --head <ref> --repo <repo>
//                       --json number,title,state`
//   - pr_create(title, body, head, base_repo) -> PrSummary :
//         shells out to `gh pr create --title <T> --body <B> --head <H>
//                       --repo <R>`
//
// Design choice: the argument-construction step is factored into pure
// `pr_list_for_head_args` / `pr_create_args` helpers so unit tests can assert
// the exact argv we hand to gh without invoking the binary. The shell-out
// wrappers themselves are thin and integration-tested via the workflow
// (gated `@requires_external` because they need an authenticated gh).
//
// Why this is the right shape: adopting a heavier abstraction (e.g., an
// octocrab client) for two operations would buy nothing — the workflow
// already pre-installs `gh`, and the failure modes that matter (auth
// failure, rate limiting) surface identically through `gh`'s own diagnostics.

use std::ffi::OsString;
use std::process::Command;

use serde::Deserialize;

/// Summary view of a GitHub PR returned by `gh pr list --json` and
/// `gh pr create --json`. The `gh` CLI returns these as JSON arrays/objects
/// with the field names below; serde derives the deserialiser directly.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct PrSummary {
    pub number: u64,
    pub title: String,
    pub state: String,
}

#[derive(Debug)]
pub enum GhError {
    /// `gh` itself failed to launch (binary missing, PATH issue, etc.)
    LaunchFailed(std::io::Error),
    /// `gh` ran but returned non-zero. Captures stderr (often contains the
    /// authentication / rate-limit diagnostic the maintainer needs).
    NonZeroExit { code: i32, stderr: String },
    /// `gh` returned zero but its stdout was not parseable as the expected
    /// JSON shape — a `gh` schema change or unexpected output.
    JsonParseError {
        underlying: String,
        raw_stdout: String,
    },
}

impl std::fmt::Display for GhError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GhError::LaunchFailed(e) => write!(f, "failed to launch gh: {e}"),
            GhError::NonZeroExit { code, stderr } => {
                write!(f, "gh exited with code {code}: {}", stderr.trim())
            }
            GhError::JsonParseError {
                underlying,
                raw_stdout,
            } => {
                write!(
                    f,
                    "gh returned unparseable JSON: {underlying}; raw stdout: {raw_stdout}"
                )
            }
        }
    }
}

impl std::error::Error for GhError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GhError::LaunchFailed(e) => Some(e),
            _ => None,
        }
    }
}

/// Merge strategy passed to `gh pr merge`. The `--squash` form (US-11 — single
/// commit on tap main per release) is the only one currently used by the
/// release pipeline; `Merge` and `Rebase` are exposed for completeness so the
/// adapter is general-purpose without inflating call-site complexity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    Squash,
    Merge,
    Rebase,
}

impl MergeStrategy {
    /// CLI flag the strategy renders to (`--squash`, `--merge`, `--rebase`).
    /// Pure function — used by `pr_merge_auto_args` and tested directly.
    pub fn flag(self) -> &'static str {
        match self {
            MergeStrategy::Squash => "--squash",
            MergeStrategy::Merge => "--merge",
            MergeStrategy::Rebase => "--rebase",
        }
    }
}

/// Build the argv vector for `gh pr merge --auto <strategy> --repo <repo>
/// <branch>`. Pure function — no I/O. Factored out so unit tests can assert
/// the exact argv without invoking gh.
///
/// The `--auto` flag arms GitHub's auto-merge: the merge fires only after all
/// required status checks (per tap-repo branch protection) pass. For the
/// release pipeline, that gate is `brew test-bot` (US-11 + ADR-013). If the
/// tap repo does not have auto-merge enabled at the repo level, `gh` exits
/// non-zero with "Auto-merge is not allowed for this repository" — surfaced
/// verbatim through `GhError::NonZeroExit`.
pub fn pr_merge_auto_args(branch: &str, repo: &str, strategy: MergeStrategy) -> Vec<OsString> {
    vec![
        OsString::from("pr"),
        OsString::from("merge"),
        OsString::from("--auto"),
        OsString::from(strategy.flag()),
        OsString::from("--repo"),
        OsString::from(repo),
        OsString::from(branch),
    ]
}

/// Build the argv vector for `gh pr list --head <ref> --repo <repo>
/// --json number,title,state`. Pure function — no I/O. Factored out so unit
/// tests can assert the exact argv without invoking gh.
pub fn pr_list_for_head_args(head_ref: &str, base_repo: &str) -> Vec<OsString> {
    vec![
        OsString::from("pr"),
        OsString::from("list"),
        OsString::from("--head"),
        OsString::from(head_ref),
        OsString::from("--repo"),
        OsString::from(base_repo),
        OsString::from("--json"),
        OsString::from("number,title,state"),
    ]
}

/// Build the argv vector for `gh pr create --title <T> --body <B>
/// --head <H> --repo <R>`. Pure function — no I/O.
///
/// We do NOT pass `--base` — `gh pr create` defaults to the repo's default
/// branch when `--base` is omitted. The walking-skeleton tap repo's default
/// branch IS `main`; production tap repos that change their default branch
/// would need an explicit `--base` flag added here (acceptable trade-off
/// for a single-maintainer project).
pub fn pr_create_args(title: &str, body: &str, head: &str, base_repo: &str) -> Vec<OsString> {
    vec![
        OsString::from("pr"),
        OsString::from("create"),
        OsString::from("--title"),
        OsString::from(title),
        OsString::from("--body"),
        OsString::from(body),
        OsString::from("--head"),
        OsString::from(head),
        OsString::from("--repo"),
        OsString::from(base_repo),
    ]
}

/// Pure decision: given the PRs returned by `pr_list_for_head` for a head
/// ref, should `bump-tap-formula` skip the `gh pr create` step?
///
/// Skip iff at least one PR with state `OPEN` exists. Closed / merged PRs
/// for the same head ref do NOT block re-creation — that's the
/// "I merged it then the maintainer re-pushed the tag" recovery flow.
///
/// `gh` returns PR state as upper-case strings (`OPEN`, `CLOSED`, `MERGED`).
/// We compare case-insensitively to be robust against future schema drift.
///
/// Used by the bump-tap-formula idempotency path (US-12 / step 03-02). The
/// helper is pure and tested directly — no I/O, no shell-out.
pub fn should_skip_pr_create(existing_prs: &[PrSummary]) -> bool {
    existing_prs
        .iter()
        .any(|pr| pr.state.eq_ignore_ascii_case("OPEN"))
}

/// `gh pr list --head <ref> --repo <repo> --json number,title,state` →
/// parsed `Vec<PrSummary>`. Empty vector ⇒ no PR exists for that head ref.
///
/// Used by the bump-tap-formula retry path (US-12 idempotency, step 03-02)
/// to detect an already-open PR and skip creation. The walking-skeleton
/// happy path doesn't call this directly.
pub fn pr_list_for_head(head_ref: &str, base_repo: &str) -> Result<Vec<PrSummary>, GhError> {
    let args = pr_list_for_head_args(head_ref, base_repo);
    let output = Command::new("gh")
        .args(&args)
        .output()
        .map_err(GhError::LaunchFailed)?;

    if !output.status.success() {
        return Err(GhError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    serde_json::from_str::<Vec<PrSummary>>(&raw).map_err(|e| GhError::JsonParseError {
        underlying: e.to_string(),
        raw_stdout: raw,
    })
}

/// `gh pr create --title <T> --body <B> --head <H> --repo <R>` →
/// parsed `PrSummary` for the freshly-created PR.
///
/// `gh pr create` prints the PR URL on success; we currently do not parse
/// that URL here because the production WS exit gate does not need the PR
/// number returned through this path — the workflow job logs it. If a future
/// step needs structured PR metadata, switch to `gh pr create ... --json`
/// (gh ≥ 2.40 supports it).
///
/// Returns a `PrSummary` with `number=0` and the input title/state="open"
/// as a placeholder until the structured-output upgrade lands.
pub fn pr_create(
    title: &str,
    body: &str,
    head: &str,
    base_repo: &str,
) -> Result<PrSummary, GhError> {
    let args = pr_create_args(title, body, head, base_repo);
    let output = Command::new("gh")
        .args(&args)
        .output()
        .map_err(GhError::LaunchFailed)?;

    if !output.status.success() {
        return Err(GhError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(PrSummary {
        number: 0,
        title: title.to_owned(),
        state: "OPEN".to_owned(),
    })
}

/// `gh pr merge --auto <strategy> --repo <repo> <branch>` → arms GitHub
/// auto-merge on the named PR (US-11). Returns `Ok(())` on success.
///
/// The PR is identified by branch name rather than PR number — `gh` resolves
/// the branch to the most recent open PR with that head ref. This lets the
/// release pipeline call this AFTER `gh pr create` without parsing the
/// created PR's URL/number from gh's stdout.
///
/// Common failure modes surfaced through `GhError::NonZeroExit`:
///   - `Auto-merge is not allowed for this repository`: tap repo does not
///     have auto-merge enabled at the repo level. One-time fix: enable in
///     repo settings (documented in RELEASING.md, step 03-03).
///   - `Pull request is not mergeable`: branch protection rejects the merge
///     (e.g., required status check still failing). Auto-merge will fire
///     once the checks turn green.
///   - `HTTP 401`: GH_TAP_TOKEN expired or lacks `repo` scope on the tap.
pub fn pr_merge_auto(branch: &str, repo: &str, strategy: MergeStrategy) -> Result<(), GhError> {
    let args = pr_merge_auto_args(branch, repo, strategy);
    let output = Command::new("gh")
        .args(&args)
        .output()
        .map_err(GhError::LaunchFailed)?;

    if !output.status.success() {
        return Err(GhError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Unit tests for the pure argv-construction helpers. These assert the
    // exact argv we hand to gh without invoking the binary — fast,
    // deterministic, and they catch the most common defect (wrong flag name,
    // wrong field-list ordering for the --json filter).
    // -------------------------------------------------------------------------

    fn argv_strings(args: &[OsString]) -> Vec<&str> {
        args.iter()
            .map(|s| s.to_str().expect("test argv must be utf-8"))
            .collect()
    }

    #[test]
    fn pr_list_for_head_args_constructs_expected_argv() {
        let args = pr_list_for_head_args("bump/v0.0.1-rc1", "jeffabailey/homebrew-modeltap");
        assert_eq!(
            argv_strings(&args),
            vec![
                "pr",
                "list",
                "--head",
                "bump/v0.0.1-rc1",
                "--repo",
                "jeffabailey/homebrew-modeltap",
                "--json",
                "number,title,state",
            ]
        );
    }

    #[test]
    fn pr_merge_auto_args_constructs_expected_argv_for_squash() {
        let args = pr_merge_auto_args(
            "bump/v0.0.1-rc1",
            "jeffabailey/homebrew-modeltap",
            MergeStrategy::Squash,
        );
        assert_eq!(
            argv_strings(&args),
            vec![
                "pr",
                "merge",
                "--auto",
                "--squash",
                "--repo",
                "jeffabailey/homebrew-modeltap",
                "bump/v0.0.1-rc1",
            ]
        );
    }

    #[test]
    fn merge_strategy_renders_expected_cli_flag() {
        assert_eq!(MergeStrategy::Squash.flag(), "--squash");
        assert_eq!(MergeStrategy::Merge.flag(), "--merge");
        assert_eq!(MergeStrategy::Rebase.flag(), "--rebase");
    }

    #[test]
    fn pr_create_args_constructs_expected_argv() {
        let args = pr_create_args(
            "modeltap 0.0.1-rc1",
            "Automated bump for tag v0.0.1-rc1.",
            "bump/v0.0.1-rc1",
            "jeffabailey/homebrew-modeltap",
        );
        assert_eq!(
            argv_strings(&args),
            vec![
                "pr",
                "create",
                "--title",
                "modeltap 0.0.1-rc1",
                "--body",
                "Automated bump for tag v0.0.1-rc1.",
                "--head",
                "bump/v0.0.1-rc1",
                "--repo",
                "jeffabailey/homebrew-modeltap",
            ]
        );
    }

    #[test]
    fn pr_summary_deserializes_from_gh_pr_list_json() {
        // Sample shape `gh pr list --json number,title,state` produces.
        let json = r#"[
            {"number": 42, "title": "modeltap 0.0.1-rc1", "state": "OPEN"},
            {"number": 7, "title": "modeltap 0.0.1-alpha", "state": "MERGED"}
        ]"#;
        let parsed: Vec<PrSummary> = serde_json::from_str(json).expect("parse gh JSON");
        assert_eq!(
            parsed,
            vec![
                PrSummary {
                    number: 42,
                    title: "modeltap 0.0.1-rc1".to_owned(),
                    state: "OPEN".to_owned(),
                },
                PrSummary {
                    number: 7,
                    title: "modeltap 0.0.1-alpha".to_owned(),
                    state: "MERGED".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn pr_summary_deserializes_empty_list() {
        // No PR for the head ref — gh returns `[]`.
        let parsed: Vec<PrSummary> = serde_json::from_str("[]").expect("parse empty list");
        assert_eq!(parsed, Vec::<PrSummary>::new());
    }

    // -------------------------------------------------------------------------
    // Unit tests for `should_skip_pr_create` — the pure idempotency-decision
    // helper that gates `gh pr create` on whether an OPEN PR for the head ref
    // already exists. Behaviors covered:
    //   * skip when at least one OPEN PR exists for the head ref
    //   * proceed (don't skip) when the only PRs for the head are CLOSED/MERGED
    //   * proceed when no PRs exist for the head
    // The OPEN-state matching is case-insensitive so we don't break under a
    // future `gh` schema that lowercases the field.
    // -------------------------------------------------------------------------

    #[test]
    fn should_skip_pr_create_when_open_pr_exists_for_head() {
        let existing = vec![PrSummary {
            number: 42,
            title: "modeltap 0.2.0".to_owned(),
            state: "OPEN".to_owned(),
        }];
        assert!(
            should_skip_pr_create(&existing),
            "an OPEN PR for the head ref must gate `gh pr create` (US-12 idempotency)"
        );
    }

    #[test]
    fn should_not_skip_pr_create_when_no_prs_exist_for_head() {
        let existing: Vec<PrSummary> = vec![];
        assert!(
            !should_skip_pr_create(&existing),
            "an empty PR list means no PR exists for the head — proceed with create"
        );
    }

    #[test]
    fn should_not_skip_pr_create_when_only_closed_or_merged_prs_exist() {
        // A previous bump for the same version was merged then the tag was
        // re-pushed (recovery flow). The head ref now has only MERGED/CLOSED
        // PRs; we MUST be able to open a fresh one.
        let existing = vec![
            PrSummary {
                number: 7,
                title: "modeltap 0.2.0".to_owned(),
                state: "MERGED".to_owned(),
            },
            PrSummary {
                number: 9,
                title: "modeltap 0.2.0".to_owned(),
                state: "CLOSED".to_owned(),
            },
        ];
        assert!(
            !should_skip_pr_create(&existing),
            "only CLOSED/MERGED PRs for the head must NOT block create (recovery flow)"
        );
    }

    #[test]
    fn should_skip_pr_create_matches_open_state_case_insensitively() {
        // `gh` currently returns upper-case state strings. Future-proof against
        // a schema change to lower-case (`open`).
        let existing = vec![PrSummary {
            number: 42,
            title: "modeltap 0.2.0".to_owned(),
            state: "open".to_owned(),
        }];
        assert!(
            should_skip_pr_create(&existing),
            "OPEN-state matching must be case-insensitive (gh schema robustness)"
        );
    }

    #[test]
    fn gh_error_display_includes_exit_code_and_stderr_for_non_zero() {
        let err = GhError::NonZeroExit {
            code: 1,
            stderr: "HTTP 401: Bad credentials\n".to_owned(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("code 1"),
            "display must include exit code; got: {msg}"
        );
        assert!(
            msg.contains("401") || msg.contains("Bad credentials"),
            "display must surface the gh stderr (auth failure); got: {msg}"
        );
    }
}
