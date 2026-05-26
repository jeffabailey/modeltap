//! M1 — Walking-skeleton acceptance test for folder-group-bulk-delete (US-05c).
//!
//! Source: `docs/feature/folder-group-bulk-delete/distill/features/folder-group-delete.feature`
//!   @walking-skeleton @us-05c @milestone-1 @ac-4 @ac-8 @ac-10 @ac-11 @ac-16
//!   @destructive @real-io @adapter-integration
//!
//!   Scenario: Devon deletes an all-unique HF repo folder and reclaims disk
//!
//! Strategy B (declared in `wave-decisions.md`): real I/O against a
//! `tempfile::TempDir`-built HF cache. The orchestration in
//! `modeltap-app::orchestration::execute_folder_delete` dispatches to the
//! REAL `HfPlugin::delete_folder` override (D1 litmus per
//! acceptance-test-plan.md §4: substituting an InMemoryHfPlugin would break
//! this test because we assert on `path.exists() == false`).
//!
//! What this test proves end-to-end:
//!   1. Devon launches `modeltap` in headless mode with HF_HOME pointing at
//!      the `devon-hf-allunique` tempdir fixture.
//!   2. Devon presses Shift+F on the cursor-targeted folder header. (The
//!      composition root resolves the folder-id from the
//!      `MODELTAP_HEADLESS_FOLDER_PATH` env var — same seam pattern as the
//!      parent's `MODELTAP_HEADLESS_DETAIL_REGS` for US-10 / US-05b.)
//!   3. Devon types the byte-exact `<author>/<repo>` path and presses Enter.
//!   4. The orchestration builds the FolderDeletePlan via the pure logic in
//!      `modeltap-core::logic::folder_group` and calls
//!      `HfPlugin::delete_folder(&plan)`.
//!   5. The 2 model blobs, 3 sidecars, and the now-empty `models--<author>--
//!      <repo>/` directory tree are removed from the real filesystem.
//!   6. The post-action summary (LastAction) reports
//!      `Last action: folder-delete bartowski/Llama-3.2-1B-Instruct-GGUF
//!      (success)`, `5 of 5 files removed`, `Reclaimed: ~2.1 GB`,
//!      `Retained: 0.0 GB`.
//!   7. A `action.folder_delete` JSONL event is emitted to
//!      `${MODELTAP_LOG_DIR}/launch.log` per kpi-instrumentation.md.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture: devon-hf-allunique (per acceptance-test-plan.md §3)
//
// Builds an HF cache tree at `<root>/hub/models--bartowski--Llama-3.2-1B-
// Instruct-GGUF/` containing:
//
//   - 2 model files (sparse for speed):
//       * Llama-3.2-1B-Instruct-Q4_K_M.gguf  (~808 MB — Q4_K_M quant)
//       * Llama-3.2-1B-Instruct-Q8_0.gguf    (~1.3 GB — Q8_0 quant)
//     Total model bytes ≈ 2.1 GB.
//
//   - 3 sidecars:
//       * README.md                                       (24 KB)
//       * Llama-3.2-1B-Instruct.imatrix                   (1.3 MB)
//       * Llama-3.2-1B-Instruct-Q4_K_M.gguf.urls          (8 KB)
//
// Each model file is stored under `blobs/<sha>` and exposed via a snapshot
// symlink under `snapshots/<rev>/<filename>` per the real HF layout.
// `refs/main` carries the rev sha. Sidecars sit alongside the snapshot
// symlinks (README.md / .imatrix / .gguf.urls) or under blobs/ (HF-internal).
// ---------------------------------------------------------------------------

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";
const REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";

// Sparse-friendly sizes — the test uses `set_len` so disk usage stays under a
// few KB per blob. The acceptance assertions only check apparent (st_size)
// totals, which the OS reports as the requested length regardless of sparsity.
const Q4_BYTES: u64 = 808 * 1024 * 1024; // ~808 MB
const Q8_BYTES: u64 = 1_300 * 1024 * 1024; // ~1.3 GB
const README_BYTES: u64 = 24 * 1024;
const IMATRIX_BYTES: u64 = 1_300 * 1024;
const URLS_BYTES: u64 = 8 * 1024;

struct DevonHfAllUniqueFixture {
    _temp: TempDir,
    hf_home: PathBuf,
    repo_dir: PathBuf,
    blob_q4: PathBuf,
    blob_q8: PathBuf,
    snap_q4: PathBuf,
    snap_q8: PathBuf,
    readme: PathBuf,
    imatrix: PathBuf,
    urls: PathBuf,
    refs_main: PathBuf,
}

impl DevonHfAllUniqueFixture {
    fn all_files(&self) -> Vec<&PathBuf> {
        vec![
            &self.blob_q4,
            &self.blob_q8,
            &self.snap_q4,
            &self.snap_q8,
            &self.readme,
            &self.imatrix,
            &self.urls,
            &self.refs_main,
        ]
    }

    fn total_model_bytes(&self) -> u64 {
        Q4_BYTES + Q8_BYTES
    }

    fn total_sidecar_bytes(&self) -> u64 {
        README_BYTES + IMATRIX_BYTES + URLS_BYTES
    }
}

/// Build the `devon-hf-allunique` HF cache fixture under a fresh tempdir.
/// Uses sparse files (`File::set_len`) so the ~2.1 GB total disk usage is
/// nominal — the on-disk apparent size matches the requested length, but
/// only metadata is written.
fn build_devon_hf_allunique_fixture() -> DevonHfAllUniqueFixture {
    let temp = tempfile::tempdir().expect("tempdir for devon-hf-allunique fixture");
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

    // Two distinct blob hashes (length 64) — fictitious but well-formed.
    let blob_q4_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let blob_q8_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let blob_q4 = blobs_dir.join(blob_q4_hash);
    let blob_q8 = blobs_dir.join(blob_q8_hash);
    write_sparse(&blob_q4, Q4_BYTES);
    write_sparse(&blob_q8, Q8_BYTES);

    // Snapshot symlinks point at the blobs via HF's two-up relative path.
    let snap_q4 = snap_dir.join("Llama-3.2-1B-Instruct-Q4_K_M.gguf");
    let snap_q8 = snap_dir.join("Llama-3.2-1B-Instruct-Q8_0.gguf");
    symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(blob_q4_hash),
        &snap_q4,
    )
    .expect("symlink snap_q4");
    symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(blob_q8_hash),
        &snap_q8,
    )
    .expect("symlink snap_q8");

    // Sidecars: README.md + .imatrix + .gguf.urls in the snapshot dir.
    let readme = snap_dir.join("README.md");
    let imatrix = snap_dir.join("Llama-3.2-1B-Instruct.imatrix");
    let urls = snap_dir.join("Llama-3.2-1B-Instruct-Q4_K_M.gguf.urls");
    write_sparse(&readme, README_BYTES);
    write_sparse(&imatrix, IMATRIX_BYTES);
    write_sparse(&urls, URLS_BYTES);

    // HF-internal: refs/main with the rev sha.
    let refs_main = refs_dir.join("main");
    fs::write(&refs_main, REV_SHA).expect("write refs/main");

    DevonHfAllUniqueFixture {
        _temp: temp,
        hf_home,
        repo_dir,
        blob_q4,
        blob_q8,
        snap_q4,
        snap_q8,
        readme,
        imatrix,
        urls,
        refs_main,
    }
}

/// Write a sparse file of `size` bytes. The OS reports the requested length
/// as `st_size` even though only metadata is allocated.
fn write_sparse(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir for sparse file");
    }
    let file = fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    file.set_len(size)
        .unwrap_or_else(|e| panic!("set_len({}) on {}: {e}", size, path.display()));
}

// ---------------------------------------------------------------------------
// Headless harness for the walking-skeleton scenario.
// ---------------------------------------------------------------------------

fn modeltap_headless(fix: &DevonHfAllUniqueFixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("HF_HOME", &fix.hf_home)
        // All other tools point at non-existent directories so the discovery
        // pass produces a single-tool inventory (HF only). This keeps the
        // acceptance scenario focused on the folder-delete dispatch path.
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        // The folder-path env-var seam is the orchestration's WS-only hook
        // for resolving the cursor-targeted folder. Production wiring (cursor
        // → FolderGroup) lands in subsequent steps; for M1 the test passes the
        // path explicitly. Mirrors the MODELTAP_HEADLESS_DETAIL_REGS pattern
        // used by US-10 / US-05b in `headless.rs`.
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

fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// THE M1 walking-skeleton scenario.
// ---------------------------------------------------------------------------

/// Scenario: Devon deletes an all-unique HF repo folder and reclaims disk.
///
/// Per `folder-group-delete.feature` § @milestone-1. Removing the `@skip` tag
/// from this scenario in the DISTILL feature file is part of step 01-05's
/// commit; the test below IS the executable form of the scenario.
///
/// Walking-skeleton litmus (D1, acceptance-test-plan.md §4): substituting an
/// in-memory HF plugin here would break this test because the post-conditions
/// assert on real filesystem state (`path.exists() == false`) under the
/// tempdir-rooted HF cache.
#[test]
fn devon_deletes_all_unique_hf_repo_folder_and_reclaims_disk() {
    let fix = build_devon_hf_allunique_fixture();

    // Pre-condition: every file the scenario will sweep currently exists.
    for p in fix.all_files() {
        assert!(
            p.exists(),
            "fixture pre-condition: {} must exist before folder-delete",
            p.display()
        );
    }
    assert!(
        fix.repo_dir.exists(),
        "fixture pre-condition: repo dir {} must exist",
        fix.repo_dir.display()
    );

    let total_reclaim = fix.total_model_bytes() + fix.total_sidecar_bytes();

    // Headless script: type the byte-exact folder path, press Enter to
    // confirm, then quit. The Shift+F that opens the dialog is synthesized at
    // the composition root from MODELTAP_HEADLESS_FOLDER_PATH (per the
    // existing detail-regs seam pattern). After Enter, the orchestration
    // dispatches to HfPlugin::delete_folder and writes the LastAction.
    let mut script = String::new();
    script.push_str("<folder-delete>"); // sentinel: orchestration opens the dialog from MODELTAP_HEADLESS_FOLDER_PATH
    script.push_str(REPO_PATH);
    script.push_str("<enter>q");

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", &script)
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    // ---------- Filesystem post-conditions (per AC-10, AC-11) -------------
    for p in fix.all_files() {
        assert!(
            !p.exists(),
            "AC-10: {} must be removed after folder-delete",
            p.display()
        );
    }
    assert!(
        !fix.repo_dir.exists(),
        "AC-11: empty repo dir {} must be removed after folder-delete",
        fix.repo_dir.display()
    );
    // The parent hub/ directory MUST still exist (only this repo's subtree
    // was swept).
    let hub = fix.hf_home.join("hub");
    assert!(
        hub.exists(),
        "AC-11: parent hub directory {} must remain after folder-delete",
        hub.display()
    );

    // ---------- TUI post-action summary (per AC-16) -----------------------
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    assert!(
        frame.contains("Last action: folder-delete"),
        "AC-16: post-action banner header must say 'Last action: folder-delete', got frame:\n{frame}"
    );
    assert!(
        frame.contains(REPO_PATH),
        "AC-16: post-action banner must include the repo path {REPO_PATH}, got frame:\n{frame}"
    );
    assert!(
        frame.contains("(success)"),
        "AC-16: post-action banner must include '(success)' for all-unique happy path, got frame:\n{frame}"
    );
    assert!(
        frame.contains("5 of 5 files removed"),
        "AC-16: post-action banner must report '5 of 5 files removed' (2 models + 3 sidecars), got frame:\n{frame}"
    );

    // ---------- JSONL instrumentation (per kpi-instrumentation.md) --------
    let events = read_jsonl_events(&log_file);
    let folder_delete_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.folder_delete"))
        .collect();
    assert_eq!(
        folder_delete_events.len(),
        1,
        "expected exactly 1 action.folder_delete JSONL event, got {}: {:#?}",
        folder_delete_events.len(),
        folder_delete_events
    );
    let event = folder_delete_events[0];
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "action.folder_delete outcome must be 'success' for the all-unique happy path, got: {}",
        event
    );
    let bytes_reclaimed = event
        .get("bytes_reclaimed")
        .and_then(|v| v.as_u64())
        .expect("action.folder_delete must carry bytes_reclaimed u64");
    assert_eq!(
        bytes_reclaimed, total_reclaim,
        "action.folder_delete.bytes_reclaimed must equal sum of model + sidecar bytes, got {} expected {}",
        bytes_reclaimed, total_reclaim
    );
    let bytes_retained = event
        .get("bytes_retained")
        .and_then(|v| v.as_u64())
        .expect("action.folder_delete must carry bytes_retained u64");
    assert_eq!(
        bytes_retained, 0,
        "action.folder_delete.bytes_retained must be 0 for the all-unique scenario, got {}",
        bytes_retained
    );
    let files_total = event
        .get("files_total")
        .and_then(|v| v.as_u64())
        .expect("action.folder_delete must carry files_total u64");
    assert_eq!(
        files_total, 5,
        "action.folder_delete.files_total must be 5 (2 models + 3 sidecars), got {}",
        files_total
    );
    let files_removed = event
        .get("files_removed")
        .and_then(|v| v.as_u64())
        .expect("action.folder_delete must carry files_removed u64");
    assert_eq!(
        files_removed, 5,
        "action.folder_delete.files_removed must be 5 for the all-unique happy path, got {}",
        files_removed
    );

    // Privacy (C5 / kpi-instrumentation §Privacy): the repo path is a logical
    // identifier (typed by the user verbatim), so it MAY appear in the JSONL
    // event as the `folder_path` field — distinct from on-disk paths. But
    // absolute filesystem paths and blob hex digests MUST NOT.
    let event_str = event.to_string();
    assert!(
        !event_str.contains("/blobs/"),
        "C5: on-disk /blobs/ paths must not appear in JSONL, got: {}",
        event_str
    );
    assert!(
        !event_str.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "C5: blob hash hex must not appear in JSONL, got: {}",
        event_str
    );
}

// ---------------------------------------------------------------------------
// Sanity probe: the fixture builder produces the expected on-disk tree
// without involving modeltap. Runs cleanly even when the orchestration is
// broken; isolates fixture failures from production-code failures.
// ---------------------------------------------------------------------------

#[test]
fn devon_hf_allunique_fixture_produces_5_files_under_models_dash_repo() {
    let fix = build_devon_hf_allunique_fixture();
    assert!(fix.repo_dir.exists());
    assert!(fix.hf_home.join("hub").exists());

    // 2 model blobs + 2 snapshot symlinks + 3 sidecars + 1 refs/main file.
    let mut all_present = 0;
    for p in fix.all_files() {
        if p.exists() {
            all_present += 1;
        }
    }
    assert_eq!(
        all_present,
        8,
        "fixture builder must produce 8 on-disk entries (2 blobs + 2 snap symlinks + 3 sidecars + refs/main)"
    );

    // Model file sizes are reported as the requested sparse length.
    let q4_meta = fs::symlink_metadata(&fix.blob_q4).expect("stat blob_q4");
    assert_eq!(q4_meta.len(), Q4_BYTES, "Q4 blob apparent size");
    let q8_meta = fs::symlink_metadata(&fix.blob_q8).expect("stat blob_q8");
    assert_eq!(q8_meta.len(), Q8_BYTES, "Q8 blob apparent size");
}
