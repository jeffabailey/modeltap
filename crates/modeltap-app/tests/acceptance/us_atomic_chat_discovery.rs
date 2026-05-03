//! Acceptance tests for the Atomic Chat (Jan-fork) plugin — the 5th real
//! production plugin. Validates the US-18 plugin extensibility contract in
//! production: a contributor adding a new tool needed ZERO changes to
//! `crates/modeltap-core/src/`. The architecture lint
//! (`tests/architecture.rs`) certifies that statically; THIS file certifies
//! it dynamically by driving the production binary against synthetic
//! Atomic Chat fixtures.
//!
//! Atomic Chat stores models under (per host OS):
//!   - macOS:     `~/Library/Application Support/Atomic Chat/data/llamacpp/models/<id>/`
//!   - Linux/WSL: `~/.config/Atomic Chat/data/llamacpp/models/<id>/`
//!
//! Each `<id>/` carries a `model.yml` (name, size_bytes, paths) plus a
//! `model.gguf`. MLX (`<data>/mlx/models/`) is OUT OF SCOPE for v1
//! (intake C3 / ADR-004 OQ-3) so `accepted_formats()` is `[Gguf]`.
//!
//! Behaviors covered (acceptance criteria):
//! 1. Atomic Chat models are discovered — synthetic 2-model fixture; both
//!    appear in models.log under tool=`Atomic Chat` with the YAML's sizes.
//! 2. Atomic Chat not installed shows benign message — fixture path absent;
//!    `Atomic Chat` MUST NOT appear in `launch.inventory.tool_errors` (it's
//!    NotInstalled, which is benign per US-15 AC-4).
//! 3. `model.yml` parse error doesn't crash discovery — fixture has 1 valid
//!    manifest plus 1 truncated manifest; the valid model surfaces normally
//!    and the broken one surfaces as a `Corrupt` entry rather than aborting
//!    the scan.
//!
//! Each test enters through the modeltap binary driving port. Assertions are
//! made against the launch.log + models.log JSONL events (driven port
//! boundary).

use std::path::Path;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Construct a `modeltap` headless command with all five plugin env-vars
/// pinned at non-existent paths so this test isolates from the developer's
/// real `$HOME`. Tests opt INTO Atomic Chat by overriding
/// `MODELTAP_ATOMIC_CHAT_DIRS`.
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

fn atomic_chat_models(models_log: &[Value]) -> Vec<&Value> {
    models_log
        .iter()
        .filter(|m| m.get("tool").and_then(|v| v.as_str()) == Some("Atomic Chat"))
        .collect()
}

/// Build a synthetic Atomic Chat tree at `root`. Each `(id, size)` pair
/// becomes a `<root>/<id>/{model.yml,model.gguf}` directory with a
/// well-formed YAML manifest. Returns `root` for ergonomic chaining.
fn write_atomic_chat_model(root: &Path, id: &str, size: u64) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).expect("create model dir");
    let yaml = format!(
        "embedding: false\nmodel_path: llamacpp/models/{id}/model.gguf\nname: {id}\nsize_bytes: {size}\n"
    );
    std::fs::write(dir.join("model.yml"), yaml).expect("write model.yml");
    std::fs::write(dir.join("model.gguf"), b"GGUF\x03\x00\x00\x00").expect("write model.gguf");
}

// ---------------------------------------------------------------------------
// Scenario 1 — Atomic Chat models are discovered.
// ---------------------------------------------------------------------------

#[test]
fn atomic_chat_models_are_discovered() {
    let temp = tempfile::tempdir().expect("tempdir for fixture");
    let models_root = temp.path();
    write_atomic_chat_model(models_root, "Qwen3-7B-Q4_K_M", 4_700_000_000);
    write_atomic_chat_model(models_root, "Llama-3-8B-Q4_K_M", 4_400_000_000);

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_ATOMIC_CHAT_DIRS", models_root);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let ac = atomic_chat_models(&models_log);

    assert_eq!(
        ac.len(),
        2,
        "Atomic Chat plugin must surface 2 models from the fixture; got {}\nentries: {:#?}",
        ac.len(),
        ac
    );

    // Every entry must report Format::Gguf (MLX out of scope for v1).
    for m in &ac {
        let format = m.get("format").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(
            format, "Gguf",
            "every Atomic Chat entry must have format=Gguf in v1; got {:?} for {:?}",
            format, m
        );
    }

    // The YAML's `size_bytes` must propagate verbatim to models.log so the
    // right-pane size column matches what Atomic Chat itself shows. We pick
    // out one of the two models and assert exactly.
    let qwen = ac
        .iter()
        .find(|m| m.get("id_in_tool").and_then(|v| v.as_str()) == Some("Qwen3-7B-Q4_K_M"))
        .expect("Qwen entry must be present");
    let size = qwen
        .get("size_bytes")
        .and_then(|v| v.as_u64())
        .expect("size_bytes must be a u64");
    assert_eq!(
        size, 4_700_000_000,
        "size_bytes must come straight from model.yml"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 — Atomic Chat not installed shows benign message.
// ---------------------------------------------------------------------------

#[test]
fn atomic_chat_not_installed_shows_benign_message() {
    // MODELTAP_ATOMIC_CHAT_DIRS already pinned at /nonexistent by
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
    let ac_in_errors = tool_errors
        .iter()
        .any(|e| e.as_str() == Some("Atomic Chat"));
    assert!(
        !ac_in_errors,
        "'Atomic Chat' must NOT appear in launch.inventory.tool_errors when its data dir is missing (NotInstalled is benign, not an error). Got tool_errors={:?}",
        tool_errors
    );

    // Models.log must contain zero Atomic Chat entries.
    let models_log = read_models_log(&log_dir);
    let ac = atomic_chat_models(&models_log);
    assert_eq!(
        ac.len(),
        0,
        "0 models when not installed; got {}\nentries: {:#?}",
        ac.len(),
        ac
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 — model.yml parse error doesn't crash discovery.
// ---------------------------------------------------------------------------

#[test]
fn model_yml_parse_error_does_not_crash_discovery() {
    let temp = tempfile::tempdir().expect("tempdir for fixture");
    let models_root = temp.path();

    // One healthy model.
    write_atomic_chat_model(models_root, "good-model", 1_024);

    // One broken model — truncated model.yml.
    let bad_dir = models_root.join("bad-model");
    std::fs::create_dir_all(&bad_dir).expect("create bad-model dir");
    std::fs::write(
        bad_dir.join("model.yml"),
        b"name: bad-model\nsize_byt", // truncated mid-key
    )
    .expect("write truncated yml");

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_ATOMIC_CHAT_DIRS", models_root);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success(); // the binary did NOT crash

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let ac = atomic_chat_models(&models_log);

    assert_eq!(
        ac.len(),
        2,
        "expected 1 healthy + 1 corrupt entry (the broken model.yml is surfaced as Corrupt, not silently dropped); got {}\nentries: {:#?}",
        ac.len(),
        ac
    );

    // The good model must be Healthy + Gguf.
    let good = ac
        .iter()
        .find(|m| m.get("id_in_tool").and_then(|v| v.as_str()) == Some("good-model"))
        .expect("good-model must be present");
    assert_eq!(
        good.get("status").and_then(|v| v.as_str()),
        Some("Healthy"),
        "good model must be Healthy; got {:?}",
        good
    );
    assert_eq!(
        good.get("format").and_then(|v| v.as_str()),
        Some("Gguf"),
        "good model must be Gguf"
    );

    // The bad model must surface as Corrupt + Other.
    let bad = ac
        .iter()
        .find(|m| m.get("id_in_tool").and_then(|v| v.as_str()) == Some("bad-model"))
        .expect("bad-model must be present as a Corrupt entry");
    assert_eq!(
        bad.get("status").and_then(|v| v.as_str()),
        Some("Corrupt"),
        "broken model must be Corrupt; got {:?}",
        bad
    );
}
