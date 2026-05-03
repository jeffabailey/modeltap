// xtask — CLI dispatcher.
//
// DELIVER step 01-02 introduced the clap-derive parser (replacing the raw
// env::args dispatcher from the DISTILL scaffold) and wired the first
// subcommand (`validate-tag`) end-to-end through the pure
// `xtask::tag::assert_tag_matches` function and the `xtask::fs_adapter` seam.
//
// Other subcommands (release-prep, render-formula, extract-changelog,
// lint-workflows) remain RED-scaffold panics; each will graduate to GREEN in
// its own DELIVER step (the roadmap walking-skeleton sequence).
//
// Subcommand surfaces are declared as clap variants up-front so that:
//   - `xtask --help` lists every planned subcommand from day one,
//   - the scaffolded subcommands fail with a clear "not implemented" message
//     rather than `clap`'s generic "unrecognised subcommand", and
//   - adding a subcommand in a later step is local to that step's variant
//     handler.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use xtask::cargo_adapter;
use xtask::cargo_toml::{assert_monotonic, parse_workspace_version, Version};
use xtask::changelog::{extract_section, ChangelogError};
use xtask::cliff_adapter;
use xtask::formula::{is_valid_sha256, render, FormulaCtx, TargetEntry};
use xtask::fs_adapter;
use xtask::git_adapter;
use xtask::lint::lint as lint_workflow;
use xtask::tag::assert_tag_matches;

/// Build-time tooling for the modeltap release pipeline.
#[derive(Debug, Parser)]
#[command(name = "xtask", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Prepare a release: refuse on dirty tree, bump Cargo.toml + Cargo.lock,
    /// regenerate CHANGELOG.md, run CI parity gates. (DELIVER: scaffold.)
    ReleasePrep {
        /// New workspace version, e.g. `0.0.1-rc1`.
        #[arg(long)]
        version: String,
    },
    /// Assert the supplied git tag equals `v` + the workspace.package.version
    /// in Cargo.toml. Reads `./Cargo.toml` from the current working directory
    /// via `xtask::fs_adapter::read_to_string`.
    ValidateTag {
        /// The git tag to validate, e.g. `v0.1.0`.
        #[arg(long)]
        tag: String,
    },
    /// Render the Homebrew formula from a Tera template + sha256 sidecars.
    /// (DELIVER: scaffold.)
    RenderFormula {
        #[arg(long)]
        version: String,
        #[arg(long)]
        template: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long = "sha256-dir")]
        sha256_dir: PathBuf,
        #[arg(long = "release-base-url")]
        release_base_url: String,
    },
    /// Extract a single `## [version]` section from CHANGELOG.md to a file.
    /// (DELIVER: scaffold.)
    ExtractChangelog {
        #[arg(long)]
        version: String,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Lint a GitHub Actions workflow file: line-count cap + `# Purpose:`
    /// comment per job. (DELIVER: scaffold.)
    LintWorkflows {
        #[arg(long)]
        workflow: PathBuf,
        #[arg(long = "max-lines")]
        max_lines: usize,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Cmd::ValidateTag { tag } => run_validate_tag(&tag),
        Cmd::ReleasePrep { version } => run_release_prep(&version),
        Cmd::RenderFormula {
            version,
            template,
            output,
            sha256_dir,
            release_base_url,
        } => run_render_formula(&version, &template, &output, &sha256_dir, &release_base_url),
        Cmd::ExtractChangelog {
            version,
            input,
            output,
        } => run_extract_changelog(&version, &input, &output),
        Cmd::LintWorkflows {
            workflow,
            max_lines,
        } => run_lint_workflows(&workflow, max_lines),
    }
}

/// `validate-tag` end-to-end:
///   1. Read `./Cargo.toml` via the `fs_adapter` seam.
///   2. Parse the workspace version (pure function).
///   3. Compare `tag` with `format!("v{version}")` (pure function).
///   4. Exit 0 on match; exit non-zero with stderr diagnostic on mismatch.
fn run_validate_tag(tag: &str) -> ExitCode {
    let cargo_toml_path = std::path::Path::new("Cargo.toml");
    let text = match fs_adapter::read_to_string(cargo_toml_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("xtask validate-tag: {e}");
            return ExitCode::from(2);
        }
    };

    let version = match parse_workspace_version(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("xtask validate-tag: cannot parse workspace version: {e}");
            return ExitCode::from(2);
        }
    };

    match assert_tag_matches(tag, &version) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}

/// `extract-changelog` end-to-end:
///   1. Parse `--version` as semver via the same `Version` newtype the rest of
///      xtask uses.
///   2. Read `--input` (typically CHANGELOG.md) via the `fs_adapter` seam.
///   3. Run the pure `extract_section` against the text.
///   4. On success: write the body to `--output`. On `SectionNotFound`: print
///      `"CHANGELOG.md has no [<version>] section"` to stderr, exit non-zero,
///      and write NO output file.
///
/// We deliberately extract BEFORE opening the output file so a failed
/// extraction never leaves a partial RELEASE_NOTES.md behind (US-05
/// @infrastructure-failure scenario requires "no RELEASE_NOTES.md file is
/// written" on missing section).
fn run_extract_changelog(
    version_str: &str,
    input: &std::path::Path,
    output: &std::path::Path,
) -> ExitCode {
    let version: Version = match version_str.parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("xtask extract-changelog: --version is not a valid semver: {e}");
            return ExitCode::from(2);
        }
    };

    let text = match fs_adapter::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("xtask extract-changelog: {e}");
            return ExitCode::from(2);
        }
    };

    let body = match extract_section(&text, &version) {
        Ok(b) => b,
        Err(ChangelogError::SectionNotFound) => {
            // Use the input file's display name (e.g. "CHANGELOG.md") so the
            // message matches the maintainer's mental model rather than a
            // hard-coded literal. The walking-skeleton failure scenario invokes
            // with `--input CHANGELOG.md`, so the produced message reads
            // "CHANGELOG.md has no [0.2.0] section".
            eprintln!("{} has no [{}] section", input.display(), version);
            return ExitCode::from(1);
        }
    };

    if let Err(e) = std::fs::write(output, body) {
        eprintln!(
            "xtask extract-changelog: failed to write {}: {e}",
            output.display()
        );
        return ExitCode::from(2);
    }

    ExitCode::SUCCESS
}

/// `lint-workflows` end-to-end:
///   1. Read the workflow file via the `fs_adapter` seam.
///   2. Run the pure `lint::lint` against the text + budget.
///   3. Decide exit code:
///        - parse error                    -> exit 2 (usage / input shape)
///        - over_budget OR missing-purpose -> exit 1 with stderr diagnostic
///        - clean                          -> exit 0
///
/// The diagnostic format on failure surfaces BOTH classes of issue when both
/// are present, so the maintainer fixes everything in one CI iteration:
///
///   ```
///   xtask lint-workflows: <path>: workflow has 270 lines, exceeds 250-line limit
///   xtask lint-workflows: <path>: jobs missing `# Purpose:` comment: build, publish
///   ```
fn run_lint_workflows(workflow_path: &std::path::Path, max_lines: usize) -> ExitCode {
    let text = match fs_adapter::read_to_string(workflow_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("xtask lint-workflows: {e}");
            return ExitCode::from(2);
        }
    };

    let report = match lint_workflow(&text, max_lines) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("xtask lint-workflows: {}: {e}", workflow_path.display());
            return ExitCode::from(2);
        }
    };

    let mut had_failure = false;

    if report.over_budget {
        eprintln!(
            "xtask lint-workflows: {}: workflow has {} lines, exceeds {}-line limit",
            workflow_path.display(),
            report.line_count,
            max_lines
        );
        had_failure = true;
    }

    if !report.jobs_missing_purpose.is_empty() {
        eprintln!(
            "xtask lint-workflows: {}: jobs missing `# Purpose:` comment: {}",
            workflow_path.display(),
            report.jobs_missing_purpose.join(", ")
        );
        had_failure = true;
    }

    if had_failure {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// `render-formula` end-to-end:
/// 1. Parse `--version` as semver via the `Version` newtype.
/// 2. Read the Tera template file via the `fs_adapter` seam.
/// 3. Walk `--sha256-dir` for files matching
///    `modeltap-{version}-{triple}.tar.gz.sha256`. For each sidecar:
///    a. Read the sidecar via `fs_adapter`.
///    b. Trim whitespace (sidecars typically end with `\n`).
///    c. Validate the trimmed content with `is_valid_sha256`. Reject with
///    the offending filename in the error if it is not exactly 64
///    lowercase hex chars.
/// 4. Build a `FormulaCtx` and render the template via the pure
///    `formula::render` function.
/// 5. Write the rendered formula to `--output`.
///
/// Sidecars are sorted by filename for deterministic output ordering across
/// runs (the rendered formula is committed to a tap repo where churn matters).
fn run_render_formula(
    version_str: &str,
    template_path: &std::path::Path,
    output_path: &std::path::Path,
    sha256_dir: &std::path::Path,
    release_base_url: &str,
) -> ExitCode {
    let version: Version = match version_str.parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("xtask render-formula: --version is not a valid semver: {e}");
            return ExitCode::from(2);
        }
    };

    let template_text = match fs_adapter::read_to_string(template_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("xtask render-formula: {e}");
            return ExitCode::from(2);
        }
    };

    let targets = match collect_targets_from_sidecar_dir(&version, sha256_dir) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("xtask render-formula: {e}");
            return ExitCode::from(1);
        }
    };

    if targets.is_empty() {
        eprintln!(
            "xtask render-formula: no sidecar files matching `modeltap-{version}-<triple>.tar.gz.sha256` found in {}",
            sha256_dir.display()
        );
        return ExitCode::from(1);
    }

    let ctx = FormulaCtx {
        version,
        release_base_url: release_base_url.to_owned(),
        targets,
    };

    let rendered = match render(&template_text, &ctx) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("xtask render-formula: {e}");
            return ExitCode::from(1);
        }
    };

    if let Err(e) = std::fs::write(output_path, rendered) {
        eprintln!(
            "xtask render-formula: failed to write {}: {e}",
            output_path.display()
        );
        return ExitCode::from(2);
    }

    ExitCode::SUCCESS
}

/// Walk `sha256_dir` for `modeltap-{version}-{triple}.tar.gz.sha256` files and
/// build a `Vec<TargetEntry>` with deterministic (filename-sorted) order.
///
/// Returns `Err(String)` with a descriptive message on:
///   - I/O error reading the directory or any sidecar
///   - A sidecar whose trimmed content is not a bare 64-char lowercase hex sha256
///     (the offending filename is included in the message per AC).
fn collect_targets_from_sidecar_dir(
    version: &Version,
    sha256_dir: &std::path::Path,
) -> Result<Vec<TargetEntry>, String> {
    let prefix = format!("modeltap-{version}-");
    let suffix = ".tar.gz.sha256";

    let entries = std::fs::read_dir(sha256_dir).map_err(|e| {
        format!(
            "failed to read sidecar directory {}: {e}",
            sha256_dir.display()
        )
    })?;

    let mut sidecar_files: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|e| format!("failed to enumerate {}: {e}", sha256_dir.display()))?;
        let path = entry.path();
        let Some(filename) = path.file_name().and_then(|f| f.to_str()) else {
            continue;
        };
        if !filename.starts_with(&prefix) || !filename.ends_with(suffix) {
            continue;
        }
        sidecar_files.push((filename.to_owned(), path));
    }
    sidecar_files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut targets: Vec<TargetEntry> = Vec::with_capacity(sidecar_files.len());
    for (filename, path) in sidecar_files {
        // Strip prefix + suffix to recover the rust target triple.
        let triple = filename[prefix.len()..filename.len() - suffix.len()].to_owned();
        let archive_name = filename[..filename.len() - ".sha256".len()].to_owned();

        let raw = fs_adapter::read_to_string(&path)
            .map_err(|e| format!("failed to read sidecar {}: {e}", path.display()))?;
        let sha256 = raw.trim().to_owned();
        if !is_valid_sha256(&sha256) {
            return Err(format!(
                "sidecar {filename} is not a bare 64-char lowercase hex sha256"
            ));
        }

        targets.push(TargetEntry {
            triple,
            archive_name,
            sha256,
        });
    }
    Ok(targets)
}

/// `release-prep` end-to-end (DELIVER step 01-06, US-01):
///   1. Refuse on dirty working tree.
///   2. Parse the current `[workspace.package].version` from `./Cargo.toml`.
///   3. Refuse non-monotonic bump (proposed must be strictly greater).
///   4. Mutate `Cargo.toml` to the new version (`cargo_adapter`).
///   5. Regenerate `CHANGELOG.md` from conventional commits since `v<current>`
///      via `cliff_adapter`.
///   6. Run CI parity gates in order: `cargo fmt --check` → `cargo clippy` →
///      `cargo test`. Halt non-zero on first failure, naming the failed gate.
///   7. Print next-step instructions (commit, push, open PR) to stdout.
///
/// Exit codes:
///   - 0  on success.
///   - 1  for refusals (dirty tree, non-monotonic bump, gate failure).
///   - 2  for I/O / parse errors that prevent reasoning about the workspace.
fn run_release_prep(version_str: &str) -> ExitCode {
    let repo = std::path::Path::new(".");
    let cargo_toml_path = repo.join("Cargo.toml");

    // 1. Dirty-tree check (refuse with NO file modification).
    match git_adapter::is_dirty(repo) {
        Ok(false) => {}
        Ok(true) => {
            eprintln!("working tree is dirty: commit or stash first");
            return ExitCode::from(1);
        }
        Err(e) => {
            eprintln!("xtask release-prep: {e}");
            return ExitCode::from(2);
        }
    }

    // 2. Parse current version.
    let cargo_toml_text = match fs_adapter::read_to_string(&cargo_toml_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("xtask release-prep: {e}");
            return ExitCode::from(2);
        }
    };
    let current = match parse_workspace_version(&cargo_toml_text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("xtask release-prep: cannot parse workspace version: {e}");
            return ExitCode::from(2);
        }
    };

    // Parse proposed version.
    let proposed: Version = match version_str.parse() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("xtask release-prep: --version is not a valid semver: {e}");
            return ExitCode::from(2);
        }
    };

    // 3. Monotonic-bump check.
    if let Err(e) = assert_monotonic(&current, &proposed) {
        eprintln!("{e}");
        return ExitCode::from(1);
    }

    // 4. Mutate Cargo.toml.
    if let Err(e) = cargo_adapter::set_workspace_version(&cargo_toml_path, &proposed) {
        eprintln!("xtask release-prep: failed to update Cargo.toml: {e}");
        return ExitCode::from(2);
    }

    // 5. Regenerate CHANGELOG.md from commits since the previous tag. We pass
    //    `Some("v<current>")` so the changelog covers commits between the
    //    previous release tag and HEAD. If the tag does not exist (e.g. first
    //    release), we fall back to "all commits".
    let since_tag = format!("v{current}");
    let cliff_result = cliff_adapter::regenerate_changelog(repo, &proposed, Some(&since_tag));
    if cliff_result.is_err() {
        // Tag may not exist — retry with no since_tag (all commits).
        if let Err(e) = cliff_adapter::regenerate_changelog(repo, &proposed, None) {
            eprintln!("xtask release-prep: failed to regenerate CHANGELOG.md: {e}");
            return ExitCode::from(2);
        }
    }

    // 6. CI parity gates in strict order.
    for gate in ["fmt", "clippy", "test"] {
        if let Err(e) = cargo_adapter::run_gate(gate, repo) {
            // Identify the failed gate (AC: "the message identifies which gate failed").
            eprintln!("xtask release-prep: CI parity gate failed: {}", e.gate());
            eprintln!("xtask release-prep: {e}");
            return ExitCode::from(1);
        }
    }

    // 7. Next-step instructions.
    println!("release-prep: success.");
    println!();
    println!("Next steps:");
    println!("  1. Review the diff: git diff Cargo.toml CHANGELOG.md");
    println!("  2. Commit the bump: git commit -am \"chore: prepare {proposed}\"");
    println!("  3. Push the branch: git push -u origin HEAD");
    println!("  4. Open a PR for review and merge.");

    ExitCode::SUCCESS
}

// `not_yet_implemented` was the RED scaffold for subcommands awaiting their
// DELIVER step. As of step 01-06 every Cmd variant has a concrete handler, so
// the helper has been removed (graduating from RED to GREEN means deleting
// the panic — there is nothing left to NotImplemented).
