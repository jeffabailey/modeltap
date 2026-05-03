//! Acceptance test for the cross-tool-model-unify WALKING SKELETON.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! the @walking-skeleton scenario is:
//!
//! "Devon reclaims disk by unifying a duplicated model from the main view"
//!
//! This is the smallest end-to-end slice that proves the v1 promise becomes
//! true: launch -> background hashing produces a non-zero Dedup-able number
//! -> Devon presses [u] from the main view (NOT Detail) -> confirms in the
//! dialog -> a real hardlink is created across two tools and the summary bar
//! reflects the reclaim, all without restarting modeltap.
//!
//! Touches US-U1, US-U2, US-U3, US-U4, US-U5, US-U6 simultaneously by design;
//! WS scenarios are intentionally vertical, not horizontal.
//!
//! REMOVE #[ignore] in DELIVER step when this scenario goes green.
//!
//! Tags: @walking-skeleton @us-u1 @us-u2 @us-u3 @us-u4 @us-u5 @us-u6
//!       @real-io @adapter-integration

#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Two-tool duplicate fixture: the smallest install that has a Dedup-able > 0.
//
// Lays out, in a single tempdir on a single filesystem:
//
//   <root>/.ollama/models/blobs/sha256-<blob>          (Ollama copy)
//   <root>/.cache/huggingface/hub/                     (HF cache)
//     models--devon--Devon-Model/
//       blobs/<hf-blob>
//       snapshots/<rev>/model.safetensors -> ../../blobs/<hf-blob>
//
// The two blob files have IDENTICAL bytes but live on SEPARATE inodes —
// this is the v1 "Dedup-able > 0 but the bar lies" condition.
// ---------------------------------------------------------------------------

struct WalkingSkeletonFixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
    hf_home: PathBuf,
    ollama_blob_path: PathBuf,
    hf_blob_path: PathBuf,
    payload_size: u64,
}

fn build_walking_skeleton_fixture() -> WalkingSkeletonFixture {
    let temp = tempfile::tempdir().expect("tempdir for walking skeleton fixture");
    let root = temp.path().to_path_buf();
    let payload_size: u64 = 4096;
    let payload: Vec<u8> = (0..payload_size as usize)
        .map(|i| (i % 251) as u8)
        .collect();

    // Ollama
    let ollama_dir = root.join(".ollama").join("models");
    let ollama_blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&ollama_blobs).expect("create ollama blobs");
    let blob_hash = "abc123def4567890abc123def4567890abc123def4567890abc123def4567890";
    let ollama_blob_path = ollama_blobs.join(format!("sha256-{}", blob_hash));
    fs::write(&ollama_blob_path, &payload).expect("write ollama blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("devon-model");
    fs::create_dir_all(&manifest_dir).expect("create ollama manifest dir");
    let manifest_path = manifest_dir.join("7b");
    let manifest_json = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":{size}}}]}}"#,
        blob = blob_hash,
        size = payload_size
    );
    fs::write(&manifest_path, manifest_json).expect("write ollama manifest");

    // Hugging Face
    let hf_home = root.join(".cache").join("huggingface");
    let hf_hub = hf_home.join("hub");
    let hf_repo_dir = hf_hub.join("models--devon--Devon-Model");
    let hf_rev = "abc123def4567890abc123def4567890abc12345";
    let hf_blobs = hf_repo_dir.join("blobs");
    let hf_snapshots = hf_repo_dir.join("snapshots").join(hf_rev);
    let hf_refs = hf_repo_dir.join("refs");
    fs::create_dir_all(&hf_blobs).expect("create hf blobs");
    fs::create_dir_all(&hf_snapshots).expect("create hf snapshot");
    fs::create_dir_all(&hf_refs).expect("create hf refs");
    let hf_blob_name = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
    let hf_blob_path = hf_blobs.join(hf_blob_name);
    // Same bytes, distinct inode (write, do not hardlink).
    fs::write(&hf_blob_path, &payload).expect("write hf blob");
    let snapshot_link = hf_snapshots.join("model.safetensors");
    let rel_target = PathBuf::from("..")
        .join("..")
        .join("blobs")
        .join(hf_blob_name);
    std::os::unix::fs::symlink(&rel_target, &snapshot_link).expect("create hf snapshot symlink");
    fs::write(hf_refs.join("main"), hf_rev).expect("write hf ref");

    WalkingSkeletonFixture {
        _temp: temp,
        ollama_dir,
        hf_home,
        ollama_blob_path,
        hf_blob_path,
        payload_size,
    }
}

// ---------------------------------------------------------------------------
// Headless harness — mirrors us_10_unify_hardlinks.rs::modeltap_headless and
// us_18_plugin_trait.rs::modeltap_headless. No new env-var seams.
// ---------------------------------------------------------------------------

fn modeltap_headless(fix: &WalkingSkeletonFixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
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

fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// THE walking skeleton scenario.
// ---------------------------------------------------------------------------

/// Devon reclaims disk by unifying a duplicated model from the main view.
///
/// This RED test fails today because:
///   1. Background hashing has not been wired (US-U1 not yet implemented).
///   2. Summary bar still hardcodes "Dedup-able: 0 B" (US-U2 not yet wired).
///   3. Pressing `u` from the main view is a no-op (US-U4 not yet wired) —
///      `headless::token_to_msg` maps `u` to `Msg::Unify`, but
///      `lift_unify_in_detail` only intercepts on the Detail screen, so on
///      Main the unify dialog never opens and no inode merge happens.
///
/// After DELIVER lands US-U1..U6, this test should go green: hashing
/// completes, dedup-able is non-zero, `u` from main view opens the dialog
/// with mates pre-populated, Enter applies the plan, and the two blobs
/// share one inode.
#[test]
fn devon_reclaims_disk_by_unifying_a_duplicated_model_from_the_main_view() {
    let fix = build_walking_skeleton_fixture();

    // Pre-condition: separate inodes (the "Dedup-able > 0" condition).
    let pre_ino_ollama = ino_of(&fix.ollama_blob_path);
    let pre_ino_hf = ino_of(&fix.hf_blob_path);
    assert_ne!(
        pre_ino_ollama, pre_ino_hf,
        "fixture precondition: ollama and hf must start on distinct inodes"
    );

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);

    // Script: <wait-for-hashing-to-complete>u<enter>q
    //
    // The headless harness today does not have an explicit "wait for hashes"
    // token; for the WS we rely on:
    //   (a) `MODELTAP_HEADLESS_INPUT` script tokens being processed
    //       sequentially with a `terminal.draw(...)` between every token
    //       (existing behavior in `headless::run`), AND
    //   (b) the DELIVER wave wiring background hashing to drive
    //       `Msg::HashComputed` events into the same per-iteration drain so
    //       that by the time the harness reaches `u`, hashing has produced
    //       at least one classification.
    //
    // Step 01-09 added the `<hash-complete>` script sentinel — a sync point
    // (not a new env-var seam; pure script-grammar) that blocks until the
    // background hash pool reports completion. The WS uses it so the
    // highlighted row is guaranteed to carry a `=` glyph (both blobs hashed
    // and classified as duplicates) by the time `u` fires from the main view.
    let script = "<hash-complete>u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // Post-condition 1 (US-U5 / US-U6): inodes merged.
    let post_ino_ollama = ino_of(&fix.ollama_blob_path);
    let post_ino_hf = ino_of(&fix.hf_blob_path);
    assert_eq!(
        post_ino_ollama, post_ino_hf,
        "WS: ollama and hf blobs must share one inode after the unify \
         (got ollama={}, hf={})",
        post_ino_ollama, post_ino_hf
    );

    // Post-condition 2 (US-U5 KPI instrumentation): action.unify event
    // recorded with outcome="success" and bytes_reclaimed > 0.
    let events = read_jsonl_events(&log_file);
    let unify_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
        .collect();
    assert_eq!(
        unify_events.len(),
        1,
        "WS: expected exactly 1 action.unify event, got {}: {:#?}",
        unify_events.len(),
        unify_events
    );
    let event = unify_events[0];
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "WS: action.unify outcome must be 'success', got {:?}",
        event.get("outcome")
    );
    let bytes_reclaimed = event
        .get("bytes_reclaimed")
        .and_then(|v| v.as_u64())
        .expect("WS: action.unify must carry bytes_reclaimed u64");
    assert_eq!(
        bytes_reclaimed, fix.payload_size,
        "WS: bytes_reclaimed must equal the duplicated model's size, got {} expected {}",
        bytes_reclaimed, fix.payload_size
    );
}

/// Sanity probe: the WS fixture builds and exposes two distinct inodes
/// holding identical bytes. Runs without modeltap; ensures the fixture
/// helper is not the source of any RED-test failure.
#[test]
fn walking_skeleton_fixture_produces_two_distinct_inodes_with_identical_bytes() {
    let fix = build_walking_skeleton_fixture();
    let ino_a = ino_of(&fix.ollama_blob_path);
    let ino_b = ino_of(&fix.hf_blob_path);
    assert_ne!(ino_a, ino_b, "fixture must have distinct inodes");
    let bytes_a = fs::read(&fix.ollama_blob_path).expect("read ollama blob");
    let bytes_b = fs::read(&fix.hf_blob_path).expect("read hf blob");
    assert_eq!(
        bytes_a, bytes_b,
        "fixture must have byte-identical content across the two blobs"
    );
    assert_eq!(bytes_a.len() as u64, fix.payload_size);
}

/// Documentation probe: the post-paint frame must include the bottom-bar
/// affordances. This test passes today (no production code change needed)
/// and serves as a tripwire — if the bottom bar text changes, it tells us
/// before the WS test gets there.
#[test]
fn walking_skeleton_smoke_post_paint_bottom_bar_renders() {
    let fix = build_walking_skeleton_fixture();
    let (mut cmd, _log_temp, _log_file) = modeltap_headless(&fix);
    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    assert!(
        frame.contains("[<-/->] tools"),
        "WS smoke: bottom bar must render, got:\n{}",
        frame
    );
}
