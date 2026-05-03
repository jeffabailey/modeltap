//! Acceptance tests for the GPT4All plugin — the 6th real production plugin.
//! Validates the US-18 plugin extensibility contract in production: a
//! contributor adding a new tool needed ZERO changes to
//! `crates/modeltap-core/src/`. The architecture lint
//! (`tests/architecture.rs`) certifies that statically; THIS file certifies
//! it dynamically by driving the production binary against synthetic GPT4All
//! fixtures.
//!
//! GPT4All stores models as flat `*.gguf` files under (per host OS):
//!   - All platforms (Python SDK):  `~/.cache/gpt4all/`
//!   - macOS desktop:               `~/Library/Application Support/nomic-ai/gpt4all-chat/`
//!   - Linux/WSL desktop:           `~/.local/share/nomic-ai/gpt4all-chat/`
//!
//! Unlike Atomic Chat, GPT4All has NO manifest layer (no `model.yml`).
//! Discovery is a depth-1 walk — each `*.gguf` becomes one `DiscoveredModel`
//! with `id_in_tool` = the filename and `size_bytes` = `metadata().len()`.
//!
//! Behaviors covered (acceptance criteria AC-G1.1..AC-G1.5):
//! 1. GPT4All models are discovered — synthetic 2-`.gguf` fixture; both appear
//!    in models.log under tool=`gpt4all` with `format=Gguf` and the on-disk
//!    file sizes verbatim.
//! 2. GPT4All not installed shows benign message — fixture path absent
//!    (`MODELTAP_GPT4ALL_DIRS` pinned to `/nonexistent` by the headless
//!    helper); `gpt4all` MUST NOT appear in `launch.inventory.tool_errors`
//!    (it's NotInstalled, which is benign per US-15 AC-4).
//! 3. Two configured dirs are both walked — `MODELTAP_GPT4ALL_DIRS="dir1:dir2"`
//!    with one `.gguf` per dir; both surface in models.log. Pivots from
//!    Atomic Chat's yml-parse-error scenario because GPT4All has no manifest
//!    layer to corrupt.
//!
//! Each test enters through the modeltap binary driving port. Assertions are
//! made against the launch.log + models.log JSONL events (driven port
//! boundary).

use std::path::Path;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Construct a `modeltap` headless command with all six plugin env-vars
/// pinned at non-existent paths so this test isolates from the developer's
/// real `$HOME`. Tests opt INTO GPT4All by overriding `MODELTAP_GPT4ALL_DIRS`.
fn modeltap_headless() -> (Command, TempDir) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all");
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

fn gpt4all_models(models_log: &[Value]) -> Vec<&Value> {
    models_log
        .iter()
        .filter(|m| m.get("tool").and_then(|v| v.as_str()) == Some("gpt4all"))
        .collect()
}

/// Write a single flat `.gguf` file directly under `root` with `payload` as
/// its body. GPT4All has no manifest layer, so a depth-1 file is the entire
/// "model on disk".
fn write_gpt4all_model(root: &Path, file_name: &str, payload: &[u8]) {
    std::fs::create_dir_all(root).expect("create root");
    std::fs::write(root.join(file_name), payload).expect("write gguf file");
}

// ---------------------------------------------------------------------------
// Scenario 1 — GPT4All models are discovered (AC-G1.1, AC-G1.2, AC-G1.3).
// ---------------------------------------------------------------------------

#[test]
fn gpt4all_models_are_discovered() {
    let temp = tempfile::tempdir().expect("tempdir for fixture");
    let models_root = temp.path();
    // Two distinct, deterministic payload sizes so we can assert size_bytes
    // straight from std::fs::metadata().
    let qwen_payload = vec![0xABu8; 4_321];
    let llama_payload = vec![0xCDu8; 2_048];
    write_gpt4all_model(models_root, "qwen2-7b.gguf", &qwen_payload);
    write_gpt4all_model(models_root, "llama3-8b.gguf", &llama_payload);

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_GPT4ALL_DIRS", models_root);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let g4a = gpt4all_models(&models_log);

    assert_eq!(
        g4a.len(),
        2,
        "GPT4All plugin must surface 2 models from the fixture; got {}\nentries: {:#?}",
        g4a.len(),
        g4a
    );

    // Every entry must report Format::Gguf — that's the only format GPT4All
    // serves up.
    for m in &g4a {
        let format = m.get("format").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(
            format, "Gguf",
            "every gpt4all entry must have format=Gguf; got {:?} for {:?}",
            format, m
        );
    }

    // size_bytes must come straight from the on-disk file length so the
    // right-pane size column matches the user's actual disk usage. We pick
    // out one model and assert exactly.
    let qwen = g4a
        .iter()
        .find(|m| m.get("id_in_tool").and_then(|v| v.as_str()) == Some("qwen2-7b.gguf"))
        .expect("qwen2-7b.gguf entry must be present");
    let size = qwen
        .get("size_bytes")
        .and_then(|v| v.as_u64())
        .expect("size_bytes must be a u64");
    assert_eq!(
        size,
        qwen_payload.len() as u64,
        "size_bytes must equal the on-disk file length (no manifest involved)"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 — GPT4All not installed shows benign message (AC-G1.4).
// ---------------------------------------------------------------------------

#[test]
fn gpt4all_not_installed_shows_benign_message() {
    // MODELTAP_GPT4ALL_DIRS already pinned at /nonexistent by
    // modeltap_headless(); don't override.
    let (mut cmd, log_temp) = modeltap_headless();

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let inv = find_event(&events, "launch.inventory")
        .unwrap_or_else(|| panic!("no launch.inventory in:\n{:?}", events));
    let tool_errors = inv
        .get("tool_errors")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("tool_errors must be present and an array; got {:?}", inv));

    // NotInstalled (the benign state) MUST NOT appear in tool_errors.
    let g4a_in_errors = tool_errors.iter().any(|e| e.as_str() == Some("gpt4all"));
    assert!(
        !g4a_in_errors,
        "'gpt4all' must NOT appear in launch.inventory.tool_errors when its data dir is missing (NotInstalled is benign, not an error). Got tool_errors={:?}",
        tool_errors
    );

    // Models.log must contain zero gpt4all entries.
    let models_log = read_models_log(&log_dir);
    let g4a = gpt4all_models(&models_log);
    assert_eq!(
        g4a.len(),
        0,
        "0 models when not installed; got {}\nentries: {:#?}",
        g4a.len(),
        g4a
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 — Two configured dirs are both walked (AC-G1.5).
// Pivots from Atomic Chat's yml-parse-error scenario: GPT4All has no
// manifest layer, but it DOES support a colon-separated multi-root config
// (Python SDK + desktop app on the same host). This proves that
// `MODELTAP_GPT4ALL_DIRS="dir1:dir2"` overrides the headless /nonexistent
// default AND that both roots are walked.
// ---------------------------------------------------------------------------

#[test]
fn gpt4all_two_dirs_both_walked() {
    let temp_a = tempfile::tempdir().expect("tempdir A");
    let temp_b = tempfile::tempdir().expect("tempdir B");
    let root_a = temp_a.path();
    let root_b = temp_b.path();
    write_gpt4all_model(root_a, "alpha.gguf", b"GGUFA");
    write_gpt4all_model(root_b, "bravo.gguf", b"GGUFB");

    let joined = format!("{}:{}", root_a.display(), root_b.display());

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_GPT4ALL_DIRS", &joined);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let g4a = gpt4all_models(&models_log);

    assert_eq!(
        g4a.len(),
        2,
        "both configured dirs must be walked → 2 models (one per root); got {}\nentries: {:#?}\nMODELTAP_GPT4ALL_DIRS={joined}",
        g4a.len(),
        g4a
    );

    let ids: Vec<&str> = g4a
        .iter()
        .filter_map(|m| m.get("id_in_tool").and_then(|v| v.as_str()))
        .collect();
    assert!(
        ids.contains(&"alpha.gguf"),
        "alpha.gguf (root A) must be present; got ids={:?}",
        ids
    );
    assert!(
        ids.contains(&"bravo.gguf"),
        "bravo.gguf (root B) must be present; got ids={:?}",
        ids
    );
}
