//! M4 third scenario — Idempotent retry after closing the holding tool
//! (folder-group-bulk-delete, step 04-02).
//!
//! Source: `docs/feature/folder-group-bulk-delete/distill/features/folder-group-delete.feature`
//!
//!   @us-05c @milestone-4 @ac-12 @destructive
//!   Scenario: Re-running folder-delete after closing the holding tool removes the remaining files
//!
//! Per ADR-010 § Concurrency: detect-and-prompt-then-retry. The first invocation
//! mirrors the M4 partial-failure scenario (Ollama holds two model files →
//! 19 of 21 files removed, two EBUSY blobs remain on disk along with their
//! containing `models--<author>--<repo>/` tree). The user then closes Ollama
//! and re-runs `Shift+F` on the now-shorter folder header. The second
//! invocation:
//!
//!   1. Stateless rediscovery (per Q7 / CLAUDE.md §"Stateless rediscovery") sees
//!      the folder with only the previously-failed files (snapshot symlinks for
//!      the 19 cleared blobs are gone, only the 2 EBUSY blobs + their snapshot
//!      symlinks survive).
//!   2. The second `FolderDeletePlan` lists only those two files in
//!      `paths_to_unlink_fully`.
//!   3. `HfPlugin::delete_folder` returns one successful `DeleteOutcome` per
//!      file — `MODELTAP_TEST_EBUSY_PATHS` is no longer set so the test seam
//!      short-circuit does not fire.
//!   4. `remove_empty_repo_tree` then sweeps the now-empty repo tree from disk.
//!
//! Plugin-contract 3.11.S.5 (idempotence on partial-then-empty) is exercised at
//! the unit layer in `plugins/hf/tests/folder_delete_contract.rs`. This test is
//! the end-to-end (driving-port → driven-port) form of the same invariant.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";
const REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";
const MODEL_FILE_COUNT: usize = 21;

// ---------------------------------------------------------------------------
// devon-hf-busy fixture — identical to the M4 partial-failure setup. The
// retry scenario builds the SAME starting fixture (21 blobs + snapshot
// symlinks + refs/main), runs the partial-failure pass first, then the retry
// pass second.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
struct DevonHfBusyFixture {
    _temp: TempDir,
    hf_home: PathBuf,
    repo_dir: PathBuf,
    blobs: Vec<PathBuf>,
    snaps: Vec<PathBuf>,
    busy_blob_paths: Vec<PathBuf>,
    busy_filenames: Vec<String>,
}

fn build_devon_hf_busy_fixture() -> DevonHfBusyFixture {
    let temp = tempfile::tempdir().expect("tempdir for devon-hf-busy fixture");
    let root = temp.path().to_path_buf();
    let hf_home = root.join(".cache").join("huggingface");
    let hub = hf_home.join("hub");
    let repo_dir = hub.join(REPO_DIR_NAME);
    let blobs_dir = repo_dir.join("blobs");
    let snap_dir = repo_dir.join("snapshots").join(REV_SHA);
    let refs_dir = repo_dir.join("refs");
    fs::create_dir_all(&blobs_dir).expect("create hf blobs dir");
    fs::create_dir_all(&snap_dir).expect("create hf snapshots dir");
    fs::create_dir_all(&refs_dir).expect("create hf refs dir");

    let quants = [
        "Q2_K", "Q3_K_S", "Q3_K_M", "Q3_K_L", "Q4_0", "Q4_1", "Q4_K_S", "Q4_K_M", "Q5_0", "Q5_1",
        "Q5_K_S", "Q5_K_M", "Q6_K", "Q8_0", "IQ1_M", "IQ1_S", "IQ2_M", "IQ2_S", "IQ3_M", "IQ3_S",
        "F16",
    ];
    assert_eq!(quants.len(), MODEL_FILE_COUNT);

    let mut blobs = Vec::with_capacity(MODEL_FILE_COUNT);
    let mut snaps = Vec::with_capacity(MODEL_FILE_COUNT);
    let mut busy_blob_paths = Vec::new();
    let mut busy_filenames = Vec::new();
    for (i, quant) in quants.iter().enumerate() {
        let hash: String = std::iter::repeat(char::from_digit((i % 10) as u32, 16).unwrap_or('a'))
            .take(64)
            .collect();
        let unique_marker = format!("{:02x}", i);
        let hash = format!("{unique_marker}{}", &hash[2..]);
        let blob = blobs_dir.join(&hash);
        write_sparse(&blob, 256 * 1024 * 1024);
        let snap_name = format!("Llama-3.2-1B-Instruct-{quant}.gguf");
        let snap = snap_dir.join(&snap_name);
        symlink(
            PathBuf::from("..").join("..").join("blobs").join(&hash),
            &snap,
        )
        .expect("symlink snap");
        if *quant == "Q4_K_M" || *quant == "Q4_0" {
            busy_blob_paths.push(blob.clone());
            busy_filenames.push(snap_name.clone());
        }
        blobs.push(blob);
        snaps.push(snap);
    }
    assert_eq!(busy_blob_paths.len(), 2, "fixture must mark 2 busy blobs");

    fs::write(refs_dir.join("main"), REV_SHA).expect("write refs/main");

    DevonHfBusyFixture {
        _temp: temp,
        hf_home,
        repo_dir,
        blobs,
        snaps,
        busy_blob_paths,
        busy_filenames,
    }
}

fn write_sparse(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir for sparse file");
    }
    let file = fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    file.set_len(size)
        .unwrap_or_else(|e| panic!("set_len({}) on {}: {e}", size, path.display()));
}

fn read_jsonl_events(log_file: &Path) -> Vec<Value> {
    let content = fs::read_to_string(log_file).unwrap_or_default();
    content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.contains(r#""schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build a fresh `assert_cmd::Command` for `modeltap` in headless mode with
/// `HF_HOME` pointing at the fixture. Returns the command, the log temp dir
/// (must outlive the command), and the log file path. Each invocation gets
/// its own log dir so the second call's JSONL events do not collide with the
/// first.
fn modeltap_cmd(hf_home: &Path) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("HF_HOME", hf_home)
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

// ===========================================================================
// M4 scenario 3: Idempotent retry after closing the holding tool.
// ===========================================================================

#[test]
fn rerun_folder_delete_after_closing_holding_tool_removes_remaining_files() {
    let fix = build_devon_hf_busy_fixture();

    // ----- First invocation: partial failure with EBUSY for 2 files --------
    let canonical_busy: Vec<String> = fix
        .busy_blob_paths
        .iter()
        .map(|p| {
            fs::canonicalize(p)
                .unwrap_or_else(|_| p.clone())
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let ebusy_env = canonical_busy.join(":");

    let mut script = String::new();
    script.push_str("<folder-delete>");
    script.push_str(REPO_PATH);
    script.push_str("<enter>q");

    let (mut cmd1, _log_temp_1, log_file_1) = modeltap_cmd(&fix.hf_home);
    cmd1.env("MODELTAP_TEST_EBUSY_PATHS", &ebusy_env)
        .env("MODELTAP_HEADLESS_INPUT", &script)
        .timeout(Duration::from_secs(60))
        .assert()
        .success();

    // Post-condition after first pass: exactly the 2 busy blobs remain.
    for busy in &fix.busy_blob_paths {
        assert!(
            busy.exists(),
            "M4-retry pre: busy blob {} must remain after the partial first pass",
            busy.display()
        );
    }
    for blob in &fix.blobs {
        if fix.busy_blob_paths.contains(blob) {
            continue;
        }
        assert!(
            !blob.exists(),
            "M4-retry pre: non-busy blob {} should already be gone after the first pass",
            blob.display()
        );
    }
    assert!(
        fix.repo_dir.exists(),
        "M4-retry pre: repo dir {} must still exist after partial-failure first pass",
        fix.repo_dir.display()
    );
    let first_events = read_jsonl_events(&log_file_1);
    let first_outcome = first_events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.folder_delete"))
        .and_then(|e| e.get("outcome").and_then(|v| v.as_str()))
        .map(str::to_string);
    assert_eq!(
        first_outcome.as_deref(),
        Some("partial"),
        "M4-retry pre: first pass must record outcome=partial"
    );

    // ----- Second invocation: holding tool has closed (no EBUSY env) -------
    //
    // We deliberately do NOT set MODELTAP_TEST_EBUSY_PATHS this time — the
    // test-harness seam treats an unset / empty env var as "no path is busy",
    // which mirrors "the user closed ollama". Stateless rediscovery walks the
    // now-shorter folder; the second `FolderDeletePlan` contains only the
    // 2 surviving blobs.
    let (mut cmd2, _log_temp_2, log_file_2) = modeltap_cmd(&fix.hf_home);
    let assert2 = cmd2
        .env_remove("MODELTAP_TEST_EBUSY_PATHS")
        .env("MODELTAP_HEADLESS_INPUT", &script)
        .timeout(Duration::from_secs(60))
        .assert()
        .success();

    // ---- Filesystem post-conditions: 2 of 2 remaining files removed ------
    for busy in &fix.busy_blob_paths {
        assert!(
            !busy.exists(),
            "M4-retry post: previously-busy blob {} must be removed after retry",
            busy.display()
        );
    }
    assert!(
        !fix.repo_dir.exists(),
        "M4-retry post: empty repo dir {} must be removed after retry success",
        fix.repo_dir.display()
    );

    // ---- TUI post-action summary -----------------------------------------
    let stdout = String::from_utf8_lossy(&assert2.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    assert!(
        frame.contains("Last action: folder-delete"),
        "M4-retry: banner header must say 'Last action: folder-delete', got frame:\n{frame}"
    );
    assert!(
        frame.contains(REPO_PATH),
        "M4-retry: banner must include the repo path {REPO_PATH}, got frame:\n{frame}"
    );
    assert!(
        frame.contains("(success"),
        "M4-retry: banner must include '(success...)' status on the retry, got frame:\n{frame}"
    );
    assert!(
        frame.contains("2 of 2 files removed"),
        "M4-retry: banner must report '2 of 2 files removed', got frame:\n{frame}"
    );

    // ---- JSONL outcome ---------------------------------------------------
    let second_events = read_jsonl_events(&log_file_2);
    let second_folder_delete: Vec<&Value> = second_events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.folder_delete"))
        .collect();
    assert_eq!(
        second_folder_delete.len(),
        1,
        "M4-retry: expected exactly one action.folder_delete event in the retry log, got {}",
        second_folder_delete.len()
    );
    let event = second_folder_delete[0];
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "M4-retry: retry outcome must be 'success', got: {event}"
    );
    let files_total = event
        .get("files_total")
        .and_then(|v| v.as_u64())
        .expect("files_total u64");
    let files_removed = event
        .get("files_removed")
        .and_then(|v| v.as_u64())
        .expect("files_removed u64");
    assert_eq!(
        files_total, 2,
        "M4-retry: retry files_total must equal 2 (the surviving subset)"
    );
    assert_eq!(
        files_removed, 2,
        "M4-retry: retry files_removed must equal 2 (all surviving files cleared)"
    );
}
