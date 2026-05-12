//! M2 — Confirmation safety acceptance tests for folder-group-bulk-delete
//! (US-05c, step 02-01).
//!
//! Source scenarios (un-skipped in `folder-group-delete.feature` by this step):
//!   @milestone-2 @ac-8 — "Wrong typed path cancels the folder delete with no destructive action"
//!   @milestone-2 @ac-9 — "Esc cancels the folder delete with no destructive action"
//!   @milestone-2 @ac-8 — "Typed path with trailing slash is treated as mismatch"
//!
//! All three drive the orchestrator's cancel-and-emit path (no fs mutation,
//! one JSONL `action.folder_delete` event with `outcome=cancelled_mismatch`
//! or `cancelled_escape` and `outcomes_count=0`).
//!
//! Strategy: real I/O against a tempdir-built HF cache (Strategy B). The
//! cancel paths MUST leave the cache directory byte-identical pre/post — a
//! `DirManifest` snapshot before invocation is compared to a fresh snapshot
//! after the headless run.
//!
//! Headless seam: `MODELTAP_HEADLESS_FOLDER_TYPED_INPUT` carries the user's
//! typed text; `MODELTAP_HEADLESS_FOLDER_DECISION_MODE` is one of:
//!   - unset / "confirm" — WS happy path (decide_on_enter against typed input)
//!   - "esc"             — force decide_on_esc (Esc cancellation)
//!   - "enter"           — decide_on_enter (the dialog's normal resolution)
//!
//! This mirrors the WS's `MODELTAP_HEADLESS_FOLDER_PATH` env-var seam — the
//! production wiring (cursor → dialog → Enter/Esc) is incremental and lands
//! across later steps; for M2 these env vars drive the orchestrator's
//! state-machine entry directly.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

use super::dir_manifest::DirManifest;

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";
const REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";

const Q4_BYTES: u64 = 808 * 1024 * 1024;
const Q8_BYTES: u64 = 1_300 * 1024 * 1024;
const README_BYTES: u64 = 24 * 1024;
const IMATRIX_BYTES: u64 = 1_300 * 1024;
const URLS_BYTES: u64 = 8 * 1024;

struct Fixture {
    _temp: TempDir,
    hf_home: PathBuf,
    hub_root: PathBuf,
    repo_dir: PathBuf,
}

fn build_fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let hf_home = root.join(".cache").join("huggingface");
    let hub = hf_home.join("hub");
    let repo_dir = hub.join(REPO_DIR_NAME);
    let blobs_dir = repo_dir.join("blobs");
    let snap_dir = repo_dir.join("snapshots").join(REV_SHA);
    let refs_dir = repo_dir.join("refs");
    fs::create_dir_all(&blobs_dir).expect("blobs dir");
    fs::create_dir_all(&snap_dir).expect("snap dir");
    fs::create_dir_all(&refs_dir).expect("refs dir");

    let blob_q4_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let blob_q8_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    write_sparse(&blobs_dir.join(blob_q4_hash), Q4_BYTES);
    write_sparse(&blobs_dir.join(blob_q8_hash), Q8_BYTES);

    symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(blob_q4_hash),
        snap_dir.join("Llama-3.2-1B-Instruct-Q4_K_M.gguf"),
    )
    .expect("symlink Q4");
    symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(blob_q8_hash),
        snap_dir.join("Llama-3.2-1B-Instruct-Q8_0.gguf"),
    )
    .expect("symlink Q8");

    write_sparse(&snap_dir.join("README.md"), README_BYTES);
    write_sparse(
        &snap_dir.join("Llama-3.2-1B-Instruct.imatrix"),
        IMATRIX_BYTES,
    );
    write_sparse(
        &snap_dir.join("Llama-3.2-1B-Instruct-Q4_K_M.gguf.urls"),
        URLS_BYTES,
    );
    fs::write(refs_dir.join("main"), REV_SHA).expect("refs/main");

    Fixture {
        _temp: temp,
        hf_home,
        hub_root: hub,
        repo_dir,
    }
}

fn write_sparse(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    let file = fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    file.set_len(size)
        .unwrap_or_else(|e| panic!("set_len({}) on {}: {e}", size, path.display()));
}

/// Build the headless command. Caller injects the per-scenario envs
/// (`MODELTAP_HEADLESS_FOLDER_TYPED_INPUT` / `MODELTAP_HEADLESS_FOLDER_DECISION_MODE`).
fn modeltap_headless(fix: &Fixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("log tempdir");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("modeltap bin");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("HF_HOME", &fix.hf_home)
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("MODELTAP_HEADLESS_FOLDER_PATH", REPO_PATH);
    (cmd, log_dir_temp, log_file)
}

fn read_jsonl_events(log_file: &Path) -> Vec<Value> {
    let content = fs::read_to_string(log_file).unwrap_or_default();
    content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

fn folder_delete_event(events: &[Value]) -> &Value {
    let v: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.folder_delete"))
        .collect();
    assert_eq!(
        v.len(),
        1,
        "expected exactly 1 action.folder_delete event, got {}: {:#?}",
        v.len(),
        v
    );
    v[0]
}

// ---------------------------------------------------------------------------
// M2.1 — Wrong typed path cancels the folder delete with no destructive action
// ---------------------------------------------------------------------------
#[test]
fn wrong_typed_path_cancels_with_no_destructive_action() {
    let fix = build_fixture();
    let pre = DirManifest::snapshot(&fix.hub_root);
    assert!(
        pre.file_count() >= 7,
        "fixture pre-condition: hub manifest has the seeded entries"
    );

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let script = "<folder-delete>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        // Wrong typed input — user spelled the repo without the author/.
        .env(
            "MODELTAP_HEADLESS_FOLDER_TYPED_INPUT",
            "Llama-3.2-1B-Instruct-GGUF",
        )
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "enter")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    // Filesystem post-condition: byte-identical.
    let post = DirManifest::snapshot(&fix.hub_root);
    assert_eq!(
        pre, post,
        "AC-8 (wrong-path mismatch): HF cache must be byte-identical pre/post the cancelled folder-delete"
    );
    assert!(
        fix.repo_dir.exists(),
        "repo dir must remain on disk after cancelled folder-delete"
    );

    let events = read_jsonl_events(&log_file);
    let event = folder_delete_event(&events);
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("cancelled_mismatch"),
        "JSONL event outcome must be 'cancelled_mismatch' for wrong-path cancellation, got {}",
        event
    );
    assert_eq!(
        event.get("outcomes_count").and_then(|v| v.as_u64()),
        Some(0),
        "outcomes_count must be 0 — the plugin was never called",
    );
    assert_eq!(
        event.get("files_removed").and_then(|v| v.as_u64()),
        Some(0),
        "files_removed must be 0 on cancellation"
    );

    // C5 privacy: no on-disk path or blob hash leakage.
    let s = event.to_string();
    assert!(!s.contains("/blobs/"), "privacy: no /blobs/ paths in JSONL");
    assert!(
        !s.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "privacy: no blob hash in JSONL"
    );

    // Sanity: the modeltap binary signalled the cancellation in stdout so the
    // user sees feedback. Banner text isn't asserted here — that lands in the
    // mixed-mode dialog work; the JSONL event is the source of truth.
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let _ = stdout; // currently unused; kept to satisfy the assert binding.
}

// ---------------------------------------------------------------------------
// M2.2 — Trailing-slash typed path is treated as mismatch
// ---------------------------------------------------------------------------
#[test]
fn trailing_slash_typed_path_is_treated_as_mismatch() {
    let fix = build_fixture();
    let pre = DirManifest::snapshot(&fix.hub_root);

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let script = "<folder-delete>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        // Byte-exact comparator: "bartowski/Llama-3.2-1B-Instruct-GGUF/" != folder.path
        .env(
            "MODELTAP_HEADLESS_FOLDER_TYPED_INPUT",
            format!("{}/", REPO_PATH),
        )
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "enter")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let post = DirManifest::snapshot(&fix.hub_root);
    assert_eq!(
        pre, post,
        "AC-8 (trailing slash): HF cache must be byte-identical pre/post the cancelled folder-delete"
    );

    let events = read_jsonl_events(&log_file);
    let event = folder_delete_event(&events);
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("cancelled_mismatch"),
        "trailing-slash input is byte-different from folder.path → cancelled_mismatch"
    );
    assert_eq!(
        event.get("outcomes_count").and_then(|v| v.as_u64()),
        Some(0),
    );
    assert_eq!(event.get("files_removed").and_then(|v| v.as_u64()), Some(0),);
}

// ---------------------------------------------------------------------------
// M2.3 — Esc cancels the folder delete with no destructive action
// ---------------------------------------------------------------------------
#[test]
fn esc_cancels_folder_delete_with_no_destructive_action() {
    let fix = build_fixture();
    let pre = DirManifest::snapshot(&fix.hub_root);

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let script = "<folder-delete>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        // Even when the typed input matches exactly, Esc must cancel.
        .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", REPO_PATH)
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "esc")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let post = DirManifest::snapshot(&fix.hub_root);
    assert_eq!(
        pre, post,
        "AC-9 (Esc cancel): HF cache must be byte-identical pre/post"
    );

    let events = read_jsonl_events(&log_file);
    let event = folder_delete_event(&events);
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("cancelled_escape"),
        "Esc cancellation must emit outcome=cancelled_escape, got {}",
        event
    );
    assert_eq!(
        event.get("outcomes_count").and_then(|v| v.as_u64()),
        Some(0),
    );
}

// ---------------------------------------------------------------------------
// Preview of M6 @property: any input != folder.path → cancelled_mismatch
// with zero DeleteOutcomes (parametrized rather than proptest to avoid
// pulling a new dependency for what is a small invariant).
// ---------------------------------------------------------------------------
#[test]
fn any_non_matching_input_cancels_with_zero_outcomes() {
    let mismatched_inputs = [
        "",                                           // empty
        "bartowski",                                  // missing /<repo>
        "BARTOWSKI/LLAMA-3.2-1B-INSTRUCT-GGUF",       // wrong case
        "bartowski/Llama-3.2-1B-Instruct-GGUF/",      // trailing slash
        "/bartowski/Llama-3.2-1B-Instruct-GGUF",      // leading slash
        "bartowski/Llama-3.2-1B-Instruct-GGUF ",      // trailing space
        " bartowski/Llama-3.2-1B-Instruct-GGUF",      // leading space
        "bartowski/Llama-3.2-1B-Instruct-GGUF\n",     // trailing newline
        "wrong/repo",                                 // syntactically valid but wrong
        "bartowski/Llama-3.2-1B-Instruct-GGUF-OTHER", // extra suffix
    ];

    for input in mismatched_inputs {
        let fix = build_fixture();
        let pre = DirManifest::snapshot(&fix.hub_root);

        let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
        let script = "<folder-delete>q";
        cmd.env("MODELTAP_HEADLESS_INPUT", script)
            .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", input)
            .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "enter")
            .timeout(Duration::from_secs(30))
            .assert()
            .success();

        let post = DirManifest::snapshot(&fix.hub_root);
        assert_eq!(
            pre, post,
            "@property: input {:?} != folder.path → fixture must be byte-identical pre/post",
            input
        );

        let events = read_jsonl_events(&log_file);
        let event = folder_delete_event(&events);
        assert_eq!(
            event.get("outcome").and_then(|v| v.as_str()),
            Some("cancelled_mismatch"),
            "@property: input {:?} != folder.path → outcome must be cancelled_mismatch, got {}",
            input,
            event
        );
        assert_eq!(
            event.get("outcomes_count").and_then(|v| v.as_u64()),
            Some(0),
            "@property: input {:?} → zero DeleteOutcomes produced",
            input
        );
    }
}
