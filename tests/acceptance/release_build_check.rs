//! Release-build absence checks — OQ-3 invariant per
//! `docs/feature/tool-model-info-sqlite-cache/design/component-boundaries.md`
//! §"Build-time enforcement" (added in step 06-01).
//!
//! Two test-only environment variables guard test-harness seams:
//!
//! - `MODELTAP_TEST_PLUGINS` — read by
//!   [`crates/modeltap-app/src/registry.rs::maybe_register_test_plugins`]
//!   under `#[cfg(any(test, feature = "test-harness"))]`. Drives the
//!   in-binary `TestTool` registration for the US-23 cache acceptance suite.
//!
//! - `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS` — read by
//!   [`crates/modeltap-store/src/repo/tools.rs::reconcile_tool`] under the
//!   same cfg gate. Drives the AC-23-10 concurrent-writers acceptance
//!   scenario by sleeping N ms before COMMIT.
//!
//! Release builds (`cargo build --release` without the `test-harness`
//! feature) MUST compile both seams out entirely. The env-var read, the
//! literal string, and the cfg-gated body all disappear from the shipped
//! Mach-O / ELF binary. These tests verify that statically via
//! `strings target/release/modeltap | grep <env-var>` — zero matches
//! permitted.
//!
//! ## Why `#[ignore]`?
//!
//! Building `--release` is slow (`cargo build --release -p modeltap-app
//! --bin modeltap` takes 60-180 s on a clean tree). Running these tests
//! in the default `cargo test` would balloon the inner-loop time. They
//! run only when invoked explicitly:
//!
//! ```sh
//! cargo test --test release_build_check -- --ignored
//! ```
//!
//! CI's release-prep job (per CLAUDE.md "Before any `git push` to main")
//! invokes them as part of the pre-push gate. The Phase 04 K-INFO
//! cache_kpi test follows the same `#[ignore]` + `--release` pattern.
//!
//! ## What about `strings`?
//!
//! `strings(1)` ships in macOS' command-line developer tools and in every
//! Linux distro's `binutils` package. The acceptance test invokes
//! `strings <binary> | grep -F <pattern>` via std::process::Command —
//! cross-platform-safe enough for the macOS-and-Linux CI matrix the
//! project targets. Windows is WSL-only per CLAUDE.md, which routes
//! through the same `strings` binary.

use std::path::PathBuf;
use std::process::Command;

/// Resolve the workspace root from `CARGO_MANIFEST_DIR` (which points at
/// the `modeltap-acceptance` crate's manifest at `<repo>/tests`).
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap_or(&manifest_dir).to_path_buf()
}

/// Build the release modeltap binary if it does not already exist, then
/// return its path. Built with `--no-default-features` so the
/// `test-harness` and `test-fixtures` features are OFF — which is the
/// only configuration where the env-var seams should be compiled out.
fn build_release_modeltap_binary() -> PathBuf {
    let workspace = workspace_root();
    // First, do the build. Even when the binary already exists, we re-run
    // `cargo build --release --no-default-features` to make sure the
    // current feature set matches — incremental rebuilds are fast when
    // there is nothing to do. The `-p modeltap-app --bin modeltap` scope
    // keeps the build narrow (no plugin/test crates).
    let status = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--no-default-features",
            "-p",
            "modeltap-app",
            "--bin",
            "modeltap",
        ])
        .current_dir(&workspace)
        .status()
        .expect("invoke cargo build --release");
    assert!(
        status.success(),
        "cargo build --release --no-default-features -p modeltap-app --bin modeltap failed"
    );
    let bin = workspace.join("target/release/modeltap");
    assert!(
        bin.exists(),
        "expected release binary at {} after successful build, but it is missing",
        bin.display()
    );
    bin
}

/// Run `strings <binary> | grep -F <needle>` and return the captured
/// stdout. We use `-F` (fixed-string) so the needle is matched literally
/// — no regex metacharacters in env-var names anyway, but cheap insurance.
///
/// Returns the full match output. Empty Vec means zero matches.
fn strings_grep(binary: &std::path::Path, needle: &str) -> Vec<String> {
    // Spawn `strings <binary>`, pipe its stdout into our own filter, and
    // return matching lines. Using a single `Command` with shell piping
    // would work too, but we want to keep portability across shells.
    let out = Command::new("strings")
        .arg(binary)
        .output()
        .expect("invoke strings(1)");
    assert!(
        out.status.success(),
        "strings {} failed: {}",
        binary.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .filter(|line| line.contains(needle))
        .map(|line| line.to_string())
        .collect()
}

/// OQ-3 — `MODELTAP_TEST_PLUGINS` MUST NOT appear in the release binary.
///
/// The registry seam at `crates/modeltap-app/src/registry.rs` is gated on
/// `#[cfg(any(test, feature = "test-harness"))]`. `cargo build --release
/// --no-default-features` disables `test-harness`, so the env-var read
/// AND the literal string constant must be stripped entirely. A grep hit
/// here means the cfg gate is broken (or someone moved the literal
/// outside the gate).
#[test]
#[ignore = "slow — runs cargo build --release; use --ignored in CI release-prep"]
fn release_build_omits_modeltap_test_plugins_env_var() {
    let bin = build_release_modeltap_binary();
    let matches = strings_grep(&bin, "MODELTAP_TEST_PLUGINS");
    assert!(
        matches.is_empty(),
        "release binary at {} contains the test-only env-var \
         'MODELTAP_TEST_PLUGINS' — the #[cfg(...)] gate in \
         crates/modeltap-app/src/registry.rs is broken. Hits:\n  {}",
        bin.display(),
        matches.join("\n  ")
    );
}

/// OQ-3 — `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS` MUST NOT appear in the
/// release binary.
///
/// The store-side seam at `crates/modeltap-store/src/repo/tools.rs` is
/// gated on `#[cfg(any(test, feature = "test-harness"))]`. The
/// `modeltap-app` `test-harness` feature forwards to
/// `modeltap-store/test-harness`, so disabling the upstream feature
/// disables the downstream gate. The string must be absent.
#[test]
#[ignore = "slow — runs cargo build --release; use --ignored in CI release-prep"]
fn release_build_omits_modeltap_debug_hold_write_lock_ms_env_var() {
    let bin = build_release_modeltap_binary();
    let matches = strings_grep(&bin, "MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS");
    assert!(
        matches.is_empty(),
        "release binary at {} contains the test-only env-var \
         'MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS' — the #[cfg(...)] gate in \
         crates/modeltap-store/src/repo/tools.rs is broken or its \
         test-harness feature is leaking through default features. Hits:\n  {}",
        bin.display(),
        matches.join("\n  ")
    );
}
