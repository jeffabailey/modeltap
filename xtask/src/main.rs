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

use xtask::cargo_toml::parse_workspace_version;
use xtask::fs_adapter;
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
        Cmd::ReleasePrep { .. } => not_yet_implemented("release-prep"),
        Cmd::RenderFormula { .. } => not_yet_implemented("render-formula"),
        Cmd::ExtractChangelog { .. } => not_yet_implemented("extract-changelog"),
        Cmd::LintWorkflows { .. } => not_yet_implemented("lint-workflows"),
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

fn not_yet_implemented(subcommand: &str) -> ExitCode {
    panic!(
        "Not yet implemented — RED scaffold. The `{subcommand}` subcommand \
         graduates from RED to GREEN in a later DELIVER step."
    );
}
