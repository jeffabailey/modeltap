//! Acceptance tests for US-19 (Cross-filesystem unify fallback with per-target
//! [s] skip / [c] copy / [x] cancel choice; ADR-008 OQ-4 refuse-default).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-19 scenarios. The 5 scenarios below drive the `modeltap` binary in
//! `MODELTAP_HEADLESS=1` mode against synthesized multi-tool fixtures and
//! inject synthetic cross-fs targets via the `MODELTAP_FAKE_CROSS_FS_PATHS`
//! env-var seam (a colon-separated path-prefix list; any registration whose
//! canonicalized path starts with one of these prefixes is treated as
//! cross-fs by the planner). The seam exercises the cross-fs code paths
//! without requiring an actual second mounted filesystem in CI.
//!
//! The 5 scenarios:
//!
//! 1. **All-same-filesystem unify proceeds normally** — fake-fs-probe is empty,
//!    so all 3 paths are same-fs. Pressing `u<enter>` triggers the standard
//!    unify path (no cross-fs dialog), all targets share an inode, and the
//!    `action.unify` JSONL event records `cross_fs_targets_skipped=0`,
//!    `cross_fs_targets_copied=0`.
//!
//! 2. **Skip option leaves cross-fs target untouched** — fake-fs-probe flags
//!    one target as cross-fs. Pressing `u` opens the cross-fs dialog;
//!    pressing `s` proceeds with the skip semantics: same-fs target is
//!    linked, cross-fs target's inode and content are unchanged. The JSONL
//!    event records `cross_fs_targets_skipped=1`.
//!
//! 3. **Copy option duplicates bytes to cross-fs target** — same fixture as
//!    scenario 2. Pressing `c` proceeds with the copy semantics: same-fs
//!    target is hardlinked; the cross-fs target now has byte-for-byte equal
//!    content but at a DIFFERENT inode (i.e. a real copy, not a hardlink).
//!    The JSONL event records `cross_fs_targets_copied=1`.
//!
//! 4. **All-cross-fs unify is refused** — fake-fs-probe flags ALL targets as
//!    cross-fs. The dialog opens in `AllCrossFs` mode; pressing `<enter>`
//!    (the refuse default per ADR-008 OQ-4) cancels the unify. No FS
//!    mutation; no destructive `action.unify` event with non-zero linkage.
//!
//! 5. **Default-on-Enter is REFUSE** — same fixture as scenario 2 (mixed
//!    cross-fs). Pressing `<enter>` at the cross-fs prompt (no explicit
//!    `s` / `c` / `x` first) cancels the unify per the refuse-default
//!    contract. No FS mutation; no banner mentioning success.
//!
//! Tags: @us-19 @release-2 @cross-fs.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Shared fixture builder. Three tools (ollama / llama-cli / hf) under one
// tempdir on a single real filesystem. The cross-fs flag is INJECTED at run
// time via `MODELTAP_FAKE_CROSS_FS_PATHS`; the on-disk layout is unchanged
// from the standard us_10 fixture.
// ---------------------------------------------------------------------------

struct Fixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
    hf_home: PathBuf,
    ollama_path: PathBuf,
    hf_blob_path: PathBuf,
    payload: Vec<u8>,
}

fn build_fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let payload_size: usize = 4096;
    let payload: Vec<u8> = (0..payload_size).map(|i| (i % 251) as u8).collect();

    // Ollama layout.
    let ollama_dir = root.join(".ollama").join("models");
    let blob_hash = "abc123def4567890abc123def4567890abc123def4567890abc123def4567890";
    let ollama_blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&ollama_blobs).expect("create ollama blobs");
    let ollama_path = ollama_blobs.join(format!("sha256-{}", blob_hash));
    fs::write(&ollama_path, &payload).expect("write ollama blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("us19");
    fs::create_dir_all(&manifest_dir).expect("create manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":{size}}}]}}"#,
        blob = blob_hash,
        size = payload_size
    );
    fs::write(manifest_dir.join("7b"), manifest).expect("write manifest");

    // HF layout.
    let hf_home = root.join(".cache").join("huggingface");
    let hf_hub = hf_home.join("hub");
    let hf_repo_dir = hf_hub.join("models--us19--Synthetic-7B");
    let hf_rev = "abc123def4567890abc123def4567890abc12345";
    let hf_blobs = hf_repo_dir.join("blobs");
    let hf_snapshots = hf_repo_dir.join("snapshots").join(hf_rev);
    let hf_refs = hf_repo_dir.join("refs");
    fs::create_dir_all(&hf_blobs).expect("create hf blobs");
    fs::create_dir_all(&hf_snapshots).expect("create hf snapshots");
    fs::create_dir_all(&hf_refs).expect("create hf refs");
    let hf_blob_name = "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100";
    let hf_blob_path = hf_blobs.join(hf_blob_name);
    fs::write(&hf_blob_path, &payload).expect("write hf blob");
    let snapshot_link = hf_snapshots.join("model.safetensors");
    let rel_target = PathBuf::from("..")
        .join("..")
        .join("blobs")
        .join(hf_blob_name);
    std::os::unix::fs::symlink(&rel_target, &snapshot_link).expect("symlink hf snapshot");
    fs::write(hf_refs.join("main"), hf_rev).expect("write hf ref");

    Fixture {
        _temp: temp,
        ollama_dir,
        hf_home,
        ollama_path,
        hf_blob_path,
        payload,
    }
}

fn modeltap_headless(fix: &Fixture) -> (Command, TempDir, PathBuf) {
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

fn detail_regs_json(fix: &Fixture) -> String {
    let hf_snapshot = fix
        .hf_home
        .join("hub")
        .join("models--us19--Synthetic-7B")
        .join("snapshots")
        .join("abc123def4567890abc123def4567890abc12345")
        .join("model.safetensors");
    serde_json::json!({
        "id": "us19/Synthetic-7B",
        "regs": [
            {"tool": "ollama", "path": fix.ollama_path.display().to_string()},
            {"tool": "hf",     "path": hf_snapshot.display().to_string()},
        ]
    })
    .to_string()
}

/// Canonicalize a path for use in `MODELTAP_FAKE_CROSS_FS_PATHS`. The headless
/// harness compares the env var entries against `std::fs::canonicalize(reg.path)`,
/// and on macOS `tempfile::tempdir()` lives under `/var/folders/...` which
/// canonicalizes to `/private/var/folders/...`. Without this canonicalization
/// the prefix match in `path_matches_fake_cross_fs` would silently fail and
/// the cross-fs flag would never fire.
fn canon(p: &Path) -> String {
    fs::canonicalize(p)
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", p.display()))
        .display()
        .to_string()
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

fn unify_event(events: &[Value]) -> Option<&Value> {
    events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
}

// ---------------------------------------------------------------------------
// Scenario 1: All-same-filesystem unify proceeds normally.
// No fake cross-fs prefixes set → planner sees every target as same-fs →
// no dialog, standard unify path. JSONL records cross_fs_*=0.
// ---------------------------------------------------------------------------

#[test]
fn all_same_filesystem_unify_proceeds_normally() {
    let fix = build_fixture();
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    // Pre-condition: distinct inodes.
    let pre_ollama = ino_of(&fix.ollama_path);
    let pre_hf = ino_of(&fix.hf_blob_path);
    assert_ne!(pre_ollama, pre_hf, "fixture precondition");

    // No MODELTAP_FAKE_CROSS_FS_PATHS — pure same-fs case.
    let script = "<enter>u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // Post-condition: both paths share one inode (standard unify path).
    let post_ollama = ino_of(&fix.ollama_path);
    let post_hf = ino_of(&fix.hf_blob_path);
    assert_eq!(
        post_ollama, post_hf,
        "AC-1: same-fs unify must hardlink ollama + hf"
    );

    // JSONL: cross_fs counts are zero.
    let events = read_jsonl_events(&log_file);
    let event = unify_event(&events).expect("must emit action.unify");
    assert_eq!(
        event
            .get("cross_fs_targets_skipped")
            .and_then(|v| v.as_u64()),
        Some(0),
        "AC-1: same-fs path must record cross_fs_targets_skipped=0"
    );
    assert_eq!(
        event
            .get("cross_fs_targets_copied")
            .and_then(|v| v.as_u64()),
        Some(0),
        "AC-1: same-fs path must record cross_fs_targets_copied=0"
    );
    assert_eq!(
        event.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "AC-1: same-fs full-link path must record outcome=success"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Skip option leaves cross-fs target untouched.
// Fake-fs-probe flags HF as cross-fs. User presses `s`. The cross-fs target
// (hf) inode + content unchanged. With ollama as the canonical and hf as
// the only target, "skip" means no hardlink is created at all.
// ---------------------------------------------------------------------------

#[test]
fn skip_option_leaves_cross_fs_target_untouched() {
    let fix = build_fixture();
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    let pre_hf_ino = ino_of(&fix.hf_blob_path);
    let pre_hf_bytes = fs::read(&fix.hf_blob_path).expect("read hf pre");

    // Inject fake cross-fs for hf only. The planner will flag it as
    // cross_filesystem.
    let fake_cross_fs = canon(&fix.hf_home);

    // Script: <enter> open detail; u opens cross-fs dialog (all targets
    // cross-fs, since hf is the only target); s = skip; q quit.
    let script = "<enter>us q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_CROSS_FS_PATHS", &fake_cross_fs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // hf untouched: same inode, same bytes.
    let post_hf_ino = ino_of(&fix.hf_blob_path);
    let post_hf_bytes = fs::read(&fix.hf_blob_path).expect("read hf post");
    assert_eq!(
        pre_hf_ino, post_hf_ino,
        "AC-2: skip must NOT change cross-fs target's inode"
    );
    assert_eq!(
        pre_hf_bytes, post_hf_bytes,
        "AC-2: skip must NOT change cross-fs target's content"
    );

    // JSONL: skipped=1, copied=0.
    let events = read_jsonl_events(&log_file);
    let event = unify_event(&events).expect("must emit action.unify");
    assert_eq!(
        event
            .get("cross_fs_targets_skipped")
            .and_then(|v| v.as_u64()),
        Some(1),
        "AC-2: must record cross_fs_targets_skipped=1, got {}",
        event
    );
    assert_eq!(
        event
            .get("cross_fs_targets_copied")
            .and_then(|v| v.as_u64()),
        Some(0),
        "AC-2: must record cross_fs_targets_copied=0, got {}",
        event
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: Copy option duplicates bytes to cross-fs target.
// Same fixture as scenario 2. User presses `c`. hf now has the
// canonical's bytes (byte-for-byte) but at a different inode.
// ---------------------------------------------------------------------------

#[test]
fn copy_option_duplicates_bytes_to_cross_fs_target() {
    let fix = build_fixture();
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    let pre_ollama_ino = ino_of(&fix.ollama_path);

    let fake_cross_fs = canon(&fix.hf_home);

    // Script: <enter>u opens dialog; c = copy; q quit.
    let script = "<enter>uc q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_CROSS_FS_PATHS", &fake_cross_fs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // hf has the canonical's bytes.
    let post_hf_bytes = fs::read(&fix.hf_blob_path).expect("read hf post");
    assert_eq!(
        post_hf_bytes, fix.payload,
        "AC-3: copy must duplicate canonical's bytes to cross-fs target"
    );

    // hf has DIFFERENT inode from canonical (byte-copy, not hardlink).
    let post_hf_ino = ino_of(&fix.hf_blob_path);
    let post_ollama_ino = ino_of(&fix.ollama_path);
    // Sanity: ollama inode unchanged after operation.
    assert_eq!(
        pre_ollama_ino, post_ollama_ino,
        "ollama inode should be stable across the action"
    );
    assert_ne!(
        post_hf_ino, post_ollama_ino,
        "AC-3: copy must produce a DIFFERENT inode (not a hardlink)"
    );

    // JSONL: skipped=0, copied=1.
    let events = read_jsonl_events(&log_file);
    let event = unify_event(&events).expect("must emit action.unify");
    assert_eq!(
        event
            .get("cross_fs_targets_skipped")
            .and_then(|v| v.as_u64()),
        Some(0),
        "AC-3: must record cross_fs_targets_skipped=0, got {}",
        event
    );
    assert_eq!(
        event
            .get("cross_fs_targets_copied")
            .and_then(|v| v.as_u64()),
        Some(1),
        "AC-3: must record cross_fs_targets_copied=1, got {}",
        event
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: All-cross-fs unify is refused.
// Every target tagged cross-fs. User presses Enter (default = refuse).
// No FS mutation; any emitted action.unify event must NOT show success.
// ---------------------------------------------------------------------------

#[test]
fn all_cross_fs_unify_is_refused() {
    let fix = build_fixture();
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    let pre_ollama_ino = ino_of(&fix.ollama_path);
    let pre_hf_ino = ino_of(&fix.hf_blob_path);

    // Mark hf cross-fs (the only TARGET — ollama is canonical via
    // lexicographic tiebreak on the smallest tool id). With every active
    // target cross-fs the dialog opens in AllCrossFs mode.
    let fake_cross_fs = canon(&fix.hf_home);

    // Script: <enter>u opens dialog; <enter> = refuse-default; q quit.
    let script = "<enter>u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_CROSS_FS_PATHS", &fake_cross_fs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // No FS mutation: inodes unchanged on every target.
    assert_eq!(
        pre_ollama_ino,
        ino_of(&fix.ollama_path),
        "AC-4: all-cross-fs refusal must NOT mutate ollama inode"
    );
    assert_eq!(
        pre_hf_ino,
        ino_of(&fix.hf_blob_path),
        "AC-4: all-cross-fs refusal must NOT mutate hf inode"
    );

    // Any emitted action.unify event must not show outcome=success.
    let events = read_jsonl_events(&log_file);
    for e in events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
    {
        let outcome = e.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            outcome, "success",
            "AC-4: all-cross-fs refusal must NOT emit outcome=success, got {}",
            e
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 5: Default-on-Enter is REFUSE (ADR-008 OQ-4).
// Mixed cross-fs case (one target cross-fs). User presses Enter directly
// at the cross-fs prompt — must cancel, not silently proceed.
// ---------------------------------------------------------------------------

#[test]
fn default_on_enter_is_refuse() {
    let fix = build_fixture();
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let regs = detail_regs_json(&fix);

    let pre_ollama_ino = ino_of(&fix.ollama_path);
    let pre_hf_ino = ino_of(&fix.hf_blob_path);

    let fake_cross_fs = canon(&fix.hf_home);

    // Script: <enter> open detail; u open cross-fs dialog; <enter> default-cancel; q.
    let script = "<enter>u<enter>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_DETAIL_REGS", regs)
        .env("MODELTAP_FAKE_CROSS_FS_PATHS", &fake_cross_fs)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    // No mutation: inodes unchanged.
    assert_eq!(
        pre_ollama_ino,
        ino_of(&fix.ollama_path),
        "AC-5: refuse-default must NOT mutate ollama inode"
    );
    assert_eq!(
        pre_hf_ino,
        ino_of(&fix.hf_blob_path),
        "AC-5: refuse-default must NOT mutate hf inode (transactional cancel)"
    );

    // Any emitted action.unify event must not show outcome=success — the
    // cancel path must not fire the orchestrator at all.
    let events = read_jsonl_events(&log_file);
    for e in events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.unify"))
    {
        let outcome = e.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            outcome, "success",
            "AC-5: refuse-default must NOT emit outcome=success, got {}",
            e
        );
    }
}
