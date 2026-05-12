//! Pre-flight refusal acceptance tests for folder-group-bulk-delete
//! (US-05c, step 04-03; ADR-010, F-FGD-8).
//!
//! Source scenarios (un-skipped in `integration-checkpoints.feature` by this
//! step):
//!   @us-05c @ac-15 @infrastructure-failure
//!     "Read-only HF cache refuses before the dialog opens"
//!   @us-05c @ac-20 @infrastructure-failure
//!     "Folder deleted out-of-band between launch and Shift+F triggers
//!      re-discovery"
//!
//! Both drive the orchestrator's pre-flight refusal path: no plugin call, no
//! filesystem mutation, exactly one JSONL `action.folder_delete` event with
//! `outcome=refused_readonly_cache` or `outcome=refused_folder_missing` and
//! `outcomes_count=0`. The HF cache directory MUST stay byte-identical
//! pre/post the refusal (asserted via `DirManifest`).
//!
//! Strategy: real I/O against a tempdir-built HF cache (Strategy B), driven
//! through the `modeltap` binary headless harness. The pre-flight checks
//! live in the composition root BEFORE `folder_delete::run` (or
//! `run_cancelled`) is dispatched — so the refusal path runs WITHOUT
//! constructing a `FolderConfirmState` and WITHOUT a dialog frame.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
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

fn write_sparse(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    let file = fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    file.set_len(size)
        .unwrap_or_else(|e| panic!("set_len({}) on {}: {e}", size, path.display()));
}

/// Build the standard `devon-hf-allunique` fixture tree (same layout as
/// `folder_delete_walking_skeleton.rs` and `folder_delete_confirmation_safety
/// .rs`). Returned in writeable mode; the read-only variant flips the
/// `hub/` permission bits AFTER seeding.
fn build_writeable_fixture() -> Fixture {
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

/// Build the `devon-hf-readonly` fixture: same layout as `devon-hf-allunique`,
/// but the `hub/` directory mode is flipped to `0555` AFTER the tree is
/// seeded so the orchestrator's `is_writable(hub)` pre-flight check returns
/// `false`. The tempdir's `Drop` cannot remove a read-only directory tree,
/// so the test restores the mode to `0755` before letting the fixture drop.
struct ReadOnlyHubFixture {
    fixture: Fixture,
}

impl ReadOnlyHubFixture {
    fn build() -> Self {
        let fixture = build_writeable_fixture();
        // Flip the hub root to read+execute-only (no write). Existing files
        // under it stay readable; new entries cannot be created or removed.
        let mut perms = fs::metadata(&fixture.hub_root)
            .expect("hub metadata")
            .permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&fixture.hub_root, perms).expect("set 0o555 on hub");
        Self { fixture }
    }
}

impl Drop for ReadOnlyHubFixture {
    fn drop(&mut self) {
        // Restore writeable mode so `TempDir::drop` can clean up.
        let _ = fs::set_permissions(&self.fixture.hub_root, fs::Permissions::from_mode(0o755));
    }
}

/// Build the `devon-hf-folder-vanished` fixture: standard `devon-hf-allunique`
/// tree, but the entire `models--<author>--<repo>/` subtree is removed
/// out-of-band BEFORE the test invocation. The `hub/` dir itself still exists
/// (and is writeable), so the `is_writable(hub)` pre-flight passes — only the
/// `exists(repo_dir)` pre-flight fails.
fn build_folder_vanished_fixture() -> Fixture {
    let fixture = build_writeable_fixture();
    // Out-of-band removal — mirrors the scenario step:
    //   "And an out-of-band process has removed the on-disk
    //    'models--bartowski--Llama-3.2-1B-Instruct-GGUF/' directory tree"
    fs::remove_dir_all(&fixture.repo_dir).expect("remove repo_dir out-of-band");
    assert!(
        !fixture.repo_dir.exists(),
        "fixture pre-condition: repo dir must be gone after out-of-band remove"
    );
    fixture
}

fn modeltap_headless(fix: &Fixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("log tempdir");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("modeltap bin");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        // Wider terminal so the refusal banner header — which puts the
        // user-facing refusal message in the `target` slot — renders without
        // being truncated by the right-pane width. Production TUIs are
        // typically 80+ cols; the refusal text fits comfortably at 160-cols
        // right-pane width.
        .env("MODELTAP_TERM_COLS", "200")
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

fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// AC-15 — Read-only HF cache refuses BEFORE the dialog opens.
// ---------------------------------------------------------------------------

#[test]
fn read_only_hf_cache_refuses_before_the_dialog_opens() {
    let ro = ReadOnlyHubFixture::build();
    let pre = DirManifest::snapshot(&ro.fixture.hub_root);
    assert!(
        pre.file_count() >= 7,
        "fixture pre-condition: hub manifest has seeded entries"
    );

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&ro.fixture);
    let script = "<folder-delete>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        // Even if the user typed a byte-exact match, the pre-flight refusal
        // MUST short-circuit before any confirm-state computation.
        .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", REPO_PATH)
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "enter")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    // ---------- Filesystem post-condition: byte-identical -----------------
    let post = DirManifest::snapshot(&ro.fixture.hub_root);
    assert_eq!(
        pre, post,
        "AC-15: HF cache must be byte-identical pre/post the read-only refusal"
    );
    assert!(
        ro.fixture.repo_dir.exists(),
        "AC-15: repo dir must remain on disk after read-only refusal"
    );

    // ---------- Right-pane banner shows the refusal text -------------------
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    assert!(
        frame.contains("Hugging Face cache is read-only -- cannot delete folder"),
        "AC-15: right pane must show 'Hugging Face cache is read-only -- cannot delete folder', \
         got frame:\n{frame}"
    );

    // ---------- JSONL outcome ---------------------------------------------
    let events = read_jsonl_events(&log_file);
    let event = folder_delete_event(&events);
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("refused_readonly_cache"),
        "AC-15: JSONL outcome must be 'refused_readonly_cache', got {event}"
    );
    assert_eq!(
        event.get("outcomes_count").and_then(|v| v.as_u64()),
        Some(0),
        "AC-15: outcomes_count must be 0 — the plugin was never called",
    );
    assert_eq!(
        event.get("files_removed").and_then(|v| v.as_u64()),
        Some(0),
        "AC-15: files_removed must be 0 on pre-flight refusal"
    );
    assert_eq!(
        event.get("bytes_reclaimed").and_then(|v| v.as_u64()),
        Some(0),
        "AC-15: bytes_reclaimed must be 0 on pre-flight refusal"
    );
}

// ---------------------------------------------------------------------------
// AC-20 — Folder deleted out-of-band between launch and Shift+F triggers
//         re-discovery.
// ---------------------------------------------------------------------------

#[test]
fn folder_deleted_out_of_band_refuses_and_signals_refresh() {
    let fix = build_folder_vanished_fixture();
    // The hub dir is still in place (writeable) — only the repo subtree is
    // gone. The pre/post manifests of `hub/` must match because the refusal
    // path performs zero filesystem mutation.
    let pre = DirManifest::snapshot(&fix.hub_root);

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let script = "<folder-delete>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", REPO_PATH)
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "enter")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    // ---------- Filesystem post-condition: byte-identical -----------------
    let post = DirManifest::snapshot(&fix.hub_root);
    assert_eq!(
        pre, post,
        "AC-20: hub/ must be byte-identical pre/post the folder-missing refusal"
    );
    assert!(
        !fix.repo_dir.exists(),
        "AC-20: repo dir must remain missing — the refusal path performs no creation"
    );

    // ---------- Right-pane banner shows the refusal text -------------------
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    assert!(
        frame.contains("folder no longer exists -- inventory will refresh"),
        "AC-20: right pane must show 'folder no longer exists -- inventory will refresh', \
         got frame:\n{frame}"
    );

    // ---------- JSONL outcome ---------------------------------------------
    let events = read_jsonl_events(&log_file);
    let event = folder_delete_event(&events);
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("refused_folder_missing"),
        "AC-20: JSONL outcome must be 'refused_folder_missing', got {event}"
    );
    assert_eq!(
        event.get("outcomes_count").and_then(|v| v.as_u64()),
        Some(0),
        "AC-20: outcomes_count must be 0 — the plugin was never called",
    );
    assert_eq!(
        event.get("files_removed").and_then(|v| v.as_u64()),
        Some(0),
        "AC-20: files_removed must be 0 on pre-flight refusal"
    );
}
