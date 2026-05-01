//! Acceptance scaffold for US-U4: Pressing `u` from main view opens the
//! unify dialog with mates pre-populated. Fixes the v1 "u hotkey is a lie
//! from main view" bug.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u4`. AC-U4.1..AC-U4.6.
//!
//! These RED tests fail today because:
//!   - `headless::lift_unify_in_detail` only intercepts `Msg::Unify` on the
//!     Detail screen; on Main, `u` is a no-op (per current
//!     `crates/modeltap-app/src/headless.rs`).
//!   - `update::handle_msg` has no `Msg::UnifyHighlighted` variant
//!     (per data-models.md "New Msg variants").
//!
//! REMOVE #[ignore] in DELIVER step when each scenario goes green.
//!
//! Tags: @us-u4

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn modeltap_headless_at(ollama: &PathBuf, hf: &PathBuf) -> (Command, TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", ollama)
        .env("HF_HOME", hf)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    (cmd, temp, log_file)
}

fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

fn read_jsonl_events(log_file: &Path) -> Vec<Value> {
    let content = fs::read_to_string(log_file).unwrap_or_default();
    content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

fn ino_of(p: &Path) -> u64 {
    fs::metadata(p)
        .unwrap_or_else(|e| panic!("stat {}: {e}", p.display()))
        .ino()
}

/// Build a duplicate fixture (separate inodes, identical bytes) and return
/// the per-tool dirs plus the two blob paths.
fn build_duplicate(temp: &TempDir) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = temp.path().to_path_buf();
    let payload = vec![0xEFu8; 4096];

    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("ollama blobs");
    let blob = "9999999999999999999999999999999999999999999999999999999999999999";
    let ollama_blob = blobs.join(format!("sha256-{}", blob));
    fs::write(&ollama_blob, &payload).expect("write ollama blob");
    let m = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("dup");
    fs::create_dir_all(&m).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m.join("7b"), manifest).expect("manifest");

    let hf_home = root.join(".cache").join("huggingface");
    let repo = hf_home.join("hub").join("models--dup--Dup-7B");
    let hf_blobs = repo.join("blobs");
    let rev = "9999999999999999999999999999999999999999";
    let snap = repo.join("snapshots").join(rev);
    let refs = repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("hf blobs");
    fs::create_dir_all(&snap).expect("snap");
    fs::create_dir_all(&refs).expect("refs");
    let hf_blob_name = "8888888888888888888888888888888888888888888888888888888888888888";
    let hf_blob = hf_blobs.join(hf_blob_name);
    fs::write(&hf_blob, &payload).expect("write hf blob");
    std::os::unix::fs::symlink(
        PathBuf::from("..").join("..").join("blobs").join(hf_blob_name),
        snap.join("model.safetensors"),
    )
    .expect("snap symlink");
    fs::write(refs.join("main"), rev).expect("hf ref");

    (ollama_dir, hf_home, ollama_blob, hf_blob)
}

/// Build a single-tool fixture (unique row — no duplicates).
fn build_single_tool(temp: &TempDir) -> PathBuf {
    let root = temp.path().to_path_buf();
    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("blobs");
    let blob = "5555555555555555555555555555555555555555555555555555555555555555";
    fs::write(
        blobs.join(format!("sha256-{}", blob)),
        vec![0xCCu8; 4096],
    )
    .expect("write blob");
    let m = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("solo");
    fs::create_dir_all(&m).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m.join("7b"), manifest).expect("manifest");
    ollama_dir
}

// ---------------------------------------------------------------------------
// AC-U4.1, AC-U4.2: u on a "=" row opens dialog with mates pre-populated.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U4 RED — DELIVER must wire u-from-main-view to Msg::UnifyHighlighted"]
fn pressing_u_on_dedup_able_row_opens_dialog_with_mates_prepopulated() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_duplicate(&temp);
    let pre_a = ino_of(&ollama_blob);
    let pre_b = ino_of(&hf_blob);
    assert_ne!(pre_a, pre_b, "fixture must start on distinct inodes");

    let (mut cmd, _temp, log_file) = modeltap_headless_at(&ollama, &hf);
    // Script: highlight first row, press u (from MAIN view, not Detail),
    // confirm with Enter, quit. After DELIVER: dialog opens, plan applies,
    // inodes merge. Today: u is a no-op on Main, so nothing happens.
    let script = "u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // Observable outcome: the two blobs must share one inode (only possible
    // if dialog opened with mates and Enter applied the plan).
    let post_a = ino_of(&ollama_blob);
    let post_b = ino_of(&hf_blob);
    assert_eq!(
        post_a, post_b,
        "AC-U4.2: pressing u on a '=' row from main view must open the \
         dialog with mates pre-populated; Enter must apply the plan and \
         merge inodes (got ollama={}, hf={})",
        post_a, post_b
    );
    let events = read_jsonl_events(&log_file);
    assert!(
        events
            .iter()
            .any(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify")),
        "AC-U4.1: an action.unify event must be emitted when Enter is \
         pressed in the dialog opened from main view"
    );
}

// ---------------------------------------------------------------------------
// AC-U4.3: u on a "#" row opens dialog in informational mode.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U4 RED — DELIVER must open dialog in informational mode for '#' rows"]
fn pressing_u_on_already_unified_row_opens_informational_dialog() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_duplicate(&temp);
    // Convert into a pre-unified state.
    fs::remove_file(&hf_blob).expect("remove hf blob to re-link");
    fs::hard_link(&ollama_blob, &hf_blob).expect("hardlink hf blob to ollama");

    let (mut cmd, _temp, _log_file) = modeltap_headless_at(&ollama, &hf);
    let script = "u<enter>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();
    assert!(
        lower.contains("already") && lower.contains("unified"),
        "AC-U4.3: pressing u on a '#' row from main view must open the \
         informational dialog showing 'already unified', got frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U4.4: u on a "-" row shows status hint, no dialog.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U4 RED — DELIVER must show status hint without opening dialog for unique rows"]
fn pressing_u_on_unique_row_shows_status_hint_no_dialog() {
    let temp = tempfile::tempdir().expect("tempdir");
    let ollama = build_single_tool(&temp);
    let hf = temp.path().join("nonexistent-hf");
    let (mut cmd, _temp, log_file) = modeltap_headless_at(&ollama, &hf);
    let script = "u<enter>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();
    assert!(
        lower.contains("unique") || lower.contains("no copies"),
        "AC-U4.4: u on a unique row must show a 'no copies in other tools' \
         hint in the status line, got:\n{}",
        frame
    );
    let events = read_jsonl_events(&log_file);
    assert!(
        events
            .iter()
            .all(|e| e.get("event").and_then(|v| v.as_str()) != Some("action.unify")),
        "AC-U4.4: u on a unique row must NOT trigger an action.unify event"
    );
}

// ---------------------------------------------------------------------------
// AC-U4.5: u on a "?" row shows "still computing" hint, no dialog.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U4 RED — DELIVER must show 'hash still computing' hint without dialog for '?' rows"]
fn pressing_u_on_pending_hash_row_shows_still_computing_hint() {
    // To keep the row as "?" at the moment the harness presses `u`, we'd
    // need either a deterministic "pause-the-hash-pool" seam or to rely on
    // the current race-free first-paint state (NFR-1 says hashing starts
    // AFTER first paint). DELIVER will choose the mechanism.
    panic!(
        "AC-U4.5 — DELIVER must wire: u on a '?' row shows 'hash still \
         computing' hint and does not open a dialog. Mechanism (pause-hash \
         seam vs. first-paint-only assertion) is crafter's choice."
    );
}

// ---------------------------------------------------------------------------
// AC-U4.6: u from Detail still works (no v1 regression).
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U4 RED — DELIVER must preserve the v1 'u from Detail' path"]
fn pressing_u_on_detail_screen_still_opens_dialog() {
    // This test is the regression net for v1's existing behavior. The v1
    // acceptance suite (us_10_unify_hardlinks.rs) already exercises this
    // path; we re-assert it here so that any DELIVER refactor that breaks
    // u-from-Detail surfaces in the cross-tool-model-unify suite too.
    //
    // Until DELIVER re-wires the dispatch path, this test is RED only
    // because we cannot YET assert "u-from-Detail still opens dialog AND
    // u-from-Main also opens dialog" simultaneously — the current
    // production code only wires Detail.
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_duplicate(&temp);
    let (mut cmd, _temp, log_file) = modeltap_headless_at(&ollama, &hf);
    // Open Detail with the existing seam, then press u, then Enter.
    let regs_json = serde_json::json!({
        "id": "dup/Dup-7B",
        "regs": [
            {"tool": "ollama", "path": ollama_blob.display().to_string()},
            {"tool": "hf", "path": hf_blob.display().to_string()},
        ]
    })
    .to_string();
    let script = "<enter>u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs_json)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let events = read_jsonl_events(&log_file);
    let unify_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
        .collect();
    assert_eq!(
        unify_events.len(),
        1,
        "AC-U4.6: u-from-Detail must still produce exactly 1 action.unify \
         event after DELIVER's main-view rewire (got {})",
        unify_events.len()
    );
}
