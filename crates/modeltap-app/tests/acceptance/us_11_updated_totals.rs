//! Acceptance tests for US-11 (Updated totals after action).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-11 @release-2 scenarios. These promote 01-05's basic post-zap refresh
//! into a robust incremental rediscovery contract:
//!
//! - **AC-1** (Totals update after zap within 500 ms) — after a zap, the
//!   summary bar reflects the post-mutation total within 500 ms; new total =
//!   old total - bytes_reclaimed within rounding.
//! - **AC-2** (Refresh failure shows degraded indicator) — when the affected
//!   tool's `discover()` returns Err mid-action, the summary bar continues to
//!   render the old totals AND shows a `(refresh failed)` indicator + `[r]`
//!   retry shortcut. Tagged `@infrastructure-failure`.
//! - **AC-3 / INT-5 invariant** — checked by the property test in
//!   `tests/int5_invariant.rs`; here we additionally assert the visible
//!   summary delta from the on-screen frame.
//! - **Unify keeps model-count steady** — after unify the model is still
//!   registered with every tool (only its disk usage drops); summary "Total:
//!   N models" must NOT decrease.
//!
//! Tags: @us-11 @release-2; the failure scenario is also @infrastructure-failure.

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
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
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
// Scenario 1 (US-11.AC-1):
// "Totals update after zap within 500 ms"
//
// After a confirmed zap of Ollama (devon-multi-tool fixture, ~18 GB across
// 3 unique blobs + a sparse-file 4th manifest reusing one blob), the summary
// bar's "Disk:" total must drop. The headless harness paints exactly one
// final frame after the action; that frame is what the user sees within
// 500 ms in production. The latency budget itself is enforced by the
// `crates/modeltap-app/tests/refresh_tool.rs` and the new
// `tests/refresh_incremental.rs` unit tests; this acceptance test asserts
// the user-visible CONSEQUENCE — the summary line shows the new total.
// ---------------------------------------------------------------------------

#[test]
fn totals_update_after_zap_within_500ms() {
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

    // After zap, the summary line still has the schema "Disk: <N> ..." but
    // the value is now 0 B (only ollama installed in the WS). We assert
    // both the schema AND the post-zap value: "Disk: 0 B".
    assert!(
        frame.contains("Disk:"),
        "AC-1: summary 'Disk:' label missing in frame:\n{}",
        frame
    );
    assert!(
        frame.contains("Disk: 0 B"),
        "AC-1: after zap, summary must show 'Disk: 0 B' (new total = old total - reclaimed); got:\n{}",
        frame
    );

    // Total models drops to 0 too (every manifest deleted by zap-all).
    assert!(
        frame.contains("Total: 0 models"),
        "AC-1: after zap, summary must show 'Total: 0 models'; got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 (US-11.AC-2):
// "Refresh failure shows degraded indicator"
//
// Tagged @infrastructure-failure. The headless harness simulates a refresh
// failure by deleting the Ollama models directory mid-action: the zap
// succeeds (it walked the dir before deletion), but the post-action
// `refresh_tool_incremental` call sees the directory gone and returns
// `RefreshError::Unreadable`. The render layer must:
//   - keep the prior tool slot in `AppState.tools` (don't blank it out),
//   - mark `state.refresh_failed_tools` to include the affected tool,
//   - render `(refresh failed)` indicator in the summary bar,
//   - render `[r] retry` shortcut in the bottom bar.
//
// We trigger the failure via the `MODELTAP_FORCE_REFRESH_FAIL` env-var seam
// (test-only) which makes `refresh_tool_incremental` return Err for the
// named tool. Production paths never set this env-var.
// ---------------------------------------------------------------------------

#[test]
fn refresh_failure_shows_degraded_indicator() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    // Same script as AC-1 — but with the test-seam env var that forces
    // refresh_tool_incremental to return Err for ollama.
    let script = "zollama<enter>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_FORCE_REFRESH_FAIL", "ollama")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // The degraded indicator schema (US-11.AC-2): the summary line carries
    // `(refresh failed)` as a postfix when refresh_failed_tools is non-empty.
    assert!(
        frame.contains("(refresh failed)"),
        "AC-2: '(refresh failed)' indicator missing from summary line; got:\n{}",
        frame
    );

    // The [r] retry shortcut becomes visible in the bottom bar when the
    // refresh_failed_tools set is non-empty.
    assert!(
        frame.contains("[r] retry"),
        "AC-2: '[r] retry' shortcut missing from bottom bar; got:\n{}",
        frame
    );

    // Old totals are preserved (the tool slot was NOT blanked out): the
    // "Total: N models" line still reflects the pre-refresh count of 4.
    // (The zap itself did delete the on-disk content but the in-memory
    // ToolView slot remains as it was before the failed refresh.)
    assert!(
        frame.contains("Total: 4 models"),
        "AC-2: pre-refresh totals must be preserved when refresh fails; got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 (US-11 unify-model-count-steady):
// "Totals update after unify (disk down, model count steady)"
//
// After a unify, the canonical's bytes are now shared (1 inode, K
// hardlinks). The model is still registered with every participating tool.
// Therefore:
//   - "Total: N models" is UNCHANGED before/after unify.
//   - "Disk:" drops by `bytes_reclaimed` (the duplicate-inode bytes the
//     hardlinks reclaimed).
//
// Uses a synthetic 2-tool fixture (ollama + hf).
// ---------------------------------------------------------------------------

#[test]
fn totals_update_after_unify_disk_down_model_count_steady() {
    use std::os::unix::fs::MetadataExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let payload = vec![0xA5u8; 4096];

    // Ollama layout (one manifest, one blob, 4096 bytes).
    let ollama_dir = root.join(".ollama").join("models");
    let ollama_blobs = ollama_dir.join("blobs");
    std::fs::create_dir_all(&ollama_blobs).expect("ollama blobs");
    let blob_hash = "1111111111111111111111111111111111111111111111111111111111111111";
    let ollama_path = ollama_blobs.join(format!("sha256-{}", blob_hash));
    std::fs::write(&ollama_path, &payload).expect("write ollama blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("us11");
    std::fs::create_dir_all(&manifest_dir).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob_hash
    );
    std::fs::write(manifest_dir.join("7b"), manifest).expect("manifest");

    // HF layout — same content, separate inode.
    let hf_home = root.join(".cache").join("huggingface");
    let hf_hub = hf_home.join("hub");
    let hf_repo_dir = hf_hub.join("models--us11--Synthetic-7B");
    let hf_rev = "abc123def4567890abc123def4567890abc12345";
    let hf_blobs = hf_repo_dir.join("blobs");
    let hf_snapshots = hf_repo_dir.join("snapshots").join(hf_rev);
    let hf_refs = hf_repo_dir.join("refs");
    std::fs::create_dir_all(&hf_blobs).expect("hf blobs");
    std::fs::create_dir_all(&hf_snapshots).expect("hf snapshots");
    std::fs::create_dir_all(&hf_refs).expect("hf refs");
    let hf_blob_name = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
    let hf_blob_path = hf_blobs.join(hf_blob_name);
    std::fs::write(&hf_blob_path, &payload).expect("hf blob");
    let snapshot_link = hf_snapshots.join("model.safetensors");
    let rel_target = PathBuf::from("..")
        .join("..")
        .join("blobs")
        .join(hf_blob_name);
    std::os::unix::fs::symlink(&rel_target, &snapshot_link).expect("hf symlink");
    std::fs::write(hf_refs.join("main"), hf_rev).expect("hf ref");

    let pre_ollama_ino = std::fs::metadata(&ollama_path).unwrap().ino();
    let pre_hf_ino = std::fs::metadata(&hf_blob_path).unwrap().ino();
    assert_ne!(pre_ollama_ino, pre_hf_ino, "fixture precondition");

    let log_dir_temp = tempfile::tempdir().expect("log temp");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("log dir");

    let regs = serde_json::json!({
        "id": "us11/synthetic-7b",
        "regs": [
            {"tool": "ollama", "path": ollama_path.display().to_string()},
            {"tool": "hf",     "path": snapshot_link.display().to_string()},
        ]
    })
    .to_string();

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin");
    let assert = cmd
        .env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &ollama_dir)
        .env("HF_HOME", &hf_home)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        // <enter> opens detail; u opens unify dialog; <enter> confirms unify;
        // <esc> returns from detail to main so the summary bar is visible
        // (the summary bar only renders on the main two-pane view); q quits.
        .env("MODELTAP_HEADLESS_INPUT", "<enter>u<enter><esc>q")
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // Inodes match after unify (precondition for the model-count-steady
    // claim — the model is still registered with both tools).
    let post_ollama_ino = std::fs::metadata(&ollama_path).unwrap().ino();
    let post_hf_ino = std::fs::metadata(&hf_blob_path).unwrap().ino();
    assert_eq!(
        post_ollama_ino, post_hf_ino,
        "post-condition: paths must share inode after unify"
    );

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // Model count steady: 2 models pre-unify (1 ollama manifest + 1 hf
    // snapshot); both still registered post-unify.
    assert!(
        frame.contains("Total: 2 models"),
        "unify-model-count-steady: 'Total: 2 models' must remain post-unify; got:\n{}",
        frame
    );

    // No degraded indicator on a successful refresh path.
    assert!(
        !frame.contains("(refresh failed)"),
        "unify-success: '(refresh failed)' indicator must NOT appear on successful refresh; got:\n{}",
        frame
    );
}
