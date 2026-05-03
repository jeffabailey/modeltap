//! Acceptance scaffold for US-U5: Unify dialog shows concrete reclaim
//! preview and applies plan.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u5`. AC-U5.1..AC-U5.7.
//!
//! These RED tests fail today because:
//!   - The v1 unify-confirm dialog renders a basic confirm prompt; the new
//!     reclaim-preview body (per-target rows + bytes saved + total reclaim)
//!     is not yet implemented (per US-U5 in user-stories.md).
//!   - `[space]` key for toggling targets is not yet wired; `Msg::ToggleTarget`
//!     does not exist.
//!
//! REMOVE #[ignore] in DELIVER step when each scenario goes green.
//!
//! Tags: @us-u5

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
        .env(
            "MODELTAP_GPT4ALL_DIRS",
            "/nonexistent/no-such-gpt4all",
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

fn build_two_blob_duplicate(temp: &TempDir) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = temp.path().to_path_buf();
    let payload = vec![0xABu8; 4096];

    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("blobs");
    let blob = "7777777777777777777777777777777777777777777777777777777777777777";
    let ollama_blob = blobs.join(format!("sha256-{}", blob));
    fs::write(&ollama_blob, &payload).expect("write");
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
    let rev = "7777777777777777777777777777777777777777";
    let snap = repo.join("snapshots").join(rev);
    let refs = repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("hf blobs");
    fs::create_dir_all(&snap).expect("snap");
    fs::create_dir_all(&refs).expect("refs");
    let hf_blob_name = "6666666666666666666666666666666666666666666666666666666666666666";
    let hf_blob = hf_blobs.join(hf_blob_name);
    fs::write(&hf_blob, &payload).expect("write hf");
    std::os::unix::fs::symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(hf_blob_name),
        snap.join("model.safetensors"),
    )
    .expect("symlink");
    fs::write(refs.join("main"), rev).expect("ref");

    (ollama_dir, hf_home, ollama_blob, hf_blob)
}

fn detail_regs_json(ollama_blob: &Path, hf_blob: &Path) -> String {
    serde_json::json!({
        "id": "dup/Dup-7B",
        "regs": [
            {"tool": "ollama", "path": ollama_blob.display().to_string()},
            {"tool": "hf", "path": hf_blob.display().to_string()},
        ]
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// AC-U5.1: dialog shows canonical, target rows with savings, total reclaim.
// ---------------------------------------------------------------------------

#[test]
fn dialog_body_shows_canonical_targets_savings_and_total_reclaim() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_blob_duplicate(&temp);
    let (mut cmd, _temp, _log_file) = modeltap_headless_at(&ollama, &hf);
    let regs = detail_regs_json(&ollama_blob, &hf_blob);
    // Open Detail (existing seam), press u (opens dialog), then quit
    // BEFORE pressing Enter so the dialog body remains visible in the
    // captured frame.
    let script = "<enter>u<esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();
    assert!(
        lower.contains("total reclaim:"),
        "AC-U5.1: dialog body must show 'Total reclaim:' label, got:\n{}",
        frame
    );
    assert!(
        lower.contains("[enter] apply") || lower.contains("[enter]apply"),
        "AC-U5.1: dialog footer must show '[Enter] Apply' action, got:\n{}",
        frame
    );
    assert!(
        lower.contains("[space] toggle") || lower.contains("[space]toggle"),
        "AC-U5.1: dialog footer must show '[space] Toggle' action, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U5.2 + AC-U5.3: toggling a target updates the total live.
// ---------------------------------------------------------------------------

#[test]
fn toggling_a_target_with_space_updates_the_total_reclaim() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_blob_duplicate(&temp);
    let (mut cmd, _temp, _log_file) = modeltap_headless_at(&ollama, &hf);
    let regs = detail_regs_json(&ollama_blob, &hf_blob);
    // Open dialog (one target — HF — since the canonical is the Ollama copy),
    // toggle the only target off with space, capture frame, then close+quit.
    let script = "<enter>u<space><esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();
    // With the only target toggled off, Total reclaim should drop to 0.
    // Accept "0 b" (formatted), or "0 " followed by SI unit.
    assert!(
        lower.contains("total reclaim: 0 b") || lower.contains("total reclaim: 0\n"),
        "AC-U5.2/U5.3: after toggling the only target off, Total reclaim \
         should drop to 0; got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U5.4: Enter applies the plan.
// ---------------------------------------------------------------------------

#[test]
fn pressing_enter_applies_the_plan_and_creates_hardlink() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_blob_duplicate(&temp);
    let pre_a = ino_of(&ollama_blob);
    let pre_b = ino_of(&hf_blob);
    assert_ne!(pre_a, pre_b, "fixture must start with separate inodes");

    let (mut cmd, _temp, log_file) = modeltap_headless_at(&ollama, &hf);
    let regs = detail_regs_json(&ollama_blob, &hf_blob);
    let script = "<enter>u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let post_a = ino_of(&ollama_blob);
    let post_b = ino_of(&hf_blob);
    assert_eq!(
        post_a, post_b,
        "AC-U5.4: Enter on the unify dialog must apply the plan, merging \
         inodes (got ollama={}, hf={})",
        post_a, post_b
    );
    let events = read_jsonl_events(&log_file);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
            .count(),
        1,
        "AC-U5.4: must emit exactly 1 action.unify event"
    );
}

// ---------------------------------------------------------------------------
// AC-U5.5: Esc cancels with no filesystem effect.
// ---------------------------------------------------------------------------

#[test]
fn pressing_esc_cancels_dialog_with_no_filesystem_change() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_blob_duplicate(&temp);
    let pre_a = ino_of(&ollama_blob);
    let pre_b = ino_of(&hf_blob);
    assert_ne!(pre_a, pre_b);

    let (mut cmd, _temp, _log_file) = modeltap_headless_at(&ollama, &hf);
    let regs = detail_regs_json(&ollama_blob, &hf_blob);
    let script = "<enter>u<esc>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let post_a = ino_of(&ollama_blob);
    let post_b = ino_of(&hf_blob);
    assert_eq!(post_a, pre_a, "AC-U5.5: ollama inode must be unchanged");
    assert_eq!(post_b, pre_b, "AC-U5.5: hf inode must be unchanged");
    assert_ne!(post_a, post_b, "AC-U5.5: Esc must NOT merge inodes");
}
