//! Acceptance tests for US-05b (Delete from one tool only — single-model
//! delete with shared-vs-unique confirmation; ADR-009).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-05b scenarios. Drives the binary in `MODELTAP_HEADLESS=1` mode against
//! synthesized fixtures (real on-disk content laid out under each tool's
//! expected layout) and asserts:
//!
//! 1. **Shared single-model delete uses [y/n] confirmation** — when the same
//!    content lives under 2 tools (Ollama blob + HF snapshot with
//!    identical bytes), pressing `[d]` opens the dialog in Shared mode; `y`
//!    confirms; the targeted tool's file is removed AND the other tool's
//!    copy is still readable; the JSONL `action.zap_one` event records
//!    `was_shared=true`, `outcome="success"`.
//! 2. **Unique single-model delete requires typed model id** — when the
//!    model is only registered with one tool, `[d]` opens the dialog in
//!    Unique mode; `y` is buffered as typed input (NOT a confirm); typing
//!    the exact model id + Enter confirms; the file is removed; JSONL
//!    `was_shared=false`.
//! 3. **Unique single-model delete cancels on wrong typed id** — typing a
//!    wrong id + Enter cancels; the file is NOT removed.
//! 4. **Esc cancels single-model delete at any point** — opening the dialog
//!    then pressing Esc closes it without touching disk.
//! 5. **Successful single-model delete emits action.zap_one event** — JSONL
//!    schema validation (tool, bytes_reclaimed, was_shared, outcome,
//!    privacy: no model names / paths / hashes).
//!
//! Per ADR-009: each scenario invokes `Tool::delete_one` (NOT a 1-element
//! `delete_all` loop). The other-tool-unaffected invariant (scenario 1) is
//! the load-bearing observable that distinguishes single-model delete from
//! the cross-tool `delete_all` path.
//!
//! Tags: @us-05b @release-1 @destructive @real-io.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture: shared content under TWO tools.
//
//   <root>/.ollama/models/blobs/sha256-<blob>            (Ollama blob)
//   <root>/.ollama/models/manifests/.../<tag>            (Ollama manifest)
//   <root>/.cache/huggingface/hub/.../snapshots/...      (HF — same bytes)
//
// The two files are written with the SAME payload but distinct inodes (no
// hardlink). This mirrors the real-world "same model, two tool installs"
// case: deleting the Ollama registration must NOT touch the HF copy.
// ---------------------------------------------------------------------------

struct SharedFixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
    hf_home: PathBuf,
    ollama_path: PathBuf,
    hf_snapshot_path: PathBuf,
    hf_blob_path: PathBuf,
    payload_size: u64,
}

fn build_shared_fixture(payload_size: u64) -> SharedFixture {
    let temp = tempfile::tempdir().expect("tempdir for shared fixture");
    let root = temp.path().to_path_buf();
    let payload: Vec<u8> = (0..payload_size as usize)
        .map(|i| (i % 251) as u8)
        .collect();

    // Ollama: <root>/.ollama/models/{manifests,blobs}
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
        .join("synthetic");
    fs::create_dir_all(&manifest_dir).expect("create ollama manifest dir");
    let manifest_path = manifest_dir.join("7b");
    let manifest_json = format!(
        r#"{{
  "schemaVersion": 2,
  "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
  "config": {{
    "mediaType": "application/vnd.docker.container.image.v1+json",
    "digest": "sha256:{blob}",
    "size": 412
  }},
  "layers": [
    {{
      "mediaType": "application/vnd.ollama.image.model",
      "digest": "sha256:{blob}",
      "size": {size}
    }}
  ]
}}
"#,
        blob = blob_hash,
        size = payload_size
    );
    fs::write(&manifest_path, manifest_json).expect("write manifest");

    // HF: <root>/.cache/huggingface/hub/... — same content, distinct inode.
    let hf_home = root.join(".cache").join("huggingface");
    let hf_hub = hf_home.join("hub");
    let hf_repo_dir = hf_hub.join("models--synthetic--Synthetic-7B");
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
    let hf_snapshot_path = hf_snapshots.join("model.safetensors");
    let rel_target = PathBuf::from("..")
        .join("..")
        .join("blobs")
        .join(hf_blob_name);
    std::os::unix::fs::symlink(&rel_target, &hf_snapshot_path).expect("create hf symlink");
    fs::write(hf_refs.join("main"), hf_rev).expect("write hf ref");

    SharedFixture {
        _temp: temp,
        ollama_dir,
        hf_home,
        ollama_path,
        hf_snapshot_path,
        hf_blob_path,
        payload_size,
    }
}

// ---------------------------------------------------------------------------
// Fixture: unique single-tool registration. Only Ollama has the model.
// ---------------------------------------------------------------------------

struct UniqueFixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
    ollama_path: PathBuf,
}

fn build_unique_fixture() -> UniqueFixture {
    let temp = tempfile::tempdir().expect("tempdir for unique fixture");
    let root = temp.path().to_path_buf();
    let ollama_dir = root.join(".ollama").join("models");
    let blob_hash = "1111111111111111111111111111111111111111111111111111111111111111";
    let ollama_blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&ollama_blobs).expect("create ollama blobs");
    let ollama_path = ollama_blobs.join(format!("sha256-{}", blob_hash));
    fs::write(&ollama_path, vec![0u8; 4096]).expect("write ollama blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("solo");
    fs::create_dir_all(&manifest_dir).expect("create manifest dir");
    let manifest_path = manifest_dir.join("7b");
    let manifest_json = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob_hash
    );
    fs::write(&manifest_path, manifest_json).expect("write manifest");
    UniqueFixture {
        _temp: temp,
        ollama_dir,
        ollama_path,
    }
}

// ---------------------------------------------------------------------------
// Headless harness builders.
// ---------------------------------------------------------------------------

fn modeltap_headless_shared(fix: &SharedFixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &fix.ollama_dir)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", &fix.hf_home);
    (cmd, log_dir_temp, log_file)
}

fn modeltap_headless_unique(fix: &UniqueFixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &fix.ollama_dir)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf");
    (cmd, log_dir_temp, log_file)
}

/// Build the JSON value for `MODELTAP_HEADLESS_DETAIL_REGS` from the shared
/// fixture (Ollama + HF registrations).
fn detail_regs_json_shared(fix: &SharedFixture) -> String {
    serde_json::json!({
        "id": "synthetic/Synthetic-7B",
        "regs": [
            {"tool": "ollama", "path": fix.ollama_path.display().to_string()},
            {"tool": "hf",     "path": fix.hf_snapshot_path.display().to_string()},
        ]
    })
    .to_string()
}

/// Build the JSON for the unique-tool fixture (Ollama only).
fn detail_regs_json_unique(fix: &UniqueFixture) -> String {
    serde_json::json!({
        "id": "synthetic/Solo-7B",
        "regs": [
            {"tool": "ollama", "path": fix.ollama_path.display().to_string()},
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

fn find_zap_one_events(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.zap_one"))
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario 1 (AC-1, AC-7): Shared single-model delete uses [y/n] confirmation.
// ---------------------------------------------------------------------------

#[test]
fn shared_single_model_delete_uses_yn_confirmation() {
    let fix = build_shared_fixture(4096);
    let hf_blob_path = fix.hf_blob_path.clone();
    let ollama_path = fix.ollama_path.clone();
    let hf_bytes_pre = fs::read(&hf_blob_path).expect("pre-read hf copy");

    let (mut cmd, _log_temp, log_file) = modeltap_headless_shared(&fix);
    let regs = detail_regs_json_shared(&fix);

    // Script: <enter> open detail; d open delete-one dialog (Shared mode
    // because regs.len() >= 2); y confirm; q quit. Target Ollama (default
    // first registration), so the hf copy must remain.
    let script = "<enter>dyq";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_HEADLESS_DELETE_TARGET", "ollama")
        .env("MODELTAP_HEADLESS_DELETE_ID_IN_TOOL", "synthetic:7b")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // The targeted tool's blob must be removed.
    assert!(
        !ollama_path.exists(),
        "AC-1: shared delete must remove Ollama blob, but it still exists at {}",
        ollama_path.display()
    );

    // Other-tool-unaffected invariant: hf copy still readable with identical
    // bytes (this is the load-bearing single-model invariant per ADR-009 —
    // distinguishes delete_one from delete_all).
    assert!(
        hf_blob_path.exists(),
        "AC-1: other tool's copy must be untouched after shared single-model delete"
    );
    let hf_bytes_post = fs::read(&hf_blob_path).expect("post-read hf copy");
    assert_eq!(
        hf_bytes_pre, hf_bytes_post,
        "AC-1: other tool's bytes must be unchanged after single-model delete"
    );

    // JSONL: action.zap_one with was_shared=true, outcome=success.
    let events = read_jsonl_events(&log_file);
    let zap_ones = find_zap_one_events(&events);
    assert_eq!(
        zap_ones.len(),
        1,
        "AC-1: expected exactly 1 action.zap_one event, got {}: events={:#?}",
        zap_ones.len(),
        zap_ones
    );
    let event = zap_ones[0];
    assert_eq!(
        event.get("was_shared").and_then(|v| v.as_bool()),
        Some(true),
        "AC-1: shared-mode delete must emit was_shared=true, got: {}",
        event
    );
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "AC-1: shared-mode confirmed delete must emit outcome=success, got: {}",
        event
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 (AC-2): Unique single-model delete requires typed model id.
//
// In Unique mode, `y` is BUFFERED as typed input (NOT a confirm). The user
// must type the full model id and press Enter to confirm.
// ---------------------------------------------------------------------------

#[test]
fn unique_single_model_delete_requires_typed_model_id() {
    let fix = build_unique_fixture();
    let ollama_path = fix.ollama_path.clone();
    let (mut cmd, _log_temp, log_file) = modeltap_headless_unique(&fix);
    let regs = detail_regs_json_unique(&fix);

    // Script: <enter> open detail; d open delete-one dialog (Unique mode,
    // single registration); type the EXACT id_in_tool "solo:7b"; Enter to
    // confirm; q quit. The leading `y` would be buffered (Unique mode
    // treats y/n as text input) but we don't include it here — we type the
    // id directly to keep the test focused on the typed-id confirmation path.
    //
    // Headless override: the dialog's `model_id` is set via
    // `MODELTAP_HEADLESS_DELETE_ID_IN_TOOL` to the Ollama-canonical
    // <repo>:<tag> form (matches what `Tool::delete_one` looks up).
    let script = "<enter>dsolo:7b<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_HEADLESS_DELETE_TARGET", "ollama")
        .env("MODELTAP_HEADLESS_DELETE_ID_IN_TOOL", "solo:7b")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    assert!(
        !ollama_path.exists(),
        "AC-2: unique delete with correctly-typed model id must remove Ollama blob"
    );

    let events = read_jsonl_events(&log_file);
    let zap_ones = find_zap_one_events(&events);
    assert_eq!(
        zap_ones.len(),
        1,
        "AC-2: expected exactly 1 action.zap_one event, got {}",
        zap_ones.len()
    );
    let event = zap_ones[0];
    assert_eq!(
        event.get("was_shared").and_then(|v| v.as_bool()),
        Some(false),
        "AC-2: unique-mode delete must emit was_shared=false, got: {}",
        event
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 (AC-3): Unique single-model delete cancels on wrong typed id.
// ---------------------------------------------------------------------------

#[test]
fn unique_single_model_delete_cancels_on_wrong_typed_id() {
    let fix = build_unique_fixture();
    let ollama_path = fix.ollama_path.clone();
    let (mut cmd, _log_temp, log_file) = modeltap_headless_unique(&fix);
    let regs = detail_regs_json_unique(&fix);

    // Type a wrong id ("WRONG"), Enter — the dialog's BYTE-EQUAL CASE-
    // SENSITIVE check returns Cancel. q quits.
    let script = "<enter>dWRONG<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_HEADLESS_DELETE_TARGET", "ollama")
        .env("MODELTAP_HEADLESS_DELETE_ID_IN_TOOL", "solo:7b")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // File must NOT have been deleted.
    assert!(
        ollama_path.exists(),
        "AC-3: wrong typed model id must NOT delete the Ollama blob"
    );
    let bytes = fs::read(&ollama_path).expect("read ollama blob");
    assert_eq!(
        bytes.len(),
        4096,
        "AC-3: file must be unchanged after cancelled delete"
    );

    // No action.zap_one with outcome=success.
    let events = read_jsonl_events(&log_file);
    let zap_ones = find_zap_one_events(&events);
    for e in &zap_ones {
        let outcome = e.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            outcome, "success",
            "AC-3: wrong typed id must not produce a 'success' zap_one event, got: {}",
            e
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 4 (AC-4): Esc cancels single-model delete at any point.
// ---------------------------------------------------------------------------

#[test]
fn esc_cancels_single_model_delete_at_any_point() {
    let fix = build_unique_fixture();
    let ollama_path = fix.ollama_path.clone();
    let (mut cmd, _log_temp, log_file) = modeltap_headless_unique(&fix);
    let regs = detail_regs_json_unique(&fix);

    // Open dialog, then Esc (DialogCancel), then quit. No mutation expected.
    let script = "<enter>d<esc>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_HEADLESS_DELETE_TARGET", "ollama")
        .env("MODELTAP_HEADLESS_DELETE_ID_IN_TOOL", "solo:7b")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    assert!(
        ollama_path.exists(),
        "AC-4: Esc must NOT delete the Ollama blob"
    );
    let bytes = fs::read(&ollama_path).expect("read ollama blob");
    assert_eq!(
        bytes.len(),
        4096,
        "AC-4: file size must be unchanged after Esc-cancelled delete"
    );

    // No successful action.zap_one event.
    let events = read_jsonl_events(&log_file);
    let zap_ones = find_zap_one_events(&events);
    for e in &zap_ones {
        let outcome = e.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            outcome, "success",
            "AC-4: Esc must not produce a 'success' zap_one event, got: {}",
            e
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 5 (AC-6): Successful single-model delete emits action.zap_one event.
//
// Schema validation per `kpi-instrumentation.md` §"action.zap_one":
//   - schema = "modeltap.launch.v1"
//   - tool   = "ollama"
//   - bytes_reclaimed >= 0 (u64)
//   - was_shared = bool
//   - outcome = "success"
//   - PRIVACY: no model names, no on-disk paths, no hashes.
// ---------------------------------------------------------------------------

#[test]
fn successful_single_model_delete_emits_action_zap_one_event() {
    let fix = build_shared_fixture(4096);
    let (mut cmd, _log_temp, log_file) = modeltap_headless_shared(&fix);
    let regs = detail_regs_json_shared(&fix);

    let script = "<enter>dyq";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_HEADLESS_DELETE_TARGET", "ollama")
        .env("MODELTAP_HEADLESS_DELETE_ID_IN_TOOL", "synthetic:7b")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    let events = read_jsonl_events(&log_file);
    let zap_ones = find_zap_one_events(&events);
    assert_eq!(
        zap_ones.len(),
        1,
        "AC-6: expected exactly 1 action.zap_one event, got {}",
        zap_ones.len()
    );
    let event = zap_ones[0];

    // Schema = modeltap.launch.v1.
    assert_eq!(
        event.get("schema").and_then(|v| v.as_str()),
        Some("modeltap.launch.v1"),
        "AC-6: schema must be modeltap.launch.v1, got: {}",
        event
    );

    // tool = "ollama".
    assert_eq!(
        event.get("tool").and_then(|v| v.as_str()),
        Some("ollama"),
        "AC-6: tool must be 'ollama', got: {}",
        event
    );

    // bytes_reclaimed numeric u64.
    let bytes_reclaimed = event
        .get("bytes_reclaimed")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("AC-6: bytes_reclaimed must be a u64, got: {}", event));
    // Note: Ollama's manifest+ref-counted-blob delete reports the full
    // blob size when no other manifest references it. We assert >= 0 (u64)
    // for schema correctness; the precise byte count is plugin-internal.
    assert!(
        bytes_reclaimed <= fix.payload_size + 4096,
        "AC-6: bytes_reclaimed sanity bound, got {} > {}",
        bytes_reclaimed,
        fix.payload_size + 4096
    );

    // was_shared bool.
    assert!(
        event.get("was_shared").and_then(|v| v.as_bool()).is_some(),
        "AC-6: was_shared must be a bool, got: {}",
        event
    );

    // outcome = "success".
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "AC-6: outcome must be 'success', got: {}",
        event
    );

    // PRIVACY (C5): NO model names, NO on-disk paths, NO hashes.
    let event_str = event.to_string();
    assert!(
        !event_str.contains("synthetic/Synthetic-7B"),
        "C5: model display id must not appear in JSONL, got: {}",
        event_str
    );
    assert!(
        !event_str.contains(".gguf") && !event_str.contains("/blobs/"),
        "C5: on-disk paths must not appear in JSONL, got: {}",
        event_str
    );
    assert!(
        !event_str.contains("abc123def4567890abc123def4567890abc123def4567890abc123def4567890"),
        "C5: blob hash hex must not appear in JSONL, got: {}",
        event_str
    );
}
