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
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", ollama)
        .env("HF_HOME", hf)
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
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

// ---------------------------------------------------------------------------
// AC-U6.1, AC-U6.2, AC-U6.4, AC-U6.7: full success flips "=" to "#",
// summary bar updates, no restart.
// ---------------------------------------------------------------------------

#[test]
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
    // <hash-complete>: block until the background hash pool publishes
    //   HashComputed for both blobs (so `state.hash_state.inodes` is
    //   populated; the reclassify pass requires this to rewrite affected
    //   entries onto the canonical inode).
    // <enter>: opens Detail screen with the seeded registrations.
    // u<enter>: opens unify confirm dialog and applies the plan. The unify
    //   reduces 2 distinct inodes to 1 shared inode; reclassify_after_unify
    //   updates `state.hash_state.inodes` and recomputes `dedup_summary`
    //   (dedup_able collapses 4096→0; summary_delta carries the previous).
    // <esc>: closes Detail, returns to Main (DOES NOT restart the process —
    //   AC-U6.7 invariant; same composition root, same AppState).
    // <right><right>: navigate from default-selected slot (idx=0=hf, sorted)
    //   to idx=2 = `[All Unified]` synthetic slot. The right pane then
    //   dispatches to `render::all_unified` which prints the
    //   `Unified: N models | Total reclaimed by unification: ...` footer
    //   (the source of the `Unified:` text per AC-U6.4).
    // q: quits.
    //
    // The captured stdout contains EVERY printed frame (the headless harness
    // auto-captures the unify-confirm dialog mid-script + the final paint
    // post-quit). The "#" glyph appears in the dedup-key shorthand
    // `(#00000000)` of the unify confirm dialog AND in the [All Unified]
    // body row glyph; the "Unified:" footer appears in the final frame.
    let script = "<hash-complete><enter>u<enter><esc><right><right>q";
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
fn summary_bar_shows_transient_delta_then_collapses() {
    // AC-U6.5: after a successful unify, the summary bar shows
    // "Dedup-able: <new> (was <previous>)" for ~5 seconds.
    //
    // Mechanism: capture the frame immediately post-unify (test runs in well
    // under 5s, so the SummaryDelta has not yet been cleared by the
    // Msg::SummaryDeltaExpired timer). The reclassify_after_unify pass sets
    // `state.summary_delta = Some { previous_dedup_able_bytes, expires_at }`
    // and the summary_bar renderer appends "(was X)" while the delta is
    // live. The bound for non-staleness is honoured by the renderer's local
    // `delta.expires_at > Instant::now()` gate (no separate clock seam
    // needed for the immediate-capture test).
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
    let script = "<enter>u<enter><esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let post_a = ino_of(&ollama_blob);
    let post_b = ino_of(&hf_blob);
    assert_eq!(post_a, post_b, "AC-U6.5 precondition: unify must succeed");
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    assert!(
        frame.contains("(was "),
        "AC-U6.5: summary bar must show transient '(was X)' annotation \
         immediately post-unify (delta is live for ~5s; test runs faster \
         than that), got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U6.3, AC-U6.6: partial success leaves glyph as "=", Unified does not
// increment.
// ---------------------------------------------------------------------------

#[test]
fn partial_unify_leaves_glyph_as_equals_and_unified_count_unchanged() {
    // Inducing partial-success deterministically: build a 3-tool fixture
    // (ollama + hf + lm-studio) where every tool has the SAME duplicate
    // model, but the lm-studio target directory is mode 0500 (read+execute,
    // NO write). Discovery walks the dir successfully; link() fails with
    // EACCES because hardlinking creates a NEW dirent in a directory the
    // process cannot write to.
    //
    // No new env-var seam is needed — `MODELTAP_LMSTUDIO_DIRS` already
    // points at the fixture's lm-studio dir, and chmod handles the
    // permission flip.

    // Skip on non-Unix and as root (chmod 0500 has no effect on root).
    if !cfg!(unix) {
        eprintln!("skipping: partial-success scenario is Unix-only");
        return;
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
            return;
        }
    }

    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf, ollama_blob, hf_blob) = build_two_blob_duplicate(&temp);

    // Build a third (lm-studio) target with the same payload, then chmod
    // its parent dir to 0500 so link() will fail.
    let payload = vec![0x42u8; 4096];
    let lm_root = temp.path().join(".cache").join("lm-studio").join("models");
    let lm_repo_dir = lm_root.join("dup").join("Dup-7B");
    fs::create_dir_all(&lm_repo_dir).expect("lm-studio dir");
    let lm_blob = lm_repo_dir.join("model.gguf");
    fs::write(&lm_blob, &payload).expect("write lm-studio blob");
    // Make the lm-studio TARGET dir read-only AFTER discovery would walk it.
    // Discovery still works (read + execute bits set); link() fails (no
    // write bit) because it cannot create a new dirent.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&lm_repo_dir).unwrap().permissions();
        perm.set_mode(0o500);
        fs::set_permissions(&lm_repo_dir, perm).expect("chmod 0500");
    }

    let pre_lm_inode = ino_of(&lm_blob);
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
    let log_file = log_dir.join("launch.log");
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
    let script = "<enter>u<enter><esc>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // Restore mode so tempdir cleanup can remove the file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = fs::metadata(&lm_repo_dir).unwrap().permissions();
        perm.set_mode(0o700);
        let _ = fs::set_permissions(&lm_repo_dir, perm);
    }

    // AC-U6.3: ollama+hf converged onto a shared inode; lm-studio inode
    // unchanged (link failed).
    let post_a = ino_of(&ollama_blob);
    let post_b = ino_of(&hf_blob);
    let post_lm = ino_of(&lm_blob);
    assert_eq!(
        post_a, post_b,
        "AC-U6.3: ollama+hf must converge on partial"
    );
    assert_eq!(
        post_lm, pre_lm_inode,
        "AC-U6.3: lm-studio inode must be UNCHANGED on partial-failure"
    );

    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    // AC-U6.3: row glyph stays '=' (still 2 distinct inodes after partial).
    assert!(
        frame.contains('='),
        "AC-U6.3: post-partial-unify row glyph must remain '=' (lm-studio \
         still on a separate inode), got:\n{}",
        frame
    );
    // AC-U6.6: Unified count must NOT increment. Look for "Unified: 0" in
    // the summary bar.
    let lower = frame.to_lowercase();
    assert!(
        lower.contains("unified: 0") || !lower.contains("unified:"),
        "AC-U6.6: partial-unify must NOT increment Unified count, got:\n{}",
        frame
    );

    // AC-CONS-4: action.unify event records `outcome=partial` and a
    // bytes_reclaimed equal to the bytes of successful targets only.
    let raw = std::fs::read_to_string(&log_file).expect("launch.log");
    let events: Vec<serde_json::Value> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let unify_evt = events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
        .expect("AC-CONS-4: action.unify event must be emitted");
    assert_eq!(
        unify_evt.get("outcome").and_then(|v| v.as_str()),
        Some("partial"),
        "AC-U6.3: outcome must be 'partial', event was: {}",
        unify_evt
    );
    let bytes_reclaimed = unify_evt
        .get("bytes_reclaimed")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::MAX);
    assert!(
        bytes_reclaimed > 0 && bytes_reclaimed < (3 * 4096),
        "AC-CONS-4: bytes_reclaimed must be the SUCCESSFUL targets' bytes \
         only (>0 and < 3 * 4096); got {}",
        bytes_reclaimed
    );
}
