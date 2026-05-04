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
