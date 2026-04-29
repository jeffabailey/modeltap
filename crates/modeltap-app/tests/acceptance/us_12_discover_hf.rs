//! Acceptance tests for US-12 (Discover Hugging Face cache models).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! lines for US-12. The HF plugin walks `${HF_HOME}/hub/` (default
//! `~/.cache/huggingface/`) for `models--<org>--<repo>/snapshots/<rev>/<file>`
//! symlinks; resolves each to a blob under `models--.../blobs/<sha256>`.
//! Broken snapshot symlinks are surfaced with `[broken: missing blob]` and
//! their bytes are NOT counted toward the disk total.
//!
//! Behaviors covered (US-12 acceptance criteria):
//! - AC-1 — `HF_HOME` env var is read; defaults are bypassed when set
//! - AC-2 — every `models--<org>--<repo>` directory is enumerated
//! - AC-3 — model id is `<org>/<repo>` path-style canonical
//! - AC-4 — format inferred from filename suffix (`.gguf`, `.safetensors`, `.bin`)
//! - AC-5 — broken snapshot symlinks reported and excluded from disk totals
//!
//! Each test enters through the modeltap binary driving port. Assertions are
//! made against the launch.log JSONL events (the driven port boundary).

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Build a named fixture tree under a fresh temp dir. Mirrors the helper in
/// `us_07_discover_llama_cli.rs` so the suites stay in sync.
fn build_fixture(name: &str) -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join(name);
    let project_root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|p| p.parent().and_then(|p| p.parent().map(PathBuf::from)))
        .expect("CARGO_MANIFEST_DIR + walk to workspace root");
    let script = project_root.join("tests/fixtures/build.sh");
    let status = StdCommand::new("bash")
        .arg(&script)
        .arg(name)
        .arg(&target)
        .status()
        .expect("spawn build.sh");
    assert!(status.success(), "fixture builder failed for {}", name);
    (temp, target)
}

/// Construct a `modeltap` headless command with all four plugin env-vars
/// pinned at non-existent paths so this test isolates from the developer's
/// real `$HOME`. The HF plugin reads `HF_HOME` for its cache root; tests
/// opt in by setting it.
fn modeltap_headless() -> (Command, TempDir) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LLAMACLI_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        // The HF plugin reads HF_HOME (the standard huggingface_hub env var)
        // for the cache root. Default to a non-existent path so each test
        // must explicitly opt in by overriding.
        .env("HF_HOME", "/nonexistent/no-such-hf-cache");
    (cmd, log_dir_temp)
}

fn read_launch_log(log_dir: &Path) -> Vec<Value> {
    let path = log_dir.join("launch.log");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read launch.log at {}: {}", path.display(), e));
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect()
}

fn find_event<'a>(events: &'a [Value], name: &str) -> Option<&'a Value> {
    events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some(name))
}

fn read_models_log(log_dir: &Path) -> Vec<Value> {
    let path = log_dir.join("models.log");
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each models.log line is JSON"))
        .collect()
}

fn hf_models(models_log: &[Value]) -> Vec<&Value> {
    models_log
        .iter()
        .filter(|m| m.get("tool").and_then(|v| v.as_str()) == Some("hf"))
        .collect()
}

// ---------------------------------------------------------------------------
// AC-1 — HF_HOME override is honored.
// ---------------------------------------------------------------------------

#[test]
fn hf_home_override_is_honored() {
    let (_temp, fixture_root) = build_fixture("devon-hf-cache");

    let (mut cmd, log_temp) = modeltap_headless();
    // Point HF_HOME at the fixture so the plugin walks <fixture>/hub/.
    cmd.env("HF_HOME", &fixture_root);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let hf = hf_models(&models_log);
    // Fixture has 3 model directories; m1 has 2 snapshot files (one
    // .safetensors + one .json/config) → the plugin emits one entry per
    // snapshot file, so:
    //   m1: model.safetensors + config.json   → 2 entries
    //   m2: model.gguf                        → 1 entry
    //   m3: model.bin (broken)                → 1 entry
    // Total: 4 entries.
    assert_eq!(
        hf.len(),
        4,
        "HF_HOME override must surface 4 snapshot entries from the fixture; got {}\nentries: {:#?}",
        hf.len(),
        hf
    );
}

// ---------------------------------------------------------------------------
// AC-2/AC-3 — Every models--<org>--<repo> dir enumerated; id is org/repo.
// ---------------------------------------------------------------------------

#[test]
fn hf_model_id_is_org_repo_path_style() {
    let (_temp, fixture_root) = build_fixture("devon-hf-cache");

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("HF_HOME", &fixture_root);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let hf = hf_models(&models_log);

    let ids: Vec<String> = hf
        .iter()
        .filter_map(|m| {
            m.get("id_in_tool")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();

    // Per AC-3, the id MUST be the path-style "org/repo" form, not the raw
    // directory name "models--meta-llama--Llama-3-8B".
    assert!(
        ids.iter().any(|i| i.starts_with("meta-llama/Llama-3-8B")),
        "AC-3: expected an id starting with 'meta-llama/Llama-3-8B'; got {:?}",
        ids
    );
    assert!(
        ids.iter()
            .any(|i| i.starts_with("mistralai/Mistral-7B-v0.3")),
        "AC-3: expected an id starting with 'mistralai/Mistral-7B-v0.3'; got {:?}",
        ids
    );

    // Negative: NO id should retain the raw "models--" prefix or the
    // double-dash separator (those are HF cache encoding, not the user-
    // visible id).
    assert!(
        !ids.iter().any(|i| i.starts_with("models--")),
        "AC-3: ids must not start with 'models--'; got {:?}",
        ids
    );
    assert!(
        !ids.iter().any(|i| i.contains("--")),
        "AC-3: ids must not contain '--' (HF cache encoding); got {:?}",
        ids
    );
}

// ---------------------------------------------------------------------------
// AC-4 — Format inferred from filename suffix.
// ---------------------------------------------------------------------------

#[test]
fn hf_format_is_inferred_from_filename_suffix() {
    let (_temp, fixture_root) = build_fixture("devon-hf-cache");

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("HF_HOME", &fixture_root);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let hf = hf_models(&models_log);

    // Find the .safetensors entry (under meta-llama/Llama-3-8B).
    let safet = hf
        .iter()
        .find(|m| {
            m.get("id_in_tool")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("meta-llama/Llama-3-8B") && s.ends_with("safetensors"))
        })
        .or_else(|| {
            // Fallback: id may not embed the filename — look for format directly.
            hf.iter().find(|m| {
                m.get("format").and_then(|v| v.as_str()) == Some("Safetensors")
                    && m.get("id_in_tool")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| s.starts_with("meta-llama/Llama-3-8B"))
            })
        })
        .unwrap_or_else(|| panic!("AC-4: expected a Safetensors entry: {:#?}", hf));
    assert_eq!(
        safet.get("format").and_then(|v| v.as_str()),
        Some("Safetensors"),
        "AC-4: model.safetensors must yield Format::Safetensors"
    );

    // Find the .gguf entry (under mistralai/Mistral-7B-v0.3).
    let gguf = hf
        .iter()
        .find(|m| {
            m.get("format").and_then(|v| v.as_str()) == Some("Gguf")
                && m.get("id_in_tool")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.starts_with("mistralai/Mistral-7B-v0.3"))
        })
        .unwrap_or_else(|| panic!("AC-4: expected a Gguf entry: {:#?}", hf));
    assert_eq!(
        gguf.get("format").and_then(|v| v.as_str()),
        Some("Gguf"),
        "AC-4: model.gguf must yield Format::Gguf"
    );
}

// ---------------------------------------------------------------------------
// AC-5 — Broken snapshot symlinks reported; excluded from disk totals.
// ---------------------------------------------------------------------------

#[test]
fn hf_broken_symlinks_are_flagged_and_excluded_from_totals() {
    let (_temp, fixture_root) = build_fixture("devon-hf-cache");

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("HF_HOME", &fixture_root);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let hf = hf_models(&models_log);

    // The corrupt-org/corrupt-repo entry must be present with a broken status.
    let broken = hf
        .iter()
        .find(|m| {
            m.get("id_in_tool")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.starts_with("corrupt-org/corrupt-repo"))
        })
        .unwrap_or_else(|| {
            panic!(
                "AC-5: corrupt-org/corrupt-repo broken-symlink entry must be present: {:#?}",
                hf
            )
        });

    let status = broken.get("status").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        status.to_ascii_lowercase().contains("broken")
            || status.to_ascii_lowercase().contains("missing"),
        "AC-5: broken-symlink entry must surface a Broken/Missing status; got {:?}",
        status
    );

    // The size of the broken entry must be 0 (or otherwise NOT contribute).
    // We assert via the launch.inventory total: the only sizes that should
    // contribute are blob1a (16 GB) + blob1b (4 KB) + blob2 (4.4 GB) =
    // 20_400_004_096 bytes. The missing blob (model.bin) MUST NOT be added.
    let events = read_launch_log(&log_dir);
    let inv = find_event(&events, "launch.inventory")
        .unwrap_or_else(|| panic!("no launch.inventory in:\n{:?}", events));
    let total = inv
        .get("total_disk_usage_bytes")
        .and_then(|v| v.as_u64())
        .expect("total_disk_usage_bytes is a number");
    let expected: u64 = 16_000_000_000 + 4096 + 4_400_000_000;
    assert_eq!(
        total, expected,
        "AC-5: total disk must equal sum of resolved blobs only \
         (broken symlink excluded); got {}, expected {}",
        total, expected
    );
}
