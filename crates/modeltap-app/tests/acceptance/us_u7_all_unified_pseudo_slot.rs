//! Acceptance scaffold for US-U7: `[All Unified]` pseudo-tool slot in left pane.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u7`. AC-U7.1..AC-U7.6.
//!
//! These RED tests fail today because:
//!   - `domain::synthetic_slot::SyntheticSlot::AllUnified` does not yet
//!     exist (per data-models.md "domain::synthetic_slot NEW").
//!   - `AppState.left_pane_slots` (replacing `tools`) is not yet refactored
//!     (per ADR-014 "heterogeneous left-pane slot enum").
//!   - `render::all_unified` does not yet exist (per component-boundaries.md).
//!
//! REMOVE #[ignore] in DELIVER step when each scenario goes green.
//!
//! Tags: @us-u7 @cross-artifact

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

fn modeltap_headless_at(ollama: &PathBuf, hf: &PathBuf) -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
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
    (cmd, temp)
}

fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_pre_unified_two_tool(temp: &TempDir) -> (PathBuf, PathBuf) {
    // Two tools sharing one inode (already unified). After hashing the
    // single shared model produces glyph "#"; the [All Unified] count is 1.
    let root = temp.path().to_path_buf();
    let payload = vec![0x55u8; 4096];

    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("blobs");
    let blob = "1212121212121212121212121212121212121212121212121212121212121212";
    let ollama_blob = blobs.join(format!("sha256-{}", blob));
    fs::write(&ollama_blob, &payload).expect("write ollama");
    let m = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("shared");
    fs::create_dir_all(&m).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m.join("7b"), manifest).expect("manifest");

    let hf_home = root.join(".cache").join("huggingface");
    let repo = hf_home.join("hub").join("models--shared--Shared-7B");
    let hf_blobs = repo.join("blobs");
    let rev = "1212121212121212121212121212121212121212";
    let snap = repo.join("snapshots").join(rev);
    let refs = repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("hf blobs");
    fs::create_dir_all(&snap).expect("snap");
    fs::create_dir_all(&refs).expect("refs");
    let hf_blob_name = "abababababababababababababababababababababababababababababababab";
    let hf_blob = hf_blobs.join(hf_blob_name);
    // Pre-unified: hardlink to the ollama blob so they share one inode.
    fs::hard_link(&ollama_blob, &hf_blob).expect("hardlink hf to ollama");
    std::os::unix::fs::symlink(
        PathBuf::from("..").join("..").join("blobs").join(hf_blob_name),
        snap.join("model.safetensors"),
    )
    .expect("snap symlink");
    fs::write(refs.join("main"), rev).expect("ref");

    (ollama_dir, hf_home)
}

// ---------------------------------------------------------------------------
// AC-U7.1: slot present in left pane below real tools.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U7 RED — DELIVER must add SyntheticSlot::AllUnified to left pane"]
fn all_unified_slot_appears_in_left_pane_below_real_tools() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf) = build_pre_unified_two_tool(&temp);
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    assert!(
        frame.contains("[All Unified]"),
        "AC-U7.1: left pane must include '[All Unified]' slot, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U7.2 + AC-U7.6: badge count agrees with summary bar.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U7 RED — DELIVER must drive AllUnified.count from dedup_summary.unified_count"]
fn all_unified_badge_matches_summary_bar_unified_count() {
    panic!(
        "AC-U7.2 + AC-U7.6 + AC-CONS-2 — DELIVER must wire \
         SyntheticSlot::AllUnified.count from the same DedupSummary.\
         unified_count that drives the summary bar's 'Unified: N'. Test \
         (after wait-for-hashing): scrape badge '(N)' from left pane and \
         'Unified: N' from summary bar; assert they are equal AND equal to \
         the count of '#' rows."
    );
}

// ---------------------------------------------------------------------------
// AC-U7.3: selecting [All Unified] filters right pane to # rows.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U7 RED — DELIVER must add render::all_unified for filtered right pane"]
fn selecting_all_unified_slot_filters_right_pane_to_hash_rows() {
    panic!(
        "AC-U7.3 — DELIVER must wire: navigation onto SyntheticSlot::\
         AllUnified causes the right-pane render to dispatch to render::\
         all_unified (per component-boundaries.md). Test (after wait-for-\
         hashing + navigation): right-pane row count equals dedup_summary.\
         unified_count; every visible row corresponds to a '#'-glyph model."
    );
}

// ---------------------------------------------------------------------------
// AC-U7.4 + AC-U7.5: row format includes name, size, tool count, savings;
// footer aggregates totals.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U7 RED — DELIVER must emit collect_unified_rows + footer total"]
fn all_unified_view_row_format_and_footer_aggregates_totals() {
    panic!(
        "AC-U7.4 + AC-U7.5 — DELIVER must implement core::logic::dedup::\
         collect_unified_rows (per data-models.md). Test asserts: each \
         row shows name + size + 'N tools' + 'saves <bytes>', and the \
         right-pane footer shows 'Unified: N models | Total reclaimed by \
         unification: <SUM>' where SUM equals the sum of per-row 'saves'."
    );
}
