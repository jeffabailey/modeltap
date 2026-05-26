//! M4 — Partial-failure acceptance tests for folder-group-bulk-delete (US-05c,
//! step 04-01).
//!
//! Source: `docs/feature/folder-group-bulk-delete/distill/features/folder-group-delete.feature`
//!
//!   @us-05c @milestone-4 @ac-12 @ac-16 @destructive @infrastructure-failure
//!   Scenario: Ollama holds 2 model files open and folder-delete continues for the rest
//!   Scenario: A permission-denied file does not block the rest of the folder
//!
//! Per ADR-010 § Concurrency: no rollback, continue-and-report. The HF plugin's
//! `delete_folder_at` per-file loop catches per-file io::Error variants
//! (EBUSY, EACCES, NotFound) and converts each into a failed `DeleteOutcome`
//! WITHOUT aborting the loop. The orchestrator aggregates the
//! `Vec<DeleteOutcome>` into a `partial` `LastAction` whose per-file reasons
//! the right-pane renderer surfaces verbatim.
//!
//! EBUSY simulation: the HF plugin's `delete_one_at` wrapper, gated behind
//! `cfg(any(test, feature = "test-harness"))`, honours the
//! `MODELTAP_TEST_EBUSY_PATHS` env-var (colon-separated absolute paths) when
//! `MODELTAP_HEADLESS=1` is also set. For each path the wrapper synthesises an
//! `io::ErrorKind::Other` matching the EBUSY semantics — no real `flock`, no
//! sibling process, portable across macOS and Linux.
//!
//! Permission-denied simulation: a fixture-built directory with mode 0555
//! containing one model file. `remove_file` on that file's blob returns
//! EACCES on every Unix; the loop converts it to a failed outcome and
//! continues with the rest of the folder.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";
const REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";

// ---------------------------------------------------------------------------
// devon-hf-busy fixture (per acceptance-test-plan.md §3).
//
// 21 model files (Q-quant variants), 0 sidecars (the M4 EBUSY scenario only
// cares about model-file partial failure). The fixture-builder records the
// absolute paths of two of the model blobs as the "busy" set; the test sets
// `MODELTAP_TEST_EBUSY_PATHS=blob_busy_1:blob_busy_2` so the HF plugin's
// test-harness seam returns EBUSY for those two files only.
// ---------------------------------------------------------------------------

const MODEL_FILE_COUNT: usize = 21;

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

    // 21 distinct quantization variants. Use predictable names so the test
    // can name the two "busy" files explicitly.
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
        // Differentiate the first character so all 21 hashes are unique even
        // though the rest is the same. Without this, e.g. Q2_K and Q4_K_S
        // would both be all-'0'.
        let unique_marker = format!("{:02x}", i);
        let hash = format!("{unique_marker}{}", &hash[2..]);
        let blob = blobs_dir.join(&hash);
        // Small sparse files — the assertions only check existence, not bytes.
        write_sparse(&blob, 256 * 1024 * 1024);
        let snap_name = format!("Llama-3.2-1B-Instruct-{quant}.gguf");
        let snap = snap_dir.join(&snap_name);
        symlink(
            PathBuf::from("..").join("..").join("blobs").join(&hash),
            &snap,
        )
        .expect("symlink snap");
        // The two "busy" files per the feature scenario:
        //   - Llama-3.2-1B-Instruct-Q4_K_M.gguf
        //   - Llama-3.2-1B-Instruct-Q4_0.gguf
        if *quant == "Q4_K_M" || *quant == "Q4_0" {
            busy_blob_paths.push(blob.clone());
            busy_filenames.push(snap_name.clone());
        }
        blobs.push(blob);
        snaps.push(snap);
    }
    assert_eq!(busy_blob_paths.len(), 2, "fixture must mark 2 busy blobs");

    // HF-internal: refs/main.
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

// ---------------------------------------------------------------------------
// devon-hf-perm fixture (per acceptance-test-plan.md §3).
//
// 1 HF repo with 5 model files where ONE blob's containing directory has
// mode 0555 (no write access). `remove_file` returns EACCES on that file.
// ---------------------------------------------------------------------------

const PERM_MODEL_FILE_COUNT: usize = 5;

#[allow(dead_code)]
struct DevonHfPermFixture {
    _temp: TempDir,
    hf_home: PathBuf,
    repo_dir: PathBuf,
    blobs: Vec<PathBuf>,
    snaps: Vec<PathBuf>,
    blob_q8_protected: PathBuf,
    blob_q8_protected_dir: PathBuf,
}

fn build_devon_hf_perm_fixture() -> DevonHfPermFixture {
    let temp = tempfile::tempdir().expect("tempdir for devon-hf-perm fixture");
    let root = temp.path().to_path_buf();
    let hf_home = root.join(".cache").join("huggingface");
    let hub = hf_home.join("hub");
    let repo_dir = hub.join(REPO_DIR_NAME);
    let blobs_root = repo_dir.join("blobs");
    let snap_dir = repo_dir.join("snapshots").join(REV_SHA);
    let refs_dir = repo_dir.join("refs");
    fs::create_dir_all(&blobs_root).expect("create blobs root");
    fs::create_dir_all(&snap_dir).expect("create snap dir");
    fs::create_dir_all(&refs_dir).expect("create refs dir");

    // 5 quants. Q8_0 (~1.3 GB) lives in its own sub-bucket dir whose mode
    // is later flipped to 0555.
    //
    // The HF plugin's `delete_one_at` ultimately calls
    // `std::fs::remove_file(blob_path)`. On Unix, `remove_file` requires
    // write permission on the PARENT directory. So we segregate Q8_0 into
    // a sub-bucket of `blobs/` and chmod that sub-bucket 0555.
    //
    // The protected sub-bucket is `blobs/protected/`. The snapshot symlink
    // resolves to `blobs/protected/<hash>`.
    let protected_dir = blobs_root.join("protected");
    fs::create_dir_all(&protected_dir).expect("create protected blob bucket");

    let quants = ["Q4_K_S", "Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0"];
    let sizes: [u64; 5] = [
        500 * 1024 * 1024,
        808 * 1024 * 1024,
        950 * 1024 * 1024,
        1_000 * 1024 * 1024,
        1_300 * 1024 * 1024,
    ];
    assert_eq!(quants.len(), PERM_MODEL_FILE_COUNT);

    let mut blobs = Vec::with_capacity(PERM_MODEL_FILE_COUNT);
    let mut snaps = Vec::with_capacity(PERM_MODEL_FILE_COUNT);
    let mut blob_q8_protected = PathBuf::new();
    for (i, quant) in quants.iter().enumerate() {
        let mut hash = format!("{:02x}", i);
        hash.push_str(&"a".repeat(62));
        let is_protected = *quant == "Q8_0";
        let blob_parent = if is_protected {
            protected_dir.clone()
        } else {
            blobs_root.clone()
        };
        let blob = blob_parent.join(&hash);
        write_sparse(&blob, sizes[i]);
        let snap_name = format!("Llama-3.2-1B-Instruct-{quant}.gguf");
        let snap = snap_dir.join(&snap_name);
        let rel_target = if is_protected {
            PathBuf::from("..")
                .join("..")
                .join("blobs")
                .join("protected")
                .join(&hash)
        } else {
            PathBuf::from("..").join("..").join("blobs").join(&hash)
        };
        symlink(rel_target, &snap).expect("symlink snap");
        if is_protected {
            blob_q8_protected = blob.clone();
        }
        blobs.push(blob);
        snaps.push(snap);
    }

    fs::write(refs_dir.join("main"), REV_SHA).expect("write refs/main");

    // Lock down the protected bucket — `remove_file` needs write+execute on
    // the parent. Mode 0555 = r-xr-xr-x removes the write bit.
    let mut perms = fs::metadata(&protected_dir)
        .expect("stat protected dir")
        .permissions();
    perms.set_mode(0o555);
    fs::set_permissions(&protected_dir, perms).expect("chmod 0555 protected dir");

    DevonHfPermFixture {
        _temp: temp,
        hf_home,
        repo_dir,
        blobs,
        snaps,
        blob_q8_protected,
        blob_q8_protected_dir: protected_dir,
    }
}

// Restore parent-dir write bit so the tempdir teardown can succeed.
fn restore_writable(dir: &Path) {
    if let Ok(meta) = fs::metadata(dir) {
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(dir, perms);
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

fn modeltap_cmd(hf_home: &Path) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
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
// M4 scenario 1: Ollama holds 2 model files open
// ===========================================================================

#[test]
fn ollama_holds_2_files_open_and_folder_delete_continues_for_the_rest() {
    let fix = build_devon_hf_busy_fixture();

    // Pre-condition: every blob and snapshot exists.
    for p in &fix.blobs {
        assert!(p.exists(), "fixture pre: blob {} must exist", p.display());
    }
    for p in &fix.snaps {
        assert!(
            fs::symlink_metadata(p).is_ok(),
            "fixture pre: snap {} must exist",
            p.display()
        );
    }

    // Compose MODELTAP_TEST_EBUSY_PATHS as the canonicalised colon-separated
    // list of the two busy blob paths. The seam matches on canonical paths
    // so symlink-vs-direct ambiguity does not leak through.
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

    let (mut cmd, _log_temp, log_file) = modeltap_cmd(&fix.hf_home);
    let assert = cmd
        .env("MODELTAP_TEST_EBUSY_PATHS", &ebusy_env)
        .env("MODELTAP_HEADLESS_INPUT", &script)
        .timeout(Duration::from_secs(60))
        .assert()
        .success();

    // ---- Filesystem post-conditions (AC-12: continue-and-report) ----------
    // The 2 busy blobs MUST remain. Their snapshot symlinks SHOULD have been
    // removed (registration_removed=true) but the blob unlink stage MAY have
    // succeeded for non-busy and failed for busy. Per ADR-010, the test-
    // harness seam returns EBUSY at the wrapper around the entire
    // `delete_one_at` call so BOTH the snapshot symlink AND the blob remain
    // (the seam short-circuits before either filesystem call). Either model
    // is acceptable as long as the busy blobs are still on disk and the
    // other 19 are gone.
    for busy in &fix.busy_blob_paths {
        assert!(
            busy.exists(),
            "AC-12: busy blob {} must remain on disk after partial failure",
            busy.display()
        );
    }
    let mut non_busy_remaining = 0u32;
    for blob in &fix.blobs {
        if fix.busy_blob_paths.contains(blob) {
            continue;
        }
        if blob.exists() {
            non_busy_remaining += 1;
        }
    }
    assert_eq!(
        non_busy_remaining, 0,
        "AC-12: every non-busy blob must be removed; got {non_busy_remaining} stragglers"
    );

    // ---- TUI post-action summary (AC-16) ---------------------------------
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    assert!(
        frame.contains("Last action: folder-delete"),
        "AC-16: banner header must say 'Last action: folder-delete', got frame:\n{frame}"
    );
    assert!(
        frame.contains(REPO_PATH),
        "AC-16: banner must include the repo path {REPO_PATH}, got frame:\n{frame}"
    );
    assert!(
        frame.contains("(partial"),
        "AC-16: banner must include '(partial...)' status, got frame:\n{frame}"
    );
    assert!(
        frame.contains("19 of 21 files removed"),
        "AC-16: banner must report '19 of 21 files removed', got frame:\n{frame}"
    );
    for filename in &fix.busy_filenames {
        assert!(
            frame.contains(filename),
            "AC-12: banner must list busy filename {filename}, got frame:\n{frame}"
        );
        assert!(
            frame.contains(&format!("{filename} reason: file open by ollama"))
                || frame.contains(&format!("{filename}\nreason: file open by ollama"))
                || frame.contains("reason: file open by ollama"),
            "AC-12: banner must surface 'reason: file open by ollama' for {filename}, got frame:\n{frame}"
        );
    }
    assert!(
        frame.contains("Press [F] again after closing ollama to finish"),
        "AC-12: banner must hint retry, got frame:\n{frame}"
    );

    // ---- JSONL outcome ----------------------------------------------------
    let events = read_jsonl_events(&log_file);
    let folder_delete_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.folder_delete"))
        .collect();
    assert_eq!(
        folder_delete_events.len(),
        1,
        "expected exactly 1 action.folder_delete JSONL event, got {}",
        folder_delete_events.len()
    );
    let event = folder_delete_events[0];
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("partial"),
        "M4: outcome must be 'partial', got: {event}"
    );
    let files_total = event
        .get("files_total")
        .and_then(|v| v.as_u64())
        .expect("files_total u64");
    assert_eq!(files_total, MODEL_FILE_COUNT as u64);
    let files_removed = event
        .get("files_removed")
        .and_then(|v| v.as_u64())
        .expect("files_removed u64");
    assert_eq!(files_removed, (MODEL_FILE_COUNT - 2) as u64);
}

// ===========================================================================
// M4 scenario 2: A permission-denied file does not block the rest of the folder
// ===========================================================================

#[test]
fn permission_denied_file_does_not_block_the_rest_of_the_folder() {
    let fix = build_devon_hf_perm_fixture();

    for p in &fix.blobs {
        assert!(p.exists(), "fixture pre: blob {} must exist", p.display());
    }
    for p in &fix.snaps {
        assert!(
            fs::symlink_metadata(p).is_ok(),
            "fixture pre: snap {} must exist",
            p.display()
        );
    }

    let mut script = String::new();
    script.push_str("<folder-delete>");
    script.push_str(REPO_PATH);
    script.push_str("<enter>q");

    let (mut cmd, _log_temp, log_file) = modeltap_cmd(&fix.hf_home);
    let result = cmd
        .env("MODELTAP_HEADLESS_INPUT", &script)
        .timeout(Duration::from_secs(60))
        .assert();
    // Restore the protected dir BEFORE any panicking assertion, otherwise
    // tempdir teardown fails on the read-only parent.
    restore_writable(&fix.blob_q8_protected_dir);
    let assert = result.success();

    // ---- Filesystem post-conditions (AC-12) -------------------------------
    assert!(
        fix.blob_q8_protected.exists(),
        "AC-12: protected blob {} must remain on disk after partial failure",
        fix.blob_q8_protected.display()
    );
    let mut non_protected_remaining = 0u32;
    for blob in &fix.blobs {
        if blob == &fix.blob_q8_protected {
            continue;
        }
        if blob.exists() {
            non_protected_remaining += 1;
        }
    }
    assert_eq!(
        non_protected_remaining, 0,
        "AC-12: every non-protected blob must be removed; got {non_protected_remaining} stragglers"
    );

    // ---- TUI post-action summary (AC-12, AC-16) ---------------------------
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    assert!(
        frame.contains("Last action: folder-delete"),
        "AC-16: banner header must say 'Last action: folder-delete', got frame:\n{frame}"
    );
    assert!(
        frame.contains(REPO_PATH),
        "AC-16: banner must include the repo path {REPO_PATH}, got frame:\n{frame}"
    );
    assert!(
        frame.contains("(partial"),
        "AC-16: banner must include '(partial...)' status, got frame:\n{frame}"
    );
    assert!(
        frame.contains("Llama-3.2-1B-Instruct-Q8_0.gguf"),
        "AC-12: banner must list the protected filename, got frame:\n{frame}"
    );
    assert!(
        frame.contains("reason: permission denied"),
        "AC-12: banner must surface 'reason: permission denied', got frame:\n{frame}"
    );

    // ---- JSONL outcome ----------------------------------------------------
    let events = read_jsonl_events(&log_file);
    let folder_delete_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.folder_delete"))
        .collect();
    assert_eq!(
        folder_delete_events.len(),
        1,
        "expected exactly 1 action.folder_delete JSONL event"
    );
    let event = folder_delete_events[0];
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("partial"),
        "M4: outcome must be 'partial', got: {event}"
    );
    let files_total = event.get("files_total").and_then(|v| v.as_u64()).unwrap();
    assert_eq!(files_total, PERM_MODEL_FILE_COUNT as u64);
    let files_removed = event.get("files_removed").and_then(|v| v.as_u64()).unwrap();
    assert_eq!(files_removed, (PERM_MODEL_FILE_COUNT - 1) as u64);
}
