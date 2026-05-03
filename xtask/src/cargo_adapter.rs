// xtask::cargo_adapter — thin shell-out wrappers around cargo + a toml_edit
// helper for mutating `[workspace.package].version`.
//
// Step: 01-06 (Walking Skeleton — PREP activity, US-01).
//
// Two surfaces:
//   1. set_workspace_version : in-place mutate a Cargo.toml file's
//      `[workspace.package].version` field via `toml_edit`, preserving
//      formatting and comments. Pure-ish (filesystem write only).
//   2. run_gate : shell out to `cargo fmt --check` / `cargo clippy` /
//      `cargo test`. Returns Ok(()) on exit 0; Err on non-zero, naming the
//      gate so the caller can identify which one failed.
//
// Both are intentionally minimal: no flag-tweaking, no caching, no parallelism.
// Per ADR-011 the xtask is a build-time tool and "boring is correct."

use std::path::Path;
use std::process::Command;

use crate::cargo_toml::Version;

#[derive(Debug)]
pub enum CargoError {
    Io(std::io::Error),
    /// The Cargo.toml text was not valid TOML.
    ParseFailed(String),
    /// The Cargo.toml had no `[workspace.package]` table or no `version` key.
    MissingField,
}

impl std::fmt::Display for CargoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CargoError::Io(e) => write!(f, "i/o error: {e}"),
            CargoError::ParseFailed(s) => write!(f, "Cargo.toml is not valid TOML: {s}"),
            CargoError::MissingField => {
                write!(f, "[workspace.package].version field is missing")
            }
        }
    }
}

impl std::error::Error for CargoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CargoError::Io(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum GateError {
    /// `cargo` itself failed to launch.
    LaunchFailed {
        gate: String,
        source: std::io::Error,
    },
    /// `cargo <gate>` ran but returned non-zero.
    NonZeroExit {
        gate: String,
        code: i32,
        stderr: String,
    },
}

impl std::fmt::Display for GateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GateError::LaunchFailed { gate, source } => {
                write!(f, "failed to launch cargo {gate}: {source}")
            }
            GateError::NonZeroExit { gate, code, .. } => {
                // Stderr from cargo can be huge and noisy; the caller's
                // dispatcher already prints it via `inherit`. We only surface
                // the gate name + exit code here so the message stays short.
                write!(f, "cargo {gate} failed with exit code {code}")
            }
        }
    }
}

impl std::error::Error for GateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GateError::LaunchFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl GateError {
    /// Which CI parity gate produced this error. Used by the CLI dispatcher to
    /// satisfy the AC "the message identifies which gate failed".
    pub fn gate(&self) -> &str {
        match self {
            GateError::LaunchFailed { gate, .. } => gate,
            GateError::NonZeroExit { gate, .. } => gate,
        }
    }
}

/// Mutate `[workspace.package].version` in `cargo_toml_path` to `new_version`,
/// preserving the file's formatting, ordering, and comments via `toml_edit`.
///
/// Returns `Err(CargoError::MissingField)` if the file does not have a
/// `[workspace.package]` table with a `version` key — release-prep should not
/// silently insert the field; if it's missing the workspace is malformed.
pub fn set_workspace_version(
    cargo_toml_path: &Path,
    new_version: &Version,
) -> Result<(), CargoError> {
    let text = std::fs::read_to_string(cargo_toml_path).map_err(CargoError::Io)?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| CargoError::ParseFailed(e.to_string()))?;

    let version_item = doc
        .get_mut("workspace")
        .and_then(|w| w.as_table_mut())
        .and_then(|t| t.get_mut("package"))
        .and_then(|p| p.as_table_mut())
        .and_then(|t| t.get_mut("version"))
        .ok_or(CargoError::MissingField)?;

    *version_item = toml_edit::value(new_version.to_string());

    std::fs::write(cargo_toml_path, doc.to_string()).map_err(CargoError::Io)?;
    Ok(())
}

/// Shell out to one of the CI parity gates (`fmt`, `clippy`, `test`) in
/// `repo`. Returns `Ok(())` on exit 0, `Err(GateError)` otherwise.
///
/// `gate` is a short human-readable label ("fmt"|"clippy"|"test") used in
/// error messages so the maintainer immediately sees which step blocked the
/// release.
///
/// We let cargo's own stdout/stderr stream to the calling terminal (`inherit`)
/// so the maintainer sees real-time progress and the actual lint/test
/// diagnostics — release-prep is interactive tooling, not a CI step.
pub fn run_gate(gate: &str, repo: &Path) -> Result<(), GateError> {
    let mut cmd = Command::new("cargo");
    match gate {
        "fmt" => {
            cmd.args(["fmt", "--all", "--", "--check"]);
        }
        "clippy" => {
            cmd.args([
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ]);
        }
        "test" => {
            cmd.args(["test", "--workspace", "--locked"]);
        }
        other => {
            // Programmer error — caller passed an unknown gate label.
            return Err(GateError::NonZeroExit {
                gate: other.to_owned(),
                code: 2,
                stderr: format!("unknown CI parity gate: {other}"),
            });
        }
    }
    cmd.current_dir(repo);

    let status = cmd.status().map_err(|source| GateError::LaunchFailed {
        gate: gate.to_owned(),
        source,
    })?;

    if status.success() {
        Ok(())
    } else {
        Err(GateError::NonZeroExit {
            gate: gate.to_owned(),
            code: status.code().unwrap_or(-1),
            stderr: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().expect("test fixture must be valid semver")
    }

    #[test]
    fn set_workspace_version_replaces_version_string_in_place() {
        let tempdir = tempfile::tempdir().unwrap();
        let cargo_toml = tempdir.path().join("Cargo.toml");
        std::fs::write(
            &cargo_toml,
            "[workspace]\n\
             resolver = \"2\"\n\
             members = []\n\
             \n\
             [workspace.package]\n\
             version = \"0.1.0\"\n\
             edition = \"2021\"\n",
        )
        .unwrap();

        set_workspace_version(&cargo_toml, &v("0.2.0")).expect("should succeed");

        let after = std::fs::read_to_string(&cargo_toml).unwrap();
        assert!(
            after.contains("version = \"0.2.0\""),
            "version should be 0.2.0 after mutation, got:\n{after}"
        );
        assert!(
            !after.contains("version = \"0.1.0\""),
            "old version 0.1.0 should be gone, got:\n{after}"
        );
        // Formatting preserved: edition is still on its own line and unchanged.
        assert!(
            after.contains("edition = \"2021\""),
            "unrelated fields should be preserved, got:\n{after}"
        );
    }

    #[test]
    fn set_workspace_version_returns_missing_field_when_no_workspace_package() {
        let tempdir = tempfile::tempdir().unwrap();
        let cargo_toml = tempdir.path().join("Cargo.toml");
        std::fs::write(
            &cargo_toml,
            "[workspace]\n\
             resolver = \"2\"\n\
             members = []\n",
        )
        .unwrap();

        let err = set_workspace_version(&cargo_toml, &v("0.2.0"))
            .expect_err("should fail without [workspace.package]");
        assert!(matches!(err, CargoError::MissingField));
    }

    #[test]
    fn run_gate_unknown_label_is_an_error() {
        let tempdir = tempfile::tempdir().unwrap();
        let err = run_gate("nonexistent-gate", tempdir.path())
            .expect_err("unknown gate label must error");
        assert_eq!(err.gate(), "nonexistent-gate");
    }
}
