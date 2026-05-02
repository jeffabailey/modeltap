//! Acceptance scaffold for US-U9: Detail screen shows the cross-tool
//! "inode proof" — a shared inode group for unified models and one group
//! per separate copy for dedup-able models.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u9`. AC-U9.1 / AC-U9.2 / AC-U9.3 / AC-U9.4.
//!
//! Driving port: the modeltap binary in headless mode (same MODELTAP_HEADLESS_INPUT
//! + MODELTAP_HEADLESS_DETAIL_REGS seam used by US-U5 / US-13). Driven boundary
//!   asserted at: the rendered TestBackend frame text (the screen that the user
//!   actually sees).
//!
//! Tags: @us-u9

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

fn modeltap_headless_at(ollama: &PathBuf, hf: &PathBuf) -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
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

/// Reflow the rendered TUI frame onto a single contiguous string so a path
/// that ratatui wrapped across multiple lines is still asserted-against as
/// a substring. Specifically: drop everything outside the box characters
/// (border-only whitespace), then strip every whitespace character so
/// "...sha256-9999\n9999..." becomes "...sha256-99999999...".
fn flat_frame(frame: &str) -> String {
    frame
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && *c != '│'
                && *c != '┌'
                && *c != '┐'
                && *c != '└'
                && *c != '┘'
                && *c != '─'
        })
        .collect()
}

/// Build a fixture with two byte-identical copies hardlinked together (one
/// inode, two paths). The headless harness uses real `std::fs::metadata`
/// to read the inode of each path it sees in MODELTAP_HEADLESS_DETAIL_REGS,
/// so a real hardlink in tmp_path is the most truthful way to exercise the
/// "shared inode" branch end-to-end.
fn build_two_path_unified(temp: &TempDir) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = temp.path().to_path_buf();
    let payload = vec![0xA9u8; 4096];

    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("blobs");
    let blob = "9999999999999999999999999999999999999999999999999999999999999999";
    let ollama_blob = blobs.join(format!("sha256-{}", blob));
    fs::write(&ollama_blob, &payload).expect("write");
    let m = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("u9unified");
    fs::create_dir_all(&m).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m.join("7b"), manifest).expect("manifest");

    let hf_home = root.join(".cache").join("huggingface");
    let repo = hf_home.join("hub").join("models--u9unified--U9-7B");
    let hf_blobs = repo.join("blobs");
    let rev = "9999999999999999999999999999999999999999";
    let snap = repo.join("snapshots").join(rev);
    let refs = repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("hf blobs");
    fs::create_dir_all(&snap).expect("snap");
    fs::create_dir_all(&refs).expect("refs");
    let hf_blob_name = "9999999999999999999999999999999999999999999999999999999999999998";
    let hf_blob = hf_blobs.join(hf_blob_name);
    // Hardlink the HF blob to the Ollama blob → ONE inode, two paths.
    fs::hard_link(&ollama_blob, &hf_blob).expect("hardlink");
    std::os::unix::fs::symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(hf_blob_name),
        snap.join("model.safetensors"),
    )
    .expect("symlink");
    fs::write(refs.join("main"), rev).expect("ref");

    (ollama_dir, hf_home, ollama_blob, hf_blob)
}

/// Build a fixture with two byte-identical but SEPARATE-inode copies
/// (the dedup-able "=" case).
fn build_two_path_dedup_able(temp: &TempDir) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = temp.path().to_path_buf();
    let payload = vec![0xB9u8; 4096];

    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("blobs");
    let blob = "8888888888888888888888888888888888888888888888888888888888888888";
    let ollama_blob = blobs.join(format!("sha256-{}", blob));
    fs::write(&ollama_blob, &payload).expect("write");
    let m = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("u9dedup");
    fs::create_dir_all(&m).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m.join("7b"), manifest).expect("manifest");

    let hf_home = root.join(".cache").join("huggingface");
    let repo = hf_home.join("hub").join("models--u9dedup--U9d-7B");
    let hf_blobs = repo.join("blobs");
    let rev = "8888888888888888888888888888888888888888";
    let snap = repo.join("snapshots").join(rev);
    let refs = repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("hf blobs");
    fs::create_dir_all(&snap).expect("snap");
    fs::create_dir_all(&refs).expect("refs");
    let hf_blob_name = "8888888888888888888888888888888888888888888888888888888888888887";
    let hf_blob = hf_blobs.join(hf_blob_name);
    // Separate write (NOT a hardlink) → two inodes.
    fs::write(&hf_blob, &payload).expect("write hf");
    std::os::unix::fs::symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(hf_blob_name),
        snap.join("model.safetensors"),
    )
    .expect("symlink");
    fs::write(refs.join("main"), rev).expect("ref");

    (ollama_dir, hf_home, ollama_blob, hf_blob)
}

fn detail_regs_json(model_id: &str, ollama_blob: &Path, hf_blob: &Path) -> String {
    serde_json::json!({
        "id": model_id,
        "regs": [
            {"tool": "ollama", "path": ollama_blob.display().to_string()},
            {"tool": "hf", "path": hf_blob.display().to_string()},
        ]
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// AC-U9.1: shared-inode group for a "#" (already-unified) model.
// ---------------------------------------------------------------------------

#[test]
fn detail_for_unified_model_shows_shared_inode_with_grouped_paths() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_path_unified(&temp);
    // Sanity: fixture really IS hardlinked.
    use std::os::unix::fs::MetadataExt;
    let ino_a = fs::metadata(&ollama_blob).expect("stat ollama").ino();
    let ino_b = fs::metadata(&hf_blob).expect("stat hf").ino();
    assert_eq!(
        ino_a, ino_b,
        "fixture sanity: hardlinked paths must share an inode"
    );

    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let regs = detail_regs_json("u9unified/U9-7B", &ollama_blob, &hf_blob);
    let script = "<enter>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();

    // AC-U9.1: the shared-inode group label appears with the actual inode #.
    let shared_label = format!("shared inode {}", ino_a);
    assert!(
        lower.contains(&shared_label),
        "AC-U9.1: detail screen must show 'Shared inode {}' for a unified model.\nFrame:\n{}",
        ino_a,
        frame
    );

    // AC-U9.1: both paths appear in the frame (grouped under the shared
    // inode). Use a flattened-whitespace view so wrapped paths still match.
    let flat = flat_frame(&frame);
    let flat_ollama: String = ollama_blob
        .display()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let flat_hf: String = hf_blob
        .display()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        flat.contains(&flat_ollama),
        "AC-U9.1: ollama blob path missing from grouped detail frame:\n{}",
        frame
    );
    assert!(
        flat.contains(&flat_hf),
        "AC-U9.1: hf blob path missing from grouped detail frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U9.2: dedup-able model groups paths by inode (one group per copy).
// ---------------------------------------------------------------------------

#[test]
fn detail_for_dedup_able_model_shows_one_inode_group_per_separate_copy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_path_dedup_able(&temp);
    use std::os::unix::fs::MetadataExt;
    let ino_a = fs::metadata(&ollama_blob).expect("stat ollama").ino();
    let ino_b = fs::metadata(&hf_blob).expect("stat hf").ino();
    assert_ne!(
        ino_a, ino_b,
        "fixture sanity: dedup-able paths must have distinct inodes"
    );

    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let regs = detail_regs_json("u9dedup/U9d-7B", &ollama_blob, &hf_blob);
    let script = "<enter>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();

    // AC-U9.2: each separate inode appears as its own group label.
    let label_a = format!("inode {}", ino_a);
    let label_b = format!("inode {}", ino_b);
    assert!(
        lower.contains(&label_a),
        "AC-U9.2: detail screen must show group label for ollama inode {}.\nFrame:\n{}",
        ino_a,
        frame
    );
    assert!(
        lower.contains(&label_b),
        "AC-U9.2: detail screen must show group label for hf inode {}.\nFrame:\n{}",
        ino_b,
        frame
    );
    // AC-U9.2: a dedup-able model must NOT advertise itself as already shared.
    assert!(
        !lower.contains("shared inode"),
        "AC-U9.2: dedup-able model must NOT render 'Shared inode' (that label is \
         reserved for hardlinked-across-tools models). Frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U9.4: missing-inode (filesystem can't tell us) → graceful informational
// text, no panic. We trigger this by pointing one registration at a path that
// does NOT exist, so std::fs::metadata returns Err and the orchestrator
// records inode = None. The detail screen must render an informational "inode:
// not available on this filesystem" line for that registration without
// crashing.
// ---------------------------------------------------------------------------

#[test]
fn detail_handles_missing_inode_with_informational_text_no_crash() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, _real_hf_blob) = build_two_path_dedup_able(&temp);
    // Replace the second registration with a path that does NOT exist on
    // disk → std::fs::metadata returns Err → DetailRegistration.inode = None.
    let phantom = temp.path().join("phantom-no-such-file");
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let regs = detail_regs_json("u9phantom/U9p-7B", &ollama_blob, &phantom);
    let script = "<enter>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success(); // success() == no panic, no non-zero exit
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();

    // AC-U9.4: informational text appears for the missing-inode registration.
    // Use the flattened-whitespace view so a phrase wrapped across cells still
    // matches.
    let _ = lower; // silence unused on this path
    let flat = flat_frame(&frame).to_lowercase();
    assert!(
        flat.contains("inode:<notavailableonthisfilesystem>")
            || flat.contains("inode:notavailableonthisfilesystem"),
        "AC-U9.4: detail screen must show 'inode: <not available on this filesystem>' \
         when a registration's inode could not be determined. Frame:\n{}",
        frame
    );
}
