//! Acceptance test for the GPT4All plugin's cross-tool dedup value
//! proposition (AC-G2.1).
//!
//! This is the payoff test for adding GPT4All to modeltap: when a user has
//! the SAME .gguf model installed via GPT4All AND via Ollama, modeltap MUST
//! detect the duplicate (same SHA-256) and the unify action MUST collapse
//! both copies onto a single inode — reclaiming half the disk.
//!
//! Pivots from `us_u1_walking_skeleton.rs` (Ollama + HF) by swapping the HF
//! cache layout for GPT4All's flat `.gguf` layout. The driving port (the
//! modeltap binary) and driven port boundary (post-unify inode equality + the
//! `action.unify` JSONL event) are identical to the WS — only the second
//! tool changes.
//!
//! Tags: @us-gpt4all @us-g2 @cross-tool-dedup @real-io @adapter-integration

#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Two-tool duplicate fixture: gpt4all + ollama, identical bytes, distinct
// inodes (the "Dedup-able > 0" condition).
//
// Lays out, in a single tempdir on a single filesystem:
//
//   <root>/.cache/gpt4all/duplicate-7b.gguf      (GPT4All flat-file layout)
//   <root>/.ollama/models/blobs/sha256-<blob>    (Ollama blob)
//   <root>/.ollama/models/manifests/.../duplicate-model:7b
//
// Both files have IDENTICAL bytes (SHA-256 collision is the dedup key) but
// live on SEPARATE inodes — until unify merges them.
// ---------------------------------------------------------------------------

struct CrossToolFixture {
    _temp: TempDir,
    gpt4all_dir: PathBuf,
    ollama_dir: PathBuf,
    gpt4all_path: PathBuf,
    ollama_blob_path: PathBuf,
    payload_size: u64,
}

fn build_cross_tool_fixture() -> CrossToolFixture {
    let temp = tempfile::tempdir().expect("tempdir for cross-tool fixture");
    let root = temp.path().to_path_buf();
    let payload_size: u64 = 4096;
    let payload: Vec<u8> = (0..payload_size as usize)
        .map(|i| (i % 251) as u8)
        .collect();

    // GPT4All: flat <root>/.cache/gpt4all/<name>.gguf — depth-1 walk, no
    // manifest layer (per us_gpt4all_discovery.rs fixture pattern).
    let gpt4all_dir = root.join(".cache").join("gpt4all");
    fs::create_dir_all(&gpt4all_dir).expect("create gpt4all dir");
    let gpt4all_path = gpt4all_dir.join("duplicate-7b.gguf");
    fs::write(&gpt4all_path, &payload).expect("write gpt4all gguf");

    // Ollama: blob + manifest (same pattern as us_u1_walking_skeleton.rs).
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
        .join("duplicate-model");
    fs::create_dir_all(&manifest_dir).expect("create ollama manifest dir");
    let manifest_path = manifest_dir.join("7b");
    let manifest_json = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":{size}}}]}}"#,
        blob = blob_hash,
        size = payload_size
    );
    fs::write(&manifest_path, manifest_json).expect("write ollama manifest");

    CrossToolFixture {
        _temp: temp,
        gpt4all_dir,
        ollama_dir,
        gpt4all_path,
        ollama_blob_path,
        payload_size,
    }
}

// ---------------------------------------------------------------------------
// Headless harness — mirrors us_u1_walking_skeleton.rs::modeltap_headless,
// pinning every other plugin at /nonexistent so only gpt4all + ollama are
// active. Same env-var seam, no new envs introduced.
// ---------------------------------------------------------------------------

fn modeltap_headless(fix: &CrossToolFixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &fix.ollama_dir)
        .env("MODELTAP_GPT4ALL_DIRS", &fix.gpt4all_dir)
        .env("HF_HOME", "/nonexistent/no-such-hf")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
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

// ---------------------------------------------------------------------------
// AC-G2.1: GPT4All + Ollama with identical bytes are unifiable end-to-end.
//
// Default selection in headless mode lands on the alphabetically-first
// installed tool. With (gpt4all, ollama) active, that is `gpt4all` (g < o);
// the right pane shows the GPT4All `.gguf` row. After background hashing
// completes, the row carries `=` (DedupAble), and pressing `u` from the
// main view opens the unify dialog with the Ollama mate pre-populated.
// Enter applies the plan; the two paths share one inode.
// ---------------------------------------------------------------------------

#[test]
fn gpt4all_and_ollama_duplicate_can_be_unified_from_main_view() {
    let fix = build_cross_tool_fixture();

    // Pre-condition: separate inodes, identical bytes (the "duplicate"
    // condition that makes the dedup engine fire).
    let pre_ino_gpt4all = ino_of(&fix.gpt4all_path);
    let pre_ino_ollama = ino_of(&fix.ollama_blob_path);
    assert_ne!(
        pre_ino_gpt4all, pre_ino_ollama,
        "fixture precondition: gpt4all and ollama must start on distinct inodes"
    );
    let pre_bytes_gpt4all = fs::read(&fix.gpt4all_path).expect("read gpt4all");
    let pre_bytes_ollama = fs::read(&fix.ollama_blob_path).expect("read ollama");
    assert_eq!(
        pre_bytes_gpt4all, pre_bytes_ollama,
        "fixture precondition: bytes must be identical so dedup engine produces a `=` glyph"
    );

    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);

    // Script: <hash-complete> blocks until the background hash pool reports
    // completion (so the highlighted GPT4All row carries `=` not `?`); then
    // `u` opens the unify dialog with the Ollama mate pre-populated; Enter
    // confirms; q quits. Same script grammar as us_u1_walking_skeleton.rs.
    let script = "<hash-complete>u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // Post-condition 1 (AC-G2.1): the two blobs share one inode, proving the
    // unify engine treats GPT4All as a first-class participant in cross-tool
    // dedup — the value proposition for adding the plugin.
    let post_ino_gpt4all = ino_of(&fix.gpt4all_path);
    let post_ino_ollama = ino_of(&fix.ollama_blob_path);
    assert_eq!(
        post_ino_gpt4all, post_ino_ollama,
        "AC-G2.1: gpt4all and ollama blobs must share one inode after the \
         unify (got gpt4all={}, ollama={})",
        post_ino_gpt4all, post_ino_ollama
    );

    // Post-condition 2 (KPI instrumentation): exactly one `action.unify`
    // event, outcome=success, bytes_reclaimed equals the duplicated payload
    // size. tools_unified must include the recipient of the hardlink.
    let events = read_jsonl_events(&log_file);
    let unify_events: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
        .collect();
    assert_eq!(
        unify_events.len(),
        1,
        "AC-G2.1: expected exactly 1 action.unify event, got {}: {:#?}",
        unify_events.len(),
        unify_events
    );
    let event = unify_events[0];
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "AC-G2.1: action.unify outcome must be 'success', got {:?}",
        event.get("outcome")
    );
    let bytes_reclaimed = event
        .get("bytes_reclaimed")
        .and_then(|v| v.as_u64())
        .expect("AC-G2.1: action.unify must carry bytes_reclaimed u64");
    assert_eq!(
        bytes_reclaimed, fix.payload_size,
        "AC-G2.1: bytes_reclaimed must equal the duplicated model's size, got {} expected {}",
        bytes_reclaimed, fix.payload_size
    );
    let tools_unified: Vec<&str> = event
        .get("tools_unified")
        .and_then(|v| v.as_array())
        .expect("AC-G2.1: action.unify must carry tools_unified array")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    // The orchestrator picks one tool as canonical and lists the OTHER(s) in
    // tools_unified (the recipients of the hardlink). With only two tools in
    // play, exactly one of {gpt4all, ollama} must appear, and it must be the
    // non-canonical one — i.e. tools_unified is a non-empty subset of the
    // installed tools.
    assert!(
        !tools_unified.is_empty(),
        "AC-G2.1: tools_unified must include at least one recipient, got {:?}",
        tools_unified
    );
    for t in &tools_unified {
        assert!(
            *t == "gpt4all" || *t == "ollama",
            "AC-G2.1: tools_unified must only mention installed tools, got {:?}",
            tools_unified
        );
    }
}

// ---------------------------------------------------------------------------
// Sanity probe: the cross-tool fixture builds and exposes two distinct
// inodes holding identical bytes. Runs without modeltap; ensures the
// fixture helper itself is not the source of any RED-test failure.
// ---------------------------------------------------------------------------

#[test]
fn cross_tool_fixture_produces_two_distinct_inodes_with_identical_bytes() {
    let fix = build_cross_tool_fixture();
    let ino_a = ino_of(&fix.gpt4all_path);
    let ino_b = ino_of(&fix.ollama_blob_path);
    assert_ne!(ino_a, ino_b, "fixture must have distinct inodes");
    let bytes_a = fs::read(&fix.gpt4all_path).expect("read gpt4all");
    let bytes_b = fs::read(&fix.ollama_blob_path).expect("read ollama blob");
    assert_eq!(
        bytes_a, bytes_b,
        "fixture must have byte-identical content across the two tool stores"
    );
    assert_eq!(bytes_a.len() as u64, fix.payload_size);
}
