// xtask::fs_adapter — the single seam through which xtask reads files.
//
// Per DESIGN component-boundaries.md and the per-step roadmap (DELIVER step
// 01-02): every later subcommand (release-prep, render-formula,
// extract-changelog, lint-workflows) reads files via THIS module. Pure
// functions in `cargo_toml`, `tag`, `formula`, `changelog`, `lint` accept
// `&str` and never touch the filesystem; the CLI dispatcher is the only
// caller of `fs_adapter::read_to_string`.
//
// Why a wrapper rather than a direct `std::fs::read_to_string` call at the
// dispatcher? Two reasons:
//   1. Makes the seam grep-able. A reviewer can `rg fs_adapter::` and see
//      every disk read xtask performs.
//   2. Standardises the error wrapping (path-in-message) so missing-file
//      diagnostics carry the path, which `std::io::Error` does not.
//
// Implemented in DELIVER step 01-02 (Walking Skeleton, US-02 — TAG activity).

use std::path::Path;

/// Read the entire contents of a file into a `String`, wrapping any I/O error
/// with the path so the failure message identifies WHICH file failed.
///
/// This is a thin, intentionally boring wrapper around
/// `std::fs::read_to_string`. The whole point is to be the ONE place file I/O
/// happens in xtask; future xtask code MUST go through this function.
pub fn read_to_string(path: impl AsRef<Path>) -> Result<String, FsError> {
    let path = path.as_ref();
    std::fs::read_to_string(path).map_err(|source| FsError {
        path: path.display().to_string(),
        source,
    })
}

/// File-I/O error tagged with the path that produced it.
#[derive(Debug)]
pub struct FsError {
    pub path: String,
    pub source: std::io::Error,
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "failed to read {}: {}", self.path, self.source)
    }
}

impl std::error::Error for FsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
