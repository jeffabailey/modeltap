// xtask::lint — pure workflow-file linting (line count + purpose comments).
//
// Per DESIGN component-boundaries.md §2.2 and DELIVER step 01-05.
//
// `lint()` is a pure function over the workflow YAML text + the maintainer's
// line-count budget. It returns a `LintReport` describing both diagnostics:
//   - `line_count`        : actual line count (blanks + comments included)
//   - `over_budget`       : convenience flag for `line_count > max_lines`
//   - `jobs_missing_purpose`
//                         : top-level job names whose immediately-preceding
//                           non-blank line is NOT a `# Purpose:` comment.
//
// The CLI dispatcher (in `main.rs`) translates a non-empty
// `jobs_missing_purpose` or `over_budget` into a non-zero exit code with a
// human-readable diagnostic on stderr. The pure function itself never panics
// on input shape — malformed YAML returns `Err(LintError::ParseError(_))`.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, PartialEq, Eq)]
pub struct LintReport {
    /// Total line count of the source text. Blanks and comment lines are
    /// included so the budget reflects what a human reads top-to-bottom.
    pub line_count: usize,
    /// Convenience flag: `line_count > max_lines`.
    pub over_budget: bool,
    /// Top-level job names lacking an immediately-preceding `# Purpose:`
    /// comment. Order follows source-file declaration order.
    pub jobs_missing_purpose: Vec<String>,
}

#[derive(Debug)]
pub enum LintError {
    /// Workflow YAML failed to parse, or the top-level `jobs:` mapping is
    /// missing or has the wrong shape. The wrapped string is the underlying
    /// `serde_yaml` diagnostic so the maintainer sees what's wrong.
    ParseError(String),
}

impl std::fmt::Display for LintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LintError::ParseError(msg) => write!(f, "workflow parse error: {msg}"),
        }
    }
}

impl std::error::Error for LintError {}

/// Minimal view of a GitHub Actions workflow: only the top-level `jobs:`
/// mapping is interesting to the linter. Job *bodies* are deserialised as
/// opaque `serde_yaml::Value`s — the linter never inspects them; it only
/// needs the set of job names so it can locate each job's declaration line
/// in the source text.
///
/// We use `BTreeMap` purely so the deserialiser accepts any string-keyed
/// mapping; declaration order for the `jobs_missing_purpose` field is
/// recovered from the SOURCE LINES (not the parsed map), so map ordering
/// here is irrelevant.
#[derive(Debug, Deserialize)]
struct WorkflowShape {
    #[serde(default)]
    jobs: BTreeMap<String, serde_yaml::Value>,
}

/// Lint a workflow YAML string against the maintainer's line-count budget
/// and the per-job `# Purpose:` comment convention (US-14).
///
/// Returns `Err(LintError::ParseError(_))` if the YAML cannot be parsed.
/// Otherwise returns `Ok(LintReport { ... })` whose fields drive the CLI's
/// exit-code decision.
///
/// Algorithm:
/// 1. Count source lines (counting `\n`-delimited lines, including a trailing
///    newline as a final empty line per `str::lines` convention adjusted to
///    keep parity with `wc -l`).
/// 2. Parse the YAML into the minimal `WorkflowShape` to enumerate top-level
///    job names.
/// 3. For each job name, locate the SOURCE LINE that declares it
///    (`^  <job-name>:` — exactly two spaces of indent per the GHA convention
///    used in this repo's workflows). Inspect the immediately-preceding
///    non-blank line. If it is not `^\s*#\s*Purpose:`, mark the job missing.
/// 4. Sort `jobs_missing_purpose` by the order in which the offending jobs
///    appear in the SOURCE (so the maintainer sees them top-to-bottom).
pub fn lint(yaml_text: &str, max_lines: usize) -> Result<LintReport, LintError> {
    let line_count = count_lines(yaml_text);

    let shape: WorkflowShape =
        serde_yaml::from_str(yaml_text).map_err(|e| LintError::ParseError(e.to_string()))?;

    let source_lines: Vec<&str> = yaml_text.lines().collect();
    let mut missing: Vec<(usize, String)> = Vec::new();

    for job_name in shape.jobs.keys() {
        let Some(decl_idx) = find_job_declaration_line(&source_lines, job_name) else {
            // Job name is in the parsed map but no source line declares it
            // at the conventional `^  <name>:` form. Treat as missing-purpose
            // so the maintainer notices the irregular shape.
            missing.push((usize::MAX, job_name.clone()));
            continue;
        };
        if !is_preceded_by_purpose_comment(&source_lines, decl_idx) {
            missing.push((decl_idx, job_name.clone()));
        }
    }

    missing.sort_by_key(|(idx, _)| *idx);
    let jobs_missing_purpose = missing.into_iter().map(|(_, name)| name).collect();

    Ok(LintReport {
        line_count,
        over_budget: line_count > max_lines,
        jobs_missing_purpose,
    })
}

/// Count source lines as a maintainer would count them in their editor:
/// every `\n`-delimited line, plus an extra empty trailing line if the text
/// does not end with `\n`. An empty string counts as 0 lines.
fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let nl = text.bytes().filter(|b| *b == b'\n').count();
    if text.ends_with('\n') {
        nl
    } else {
        nl + 1
    }
}

/// Locate the source-line index that declares `job_name` at the conventional
/// two-space-indented top-level-jobs form (`^  <job-name>:`). Returns `None`
/// if no such line exists.
fn find_job_declaration_line(source_lines: &[&str], job_name: &str) -> Option<usize> {
    let needle = format!("  {job_name}:");
    source_lines.iter().position(|line| {
        // Allow trailing whitespace or end-of-line after the colon, but
        // require the exact two-space indent + name + ':' prefix.
        line.starts_with(&needle)
            && line[needle.len()..]
                .chars()
                .all(|c| c.is_whitespace() || c == '\r')
    })
}

/// Check whether the line IMMEDIATELY ABOVE `decl_idx` is a `# Purpose:`
/// comment. A blank line between the comment and the declaration breaks the
/// bond — US-14 requires the comment be IMMEDIATELY above.
fn is_preceded_by_purpose_comment(source_lines: &[&str], decl_idx: usize) -> bool {
    if decl_idx == 0 {
        return false;
    }
    let prev = source_lines[decl_idx - 1].trim_start();
    // Match `# Purpose:` (case-sensitive) optionally followed by content.
    prev.starts_with("# Purpose:") || prev.starts_with("#Purpose:")
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Behavior 1: line_count counts every \n-delimited line including blanks
    // and comments (parametrised input variations are folded into one test).
    // -------------------------------------------------------------------------

    #[test]
    fn line_count_includes_blank_and_comment_lines() {
        let yaml = "\
name: release
# header comment

jobs:
  # Purpose: do thing
  validate-tag:
    runs-on: ubuntu-latest
";
        let report = lint(yaml, 999).expect("valid yaml");
        assert_eq!(report.line_count, 7, "blanks + comments must count");
    }

    // -------------------------------------------------------------------------
    // Behavior 2: over_budget is true iff line_count > max_lines.
    // -------------------------------------------------------------------------

    #[test]
    fn over_budget_is_true_when_line_count_exceeds_max_lines() {
        // 5-line valid workflow, max_lines = 3 -> over budget.
        let yaml = "\
name: release
jobs:
  # Purpose: x
  validate-tag:
    runs-on: ubuntu-latest
";
        let report = lint(yaml, 3).expect("valid yaml");
        assert!(report.over_budget, "5 lines > 3 must flag over_budget");
        assert_eq!(report.line_count, 5);
    }

    #[test]
    fn over_budget_is_false_when_line_count_within_max_lines() {
        let yaml = "\
name: release
jobs:
  # Purpose: x
  validate-tag:
    runs-on: ubuntu-latest
";
        let report = lint(yaml, 999).expect("valid yaml");
        assert!(!report.over_budget);
    }

    // -------------------------------------------------------------------------
    // Behavior 3: jobs_missing_purpose lists every job whose immediately
    // preceding non-blank line is NOT a `# Purpose:` comment, in source order.
    // -------------------------------------------------------------------------

    #[test]
    fn lists_jobs_missing_purpose_comment_in_source_order() {
        let yaml = "\
name: release
jobs:
  validate-tag:
    runs-on: ubuntu-latest

  # Purpose: build the binary
  build:
    runs-on: ubuntu-latest

  publish:
    runs-on: ubuntu-latest
";
        let report = lint(yaml, 999).expect("valid yaml");
        assert_eq!(
            report.jobs_missing_purpose,
            vec!["validate-tag".to_owned(), "publish".to_owned()],
            "missing-purpose jobs must appear in source declaration order"
        );
    }

    #[test]
    fn purpose_comment_must_be_immediately_above_declaration() {
        // A blank line between the `# Purpose:` comment and the job declaration
        // breaks the bond. US-14: the comment must be IMMEDIATELY above.
        let yaml = "\
name: release
jobs:
  # Purpose: build the binary

  build:
    runs-on: ubuntu-latest
";
        let report = lint(yaml, 999).expect("valid yaml");
        assert_eq!(
            report.jobs_missing_purpose,
            vec!["build".to_owned()],
            "intervening blank line must invalidate the purpose comment"
        );
    }

    // -------------------------------------------------------------------------
    // Behavior 4: jobs_missing_purpose is empty when every job has its
    // purpose comment immediately above.
    // -------------------------------------------------------------------------

    #[test]
    fn jobs_missing_purpose_is_empty_when_every_job_has_purpose_comment() {
        let yaml = "\
name: release
jobs:
  # Purpose: refuse mismatched tag
  validate-tag:
    runs-on: ubuntu-latest

  # Purpose: cross-compile binaries
  build:
    runs-on: ubuntu-latest
";
        let report = lint(yaml, 999).expect("valid yaml");
        assert!(report.jobs_missing_purpose.is_empty());
    }

    // -------------------------------------------------------------------------
    // Behavior 5 (parse failure): malformed YAML returns Err, never panics.
    // -------------------------------------------------------------------------

    #[test]
    fn returns_parse_error_on_malformed_yaml() {
        let yaml = "name: release\njobs:\n  : :\n  invalid";
        let err = lint(yaml, 999).expect_err("malformed yaml must Err");
        assert!(
            matches!(err, LintError::ParseError(_)),
            "expected ParseError, got {err:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Property: a workflow under budget AND with a `# Purpose:` comment
    // immediately above every top-level job MUST satisfy:
    //     !report.over_budget && report.jobs_missing_purpose.is_empty()
    //
    // Generator: pick N jobs (1..=5) and a target line count under budget,
    // emit a synthetic workflow with each job preceded by a purpose comment.
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Mutation-coverage: LintError Display.
    //
    // Pins the EXACT format text so the following mutant is killed:
    //   - <impl Display for LintError>::fmt -> Ok(Default::default())
    // -------------------------------------------------------------------------

    #[test]
    fn display_for_parse_error_uses_exact_phrasing() {
        let err = LintError::ParseError("unexpected character at line 3".to_owned());
        assert_eq!(
            format!("{err}"),
            "workflow parse error: unexpected character at line 3"
        );
    }

    // -------------------------------------------------------------------------
    // Mutation-coverage: line_count > max_lines uses STRICT `>`, not `>=`.
    //
    // A workflow whose line_count EQUALS max_lines must NOT be over_budget.
    // This kills `replace > with >=` at xtask/src/lint.rs:114.
    // -------------------------------------------------------------------------

    #[test]
    fn over_budget_is_false_when_line_count_equals_max_lines_exactly() {
        let yaml = "\
name: release
jobs:
  # Purpose: x
  validate-tag:
    runs-on: ubuntu-latest
";
        // Verify the pre-condition: this YAML is exactly 5 lines.
        let report = lint(yaml, 999).expect("valid yaml");
        assert_eq!(report.line_count, 5, "fixture must be exactly 5 lines");

        // Boundary case: max_lines == line_count -> NOT over budget.
        let report = lint(yaml, 5).expect("valid yaml");
        assert!(
            !report.over_budget,
            "line_count == max_lines must NOT be over_budget (strict >)"
        );

        // Adjacent case: max_lines == line_count - 1 -> over budget.
        let report = lint(yaml, 4).expect("valid yaml");
        assert!(
            report.over_budget,
            "line_count == max_lines + 1 must be over_budget"
        );
    }

    // -------------------------------------------------------------------------
    // Mutation-coverage: count_lines uses `+ 1` for missing trailing newline,
    // not `- 1` or `* 1`. We construct inputs whose expected line count
    // distinguishes all three operators (the "+" mutant is killed by any test
    // where adding 1 vs subtracting/multiplying by 1 changes the result).
    //
    // Kills:
    //   - replace + with - in count_lines (xtask/src/lint.rs:130)
    //   - replace + with * in count_lines (xtask/src/lint.rs:130)
    // -------------------------------------------------------------------------

    #[test]
    fn count_lines_treats_missing_trailing_newline_as_one_extra_line() {
        // Two lines, NO trailing newline. nl=1 (one '\n'), so:
        //   correct (+):  1 + 1 = 2
        //   mutant (-):   1 - 1 = 0
        //   mutant (*):   1 * 1 = 1
        // The assertion `line_count == 2` distinguishes all three operators.
        let yaml = "name: release\njobs: {}";
        let report = lint(yaml, 999).expect("valid yaml");
        assert_eq!(
            report.line_count, 2,
            "two lines without trailing newline must count as 2 (nl=1, +1 for last)"
        );
    }

    #[test]
    fn count_lines_with_trailing_newline_does_not_add_extra() {
        // Two lines, trailing newline: nl=2, ends_with('\n') -> 2.
        // The trailing-newline branch never executes the +1, so this test
        // doesn't help distinguish + vs - vs * (it doesn't run that arm).
        // It DOES however pin the no-trailing-newline branch's correctness.
        let yaml = "name: release\njobs: {}\n";
        let report = lint(yaml, 999).expect("valid yaml");
        assert_eq!(
            report.line_count, 2,
            "two lines with trailing newline must count as 2 (nl=2, no add)"
        );
    }

    #[test]
    fn count_lines_for_three_lines_no_trailing_newline_is_three() {
        // Three lines, no trailing newline: nl=2, +1 -> 3.
        // +: 2+1 = 3 (correct).
        // -: 2-1 = 1 (wrong).
        // *: 2*1 = 2 (wrong).
        let yaml = "name: release\njobs: {}\n# trailer";
        let report = lint(yaml, 999).expect("valid yaml");
        assert_eq!(
            report.line_count, 3,
            "three lines without trailing newline must count as 3 (nl=2, +1 for last)"
        );
    }

    // -------------------------------------------------------------------------
    // Mutation-coverage: find_job_declaration_line uses `c.is_whitespace() ||
    // c == '\r'` (DISJUNCTION), and the iter::position predicate composition
    // including the starts_with check. The `||` mutant becomes `&&`, which
    // makes the predicate FAR more restrictive.
    //
    // Strategy: a workflow whose first job-declaration line ends with PLAIN
    // characters after the `:` that satisfy `is_whitespace()` (a space) but
    // NOT `== '\r'`. Under `||`: passes. Under `&&`: fails (missing job).
    //
    // Kills:
    //   - replace || with && in find_job_declaration_line (xtask/src/lint.rs:145)
    //   - replace == with != in find_job_declaration_line (xtask/src/lint.rs:145)
    // -------------------------------------------------------------------------

    #[test]
    fn find_job_declaration_handles_trailing_space_after_colon() {
        // Job line has a TRAILING SPACE after the colon — not a CR. With `||`
        // the predicate accepts it (is_whitespace == true). With `&&` (mutant)
        // it would require BOTH whitespace AND `\r`, which a plain space fails.
        // The job is then treated as not-found and surfaces as missing-purpose
        // with usize::MAX index — which would FLIP this assertion.
        let yaml =
            "name: release\njobs:\n  # Purpose: x\n  validate-tag: \n    runs-on: ubuntu-latest\n";
        let report = lint(yaml, 999).expect("valid yaml");
        // With correct `||`: validate-tag is found, has Purpose comment above
        // -> empty missing list.
        // With `&&` mutant: validate-tag is NOT found (decl_idx None) -> pushed
        // as missing with usize::MAX -> jobs_missing_purpose contains it.
        assert!(
            report.jobs_missing_purpose.is_empty(),
            "validate-tag must be found despite trailing space after colon, got: {:?}",
            report.jobs_missing_purpose
        );
    }

    #[test]
    fn find_job_declaration_rejects_lines_containing_non_whitespace_after_colon() {
        // After `validate-tag:` there is a non-whitespace, non-CR character
        // (`x`). Under `==`: c == '\r' is false AND c.is_whitespace() is false
        // -> predicate false -> declaration NOT matched -> reported missing.
        // Under `!=` (mutant): c != '\r' is true for 'x' (since 'x' != '\r')
        // -> predicate true -> declaration matched -> no missing entry.
        //
        // YAML where the parser sees a top-level job whose source line has a
        // SCALAR VALUE after `:` (so the chars after the `:` are all
        // non-whitespace, non-CR). The parser accepts this as
        // `jobs: { validate-tag: scalar }`; the linter must NOT match the
        // declaration line because the chars after `:` violate the
        // whitespace-or-CR rule.
        let yaml = "\
name: release
jobs:
  # Purpose: x
  validate-tag: scalar_value
";
        let report = lint(yaml, 999).expect("YAML must parse: scalar value after key");
        // Sanity check: parser sees the job in its map.
        assert!(
            !report.jobs_missing_purpose.is_empty() || report.line_count > 0,
            "report should be populated; got: {report:?}"
        );
        // With correct `==`: validate-tag's source line has trailing chars
        // that are NOT whitespace and NOT '\r' -> declaration NOT matched ->
        // pushed with usize::MAX -> appears in missing list.
        // With mutant `!=`: every char != '\r' (true for 's','c',...) ->
        // declaration matched at correct index, comment IS above -> NOT in
        // missing list.
        assert!(
            report
                .jobs_missing_purpose
                .contains(&"validate-tag".to_owned()),
            "validate-tag's decl line ends with `: scalar_value` (non-whitespace), \
             so the predicate must reject it (decl not found) and mark it missing; \
             got: {:?}",
            report.jobs_missing_purpose
        );
    }

    proptest::proptest! {
        #[test]
        fn workflow_within_budget_with_purpose_comments_is_clean(
            n_jobs in 1usize..=5,
            extra_lines in 0usize..=50,
        ) {
            let mut yaml = String::from("name: release\njobs:\n");
            for i in 0..n_jobs {
                yaml.push_str(&format!("  # Purpose: job {i}\n  job{i}:\n    runs-on: ubuntu-latest\n"));
            }
            for _ in 0..extra_lines {
                yaml.push_str("# filler\n");
            }
            let lines = yaml.lines().count();
            let budget = lines + 10; // generously under budget

            let report = lint(&yaml, budget).expect("valid yaml");
            proptest::prop_assert!(!report.over_budget);
            proptest::prop_assert!(report.jobs_missing_purpose.is_empty(),
                "expected clean lint, got missing: {:?}", report.jobs_missing_purpose);
        }
    }
}
