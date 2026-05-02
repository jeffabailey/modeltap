//! Acceptance scaffold for US-U3: Row glyph reflects dedup state.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u3`. Glyphs: ?/~/-/=/# per architecture-design.md §6.2.
//!
//! These RED tests fail today because:
//!   - `core::logic::dedup::compute_dedup_glyph` does not yet exist
//!     (per data-models.md "logic::dedup — new pure functions").
//!   - `render::row` does not yet render a fixed dedup-glyph column
//!     (per component-boundaries.md "render::row MODIFIED").
//!
//! REMOVE #[ignore] in DELIVER step when each scenario goes green.
//!
//! Tags: @us-u3

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

fn modeltap_headless_at(ollama_dir: &PathBuf, hf_home: &PathBuf) -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", ollama_dir)
        .env("HF_HOME", hf_home)
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

fn build_two_blob_duplicate(temp: &TempDir, hardlinked: bool) -> (PathBuf, PathBuf) {
    let root = temp.path().to_path_buf();
    let payload = vec![0xCDu8; 4096];

    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("create ollama blobs");
    let blob = "abcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcd";
    let ollama_blob = blobs.join(format!("sha256-{}", blob));
    fs::write(&ollama_blob, &payload).expect("write ollama blob");
    let m_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("dup");
    fs::create_dir_all(&m_dir).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m_dir.join("7b"), manifest).expect("write manifest");

    let hf_home = root.join(".cache").join("huggingface");
    let repo = hf_home.join("hub").join("models--dup--Dup-7B");
    let hf_blobs = repo.join("blobs");
    let rev = "abcdabcdabcdabcdabcdabcdabcdabcdabcdabcd";
    let snap = repo.join("snapshots").join(rev);
    let refs = repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("hf blobs");
    fs::create_dir_all(&snap).expect("hf snap");
    fs::create_dir_all(&refs).expect("hf refs");
    let hf_blob_name = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let hf_blob = hf_blobs.join(hf_blob_name);
    if hardlinked {
        fs::hard_link(&ollama_blob, &hf_blob).expect("hardlink hf blob");
    } else {
        fs::write(&hf_blob, &payload).expect("write hf blob");
    }
    std::os::unix::fs::symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(hf_blob_name),
        snap.join("model.safetensors"),
    )
    .expect("hf snap symlink");
    fs::write(refs.join("main"), rev).expect("write hf ref");

    (ollama_dir, hf_home)
}

#[test]
fn dedup_able_model_shows_equals_glyph_after_hashing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf) = build_two_blob_duplicate(&temp, /* hardlinked = */ false);
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    // Drive the hash pool to completion before sampling the frame: otherwise
    // the row glyph is still `?` (Pending). `<hash-complete>q` blocks until
    // the worker reports completion (added in step 01-09), then quits.
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    // After DELIVER lands US-U1+U3, a "=" character must appear next to the
    // duplicated model row in a fixed column. The exact column position is
    // crafter's choice (per data-models.md "render::row MODIFIED"); the
    // assertion is presence-on-some-row.
    assert!(
        frame.contains("="),
        "AC-U3.1: dedup-able row must show '=' glyph somewhere in the right pane, frame:\n{}",
        frame
    );
}

#[test]
fn already_unified_model_shows_hash_glyph_not_equals() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf) = build_two_blob_duplicate(&temp, /* hardlinked = */ true);
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    // After DELIVER: the inode-equality test in compute_dedup_glyph routes a
    // pre-hardlinked model to AlreadyUnified ("#") not DedupAble ("=").
    assert!(
        frame.contains("#"),
        "AC-U3.2: already-unified row must show '#' glyph, frame:\n{}",
        frame
    );
}

#[test]
fn pre_hash_row_shows_pending_glyph() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf) = build_two_blob_duplicate(&temp, false);
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    // First-paint frame: hashing has not started (NFR-1: paint precedes pool
    // spawn). Every row's dedup column must show "?".
    assert!(
        frame.contains("?"),
        "AC-U3.2: first-paint rows must show '?' glyph, frame:\n{}",
        frame
    );
}

#[test]
fn unique_model_shows_dash_glyph_after_hashing() {
    let temp = tempfile::tempdir().expect("tempdir");
    // Build a single-blob Ollama install — no HF, no duplicates anywhere.
    let root = temp.path().to_path_buf();
    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("create blobs");
    let blob = "0000000000000000000000000000000000000000000000000000000000000001";
    fs::write(blobs.join(format!("sha256-{}", blob)), vec![0u8; 4096]).expect("write blob");
    let m_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("solo");
    fs::create_dir_all(&m_dir).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m_dir.join("7b"), manifest).expect("write manifest");
    let hf_home = root.join("nonexistent-hf");
    let (mut cmd, _temp) = modeltap_headless_at(&ollama_dir, &hf_home);
    // `<hash-complete>q` blocks on the hash pool, then quits — so by the time
    // we sample the frame the unique blob has been classified as `Unique`
    // (glyph `-`), not `Pending` (glyph `?`).
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    // The "Dedup-able: 0 B" summary line also contains a "-" inside the
    // word "Dedup-able" — strip that line from consideration so the
    // assertion really measures the row glyph and not the bar's label.
    let row_pane = frame
        .lines()
        .filter(|l| !l.contains("Dedup-able"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        row_pane.contains('-'),
        "AC-U3.4: unique-model row must show '-' glyph in the right pane \
         (excluding the 'Dedup-able' summary line). Full frame:\n{}",
        frame
    );
}

/// RAII guard that restores a file's permissions on drop. Required because
/// `tempfile::TempDir`'s recursive cleanup fails on a mode-000 file (cannot
/// stat / remove). We restore mode 0o600 in `Drop` so the tempdir teardown
/// succeeds even if the test panics.
struct PermRestoreGuard {
    path: PathBuf,
}

impl Drop for PermRestoreGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&self.path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = fs::set_permissions(&self.path, perms);
        }
    }
}

#[test]
fn hash_failure_row_shows_dash_with_bang_decorator() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().expect("tempdir");
    // Build a single-blob Ollama install — then make the blob unreadable
    // (mode 000) so the background hash worker fails to open it. Per the
    // BR-3 / DedupGlyph::Failed → "-!" mapping in render::row, the row
    // glyph must render as `-!` (the `-` for unique-by-default plus the
    // `!` decorator for "we couldn't verify because hashing failed").
    let root = temp.path().to_path_buf();
    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("create blobs");
    let blob = "0000000000000000000000000000000000000000000000000000000000000002";
    let blob_path = blobs.join(format!("sha256-{}", blob));
    fs::write(&blob_path, vec![0u8; 4096]).expect("write blob");
    let m_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("noread");
    fs::create_dir_all(&m_dir).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m_dir.join("7b"), manifest).expect("write manifest");

    // Make the blob unreadable. The guard restores perms on drop so the
    // tempdir cleanup can recurse-delete the file even if this test panics.
    let mut perms = fs::metadata(&blob_path)
        .expect("stat blob before chmod")
        .permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&blob_path, perms).expect("chmod 000 blob");
    let _restore = PermRestoreGuard {
        path: blob_path.clone(),
    };

    let hf_home = root.join("nonexistent-hf");
    let (mut cmd, _temp) = modeltap_headless_at(&ollama_dir, &hf_home);
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    // AC-U3.5: hash-failure row glyph is `-!` (Unique-by-default + Failed
    // decorator). The `Dedup-able:` summary label contains `-` but not the
    // `-!` literal, so a substring search for `-!` is unambiguous.
    assert!(
        frame.contains("-!"),
        "AC-U3.5: hash-failure row must render '-!' (dash + bang decorator) \
         per render::row::dedup_glyph_text(DedupGlyph::Failed). Full frame:\n{}",
        frame
    );
}
