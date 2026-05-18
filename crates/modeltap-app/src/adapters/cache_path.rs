//! Cache-path resolver (AC-23-1).
//!
//! Resolution order:
//!   1. `cli_override` (e.g. a future `--cache-path <path>` flag) — highest
//!      priority so tests and power users can pin a location.
//!   2. `env_override` — the production caller passes
//!      `std::env::var_os("MODELTAP_CACHE_PATH").as_deref()`.
//!   3. `dirs::data_dir().join("modeltap").join("cache.sqlite")` — the
//!      documented default per technology-stack.md §3 + acceptance-criteria
//!      AC-23-1.
//!
//! Returns `CachePathError::NoDataDir` only when `dirs::data_dir()` itself
//! cannot resolve (extremely rare on supported platforms — macOS, Linux,
//! WSL — but possible if `$HOME` is unset in tests). Callers must propagate
//! the error; the launch path falls back to cold-start on cache failure
//! (C-INFO-2).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CachePathError {
    #[error("could not determine a default data directory (no $HOME / unsupported platform)")]
    NoDataDir,
}

/// Resolve the path of the SQLite cache file.
///
/// `cli_override` wins if `Some`; otherwise the `MODELTAP_CACHE_PATH`
/// environment variable (passed in as `env_override` for testability) wins
/// if `Some`; otherwise fall back to `dirs::data_dir()/modeltap/cache.sqlite`.
pub fn resolve(
    cli_override: Option<&Path>,
    env_override: Option<&OsStr>,
) -> Result<PathBuf, CachePathError> {
    if let Some(p) = cli_override {
        return Ok(p.to_path_buf());
    }
    if let Some(env) = env_override {
        return Ok(PathBuf::from(env));
    }
    let base = dirs::data_dir().ok_or(CachePathError::NoDataDir)?;
    Ok(base.join("modeltap").join("cache.sqlite"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn cli_override_wins_over_env_override() {
        let cli = PathBuf::from("/cli/cache.sqlite");
        let env = OsString::from("/env/cache.sqlite");
        let got = resolve(Some(cli.as_path()), Some(env.as_os_str())).expect("resolves");
        assert_eq!(got, cli);
    }

    #[test]
    fn env_override_used_when_no_cli() {
        let env = OsString::from("/env-only/cache.sqlite");
        let got = resolve(None, Some(env.as_os_str())).expect("resolves");
        assert_eq!(got, PathBuf::from("/env-only/cache.sqlite"));
    }

    #[test]
    fn default_path_ends_with_modeltap_cache_sqlite() {
        let got = resolve(None, None).expect("dirs::data_dir() resolves on this host");
        let tail: PathBuf = got
            .iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(tail, PathBuf::from("modeltap").join("cache.sqlite"));
    }
}
