//! Acceptance tests for US-17 (Running-tool detect-and-prompt-then-retry,
//! intake Q5; ADR-007 thiserror+anyhow boundary).
//!
//! Per intake Q5: when the user attempts a unify or delete-one action while a
//! registered tool process holds in-scope files open, modeltap REFUSES the
//! action and prompts close-and-retry — NOT a soft-warning. The user must
//! close the tool and press [r] to retry; pressing [Esc] cancels. While the
//! prompt is open, NO filesystem mutation may occur.
//!
//! Detection happens via `lsof` on macOS/Linux (`cfg!(unix)`-gated). On
//! systems where `lsof` is missing (stripped containers), the dialog reads
//! "Running-tool detection unavailable on this system" and the user may
//! proceed at own risk.
//!
//! ## Test seam: MODELTAP_FAKE_LSOF_OUTPUT
//!
//! The real `lsof_adapter` checks `MODELTAP_FAKE_LSOF_OUTPUT` first; when set
//! it parses the env-var contents as if `lsof` had emitted them, never
//! invoking the real subprocess. This lets CI exercise every branch
//! deterministically without needing a real running tool process.
//!
//! Sentinel value `MODELTAP_FAKE_LSOF_OUTPUT=__UNAVAILABLE__` simulates lsof
//! being missing (the adapter returns `Err(ProbeError::LsofUnavailable)`).
//! Sentinel value `MODELTAP_FAKE_LSOF_OUTPUT=__EMPTY__` simulates lsof
//! exiting 1 with no matches (no running tool — proceed normally).
//!
//! ## The 4 scenarios
//!
//! 1. **Running tool surfaces close-and-retry prompt** — fake lsof returns
//!    `ollama (PID 1234)` for the in-scope file; press `u`; the cross-fs
//!    dialog is bypassed and the running-tool dialog opens; verify NO
//!    mutation (inode + bytes unchanged) and dialog text contains "Close"
//!    and "retry".
//!
//! 2. **No running tools, no warning** — fake lsof returns no matches; press
//!    `u`; expect the normal unify dialog (no running-tool prompt). The
//!    JSONL log has an `action.unify` event.
//!
//! 3. **lsof unavailable surfaces explicit message** — fake lsof simulates
//!    "command not found"; expect a frame containing
//!    "Running-tool detection unavailable" so the user can decide to proceed.
//!
//! 4. **Detection latency is below the 500 ms p99 budget** — invoke
//!    `detect_running_tools` 50 times against the fake-lsof seam; assert
//!    every invocation completes in < 500 ms (the seam path skips the
//!    subprocess so this is mostly a guard that the dialog raise itself
//!    isn't blocking).
//!
//! Tags: @us-17 @release-2 @running-tool

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Shared fixture builder. A single tool (ollama) registration plus an HF
// snapshot — same content under two trees so unify has something to do. The
// running-tool flag is INJECTED at run time via `MODELTAP_FAKE_LSOF_OUTPUT`;
// the on-disk layout is unchanged from the standard us_10 fixture.
// ---------------------------------------------------------------------------

struct Fixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
    hf_home: PathBuf,
    ollama_path: PathBuf,
    hf_blob_path: PathBuf,
    hf_snapshot_path: PathBuf,
    #[allow(dead_code)]
    payload: Vec<u8>,
}

fn build_fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let payload_size: usize = 4096;
    let payload: Vec<u8> = (0..payload_size).map(|i| (i % 251) as u8).collect();

    // Ollama layout.
    let ollama_dir = root.join(".ollama").join("models");
    let blob_hash = "abc123def4567890abc123def4567890abc123def4567890abc123def4567890";
    let ollama_blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&ollama_blobs).expect("create ollama blobs");
    let ollama_path = ollama_blobs.join(format!("sha256-{}", blob_hash));
    fs::write(&ollama_path, &payload).expect("write ollama blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("us17");
    fs::create_dir_all(&manifest_dir).expect("create manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":{size}}}]}}"#,
        blob = blob_hash,
        size = payload_size
    );
    fs::write(manifest_dir.join("7b"), manifest).expect("write manifest");

    // HF layout — a separate tree with the same payload so unify has something
    // to deduplicate.
    let hf_home = root.join(".cache").join("huggingface");
    let hf_hub = hf_home.join("hub");
    let hf_repo_dir = hf_hub.join("models--us17--Synthetic-7B");
    let hf_rev = "abc123def4567890abc123def4567890abc12345";
    let hf_blobs = hf_repo_dir.join("blobs");
    let hf_snapshots = hf_repo_dir.join("snapshots").join(hf_rev);
    let hf_refs = hf_repo_dir.join("refs");
    fs::create_dir_all(&hf_blobs).expect("create hf blobs");
    fs::create_dir_all(&hf_snapshots).expect("create hf snapshots");
    fs::create_dir_all(&hf_refs).expect("create hf refs");
    let hf_blob_name = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
    let hf_blob_path = hf_blobs.join(hf_blob_name);
    fs::write(&hf_blob_path, &payload).expect("write hf blob");
    let snapshot_link = hf_snapshots.join("model.safetensors");
    let rel_target = PathBuf::from("..")
        .join("..")
        .join("blobs")
        .join(hf_blob_name);
    std::os::unix::fs::symlink(&rel_target, &snapshot_link).expect("symlink hf snapshot");
    fs::write(hf_refs.join("main"), hf_rev).expect("write hf ref");

    Fixture {
        _temp: temp,
        ollama_dir,
        hf_home,
        ollama_path,
        hf_blob_path,
        hf_snapshot_path: snapshot_link,
        payload,
    }
}

fn modeltap_headless(fix: &Fixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &fix.ollama_dir)
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("HF_HOME", &fix.hf_home)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    (cmd, log_dir_temp, log_file)
}

fn detail_regs_json(fix: &Fixture) -> String {
    serde_json::json!({
        "id": "us17/Synthetic-7B",
        "regs": [
            {"tool": "ollama", "path": fix.ollama_path.display().to_string()},
            {"tool": "hf",     "path": fix.hf_snapshot_path.display().to_string()},
        ]
    })
    .to_string()
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

/// Build an lsof output line as the BSD lsof command emits it: COMMAND PID
/// USER FD TYPE DEVICE SIZE/OFF NODE NAME columns separated by whitespace.
/// The adapter only consumes COMMAND, PID, and NAME; the other columns must
/// exist for the line to parse but their values are inert.
fn fake_lsof_running(tool: &str, pid: u32, file_path: &Path) -> String {
    format!(
        "COMMAND     PID       USER   FD   TYPE DEVICE   SIZE/OFF NODE NAME\n{tool:<11} {pid:<9} testuser  3r   REG    1,4        0t0  100 {}\n",
        file_path.display()
    )
}

// ---------------------------------------------------------------------------
// Scenario 1: Running tool surfaces close-and-retry prompt + NO mutation
// ---------------------------------------------------------------------------

#[test]
fn running_tool_surfaces_close_and_retry_prompt() {
    let fix = build_fixture();
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    // Capture pre-action state so we can prove no mutation while the dialog
    // is open.
    let pre_ollama_ino = ino_of(&fix.ollama_path);
    let pre_hf_ino = ino_of(&fix.hf_blob_path);
    let pre_ollama_bytes = fs::read(&fix.ollama_path).expect("read ollama pre");
    let pre_hf_bytes = fs::read(&fix.hf_blob_path).expect("read hf pre");

    // Simulate `ollama` holding the ollama blob open. The fake-lsof seam
    // returns the pre-formatted lsof output, the adapter parses it, and the
    // unify gate refuses with the running-tool dialog.
    let fake = fake_lsof_running("ollama", 1234, &fix.ollama_path);

    // Script: <enter> open detail; u attempt unify (gate blocks).
    // The dialog opens; we press <esc> to dismiss without retrying.
    let script = "<enter>u<esc>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_LSOF_OUTPUT", fake)
        .timeout(Duration::from_secs(20));

    let output = cmd.output().expect("run modeltap");
    assert!(
        output.status.success(),
        "modeltap should exit cleanly even when unify is gated, got {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let frame = String::from_utf8_lossy(&output.stdout);

    // AC-1 + AC-2: the dialog must mention "running" + a close-and-retry
    // affordance. Per intake Q5 the wording is "<tool> is running and has
    // this file open. Close <tool> and retry."
    assert!(
        frame.to_lowercase().contains("running")
            && frame.to_lowercase().contains("close")
            && (frame.to_lowercase().contains("retry")
                || frame.contains("[r]")
                || frame.contains("(r)")),
        "AC-2: running-tool dialog must contain 'running'/'close'/'retry' text, got frame:\n{}",
        frame
    );

    // No-mutation invariant: while the running-tool dialog was open and then
    // dismissed, neither the ollama blob nor the hf blob may have changed.
    assert_eq!(
        ino_of(&fix.ollama_path),
        pre_ollama_ino,
        "AC-6: ollama inode must NOT change while running-tool dialog is open"
    );
    assert_eq!(
        ino_of(&fix.hf_blob_path),
        pre_hf_ino,
        "AC-6: hf inode must NOT change while running-tool dialog is open"
    );
    assert_eq!(
        fs::read(&fix.ollama_path).unwrap(),
        pre_ollama_bytes,
        "AC-6: ollama bytes must NOT change while running-tool dialog is open"
    );
    assert_eq!(
        fs::read(&fix.hf_blob_path).unwrap(),
        pre_hf_bytes,
        "AC-6: hf bytes must NOT change while running-tool dialog is open"
    );

    // No `action.unify` JSONL event with outcome=success — the gate refused
    // the action.
    let events = read_jsonl_events(&log_file);
    for e in events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
    {
        let outcome = e.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            outcome, "success",
            "AC-2: gate must NOT emit outcome=success when running-tool detected, got {}",
            e
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 2: No running tools, no warning — normal unify proceeds
// ---------------------------------------------------------------------------

#[test]
fn no_running_tools_no_warning() {
    let fix = build_fixture();
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    // The fake-lsof seam returns __EMPTY__ — adapter behaves as if lsof exited
    // 1 with no output (no in-scope files open).
    let script = "<enter>u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_LSOF_OUTPUT", "__EMPTY__")
        .timeout(Duration::from_secs(20));

    let output = cmd.output().expect("run modeltap");
    assert!(
        output.status.success(),
        "modeltap should exit cleanly when no running tools, got {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let frame = String::from_utf8_lossy(&output.stdout);

    // The frame must NOT contain a running-tool prompt.
    assert!(
        !(frame
            .to_lowercase()
            .contains("running and has this file open")
            || frame.to_lowercase().contains("close ollama and retry")
            || frame.to_lowercase().contains("close hf and retry")),
        "AC-1: must NOT show running-tool prompt when no tools running, got frame:\n{}",
        frame
    );

    // The action.unify event must record outcome=success (normal path).
    let events = read_jsonl_events(&log_file);
    let unify_event = events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"));
    assert!(
        unify_event.is_some(),
        "AC-1: when no running tool, unify must proceed and emit action.unify"
    );

    // Inodes share — unify went through.
    assert_eq!(
        ino_of(&fix.ollama_path),
        ino_of(&fix.hf_blob_path),
        "AC-1: when no running tool, unify must hardlink targets normally"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: lsof unavailable surfaces explicit message
// ---------------------------------------------------------------------------

#[test]
fn lsof_unavailable_surfaces_explicit_message() {
    let fix = build_fixture();
    let (mut cmd, _log_temp, _log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    // Sentinel: simulate lsof binary being missing.
    let script = "<enter>u<esc>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_LSOF_OUTPUT", "__UNAVAILABLE__")
        .timeout(Duration::from_secs(20));

    let output = cmd.output().expect("run modeltap");
    assert!(
        output.status.success(),
        "modeltap should exit cleanly when lsof unavailable"
    );
    let frame = String::from_utf8_lossy(&output.stdout);

    // The dialog must contain an explicit "Running-tool detection unavailable"
    // message so the user knows the safety check was skipped.
    assert!(
        frame.contains("Running-tool detection unavailable"),
        "AC-3: lsof-unavailable must surface explicit 'Running-tool detection unavailable' message, got frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: Detection latency p99 < 500 ms
// ---------------------------------------------------------------------------

#[test]
fn detection_latency_under_500ms_p99() {
    use modeltap_app::lsof_adapter::LsofAdapter;
    use modeltap_core::ports::fs_probe::FsProbe;

    let fix = build_fixture();

    // Build the adapter with the fake-lsof seam. We measure each call's
    // wall-clock time and assert the p99 is below 500 ms. Since the seam
    // bypasses the subprocess, every call is essentially a string parse —
    // this is more of a regression guard than a real perf test.
    std::env::set_var(
        "MODELTAP_FAKE_LSOF_OUTPUT",
        fake_lsof_running("ollama", 1234, &fix.ollama_path),
    );

    let adapter = LsofAdapter::new();
    let target_paths: Vec<PathBuf> = vec![fix.ollama_path.clone()];

    let mut latencies: Vec<u128> = Vec::new();
    for _ in 0..50 {
        let start = Instant::now();
        let _ = adapter
            .detect_running_tools(&target_paths)
            .expect("detect_running_tools must succeed under fake-lsof");
        latencies.push(start.elapsed().as_millis());
    }

    std::env::remove_var("MODELTAP_FAKE_LSOF_OUTPUT");

    latencies.sort();
    let p99 = latencies[(latencies.len() * 99) / 100];
    assert!(
        p99 < 500,
        "AC-4: detection p99 must be < 500 ms, got {} ms (samples={:?})",
        p99,
        latencies
    );
}
