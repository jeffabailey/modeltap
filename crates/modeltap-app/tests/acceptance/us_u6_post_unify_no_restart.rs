//! Acceptance scaffold for US-U6: Post-unify row glyph and summary bar
//! update without restart.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u6`. AC-U6.1..AC-U6.7.
//!
//! These RED tests fail today because:
//!   - `actions::reclassify::reclassify_after_unify` does not yet exist
//!     (per component-boundaries.md "actions::reclassify NEW (small)").
//!   - The TUI does not yet consume `Msg::UnifyApplied { outcome }`
//!     to drive re-classification (per data-models.md "New Msg variants").
//!
//! REMOVE #[ignore] in DELIVER step when each scenario goes green.
//!
//! Tags: @us-u6

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

fn modeltap_headless_at(ollama: &PathBuf, hf: &PathBuf) -> (Command, TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
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

fn ino_of(p: &Path) -> u64 {
    fs::metadata(p).expect("stat").ino()
}

fn build_two_blob_duplicate(temp: &TempDir) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = temp.path().to_path_buf();
    let payload = vec![0x42u8; 4096];
    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("blobs");
    let blob = "4444444444444444444444444444444444444444444444444444444444444444";
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
    let rev = "4444444444444444444444444444444444444444";
    let snap = repo.join("snapshots").join(rev);
    let refs = repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("hf blobs");
    fs::create_dir_all(&snap).expect("snap");
    fs::create_dir_all(&refs).expect("refs");
    let hf_blob_name = "3333333333333333333333333333333333333333333333333333333333333333";
    let hf_blob = hf_blobs.join(hf_blob_name);
    fs::write(&hf_blob, &payload).expect("write hf");
    std::os::unix::fs::symlink(
        PathBuf::from("..").join("..").join("blobs").join(hf_blob_name),
        snap.join("model.safetensors"),
    )
    .expect("symlink");
    fs::write(refs.join("main"), rev).expect("ref");

    (ollama_dir, hf_home, ollama_blob, hf_blob)
}

// ---------------------------------------------------------------------------
// AC-U6.1, AC-U6.2, AC-U6.4, AC-U6.7: full success flips "=" to "#",
// summary bar updates, no restart.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U6 RED — DELIVER must wire reclassify-after-unify (no restart required)"]
fn successful_unify_flips_glyph_and_updates_summary_bar_without_restart() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_blob_duplicate(&temp);
    let (mut cmd, _temp, _log_file) = modeltap_headless_at(&ollama, &hf);
    let regs = serde_json::json!({
        "id": "dup/Dup-7B",
        "regs": [
            {"tool": "ollama", "path": ollama_blob.display().to_string()},
            {"tool": "hf", "path": hf_blob.display().to_string()},
        ]
    })
    .to_string();
    // Open Detail, press u, confirm with Enter (unify runs), Esc back to
    // main (DOES NOT restart), q. The captured FINAL frame must show the
    // glyph "#" for the unified row and a non-zero "Unified" count in the
    // summary bar — both reflecting the post-unify state in the SAME
    // session.
    let script = "<enter>u<enter><esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let post_a = ino_of(&ollama_blob);
    let post_b = ino_of(&hf_blob);
    assert_eq!(post_a, post_b, "AC-U6.2 precondition: unify must succeed");
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    assert!(
        frame.contains("#"),
        "AC-U6.2: post-unify row glyph must be '#' in the same session, got:\n{}",
        frame
    );
    let lower = frame.to_lowercase();
    // The Unified count in the summary bar — exact text "Unified: " is
    // reasonable; crafter may use different casing/format.
    assert!(
        lower.contains("unified:"),
        "AC-U6.4: summary bar must show 'Unified:' count post-unify, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U6.5: transient (was X) delta then collapses.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U6 RED — DELIVER must add SummaryDelta with 5s expiry"]
fn summary_bar_shows_transient_delta_then_collapses() {
    panic!(
        "AC-U6.5 — DELIVER must add SummaryDelta {{ previous_dedup_able_bytes, \
         expires_at }} and Msg::SummaryDeltaExpired. Test mechanism: capture \
         frame immediately post-unify (must contain '(was ...)'); advance \
         a synthetic clock or wait for expiry; capture frame again (must \
         not contain '(was ...)'). Synthetic-clock seam vs. real wait is \
         crafter's choice."
    );
}

// ---------------------------------------------------------------------------
// AC-U6.3, AC-U6.6: partial success leaves glyph as "=", Unified does not
// increment.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "US-U6 RED — DELIVER must keep '=' glyph on partial-success and not increment Unified"]
fn partial_unify_leaves_glyph_as_equals_and_unified_count_unchanged() {
    // Inducing partial-success deterministically requires a multi-target
    // plan with one target failing. The cleanest mechanism in v1 is the
    // existing read-only directory pattern (one of the target dirs has
    // mode 0500 so link() fails with EACCES).
    //
    // DELIVER will need the multi-target fixture (3 tools with one
    // duplicated model, one tool's dir read-only). We do NOT need a new
    // env-var seam — chmod is enough.
    panic!(
        "AC-U6.3 + AC-U6.6 — DELIVER must build a 3-tool partial-failure \
         fixture (one target dir mode 0500). Test asserts: post-action, \
         row glyph stays '=', Unified count unchanged, Dedup-able \
         decreases by ONLY the bytes of the successful target."
    );
}
