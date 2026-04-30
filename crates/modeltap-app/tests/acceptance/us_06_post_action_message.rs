//! Acceptance tests for US-06 (Post-action message with reclaim/retain breakdown).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @walking-skeleton @us-06 scenarios. The 4 scenarios are:
//!
//! 1. **Successful zap shows reclaimed and retained bytes** (active in WS).
//!    The right pane renders `Last action: zap <tool> (success)` header and
//!    `Reclaimed: <N> GB (<M> GB retained — also linked from other tools)`
//!    body after a confirmed zap. The summary bar updates total disk usage
//!    within 500 ms. Adapted from the master-acceptance "llama-cli" target —
//!    in the WS slice only Ollama has a populated fixture, so the assertion
//!    targets `zap ollama (success)` (per the same WS-adaptation note in
//!    us_05_zap_all.rs). Behavioral intent — header + body schema, summary
//!    refresh — is preserved.
//!
//! 2. **Last-action message clears when Devon navigates** (active in WS). After
//!    a zap, pressing Right Arrow advances to the next tool slot AND clears
//!    the last-action header from the right pane.
//!
//! 3. **Successful unify shows hardlink count** (`#[ignore]` — re-enable when
//!    03-02 lands). The unify action does not exist in the WS slice; the
//!    scenario is preserved as the regression net for 03-02. Re-enable by
//!    removing the `#[ignore]` once `Tool::link` returns success outcomes
//!    from a real plugin.
//!
//! 4. **Partial unify shows partial-success message** (`#[ignore]` — re-enable
//!    when 03-03 lands). Cross-filesystem partial-success requires the
//!    devon-cross-fs fixture and `LinkOutcome::Failed` plumbing, neither of
//!    which exist in the WS slice.
//!
//! Tags: @us-06 @walking-skeleton.

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

fn build_fixture(name: &str) -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join(name);
    let project_root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|p| p.parent().and_then(|p| p.parent().map(PathBuf::from)))
        .expect("CARGO_MANIFEST_DIR + walk to workspace root");
    let script = project_root.join("tests/fixtures/build.sh");
    let status = StdCommand::new("bash")
        .arg(&script)
        .arg(name)
        .arg(&target)
        .status()
        .expect("spawn build.sh");
    assert!(status.success(), "fixture builder failed for {}", name);
    let ollama_dir = target.join(".ollama").join("models");
    (temp, ollama_dir)
}

fn modeltap_headless(ollama_dir: Option<&Path>) -> (Command, TempDir) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "100")
        // Pin the other plugins at non-existent paths so this test isolates
        // from the developer's real Ollama / llama-cli / HF / lm-studio installs.
        .env("MODELTAP_LLAMACLI_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache");
    if let Some(dir) = ollama_dir {
        cmd.env("MODELTAP_OLLAMA_DIR", dir);
    } else {
        cmd.env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama");
    }
    (cmd, log_dir_temp)
}

fn frame_text(stdout: &str) -> String {
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect();
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Scenario 1 (US-06.AC-1, AC-2, AC-4 / US-11.AC-1):
// "Successful zap shows reclaimed and retained bytes"
//
// Adapted from the master-acceptance llama-cli target (see file-level note).
// On a successful zap of Ollama (devon-multi-tool fixture, 4 manifests over
// 3 unique blobs ≈ 18.0 GB), the right pane must render:
//   - Header: "Last action: zap ollama (success)"
//   - Body:   "Reclaimed: <N> GB" (retained part is "0 GB" in the WS slice
//             because no shared models exist; the header text uses the
//             "(<M> GB retained — also linked from other tools)" schema only
//             when M > 0; the test asserts the schema-relevant substrings).
//
// Note on bytes: the devon-multi-tool fixture has a 4th manifest whose blob
// equals one of the other 3 (the "codellama" manifest reuses an earlier
// blob), so unique-blob bytes < sum-of-manifests bytes. The test asserts
// "Reclaimed:" and "GB" appearance rather than an exact byte total to
// stay robust against fixture tweaks.
// ---------------------------------------------------------------------------

#[test]
fn successful_zap_shows_reclaimed_and_retained_bytes() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    // Default selection lands on ollama. Press z, type "ollama", Enter, q.
    let script = "zollama<enter>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // Header: "Last action: zap ollama (success)" — exact schema match.
    assert!(
        frame.contains("Last action: zap ollama (success)"),
        "AC-1: expected exact header 'Last action: zap ollama (success)' in frame, got:\n{}",
        frame
    );

    // Body: "Reclaimed: <N> GB" — schema match. Retained-bytes parenthetical
    // is required when M > 0; for the WS slice with no cross-tool sharing
    // we only require the "Reclaimed:" prefix and a "GB" unit.
    assert!(
        frame.contains("Reclaimed:") && frame.contains("GB"),
        "AC-2: expected 'Reclaimed: <N> GB' body in frame, got:\n{}",
        frame
    );

    // AC-4 / US-11.AC-1: summary bar shows updated total disk usage. With
    // all blobs zapped, the summary "Disk:" line should reflect the post-
    // zap inventory (0 bytes for ollama, the only installed tool in WS).
    // The summary bar is rendered as part of the bottom row; we assert the
    // schema (the substring "Disk:") appears in the frame.
    assert!(
        frame.contains("Disk:"),
        "US-11.AC-1: summary bar must show 'Disk:' total, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 (US-06 nav-clears):
// "Last-action message clears when Devon navigates"
//
// After a successful zap of Ollama, Devon presses Right Arrow to advance to
// the next tool slot. The right pane must NO LONGER render the "Last action"
// header — it must show the new tool's models (or empty/not-installed
// message). This is a state-clearing assertion: the post-action message is
// in-memory only and is cleared on any navigation Msg.
// ---------------------------------------------------------------------------

#[test]
fn last_action_message_clears_when_devon_navigates() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    // zap → confirm → press Right Arrow → quit. The Right Arrow advances
    // from ollama to the alphabetically-next tool (hf), which is
    // NotInstalled in the WS slice. The right pane must not show the
    // last-action header.
    let script = "zollama<enter><right>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // The "Last action" header must NOT appear after navigation.
    assert!(
        !frame.contains("Last action: zap"),
        "nav-clears: 'Last action: zap' header must be cleared after Right Arrow, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 (US-06 unify-success — un-ignored in 03-02):
// "Successful unify shows hardlink count"
//
// On successful unify, the right pane shows:
//   Header: "Last action: unify <model-id> (success)"
//   Body:   "Reclaimed: <N> B (1 inode, <K> hardlinks)"
//
// Drives the headless harness against a multi-tool shared-content fixture;
// after pressing `<enter>u<enter>q` the post-action banner records the unify
// outcome with the structured `extra` line "1 inode, K hardlinks" per
// `LastAction::for_unify_success` (US-06 schema).
// ---------------------------------------------------------------------------

#[test]
fn successful_unify_shows_hardlink_count() {
    use std::os::unix::fs::MetadataExt;

    // Build a shared-content fixture with 2 tools (ollama + llama-cli) so
    // unify produces 1 hardlink (2 paths → 1 inode, 2 hardlinks counts the
    // canonical's link count after unify).
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let payload = vec![0xCAu8; 4096];

    // Ollama layout.
    let ollama_dir = root.join(".ollama").join("models");
    let ollama_blobs = ollama_dir.join("blobs");
    std::fs::create_dir_all(&ollama_blobs).expect("ollama blobs");
    let blob_hash = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let ollama_path = ollama_blobs.join(format!("sha256-{}", blob_hash));
    std::fs::write(&ollama_path, &payload).expect("write ollama blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("us06");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob_hash
    );
    std::fs::write(manifest_dir.join("7b"), manifest).expect("manifest");

    // llama-cli layout (separate inode, same content).
    let llama_dir = root.join("llms");
    std::fs::create_dir_all(&llama_dir).expect("llama dir");
    let llama_path = llama_dir.join("us06-7b.gguf");
    let mut gguf_bytes = b"GGUF".to_vec();
    gguf_bytes.extend(&payload[..payload.len() - 4]);
    std::fs::write(&llama_path, &gguf_bytes).expect("llama gguf");

    let pre_ollama_ino = std::fs::metadata(&ollama_path).unwrap().ino();
    let pre_llama_ino = std::fs::metadata(&llama_path).unwrap().ino();
    assert_ne!(pre_ollama_ino, pre_llama_ino, "fixture precondition");

    let log_dir_temp = tempfile::tempdir().expect("log temp");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("log dir");

    let regs = serde_json::json!({
        "id": "us06/synthetic-7b",
        "regs": [
            {"tool": "ollama",    "path": ollama_path.display().to_string()},
            {"tool": "llama-cli", "path": llama_path.display().to_string()},
        ]
    })
    .to_string();

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin");
    let assert = cmd
        .env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &ollama_dir)
        .env("MODELTAP_LLAMACLI_DIRS", &llama_dir)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf")
        .env("MODELTAP_HEADLESS_INPUT", "<enter>u<enter>q")
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // Inodes match after unify.
    let post_ollama_ino = std::fs::metadata(&ollama_path).unwrap().ino();
    let post_llama_ino = std::fs::metadata(&llama_path).unwrap().ino();
    assert_eq!(
        post_ollama_ino, post_llama_ino,
        "post-condition: paths must share inode after unify"
    );

    // The headless harness prints the final TestBackend frame. After
    // `Msg::SetLastAction(unify_success)` the right-pane banner renders:
    //   "Last action: unify us06/synthetic-7b (success)"
    //   "Reclaimed: <N> B (1 inode, K hardlinks)"
    // We assert the hardlink-count phrasing schema; the exact reclaim byte
    // count varies by classify() prorating logic so we only assert the
    // "1 inode" + "hardlinks" markers from `LastAction::for_unify_success`.
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    assert!(
        frame.contains("Last action: unify"),
        "unify-success banner header missing in frame:\n{}",
        frame
    );
    // "1 inode" comes from the `extra` line of LastAction::for_unify_success.
    assert!(
        frame.contains("1 inode"),
        "AC-3: '1 inode' marker missing from unify-success body:\n{}",
        frame
    );
    assert!(
        frame.contains("hardlinks"),
        "AC-3: 'hardlinks' marker missing from unify-success body:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 4 (US-06 partial — un-ignored in 03-03):
// "Partial unify shows partial-success message"
//
// On partial unify, the right pane renders:
//   Header: "Last action: unify <model-id> (partial: <N> of <M> targets linked)"
//   Body:   per-target failure reason(s)
//
// We synthesize the partial outcome by injecting a cross-fs target via the
// US-19 `MODELTAP_FAKE_CROSS_FS_PATHS` seam AND making that target's parent
// directory unwritable so the user-chosen `c` (copy) path FAILS at the
// write+rename step. The orchestrator records 1 failure + 1 success →
// outcome=Partial → the partial-success banner schema fires.
// ---------------------------------------------------------------------------

#[test]
fn partial_unify_shows_partial_success_message() {
    use std::os::unix::fs::PermissionsExt;

    // 3-tool fixture so we can produce a TRUE partial outcome:
    //   ollama (canonical) + hf (same-fs, will hardlink) + llama-cli
    //   (cross-fs target whose copy will FAIL because its parent dir is
    //   chmod 0o500). Result: 1 success + 1 failure → UnifyResult::Partial.
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let payload = vec![0xC3u8; 4096];

    let ollama_dir = root.join(".ollama").join("models");
    let ollama_blobs = ollama_dir.join("blobs");
    std::fs::create_dir_all(&ollama_blobs).expect("ollama blobs");
    let blob_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let ollama_path = ollama_blobs.join(format!("sha256-{}", blob_hash));
    std::fs::write(&ollama_path, &payload).expect("write ollama blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("us06p");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob_hash
    );
    std::fs::write(manifest_dir.join("7b"), manifest).expect("manifest");

    // HF same-fs target — will hardlink successfully.
    let hf_home = root.join(".cache").join("huggingface");
    let hf_hub = hf_home.join("hub");
    let hf_repo_dir = hf_hub.join("models--us06p--Synthetic-7B");
    let hf_rev = "abc123def4567890abc123def4567890abc12345";
    let hf_blobs = hf_repo_dir.join("blobs");
    let hf_snapshots = hf_repo_dir.join("snapshots").join(hf_rev);
    let hf_refs = hf_repo_dir.join("refs");
    std::fs::create_dir_all(&hf_blobs).expect("hf blobs");
    std::fs::create_dir_all(&hf_snapshots).expect("hf snapshots");
    std::fs::create_dir_all(&hf_refs).expect("hf refs");
    let hf_blob_name = "ddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd000";
    let hf_blob_path = hf_blobs.join(hf_blob_name);
    std::fs::write(&hf_blob_path, &payload).expect("hf blob");
    let snapshot_link = hf_snapshots.join("model.safetensors");
    let rel_target = PathBuf::from("..")
        .join("..")
        .join("blobs")
        .join(hf_blob_name);
    std::os::unix::fs::symlink(&rel_target, &snapshot_link).expect("hf symlink");
    std::fs::write(hf_refs.join("main"), hf_rev).expect("hf ref");

    // llama-cli on a separate dir (will be marked cross-fs by the fake-fs
    // probe; its parent dir is made read-only so cross-fs Copy fails).
    let llama_dir = root.join("llms");
    std::fs::create_dir_all(&llama_dir).expect("llama dir");
    let llama_path = llama_dir.join("us06p-7b.gguf");
    let mut gguf = b"GGUF".to_vec();
    gguf.extend(&payload[..payload.len() - 4]);
    std::fs::write(&llama_path, &gguf).expect("write llama gguf");

    // Lock the llama-cli parent dir to read-only so the Copy-fallback's
    // create_dir_all/rename in the temp file path will fail with EACCES.
    let mut perms = std::fs::metadata(&llama_dir).unwrap().permissions();
    perms.set_mode(0o500);
    std::fs::set_permissions(&llama_dir, perms).expect("readonly llama dir");

    let log_dir_temp = tempfile::tempdir().expect("log temp");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("log dir");

    let hf_snapshot_path = hf_snapshots.join("model.safetensors");
    let regs = serde_json::json!({
        "id": "us06p/synthetic-7b",
        "regs": [
            {"tool": "ollama",    "path": ollama_path.display().to_string()},
            {"tool": "hf",        "path": hf_snapshot_path.display().to_string()},
            {"tool": "llama-cli", "path": llama_path.display().to_string()},
        ]
    })
    .to_string();

    let fake_cross_fs = std::fs::canonicalize(&llama_dir)
        .expect("canonicalize llama_dir")
        .display()
        .to_string();

    // Script: <enter> open detail; u opens cross-fs dialog (mixed: ollama+hf
    // same-fs, llama-cli cross-fs); c = copy (cross-fs copy fails for
    // llama-cli because the parent dir is read-only); q quit.
    let script = "<enter>uc q";

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin");
    let output = cmd
        .env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &ollama_dir)
        .env("MODELTAP_LLAMACLI_DIRS", &llama_dir)
        .env("HF_HOME", &hf_home)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_CROSS_FS_PATHS", &fake_cross_fs)
        .timeout(Duration::from_secs(20))
        .assert();

    // Restore writability so the tempdir cleanup can run.
    let mut restore = std::fs::metadata(&llama_dir).unwrap().permissions();
    restore.set_mode(0o700);
    let _ = std::fs::set_permissions(&llama_dir, restore);

    let assert = output.success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // Partial banner schema: "(partial:" appears in the header. The exact
    // count phrasing comes from `LastAction::for_unify_partial`.
    assert!(
        frame.contains("(partial") || frame.contains("partial:"),
        "AC-4: partial-success banner header missing in frame:\n{}",
        frame
    );
}
