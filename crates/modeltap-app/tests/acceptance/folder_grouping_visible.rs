//! Live integration test: HF right pane MUST render folder-header rows
//! when the active tool is `hf`.
//!
//! Background: every piece of folder-group-bulk-delete (US-05c) was built
//! through steps 01-01..06-02 — `group_by_hf_repo`, `FolderGroup`,
//! `render_folder_header_line`, `execute_folder_delete`, the dialog state
//! machine, the orchestrator — yet the live right-pane render path
//! (`render::right_pane::render`) still iterates `tool.model_ids[i]` flat
//! and never emits a folder-header line. The user, inspecting the
//! installed `modeltap` binary against a real HF cache, observed the
//! regression: no `[+] author/repo` or `[-] author/repo` lines appear.
//!
//! This test is the load-bearing one for the recovery step. It launches
//! `modeltap` in headless mode against a real tempdir HF cache containing
//! one repo with 5 .gguf quant variants, focuses the HF tool, captures the
//! rendered frame, and asserts:
//!
//!   1. A folder-header row for `bartowski/Llama-3.2-1B-Instruct-GGUF`
//!      appears, with `5 files` in the count summary (proves the row is
//!      the grouped header, not a per-file model row).
//!   2. The five per-file rows (one per quant variant) DO NOT appear by
//!      themselves above any folder header — i.e. the right pane is no
//!      longer flat.
//!
//! Negative: before the integration lands, the frame contains five
//! `bartowski/Llama-3.2-1B-Instruct-GGUF/Llama-3.2-1B-Instruct-...gguf`
//! lines and NO `5 files` summary. After the integration lands, the
//! folder-header line is present.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";
const REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";

// Sparse sizes — apparent (st_size) only. The render only needs id + size.
const Q2_K_BYTES: u64 = 580 * 1024 * 1024; // ~580 MB
const Q4_K_M_BYTES: u64 = 808 * 1024 * 1024; // ~808 MB
const Q5_K_M_BYTES: u64 = 950 * 1024 * 1024; // ~950 MB
const Q6_K_BYTES: u64 = 1_100 * 1024 * 1024; // ~1.1 GB
const Q8_0_BYTES: u64 = 1_300 * 1024 * 1024; // ~1.3 GB

struct DevonHfBartowskiFixture {
    _temp: TempDir,
    hf_home: PathBuf,
}

/// Build an HF cache fixture mimicking the user's real-life cache: one repo
/// (`bartowski/Llama-3.2-1B-Instruct-GGUF`) with 5 quant variants under one
/// snapshot directory. Sparse blobs keep the on-disk footprint tiny.
fn build_devon_hf_bartowski_fixture() -> DevonHfBartowskiFixture {
    let temp = tempfile::tempdir().expect("tempdir for devon-hf-bartowski fixture");
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

    for (variant, size, blob_hex) in [
        (
            "Q2_K",
            Q2_K_BYTES,
            "1111111111111111111111111111111111111111111111111111111111111111",
        ),
        (
            "Q4_K_M",
            Q4_K_M_BYTES,
            "2222222222222222222222222222222222222222222222222222222222222222",
        ),
        (
            "Q5_K_M",
            Q5_K_M_BYTES,
            "3333333333333333333333333333333333333333333333333333333333333333",
        ),
        (
            "Q6_K",
            Q6_K_BYTES,
            "4444444444444444444444444444444444444444444444444444444444444444",
        ),
        (
            "Q8_0",
            Q8_0_BYTES,
            "5555555555555555555555555555555555555555555555555555555555555555",
        ),
    ] {
        let blob_path = blobs_dir.join(blob_hex);
        write_sparse(&blob_path, size);
        let filename = format!("Llama-3.2-1B-Instruct-{variant}.gguf");
        let snap_link = snap_dir.join(&filename);
        symlink(
            PathBuf::from("..").join("..").join("blobs").join(blob_hex),
            &snap_link,
        )
        .expect("symlink quant variant");
    }

    fs::write(refs_dir.join("main"), REV_SHA).expect("write refs/main");

    DevonHfBartowskiFixture {
        _temp: temp,
        hf_home,
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

fn modeltap_headless(fix: &DevonHfBartowskiFixture) -> (Command, TempDir) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        // Generous width so long folder-header lines are not truncated.
        .env("MODELTAP_TERM_COLS", "140")
        .env("HF_HOME", &fix.hf_home)
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    (cmd, log_dir_temp)
}

/// Strip the headless mode's trailing session-summary JSON so we only see
/// the rendered TUI frame.
fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn hf_right_pane_renders_folder_header_for_grouped_repo() {
    let fix = build_devon_hf_bartowski_fixture();

    // Default selection lands on the alphabetically-first INSTALLED tool.
    // With only HF installed (every other plugin path is /nonexistent),
    // the default selection is the `hf` tool — no navigation needed.
    let (mut cmd, _log) = modeltap_headless(&fix);
    let assert = cmd
        // --quit-after-paint paints exactly one frame and exits cleanly.
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // ---------------- AC: folder-header line is visible -----------------
    // The right pane MUST render the grouped folder header for the repo.
    // The header carries both the canonical `<author>/<repo>` and a
    // `N files` count — together that is proof the row is the GROUPED
    // header from `render_folder_header_line`, not one of the per-file
    // model rows from `render_row_basic` (which never contains `5 files`).
    let has_folder_header_line = frame
        .lines()
        .any(|line| line.contains(REPO_PATH) && line.contains("5 files"));
    assert!(
        has_folder_header_line,
        "AC: expected a folder-header line containing `{}` AND `5 files` in the rendered HF right pane.\n\
         This means `group_by_hf_repo` is wired into `render::right_pane::render` for HF.\n\n\
         Captured frame:\n{}",
        REPO_PATH, frame
    );

    // -------- Negative: flat per-file rows must NOT appear without a header --------
    // Before the integration lands, the right pane renders one line per
    // model id (e.g. `bartowski/Llama-3.2-1B-Instruct-Q4_K_M.gguf`). After
    // the integration, those per-file ids may STILL appear (as child rows
    // beneath the always-expanded header) — but they must be preceded by a
    // header line. Concretely: it is illegal for the frame to contain a
    // `*-Q4_K_M.gguf` line without ALSO containing the `5 files` header.
    let quant_line_present = frame
        .lines()
        .any(|l| l.contains("Llama-3.2-1B-Instruct-Q4_K_M.gguf"));
    if quant_line_present {
        assert!(
            has_folder_header_line,
            "Negative AC: per-file quant rows are visible but the folder-header line is missing — \
             this means the right pane is rendering FLAT, not GROUPED.\n\n\
             Captured frame:\n{}",
            frame
        );
    }
}
