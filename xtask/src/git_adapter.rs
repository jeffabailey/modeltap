// xtask::git_adapter — thin shell-out wrapper around git.
//
// Step: 01-06 (Walking Skeleton — PREP activity, US-01).
//
// Per DESIGN component-boundaries.md §2.3, adapters are kept THIN: one struct
// method per shell-out, no logic. They translate a typed input to a CLI
// invocation and a CLI exit-code/stdout to a typed Result.
//
// Currently exposes:
//   - is_dirty(repo)     -> bool          : `git status --porcelain` empty ⇒ clean
//   - dirty_paths(repo)  -> Vec<String>   : the porcelain lines, one per dirty entry
//
// Future xtask subcommands will extend this module with `current_ref`,
// `tag_exists`, `commits_since_tag`, etc. Each such helper remains a 1:1
// shell-out wrapper — non-trivial reasoning belongs in pure functions in
// `cargo_toml`/`changelog`/`tag` modules.

use std::path::Path;
use std::process::Command;

#[derive(Debug)]
pub enum GitError {
    /// `git` itself failed to launch (binary missing, repo path invalid, etc.)
    LaunchFailed(std::io::Error),
    /// `git` ran but returned non-zero. Captures stderr for diagnostics.
    NonZeroExit { code: i32, stderr: String },
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::LaunchFailed(e) => write!(f, "failed to launch git: {e}"),
            GitError::NonZeroExit { code, stderr } => {
                write!(f, "git exited with code {code}: {}", stderr.trim())
            }
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GitError::LaunchFailed(e) => Some(e),
            _ => None,
        }
    }
}

/// Return `true` when the working tree at `repo` has any uncommitted change
/// (staged, unstaged, or untracked) — i.e. `git status --porcelain` produces
/// non-empty output.
///
/// We use `--porcelain` (v1) because its output is stable across git versions
/// and trivially cheap to test for emptiness. `--porcelain=v2` carries more
/// detail than we need for a boolean clean check.
pub fn is_dirty(repo: &Path) -> Result<bool, GitError> {
    dirty_paths(repo).map(|paths| !paths.is_empty())
}

/// Return the porcelain-formatted lines from `git status --porcelain` at
/// `repo`. An empty `Vec` means the tree is clean. Each entry is the raw
/// line as git emits it (`XY path`), preserving the staged/unstaged/untracked
/// distinction in the leading two columns. Callers that just want a boolean
/// should use [`is_dirty`]; callers that want to *show the user what's dirty*
/// (e.g. release-prep on refusal) should use this one.
///
/// Lines are returned in git's own order — typically staged → unstaged →
/// untracked. We do not sort or deduplicate; a downstream display layer can
/// decide.
pub fn dirty_paths(repo: &Path) -> Result<Vec<String>, GitError> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .map_err(GitError::LaunchFailed)?;

    if !output.status.success() {
        return Err(GitError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Initialise a fresh git repo in `path` with one initial commit so that
    /// HEAD exists. Returns nothing — panics on any setup failure since this
    /// is test fixture code.
    fn init_repo_with_initial_commit(path: &Path) {
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
        std::fs::write(path.join("seed.txt"), "x\n").unwrap();
        run(&["add", "seed.txt"]);
        run(&["commit", "--quiet", "-m", "initial"]);
    }

    #[test]
    fn is_dirty_returns_false_for_clean_tree() {
        let repo = tempfile::tempdir().unwrap();
        init_repo_with_initial_commit(repo.path());

        let dirty = is_dirty(repo.path()).expect("is_dirty should succeed on clean tree");
        assert!(!dirty, "freshly committed tree must report clean");
    }

    #[test]
    fn is_dirty_returns_true_for_untracked_file() {
        let repo = tempfile::tempdir().unwrap();
        init_repo_with_initial_commit(repo.path());

        std::fs::write(repo.path().join("untracked.txt"), "y\n").unwrap();

        let dirty = is_dirty(repo.path()).expect("is_dirty should succeed");
        assert!(dirty, "untracked file must make the tree dirty");
    }

    #[test]
    fn is_dirty_returns_true_for_modified_tracked_file() {
        let repo = tempfile::tempdir().unwrap();
        init_repo_with_initial_commit(repo.path());

        std::fs::write(repo.path().join("seed.txt"), "MODIFIED\n").unwrap();

        let dirty = is_dirty(repo.path()).expect("is_dirty should succeed");
        assert!(dirty, "modified tracked file must make the tree dirty");
    }

    #[test]
    fn dirty_paths_empty_for_clean_tree() {
        let repo = tempfile::tempdir().unwrap();
        init_repo_with_initial_commit(repo.path());

        let paths = dirty_paths(repo.path()).expect("dirty_paths should succeed on clean tree");
        assert!(
            paths.is_empty(),
            "clean tree must produce no porcelain lines"
        );
    }

    #[test]
    fn dirty_paths_lists_modified_and_untracked_with_porcelain_prefix() {
        let repo = tempfile::tempdir().unwrap();
        init_repo_with_initial_commit(repo.path());

        std::fs::write(repo.path().join("seed.txt"), "MODIFIED\n").unwrap();
        std::fs::write(repo.path().join("new.txt"), "y\n").unwrap();

        let paths = dirty_paths(repo.path()).expect("dirty_paths should succeed");
        assert_eq!(paths.len(), 2, "expected one line per dirty entry");

        // Modified-but-not-staged tracked file: `XY` columns are ` M`.
        assert!(
            paths.iter().any(|line| line == " M seed.txt"),
            "modified seed.txt must appear with ` M` prefix; got {paths:?}"
        );
        // Untracked file: `XY` columns are `??`.
        assert!(
            paths.iter().any(|line| line == "?? new.txt"),
            "untracked new.txt must appear with `??` prefix; got {paths:?}"
        );
    }
}
