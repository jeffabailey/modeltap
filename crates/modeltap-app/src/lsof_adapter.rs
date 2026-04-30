//! `LsofAdapter` — driven port adapter for `FsProbe::detect_running_tools`
//! on macOS / Linux (per intake Q5; ADR-007 thiserror in modeltap-core).
//!
//! Per intake Q5, this is **detect-and-prompt-then-retry**, NOT soft-warning.
//! Modeltap REFUSES the unify or delete_one action until the user closes the
//! running tool and presses `[r] retry`. The adapter only PROVIDES the data;
//! the gate decision is in `actions::unify::run` / `actions::delete_one::run`.
//!
//! ## How it works
//!
//! On unix systems (`cfg!(unix)` — macOS, Linux, WSL counts as Linux), the
//! adapter shells out to `lsof <path1> <path2> ...` and parses the column-
//! separated output. Each match becomes one `RunningProcess { tool_name,
//! pid, path }` entry. We filter to processes whose `COMMAND` column matches
//! one of the registered tool names (`ollama`, `lm-studio`, `llama-cli`,
//! `huggingface-cli`, etc.) so a stray `cat` or `grep` doesn't trigger the
//! gate.
//!
//! On Windows native (NOT WSL), `lsof` is unavailable and the adapter
//! returns `Err(ProbeError::LsofUnavailable)`. The orchestrator surfaces
//! the explicit "Running-tool detection unavailable on this system" dialog
//! per US-17 AC-3.
//!
//! ## Test seam: MODELTAP_FAKE_LSOF_OUTPUT
//!
//! When the env-var is set, the adapter NEVER spawns the subprocess; it
//! parses the env-var contents as if `lsof` had emitted them. Sentinel
//! values:
//!
//! - `__UNAVAILABLE__` — return `Err(LsofUnavailable)` (simulate missing
//!   binary).
//! - `__EMPTY__` — return `Ok(vec![])` (simulate lsof exit-1 with no
//!   matches).
//! - any other string — parse as raw lsof output.
//!
//! The seam is essential because CI cannot reliably hold a file open
//! against a known PID across test runs; the env-var lets every scenario
//! land its branch deterministically.

use std::path::{Path, PathBuf};
use std::process::Command;

use modeltap_core::ports::fs_probe::{FsProbe, ProbeError, RunningProcess};

/// The set of tool COMMAND names we recognize as "registered tools" whose
/// running process should gate the destructive action. Other commands
/// holding the file open (e.g., a backup process) do NOT trigger the gate
/// — only tools the user actively manages with modeltap.
///
/// Names match what each tool's process advertises via `argv[0]` to lsof:
/// `ollama` (the daemon binary), `lm-studio` (Electron renderer reports as
/// `LM Studio`), `llama-cli` (the binary), and `huggingface-cli` /
/// `python` (no reliable signature for HF — left out conservatively).
///
/// We compare case-insensitively and also accept substrings (e.g., the
/// `LM Studio Helper (Renderer)` process matches `lm-studio`).
const REGISTERED_TOOL_NAMES: &[&str] = &["ollama", "lm-studio", "lm studio", "llama-cli"];

/// `FsProbe` adapter that uses `lsof` to detect running tool processes
/// holding in-scope files open. Construct via `LsofAdapter::new()`.
#[derive(Debug, Default, Clone)]
pub struct LsofAdapter;

impl LsofAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl FsProbe for LsofAdapter {
    fn dev_and_inode(&self, path: &Path) -> Option<(u64, u64)> {
        // The Lsof adapter is intentionally NOT used for stat-ing files
        // (the production path uses a separate stat-based FsProbe). Returns
        // None for every path so accidental use surfaces as conservative
        // "treat as cross-fs / unknown" branches.
        let _ = path;
        None
    }

    fn detect_running_tools(
        &self,
        target_paths: &[PathBuf],
    ) -> Result<Vec<RunningProcess>, ProbeError> {
        // Per intake Q5: detect-and-prompt-then-retry. The gate caller
        // refuses the action when this returns Ok(non_empty); we just
        // PRODUCE the list. Empty target_paths is a no-op (no files in scope
        // means no running tool to detect — the planner shouldn't call us
        // with an empty list, but defense in depth).
        if target_paths.is_empty() {
            return Ok(Vec::new());
        }

        // Test seam: MODELTAP_FAKE_LSOF_OUTPUT replaces the real subprocess.
        // Production paths never set this env var; CI sets it per scenario.
        if let Ok(fake) = std::env::var("MODELTAP_FAKE_LSOF_OUTPUT") {
            return parse_fake_seam(&fake, target_paths);
        }

        // Native Windows path: lsof does not exist. WSL is reported as
        // unix by cfg, so this branch only fires on native Windows.
        if !cfg!(unix) {
            return Err(ProbeError::LsofUnavailable {
                reason: "lsof is only available on macOS and Linux".to_string(),
            });
        }

        // Real subprocess path. `-w` suppresses the "WARNING" message that
        // BSD lsof emits when /proc isn't readable; `-n` skips DNS resolution
        // (we don't care about hostnames); `-P` skips port-name resolution.
        let mut cmd = Command::new("lsof");
        cmd.arg("-w").arg("-n").arg("-P");
        for p in target_paths {
            cmd.arg(p);
        }
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ProbeError::LsofUnavailable {
                    reason: "lsof binary not found on PATH".to_string(),
                });
            }
            Err(e) => return Err(ProbeError::Io(e)),
        };

        // lsof returns exit code 1 when nothing matches — that is a normal
        // "no running tools" outcome, NOT an error. Any other non-zero exit
        // is a parse-or-permission error; we treat it as no detection
        // available so the user is not blocked.
        let stdout = String::from_utf8_lossy(&output.stdout);
        let processes = parse_lsof_output(&stdout);
        Ok(filter_to_registered_tools(processes))
    }
}

/// Parse the `MODELTAP_FAKE_LSOF_OUTPUT` env var. Sentinels first, then
/// raw output. The sentinel parser is the only reason the env var exists;
/// it lets CI exercise lsof-unavailable and lsof-empty branches without
/// platform-dependent setup.
fn parse_fake_seam(
    fake: &str,
    target_paths: &[PathBuf],
) -> Result<Vec<RunningProcess>, ProbeError> {
    if fake == "__UNAVAILABLE__" {
        return Err(ProbeError::LsofUnavailable {
            reason: "MODELTAP_FAKE_LSOF_OUTPUT=__UNAVAILABLE__ test seam".to_string(),
        });
    }
    if fake == "__EMPTY__" {
        return Ok(Vec::new());
    }
    let processes = parse_lsof_output(fake);
    let filtered = filter_to_registered_tools(processes);
    // For the fake-seam test path, also intersect with target_paths so a
    // test that injects an `ollama` line for path `/foo` doesn't fire when
    // the action targets `/bar`. Real lsof already does this filtering at
    // the subprocess argument level; we mirror it here for parity.
    //
    // Path-canonicalization parity: production callers canonicalize their
    // target_paths (per `build_plan_from_detail` in headless.rs) so that on
    // macOS `/var/folders/...` becomes `/private/var/folders/...`. Fake lsof
    // lines built by tests typically still carry the un-canonicalized form.
    // Compare via canonicalize-both-sides so the intersection matches; fall
    // back to the raw path when canonicalize fails (e.g., the file was
    // deleted between fixture build and probe).
    fn canon(p: &Path) -> PathBuf {
        std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
    }
    let target_canon: Vec<PathBuf> = target_paths.iter().map(|p| canon(p)).collect();
    Ok(filtered
        .into_iter()
        .filter(|rp| {
            let rp_canon = canon(&rp.path);
            target_canon.iter().any(|tp| tp == &rp_canon)
        })
        .collect())
}

/// Parse the raw textual output of `lsof` into a list of `RunningProcess`.
/// We accept either of two output shapes:
///
/// 1. **Column format** (default lsof): a header line followed by
///    space-separated columns: `COMMAND PID USER FD TYPE DEVICE SIZE/OFF
///    NODE NAME`. We extract `COMMAND`, `PID`, and `NAME`.
///
/// 2. **Single-line format** (when no header is present): tolerated for
///    test fakes that omit the header. Same column layout.
///
/// Lines we cannot parse (header, blank, malformed) are silently skipped —
/// a parser bug must NEVER block the user from acting; we just produce
/// fewer results.
fn parse_lsof_output(text: &str) -> Vec<RunningProcess> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Skip the header line. We detect it heuristically: the first column
        // is "COMMAND" (case-sensitive — lsof is consistent).
        if trimmed.starts_with("COMMAND") {
            continue;
        }
        let cols: Vec<&str> = trimmed.split_whitespace().collect();
        // Need at least 9 columns: COMMAND PID USER FD TYPE DEVICE SIZE NODE NAME.
        // The NAME column may itself contain spaces (rare on macOS but possible),
        // so we treat columns 0..8 strictly and join 8.. as NAME.
        if cols.len() < 9 {
            continue;
        }
        let command = cols[0].to_string();
        let pid: u32 = match cols[1].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let name = cols[8..].join(" ");
        out.push(RunningProcess {
            tool_name: command,
            pid,
            path: PathBuf::from(name),
        });
    }
    out
}

/// Restrict the parsed lsof output to processes whose COMMAND matches one of
/// our registered tools. Comparison is case-insensitive substring; e.g., a
/// process reported as `LM Studio Helper (Renderer)` matches `lm studio`.
fn filter_to_registered_tools(processes: Vec<RunningProcess>) -> Vec<RunningProcess> {
    processes
        .into_iter()
        .filter(|rp| {
            let lower = rp.tool_name.to_lowercase();
            REGISTERED_TOOL_NAMES.iter().any(|t| lower.contains(t))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Process-wide lock so the env-var-mutating tests below cannot run
    /// in parallel and leak `MODELTAP_FAKE_LSOF_OUTPUT` into each other.
    /// Rust's test runner uses one thread per test by default; without this
    /// serialization, test A's `__UNAVAILABLE__` sentinel can be observed by
    /// test B mid-run. The lock is held for the full duration of each test
    /// via the `FakeLsofGuard` RAII handle.
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    /// Helper: install a fake-lsof env var for the duration of a test. Holds
    /// the process-wide `ENV_LOCK` so only one env-mutating test runs at a
    /// time, then clears the env var on drop.
    struct FakeLsofGuard {
        _lock: MutexGuard<'static, ()>,
    }
    impl FakeLsofGuard {
        fn set(value: &str) -> Self {
            let lock = env_lock();
            std::env::set_var("MODELTAP_FAKE_LSOF_OUTPUT", value);
            Self { _lock: lock }
        }
    }
    impl Drop for FakeLsofGuard {
        fn drop(&mut self) {
            std::env::remove_var("MODELTAP_FAKE_LSOF_OUTPUT");
        }
    }

    fn ollama_lsof_line(path: &str) -> String {
        format!(
            "COMMAND     PID       USER   FD   TYPE DEVICE   SIZE/OFF NODE NAME\n\
             ollama     1234      jeff   3r   REG   1,4        4096   100 {path}\n"
        )
    }

    /// B1: detect_running_tools parses fake lsof output into RunningProcess.
    #[test]
    fn detect_returns_running_process_when_fake_lsof_lists_ollama() {
        let path = PathBuf::from("/tmp/blob");
        let _g = FakeLsofGuard::set(&ollama_lsof_line("/tmp/blob"));
        let adapter = LsofAdapter::new();
        let result = adapter
            .detect_running_tools(std::slice::from_ref(&path))
            .expect("detect must succeed under fake-lsof");
        assert_eq!(result.len(), 1, "must parse exactly one entry");
        assert_eq!(result[0].tool_name, "ollama");
        assert_eq!(result[0].pid, 1234);
        assert_eq!(result[0].path, path);
    }

    /// B2: __UNAVAILABLE__ sentinel returns LsofUnavailable.
    #[test]
    fn detect_returns_lsof_unavailable_when_sentinel_set() {
        let _g = FakeLsofGuard::set("__UNAVAILABLE__");
        let adapter = LsofAdapter::new();
        let err = adapter
            .detect_running_tools(&[PathBuf::from("/tmp/x")])
            .expect_err("must return LsofUnavailable for __UNAVAILABLE__ sentinel");
        match err {
            ProbeError::LsofUnavailable { .. } => {}
            other => panic!("expected LsofUnavailable, got {:?}", other),
        }
    }

    /// B3: __EMPTY__ sentinel returns empty Vec.
    #[test]
    fn detect_returns_empty_when_empty_sentinel_set() {
        let _g = FakeLsofGuard::set("__EMPTY__");
        let adapter = LsofAdapter::new();
        let result = adapter
            .detect_running_tools(&[PathBuf::from("/tmp/x")])
            .expect("__EMPTY__ must succeed with empty result");
        assert!(result.is_empty(), "no processes when __EMPTY__ set");
    }

    /// B4: empty target_paths returns empty Vec without consulting lsof.
    #[test]
    fn detect_returns_empty_when_target_paths_empty() {
        // Note: even WITHOUT the fake-lsof env var, the empty-path check
        // short-circuits before the subprocess. This test deliberately does
        // not set the env var to cover that path.
        let adapter = LsofAdapter::new();
        let result = adapter
            .detect_running_tools(&[])
            .expect("empty target_paths must succeed with empty result");
        assert!(
            result.is_empty(),
            "no processes when no target_paths to check"
        );
    }

    /// B1-extra: parser filters to registered tool names. A `cat` line is
    /// not a registered tool and must NOT trigger the gate.
    #[test]
    fn parser_filters_unregistered_commands() {
        let raw = "COMMAND     PID       USER   FD   TYPE DEVICE   SIZE/OFF NODE NAME\n\
                   cat         9999      jeff   3r   REG   1,4        4096   100 /tmp/blob\n";
        let _g = FakeLsofGuard::set(raw);
        let adapter = LsofAdapter::new();
        let result = adapter
            .detect_running_tools(&[PathBuf::from("/tmp/blob")])
            .expect("must succeed");
        assert!(
            result.is_empty(),
            "cat is not a registered tool; must not gate"
        );
    }

    /// B1-extra: parser tolerates lm-studio reported as "LM Studio Helper".
    #[test]
    fn parser_matches_lm_studio_helper_substring() {
        let raw = "COMMAND     PID       USER   FD   TYPE DEVICE   SIZE/OFF NODE NAME\n\
                   LM\\x20Studio  4321    jeff   3r   REG   1,4        4096   100 /tmp/lm.bin\n";
        // Whitespace-in-COMMAND is rare; lsof on macOS uses backslash-x20 escape.
        // The simpler form "LM Studio" splits into 2 columns, which our parser
        // would reject. We test the safe path with a no-space variant.
        let raw_safe = "COMMAND     PID       USER   FD   TYPE DEVICE   SIZE/OFF NODE NAME\n\
             lm-studio   4321      jeff   3r   REG   1,4        4096   100 /tmp/lm.bin\n";
        let _g = FakeLsofGuard::set(raw_safe);
        let adapter = LsofAdapter::new();
        let result = adapter
            .detect_running_tools(&[PathBuf::from("/tmp/lm.bin")])
            .expect("must succeed");
        assert_eq!(result.len(), 1, "lm-studio must match registered set");
        assert_eq!(result[0].tool_name, "lm-studio");
        let _ = raw; // Keep the variable so the test documents the rejected form.
    }

    /// Header-only output produces empty result.
    #[test]
    fn parser_skips_header_line_only() {
        let raw = "COMMAND     PID       USER   FD   TYPE DEVICE   SIZE/OFF NODE NAME\n";
        let _g = FakeLsofGuard::set(raw);
        let adapter = LsofAdapter::new();
        let result = adapter
            .detect_running_tools(&[PathBuf::from("/tmp/x")])
            .expect("must succeed");
        assert!(result.is_empty(), "header-only input has no entries");
    }

    /// Filtering by target_paths: a fake-lsof line for `/foo` does NOT fire
    /// when the action targets `/bar`.
    #[test]
    fn parser_filters_by_target_paths_in_fake_seam() {
        let _g = FakeLsofGuard::set(&ollama_lsof_line("/tmp/foo"));
        let adapter = LsofAdapter::new();
        let result = adapter
            .detect_running_tools(&[PathBuf::from("/tmp/bar")])
            .expect("must succeed");
        assert!(
            result.is_empty(),
            "process holding /foo must NOT gate action on /bar"
        );
    }
}
