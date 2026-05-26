//! Acceptance scaffold for US-U2: Wire dedup-able bytes from classifier to
//! summary bar. Fixes the v1 hardcoded `"Dedup-able: 0 B"` lie.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u2`. AC-U2.1, AC-U2.2, AC-U2.3, AC-U2.4, AC-U2.5.
//!
//! These RED tests fail today because:
//!   - `crates/modeltap-tui/src/render/summary_bar.rs:36` still hardcodes
//!     `"Dedup-able: 0 B"` (per architecture-design.md §summary).
//!   - `core::logic::dedup` does not yet expose a `dedup_summary` aggregator
//!     (per data-models.md "logic::dedup — new pure functions").
//!
//! REMOVE #[ignore] in DELIVER step when each scenario goes green.
//!
//! Tags: @us-u2 @cross-artifact

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helper: two-blob duplicate install (mirrors the WS fixture).
// ---------------------------------------------------------------------------

struct DuplicateFixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
    hf_home: PathBuf,
    payload_size: u64,
}

fn build_duplicate_fixture() -> DuplicateFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let payload_size: u64 = 4096;
    let payload: Vec<u8> = (0..payload_size as usize)
        .map(|i| (i % 251) as u8)
        .collect();

    let ollama_dir = root.join(".ollama").join("models");
    let ollama_blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&ollama_blobs).expect("create ollama blobs");
    let blob_hash = "1111111111111111111111111111111111111111111111111111111111111111";
    let ollama_blob = ollama_blobs.join(format!("sha256-{}", blob_hash));
    fs::write(&ollama_blob, &payload).expect("write ollama blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("dup");
    fs::create_dir_all(&manifest_dir).expect("create manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":{size}}}]}}"#,
        blob = blob_hash,
        size = payload_size
    );
    fs::write(manifest_dir.join("7b"), manifest).expect("write manifest");

    let hf_home = root.join(".cache").join("huggingface");
    let hf_repo = hf_home.join("hub").join("models--dup--Dup-7B");
    let hf_blobs = hf_repo.join("blobs");
    let hf_rev = "1111111111111111111111111111111111111111";
    let hf_snapshots = hf_repo.join("snapshots").join(hf_rev);
    let hf_refs = hf_repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("create hf blobs");
    fs::create_dir_all(&hf_snapshots).expect("create hf snapshot");
    fs::create_dir_all(&hf_refs).expect("create hf refs");
    let hf_blob_name = "2222222222222222222222222222222222222222222222222222222222222222";
    let hf_blob = hf_blobs.join(hf_blob_name);
    fs::write(&hf_blob, &payload).expect("write hf blob");
    std::os::unix::fs::symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(hf_blob_name),
        hf_snapshots.join("model.safetensors"),
    )
    .expect("hf snapshot symlink");
    fs::write(hf_refs.join("main"), hf_rev).expect("write hf ref");

    DuplicateFixture {
        _temp: temp,
        ollama_dir,
        hf_home,
        payload_size,
    }
}

fn modeltap_headless(fix: &DuplicateFixture) -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
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
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    (cmd, temp)
}

fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

fn capture_frame(fix: &DuplicateFixture) -> String {
    let (mut cmd, _temp) = modeltap_headless(fix);
    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    frame_text(&stdout)
}

// ---------------------------------------------------------------------------
// AC-U2.1 + AC-U2.3: summary bar shows "computing..." while hashing pending,
// NOT a hardcoded "0 B". This is the v1-bug regression test.
// ---------------------------------------------------------------------------

#[test]
fn summary_bar_shows_computing_while_hashing_pending() {
    let fix = build_duplicate_fixture();
    let frame = capture_frame(&fix);
    // While hashing has not produced any classification, the bar must say
    // "computing..." not a hardcoded number. The walking-skeleton harness
    // captures the first-paint frame, when no hashes have completed yet.
    assert!(
        frame.contains("Dedup-able: computing..."),
        "AC-U2.3: summary bar must show 'Dedup-able: computing...' before any hash completes, got frame:\n{}",
        frame
    );
}

#[test]
fn summary_bar_does_not_show_hardcoded_dedup_able_zero_during_hashing() {
    let fix = build_duplicate_fixture();
    let frame = capture_frame(&fix);
    // The v1 bug: literal `Dedup-able: 0 B` shipped regardless of state.
    // After US-U2 lands, this exact string is forbidden during the hashing
    // phase (the bar must say "computing..." instead). After hashing
    // completes the same string would only appear when there are no
    // duplicates — but our fixture has duplicates, so the post-hash bar
    // must show a non-zero value.
    assert!(
        !frame.contains("Dedup-able: 0 B"),
        "AC-U2.1: v1 hardcoded literal 'Dedup-able: 0 B' must not appear in a duplicate-bearing install, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U2.2 + AC-U2.4 + AC-CONS-1: bar reads from same source as row glyphs;
// sum of "=" rows == bar value once hashing is complete.
// ---------------------------------------------------------------------------

#[test]
fn summary_bar_value_equals_sum_of_dedup_able_row_sizes() {
    let fix = build_duplicate_fixture();
    // The fixture has exactly ONE duplicated model of `payload_size` bytes
    // across 2 tools — so post-hash, dedup_able_bytes == 1 * payload_size
    // (we'd reclaim one of the two copies).
    //
    // Step 01-09 added the `<hash-complete>` script sentinel which blocks
    // until the background hash pool reports completion, so we can
    // deterministically observe the post-hash summary bar.
    let (mut cmd, _temp) = modeltap_headless(&fix);
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    // `format_size(4096)` produces "4096 B" (4096 < 1_000_000 = MB).
    let expected = format!("Dedup-able: {} B", fix.payload_size);
    assert!(
        frame.contains(&expected),
        "AC-U2.2 + AC-CONS-1: summary-bar dedup-able bytes must equal the \
         sum of sizes of '=' rows. Expected '{}' for two-tool single-duplicate \
         fixture (payload_size={}), got frame:\n{}",
        expected,
        fix.payload_size,
        frame
    );
    // AC-U2.3: post-hashing the bar must NOT still say "computing..."
    assert!(
        !frame.contains("Dedup-able: computing..."),
        "AC-U2.3: post-hash bar must transition out of 'computing...', got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U2.5: honest zero when no duplicates.
// ---------------------------------------------------------------------------

struct UniqueFixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
}

fn build_unique_only_fixture() -> UniqueFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("create blobs");
    fs::write(
        blobs.join("sha256-3333333333333333333333333333333333333333333333333333333333333333"),
        vec![0xAAu8; 4096],
    )
    .expect("write unique blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("solo");
    fs::create_dir_all(&manifest_dir).expect("create manifest");
    let manifest = r#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:3333333333333333333333333333333333333333333333333333333333333333","size":412},"layers":[{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:3333333333333333333333333333333333333333333333333333333333333333","size":4096}]}"#;
    fs::write(manifest_dir.join("7b"), manifest).expect("write manifest");
    UniqueFixture {
        _temp: temp,
        ollama_dir,
    }
}

#[test]
fn summary_bar_shows_honest_zero_when_no_duplicates_after_hashing() {
    let fix = build_unique_only_fixture();
    let temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &fix.ollama_dir)
        .env("HF_HOME", "/nonexistent")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent")
        .env("MODELTAP_ATOMIC_CHAT_DIRS", "/nonexistent")
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent")
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(20));
    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    // AC-U2.5: with only-unique blobs, post-hash the bar must show an
    // honest zero ("Dedup-able: 0 B") — NOT the v1 hardcoded literal that
    // appeared regardless of state, AND NOT the "computing..." placeholder
    // that should only appear while hashing is in flight.
    assert!(
        frame.contains("Dedup-able: 0 B"),
        "AC-U2.5: bar must show honest 'Dedup-able: 0 B' once hashing \
         confirms no duplicates, got frame:\n{}",
        frame
    );
    assert!(
        !frame.contains("Dedup-able: computing..."),
        "AC-U2.5: post-hash bar must transition out of 'computing...', got:\n{}",
        frame
    );
}

#[allow(dead_code)]
fn assert_path_exists(p: &Path) {
    assert!(p.exists(), "path must exist: {}", p.display());
}
