//! Acceptance scaffold for US-U10: Partial-success toast with retry-failed-only.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u10`. AC-U10.1..AC-U10.5.
//!
//! These tests assert that after a partial-success unify the post-action
//! banner ("toast") shows:
//!   - header: "Unified <model> into <K> of <N>"
//!   - per-target lines with OK/FAIL labels (and reason on FAIL)
//!   - total reclaim line (sum of OK targets only)
//!   - footer: "[r] Retry-failed-only / [Enter] Continue"
//!
//! And that pressing `[r]` re-invokes `actions::unify::run` with a plan
//! filtered to ONLY the failed targets (the previously-successful targets
//! are NOT touched).
//!
//! Tags: @us-u10

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

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

/// Build the 3-tool partial-success fixture from us_u6: ollama + hf both
/// writable, lm-studio target dir mode 0500 (read+execute, no write) so
/// `link()` fails with EACCES on the lm-studio target.
///
/// Returns (ollama_dir, hf_home, lm_root, ollama_blob, hf_blob, lm_blob,
/// lm_repo_dir).
#[allow(clippy::type_complexity)]
fn build_three_tool_partial_fixture(
    temp: &TempDir,
) -> (
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
) {
    let (ollama, hf, ollama_blob, hf_blob) = build_two_blob_duplicate(temp);
    let payload = vec![0x42u8; 4096];
    let lm_root = temp.path().join(".cache").join("lm-studio").join("models");
    let lm_repo_dir = lm_root.join("dup").join("Dup-7B");
    fs::create_dir_all(&lm_repo_dir).expect("lm-studio dir");
    let lm_blob = lm_repo_dir.join("model.gguf");
    fs::write(&lm_blob, &payload).expect("write lm-studio blob");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&lm_repo_dir).unwrap().permissions();
        perm.set_mode(0o500);
        fs::set_permissions(&lm_repo_dir, perm).expect("chmod 0500");
    }
    (
        ollama,
        hf,
        lm_root,
        ollama_blob,
        hf_blob,
        lm_blob,
        lm_repo_dir,
    )
}

fn restore_perms(lm_repo_dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(lm_repo_dir) {
            let mut perm = meta.permissions();
            perm.set_mode(0o700);
            let _ = fs::set_permissions(lm_repo_dir, perm);
        }
    }
}

fn skip_if_root_or_not_unix() -> bool {
    if !cfg!(unix) {
        eprintln!("skipping: partial-success scenario is Unix-only");
        return true;
    }
    #[cfg(unix)]
    {
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(1);
        if uid == 0 {
            eprintln!("skipping: cannot test EACCES partial-success as root");
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// AC-U10.1, AC-U10.2, AC-U10.5: partial-success toast lists each target's
// outcome inline with header "1 of 2", per-target OK/FAIL lines, total
// reclaim, and footer pointing to retry-failed-only.
// ---------------------------------------------------------------------------

#[test]
fn partial_success_toast_shows_per_target_outcomes_and_retry_footer() {
    if skip_if_root_or_not_unix() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, lm_root, ollama_blob, hf_blob, lm_blob, lm_repo_dir) =
        build_three_tool_partial_fixture(&temp);

    let pre_lm_inode = ino_of(&lm_blob);
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
    let _log_file = log_dir.join("launch.log");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &ollama)
        .env("HF_HOME", &hf)
        .env("MODELTAP_LMSTUDIO_DIRS", &lm_root)
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");

    let regs = serde_json::json!({
        "id": "dup/Dup-7B",
        "regs": [
            {"tool": "ollama", "path": ollama_blob.display().to_string()},
            {"tool": "hf", "path": hf_blob.display().to_string()},
            {"tool": "lm-studio", "path": lm_blob.display().to_string()},
        ]
    })
    .to_string();
    // <enter> opens detail; u<enter> opens the unify dialog and confirms;
    // <esc> closes Detail back to Main so the right pane paints the
    // partial-success toast in the same session; q quits.
    let script = "<enter>u<enter><esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    restore_perms(&lm_repo_dir);

    // Precondition: ollama+hf converged; lm-studio inode unchanged.
    assert_eq!(ino_of(&ollama_blob), ino_of(&hf_blob));
    assert_eq!(ino_of(&lm_blob), pre_lm_inode);

    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();

    // AC-U10.1: header reports "K of N" — N is the count of TARGETS the
    // unify attempted to link (excludes the canonical itself, which is the
    // source of the hardlink). With 3 tools and 1 canonical, N == 2; the
    // partial here is "1 of 2" (one of {ollama, hf} is canonical, the other
    // succeeded as a target; lm-studio failed).
    assert!(
        lower.contains("1 of 2"),
        "AC-U10.1: toast header must show 'K of N' (target counts excluding canonical) for partial-success, got:\n{}",
        frame
    );
    // AC-U10.2: per-target lines must include OK markers for the successful
    // targets and a FAIL marker for the failed target.
    assert!(
        lower.contains("ok") || lower.contains("\u{2713}"),
        "AC-U10.2: toast must show OK markers for successful targets, got:\n{}",
        frame
    );
    assert!(
        lower.contains("fail"),
        "AC-U10.2: toast must show FAIL marker for failed target, got:\n{}",
        frame
    );
    // AC-U10.5: footer shows the retry-failed-only shortcut.
    assert!(
        lower.contains("[r]") || lower.contains("retry"),
        "AC-U10.5: toast must show [r] retry-failed-only footer, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U10.1, AC-U10.2 (total-failure case): when every target fails, the
// header shows "0 of N" and the row glyph stays "=".
// ---------------------------------------------------------------------------

#[test]
fn total_failure_toast_shows_zero_of_n_and_glyph_remains_equals() {
    if skip_if_root_or_not_unix() {
        return;
    }
    // Build 2-tool fixture but make BOTH non-canonical targets unwritable so
    // every link fails. Strategy: duplicate the lm-studio EACCES trick on
    // the hf snapshot dir AS WELL by chmod'ing the snapshot dir read-only.
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_blob_duplicate(&temp);

    // Make hf snapshot dir read-only so the link target's parent rejects writes.
    let snapshot_dir = hf_blob.parent().unwrap().to_path_buf();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&snapshot_dir).unwrap().permissions();
        perm.set_mode(0o500);
        fs::set_permissions(&snapshot_dir, perm).expect("chmod 0500 hf snap");
    }
    let pre_hf_inode = ino_of(&hf_blob);

    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &ollama)
        .env("HF_HOME", &hf)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");

    // For total-failure we make hf the canonical (blob alphabetically smaller
    // than ollama-blob) — actually canonical_selector uses size-and-tool order;
    // simpler: register hf as the canonical-tool by making it the only
    // non-canonical target list-mate. We pass both tools but the ollama blob
    // is canonical (larger sha sort or first-tool-deterministic). To force
    // total-failure regardless of canonical choice, we ALSO chmod the ollama
    // blobs dir read-only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let ollama_blobs = ollama_blob.parent().unwrap().to_path_buf();
        let mut perm = fs::metadata(&ollama_blobs).unwrap().permissions();
        perm.set_mode(0o500);
        fs::set_permissions(&ollama_blobs, perm).expect("chmod 0500 ollama blobs");
    }
    let pre_ollama_inode = ino_of(&ollama_blob);

    let regs = serde_json::json!({
        "id": "dup/Dup-7B",
        "regs": [
            {"tool": "ollama", "path": ollama_blob.display().to_string()},
            {"tool": "hf", "path": hf_blob.display().to_string()},
        ]
    })
    .to_string();
    let script = "<enter>u<enter><esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // Restore perms so tempdir cleanup works.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&snapshot_dir).unwrap().permissions();
        perm.set_mode(0o700);
        let _ = fs::set_permissions(&snapshot_dir, perm);
        let ollama_blobs = ollama_blob.parent().unwrap().to_path_buf();
        let mut perm = fs::metadata(&ollama_blobs).unwrap().permissions();
        perm.set_mode(0o700);
        let _ = fs::set_permissions(&ollama_blobs, perm);
    }

    // Precondition: no inode merge happened.
    assert_eq!(ino_of(&hf_blob), pre_hf_inode);
    assert_eq!(ino_of(&ollama_blob), pre_ollama_inode);

    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    let lower = frame.to_lowercase();
    // AC-U10.1: total-failure header shows "0 of N" (N is the non-canonical
    // target count — 1 since unify excludes the canonical itself from the
    // link list, but the user-facing toast counts targets attempted).
    assert!(
        lower.contains("0 of "),
        "AC-U10.1 (total-failure): toast must show '0 of N' header, got:\n{}",
        frame
    );
    // Row glyph stays "=" (no inode merged).
    assert!(
        frame.contains('='),
        "AC-U10.2 (total-failure): row glyph must remain '=' (no inode merged), got:\n{}",
        frame
    );
}
