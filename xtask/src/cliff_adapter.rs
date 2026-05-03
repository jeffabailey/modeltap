// xtask::cliff_adapter — pure-Rust changelog generation by walking commits via
// `git log` and grouping by conventional-commit prefix.
//
// Step: 01-06 (Walking Skeleton — PREP activity, US-01).
//
// DECISION: For the Walking Skeleton, we use a hand-written changelog
// generator rather than shelling out to `git-cliff`. Reasons:
//   - Zero new external binary dependency for CI / contributors
//   - Deterministic output (no third-party version drift)
//   - Trivial to test with real git tempdirs
// `git-cliff` integration remains a future enhancement (see
// component-boundaries.md §8 — the cliff.toml config is still authored at
// repo root for the eventual swap).
//
// Per DESIGN component-boundaries.md §2.3 the adapter remains thin: one
// public function that takes typed input and writes a single artifact
// (CHANGELOG.md). Grouping rules and templating live HERE because they are
// tied to the keep-a-changelog convention this adapter implements; if and
// when we swap to git-cliff, the rules move into cliff.toml and this module
// shrinks to a single Command spawn.

use std::path::Path;
use std::process::Command;

use crate::cargo_toml::Version;

#[derive(Debug)]
pub enum CliffError {
    Io(std::io::Error),
    GitFailed { code: i32, stderr: String },
}

impl std::fmt::Display for CliffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliffError::Io(e) => write!(f, "i/o error: {e}"),
            CliffError::GitFailed { code, stderr } => {
                write!(f, "git log failed with exit code {code}: {}", stderr.trim())
            }
        }
    }
}

impl std::error::Error for CliffError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliffError::Io(e) => Some(e),
            _ => None,
        }
    }
}

/// Conventional-commit type → keep-a-changelog section heading.
///
/// Mapping per component-boundaries.md §8 (the future cliff.toml grouping).
/// Anything that does not match a known prefix is bucketed under "Misc".
fn section_for_prefix(prefix: &str) -> &'static str {
    match prefix {
        "feat" => "Added",
        "fix" => "Fixed",
        "docs" => "Documentation",
        "refactor" => "Changed",
        "perf" => "Performance",
        _ => "Misc",
    }
}

/// Display order for the grouping headings (so `Added` always appears before
/// `Fixed` before `Documentation`, regardless of commit insertion order). A
/// section that has no commits is omitted.
const SECTION_ORDER: &[&str] = &[
    "Added",
    "Fixed",
    "Changed",
    "Performance",
    "Documentation",
    "Misc",
];

/// Regenerate `CHANGELOG.md` at `repo`, prepending a new `## [version]`
/// section that groups commits since `since_tag` (or all commits, when
/// `since_tag` is `None`) by conventional-commit type.
///
/// The new section is PREPENDED to any existing CHANGELOG.md (or creates a
/// fresh one with a standard keep-a-changelog header if none exists). This
/// preserves prior release history.
pub fn regenerate_changelog(
    repo: &Path,
    version: &Version,
    since_tag: Option<&str>,
) -> Result<(), CliffError> {
    let commits = collect_commits_since(repo, since_tag)?;
    let new_section = render_section(version, &commits);
    write_changelog(repo, &new_section)
}

/// Collect `(prefix, subject)` pairs from `git log`, oldest-first.
///
/// `since_tag = None` ⇒ all commits.
/// `since_tag = Some("v0.1.0")` ⇒ commits reachable from HEAD but not from
/// `v0.1.0` (i.e. `git log v0.1.0..HEAD`). If the tag does not exist git's
/// non-zero exit is returned as `CliffError::GitFailed`.
fn collect_commits_since(
    repo: &Path,
    since_tag: Option<&str>,
) -> Result<Vec<(String, String)>, CliffError> {
    let mut cmd = Command::new("git");
    cmd.args(["log", "--pretty=format:%s", "--reverse"]);
    if let Some(tag) = since_tag {
        cmd.arg(format!("{tag}..HEAD"));
    }
    cmd.current_dir(repo);

    let output = cmd.output().map_err(CliffError::Io)?;
    if !output.status.success() {
        return Err(CliffError::GitFailed {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();
    for line in stdout.lines() {
        if let Some((prefix, subject)) = split_conventional(line) {
            commits.push((prefix.to_owned(), subject.to_owned()));
        } else {
            // Non-conventional commit ⇒ bucket under "Misc" with full subject.
            commits.push((String::from("_other"), line.to_owned()));
        }
    }
    Ok(commits)
}

/// Split a commit subject into `(prefix, rest)` using stdlib pattern matching
/// (no regex, per project convention). A conventional-commit subject is
/// `<type>(<scope>)?: <subject>`. We accept either `feat: ...` or
/// `feat(ci): ...`.
///
/// Returns `None` when the subject does not match the conventional-commit shape.
fn split_conventional(subject: &str) -> Option<(&str, &str)> {
    // Find the first ':'.
    let colon = subject.find(':')?;
    let head = &subject[..colon];
    let tail = subject[colon + 1..].trim_start();

    // Strip an optional `(scope)` suffix from the head.
    let prefix = match head.find('(') {
        Some(open) if head.ends_with(')') => &head[..open],
        Some(_) => head, // unbalanced, treat whole head as the prefix candidate
        None => head,
    };

    // Validate that the prefix is non-empty and lowercase-alphabetic. We
    // intentionally accept any lowercase token so unknown prefixes (e.g.
    // `style:`, `test:`) still parse and fall through to the "Misc" bucket.
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_lowercase()) {
        return None;
    }

    Some((prefix, tail))
}

/// Render the markdown section for `version` from `commits`, grouped by
/// conventional-commit type. Sections with no commits are omitted.
///
/// The output ends with exactly one trailing newline so successive sections
/// stack cleanly when prepended to an existing CHANGELOG.md.
fn render_section(version: &Version, commits: &[(String, String)]) -> String {
    use std::collections::BTreeMap;

    // Bucket commits by section heading.
    let mut buckets: BTreeMap<&'static str, Vec<&str>> = BTreeMap::new();
    for (prefix, subject) in commits {
        let heading = section_for_prefix(prefix);
        buckets.entry(heading).or_default().push(subject.as_str());
    }

    let mut out = String::new();
    out.push_str(&format!("## [{version}]\n\n"));

    let mut any_section_emitted = false;
    for &heading in SECTION_ORDER {
        if let Some(items) = buckets.get(heading) {
            any_section_emitted = true;
            out.push_str(&format!("### {heading}\n\n"));
            for item in items {
                out.push_str(&format!("- {item}\n"));
            }
            out.push('\n');
        }
    }

    if !any_section_emitted {
        // Empty release — still emit a placeholder so the section heading is
        // visible. This is the "no commits since last tag" case which can
        // happen for re-tag attempts.
        out.push_str("_No notable changes._\n\n");
    }

    out
}

/// Prepend `new_section` to `repo/CHANGELOG.md`, creating the file with a
/// keep-a-changelog header if it does not yet exist.
fn write_changelog(repo: &Path, new_section: &str) -> Result<(), CliffError> {
    let path = repo.join("CHANGELOG.md");

    let final_text = if path.exists() {
        let existing = std::fs::read_to_string(&path).map_err(CliffError::Io)?;
        // Find where the first existing `## [` section begins; everything
        // before it is the file header (keep-a-changelog preamble). Insert
        // the new section between header and first existing section.
        if let Some(idx) = existing.find("\n## [") {
            // +1 to keep the leading newline as the boundary.
            let (header, rest) = existing.split_at(idx + 1);
            format!("{header}{new_section}{rest}")
        } else {
            // No prior sections — append to whatever header is there.
            format!("{existing}\n{new_section}")
        }
    } else {
        format!(
            "# Changelog\n\
             \n\
             All notable changes to this project will be documented in this file.\n\
             \n\
             {new_section}"
        )
    };

    std::fs::write(&path, final_text).map_err(CliffError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn v(s: &str) -> Version {
        s.parse().expect("test fixture must be valid semver")
    }

    fn init_repo(path: &Path) {
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(path)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .status()
                .expect("invoke git");
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["init", "--quiet", "--initial-branch=main"]);
        // Need at least one commit before further commits can be tagged or logged.
        std::fs::write(path.join("seed.txt"), "x\n").unwrap();
        run(&["add", "seed.txt"]);
        run(&["commit", "--quiet", "-m", "chore: initial"]);
    }

    fn add_commit(path: &Path, msg: &str, file: &str) {
        std::fs::write(path.join(file), "x\n").unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(path)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .status()
                .expect("invoke git");
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["add", file]);
        run(&["commit", "--quiet", "-m", msg]);
    }

    #[test]
    fn split_conventional_parses_simple_prefix() {
        let (p, t) = split_conventional("feat: add something").unwrap();
        assert_eq!(p, "feat");
        assert_eq!(t, "add something");
    }

    #[test]
    fn split_conventional_parses_scoped_prefix() {
        let (p, t) = split_conventional("fix(ci): correct release pipeline").unwrap();
        assert_eq!(p, "fix");
        assert_eq!(t, "correct release pipeline");
    }

    #[test]
    fn split_conventional_rejects_non_conventional() {
        assert!(split_conventional("just some random text").is_none());
        assert!(split_conventional("BREAKING CHANGE: x").is_none());
    }

    #[test]
    fn regenerate_changelog_writes_section_grouped_by_type() {
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        add_commit(repo.path(), "feat: new feature", "feat.txt");
        add_commit(repo.path(), "fix: bug squash", "fix.txt");
        add_commit(repo.path(), "docs: update readme", "docs.txt");

        regenerate_changelog(repo.path(), &v("0.2.0"), None)
            .expect("regenerate_changelog should succeed");

        let changelog = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        assert!(changelog.contains("## [0.2.0]"), "got:\n{changelog}");
        assert!(changelog.contains("### Added"), "got:\n{changelog}");
        assert!(changelog.contains("### Fixed"), "got:\n{changelog}");
        assert!(changelog.contains("### Documentation"), "got:\n{changelog}");
        assert!(
            changelog.contains("- new feature"),
            "feat subject should appear under Added; got:\n{changelog}"
        );
        assert!(
            changelog.contains("- bug squash"),
            "fix subject should appear under Fixed; got:\n{changelog}"
        );
    }

    #[test]
    fn regenerate_changelog_prepends_to_existing_changelog() {
        let repo = tempfile::tempdir().unwrap();
        init_repo(repo.path());
        // Pre-existing CHANGELOG with a 0.1.0 section.
        std::fs::write(
            repo.path().join("CHANGELOG.md"),
            "# Changelog\n\nAll notable changes...\n\n## [0.1.0]\n\nInitial release.\n",
        )
        .unwrap();
        add_commit(repo.path(), "feat: another feature", "another.txt");

        regenerate_changelog(repo.path(), &v("0.2.0"), None)
            .expect("regenerate_changelog should succeed");

        let changelog = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        // Both sections present.
        assert!(changelog.contains("## [0.2.0]"), "got:\n{changelog}");
        assert!(changelog.contains("## [0.1.0]"), "got:\n{changelog}");
        // 0.2.0 must appear BEFORE 0.1.0 (newest at top).
        let idx_new = changelog.find("## [0.2.0]").unwrap();
        let idx_old = changelog.find("## [0.1.0]").unwrap();
        assert!(
            idx_new < idx_old,
            "newest section must precede older sections; got:\n{changelog}"
        );
        // Old content survived.
        assert!(
            changelog.contains("Initial release."),
            "prior section content must be preserved; got:\n{changelog}"
        );
    }
}
