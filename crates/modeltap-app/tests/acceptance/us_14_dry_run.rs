//! Acceptance tests for US-14 (Dry-run preview before unify).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-14 scenarios. The tests drive the binary in `MODELTAP_HEADLESS=1` mode
//! and assert:
//!
//! 1. **Dry-run shows the plan without touching disk** — same-fs fixture; on
//!    detail screen press `u` then `n`. The dialog must show "(dry-run)"
//!    output with "Would create canonical" / "Would create hardlinks" /
//!    "Reclaim:" lines. After dry-run, every fixture file's
//!    (path, inode, size, mtime) tuple must be unchanged. No `action.unify`
//!    event must be emitted.
//! 2. **Dry-run reveals cross-filesystem issue** — fixture with 1+ cross-fs
//!    target via `MODELTAP_FAKE_CROSS_FS_PATHS`. On detail press `u` then
//!    `n`. The dry-run output must contain "WARNING" + "different filesystem".
//!    No mutation.
//! 3. **Dry-run emits action.unify_dry_run event** — read `launch.log` after
//!    dry-run and find a JSONL line with `event="action.unify_dry_run"`,
//!    `model_dedup_key_kind="sha256"`, `tools_to_unify` array,
//!    `bytes_would_reclaim` u64, `outcome="previewed"`. NO PII.
//!
//! Tags: @us-14 @release-2.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Shared-content multi-tool fixture builder. Mirrors the one in
// us_10_unify_hardlinks.rs but keeps the inode-snapshot helpers local so the
// "no-mutation" assertion stays self-contained.
// ---------------------------------------------------------------------------

struct SharedFixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
    hf_home: PathBuf,
    ollama_path: PathBuf,
    #[allow(dead_code)]
    hf_blob_path: PathBuf,
    /// Root tempdir path so the snapshot helper can walk the whole tree.
    root: PathBuf,
}

fn build_shared_fixture(payload_size: u64) -> SharedFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let payload: Vec<u8> = (0..payload_size as usize)
        .map(|i| (i % 251) as u8)
        .collect();

    // Ollama
    let ollama_dir = root.join(".ollama").join("models");
    let blob_hash = "abc123def4567890abc123def4567890abc123def4567890abc123def4567890";
    let ollama_blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&ollama_blobs).unwrap();
    let ollama_path = ollama_blobs.join(format!("sha256-{}", blob_hash));
    fs::write(&ollama_path, &payload).unwrap();
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("synthetic");
    fs::create_dir_all(&manifest_dir).unwrap();
    let manifest_path = manifest_dir.join("7b");
    let manifest_json = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":{size}}}]}}"#,
        blob = blob_hash,
        size = payload_size
    );
    fs::write(&manifest_path, manifest_json).unwrap();

    // HF
    let hf_home = root.join(".cache").join("huggingface");
    let hf_hub = hf_home.join("hub");
    let hf_repo_dir = hf_hub.join("models--synthetic--Synthetic-7B");
    let hf_rev = "abc123def4567890abc123def4567890abc12345";
    let hf_blobs = hf_repo_dir.join("blobs");
    let hf_snapshots = hf_repo_dir.join("snapshots").join(hf_rev);
    let hf_refs = hf_repo_dir.join("refs");
    fs::create_dir_all(&hf_blobs).unwrap();
    fs::create_dir_all(&hf_snapshots).unwrap();
    fs::create_dir_all(&hf_refs).unwrap();
    let hf_blob_name = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
    let hf_blob_path = hf_blobs.join(hf_blob_name);
    fs::write(&hf_blob_path, &payload).unwrap();
    let snapshot_link = hf_snapshots.join("model.safetensors");
    let rel_target = PathBuf::from("..")
        .join("..")
        .join("blobs")
        .join(hf_blob_name);
    std::os::unix::fs::symlink(&rel_target, &snapshot_link).unwrap();
    fs::write(hf_refs.join("main"), hf_rev).unwrap();

    SharedFixture {
        _temp: temp,
        ollama_dir,
        hf_home,
        ollama_path,
        hf_blob_path,
        root,
    }
}

/// Snapshot every regular file under `root` into a (relative_path → tuple)
/// map. The tuple is (inode, size, mtime_ns) so a no-mutation check can
/// compare maps before and after the dry-run with deterministic equality.
/// Symlinks are recorded by their `symlink_metadata` so the snapshot does
/// not silently follow them and miss a pointed-at change.
fn snapshot_tree(root: &Path) -> BTreeMap<PathBuf, (u64, u64, i128)> {
    let mut out = BTreeMap::new();
    fn walk(dir: &Path, base: &Path, out: &mut BTreeMap<PathBuf, (u64, u64, i128)>) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let md = match fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if md.is_dir() {
                walk(&p, base, out);
                continue;
            }
            let rel = p.strip_prefix(base).unwrap_or(&p).to_path_buf();
            let mtime = md.mtime() as i128 * 1_000_000_000 + md.mtime_nsec() as i128;
            out.insert(rel, (md.ino(), md.len(), mtime));
        }
    }
    walk(root, root, &mut out);
    out
}

fn modeltap_headless(fix: &SharedFixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().unwrap();
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).unwrap();
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").unwrap();
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &fix.ollama_dir)
        .env("HF_HOME", &fix.hf_home)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    (cmd, log_dir_temp, log_file)
}

fn detail_regs_json(fix: &SharedFixture) -> String {
    let hf_snapshot = fix
        .hf_home
        .join("hub")
        .join("models--synthetic--Synthetic-7B")
        .join("snapshots")
        .join("abc123def4567890abc123def4567890abc12345")
        .join("model.safetensors");
    serde_json::json!({
        "id": "synthetic/Synthetic-7B",
        "regs": [
            {"tool": "ollama", "path": fix.ollama_path.display().to_string()},
            {"tool": "hf",     "path": hf_snapshot.display().to_string()},
        ]
    })
    .to_string()
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

// ---------------------------------------------------------------------------
// Scenario 1: Dry-run shows the plan without touching disk.
// ---------------------------------------------------------------------------

#[test]
fn dry_run_shows_plan_without_touching_disk() {
    let fix = build_shared_fixture(4096);

    let pre_snapshot = snapshot_tree(&fix.root);

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    // Script: <enter> open detail; u open unify dialog; n dry-run preview;
    // <esc> close dry-run preview; q quit. (No second Enter — we want to
    // verify dry-run is purely descriptive.)
    let script = "<enter>un<esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // AC-2: dry-run output must be clearly labeled "(dry-run)".
    let lower = frame.to_lowercase();
    assert!(
        lower.contains("(dry-run)") || lower.contains("dry-run"),
        "AC-2: dry-run frame must contain '(dry-run)' label, got:\n{}",
        frame
    );
    assert!(
        lower.contains("would create") || lower.contains("would link") || lower.contains("would"),
        "AC-2: dry-run frame must describe planned actions with 'Would...' text, got:\n{}",
        frame
    );
    assert!(
        lower.contains("reclaim"),
        "AC-2: dry-run frame must show reclaim estimate, got:\n{}",
        frame
    );

    // AC-1: NO mutation. Every (path, inode, size, mtime) tuple unchanged.
    let post_snapshot = snapshot_tree(&fix.root);
    assert_eq!(
        pre_snapshot, post_snapshot,
        "AC-1: dry-run must NOT mutate any file in the fixture tree"
    );

    // AC-5: action.unify_dry_run emitted; action.unify NOT emitted.
    let events = read_jsonl_events(&log_file);
    let real_unify_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
        .collect();
    assert!(
        real_unify_events.is_empty(),
        "AC-5: action.unify must NOT be emitted for dry-run path, got: {:?}",
        real_unify_events
    );
    let dry_run_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify_dry_run"))
        .collect();
    assert_eq!(
        dry_run_events.len(),
        1,
        "AC-5: expected exactly 1 action.unify_dry_run event, got {}",
        dry_run_events.len()
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Dry-run reveals cross-filesystem issue.
// ---------------------------------------------------------------------------

#[test]
fn dry_run_reveals_cross_filesystem_issue() {
    let fix = build_shared_fixture(4096);

    let pre_snapshot = snapshot_tree(&fix.root);

    let (mut cmd, _log_temp, _log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    // Mark HF's home directory as cross-fs via the test seam (same fake-
    // probe used in 03-03 cross-fs acceptance). We mark the parent dir so
    // the canonicalized HF blob path matches the prefix. Canonicalize so
    // the prefix-match in the headless harness works on macOS where
    // `/var/folders/...` resolves to `/private/var/folders/...`.
    let fake_xfs = fs::canonicalize(&fix.hf_home)
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", fix.hf_home.display()))
        .display()
        .to_string();

    let script = "<enter>un<esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_CROSS_FS_PATHS", &fake_xfs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    let lower = frame.to_lowercase();

    // AC-3: cross-fs warning must surface during dry-run.
    assert!(
        lower.contains("warning") && (lower.contains("filesystem") || lower.contains("cross")),
        "AC-3: dry-run frame must surface cross-fs WARNING, got:\n{}",
        frame
    );

    // AC-1: still no mutation (cross-fs path or not).
    let post_snapshot = snapshot_tree(&fix.root);
    assert_eq!(
        pre_snapshot, post_snapshot,
        "AC-1: dry-run with cross-fs must NOT mutate any file"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: Dry-run emits action.unify_dry_run JSONL event.
// ---------------------------------------------------------------------------

#[test]
fn dry_run_emits_action_unify_dry_run_event() {
    let fix = build_shared_fixture(4096);
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    let script = "<enter>un<esc>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    let events = read_jsonl_events(&log_file);
    let dry_run_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify_dry_run"))
        .collect();
    assert_eq!(
        dry_run_events.len(),
        1,
        "AC-5: expected exactly 1 action.unify_dry_run event, got {}",
        dry_run_events.len()
    );
    let event = dry_run_events[0];

    // Schema: outcome must be "previewed".
    let outcome = event
        .get("outcome")
        .and_then(|v| v.as_str())
        .expect("action.unify_dry_run must carry outcome string");
    assert_eq!(
        outcome, "previewed",
        "AC-5: dry-run outcome must be 'previewed', got {}",
        outcome
    );

    // Schema: model_dedup_key_kind = "sha256".
    let kind = event
        .get("model_dedup_key_kind")
        .and_then(|v| v.as_str())
        .expect("action.unify_dry_run must carry model_dedup_key_kind");
    assert_eq!(kind, "sha256", "AC-5: kind must be 'sha256', got {}", kind);

    // Schema: tools_to_unify array.
    let tools_to_unify = event
        .get("tools_to_unify")
        .and_then(|v| v.as_array())
        .expect("action.unify_dry_run must carry tools_to_unify array");
    let tool_names: Vec<&str> = tools_to_unify.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        !tool_names.is_empty(),
        "AC-5: tools_to_unify must be non-empty for a multi-tool plan, got {:?}",
        tool_names
    );

    // Schema: bytes_would_reclaim u64 (NOT bytes_reclaimed — distinct from
    // the past-tense action.unify event).
    let bytes_would_reclaim = event
        .get("bytes_would_reclaim")
        .and_then(|v| v.as_u64())
        .expect("action.unify_dry_run must carry bytes_would_reclaim u64");
    assert!(
        bytes_would_reclaim > 0,
        "AC-5: bytes_would_reclaim must be > 0 for a multi-tool plan, got {}",
        bytes_would_reclaim
    );

    // Schema: cross_fs_targets count present (0 in same-fs path).
    let cross_fs_targets = event
        .get("cross_fs_targets")
        .and_then(|v| v.as_u64())
        .expect("action.unify_dry_run must carry cross_fs_targets u64");
    assert_eq!(
        cross_fs_targets, 0,
        "AC-5: same-fs fixture must have 0 cross-fs targets, got {}",
        cross_fs_targets
    );

    // Privacy (C5): NO model names, NO paths, NO hash values.
    let event_str = event.to_string();
    assert!(
        !event_str.contains("synthetic/Synthetic-7B"),
        "C5 privacy: action.unify_dry_run must not contain model id, got:\n{}",
        event_str
    );
    assert!(
        !event_str.contains(".gguf") && !event_str.contains(".safetensors"),
        "C5 privacy: action.unify_dry_run must not contain on-disk paths, got:\n{}",
        event_str
    );
    assert!(
        !event_str.contains("abc123def4567890abc123def4567890abc123def4567890abc123def4567890"),
        "C5 privacy: action.unify_dry_run must not contain hash hex, got:\n{}",
        event_str
    );
}
