//! M6 — KPI guardrail acceptance tests for folder-group-bulk-delete
//! (US-05c, step 06-01).
//!
//! Source scenarios (un-skipped in `folder-group-delete.feature` by this step):
//!   @milestone-6 @kpi-instrumentation @destructive
//!     — "Keystroke count for a 20-file folder is bounded and independent of file count"
//!   @milestone-6 @ac-8 @property @kpi-instrumentation
//!     — "Every aborted typed-confirmation results in zero filesystem mutations"
//!
//! K-FGD-2 (outcome-kpis.md): keystroke_count <= 40 for a 20-file folder AND
//! identical bound for a 5-file companion case — keystroke_count must be
//! INDEPENDENT of file_count. The whole point of the feature is collapsing
//! the per-file [d]+typed-confirm loop (~22 keys * N_files) into a single
//! typed-confirm pass (~30 chars + Enter).
//!
//! K-FGD-3 (outcome-kpis.md): mis-target rate guardrail — ANY input that is
//! not the byte-exact `folder_group.path` MUST yield `outcome ==
//! "cancelled_mismatch"`, `outcomes_count == 0`, and a byte-identical
//! filesystem pre/post (DirManifest).
//!
//! Strategy B (declared in `wave-decisions.md`): real I/O against tempdir-
//! built HF caches. The headless seam (MODELTAP_HEADLESS_FOLDER_TYPED_INPUT /
//! MODELTAP_HEADLESS_FOLDER_DECISION_MODE) drives the orchestrator's state-
//! machine entry directly per step 02-01's pattern; the production wiring
//! (cursor -> dialog -> Enter/Esc keystroke stream) is incremental.
//!
//! Per D3 (wave-decisions.md "Keystroke-count bound"): keystroke_count is
//! computed from dialog-open to Enter — Shift+F itself is excluded because
//! it transitions FROM main view TO dialog state. Backspace and Ctrl+W
//! count toward the total. In the headless harness the count is derived
//! from `typed_input.chars().count() + 1` (the +1 is the final Enter or Esc),
//! which matches what the production TUI's dialog state machine accumulates
//! when the user types each character exactly once and presses Enter.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

use super::dir_manifest::DirManifest;

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";
const REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";

// Per-file apparent sizes for the multi-file fixtures. Sparse on disk
// (`File::set_len`) — only metadata is allocated.
const PER_MODEL_BYTES: u64 = 100 * 1024 * 1024; // 100 MB per quant

struct Fixture {
    _temp: TempDir,
    hf_home: PathBuf,
    hub_root: PathBuf,
    /// On-disk repo dir — kept for parity with the M2/WS fixtures so future
    /// scenarios can assert on it without reshape; currently unused in M6.
    #[allow(dead_code)]
    repo_dir: PathBuf,
    /// Number of model files seeded (variable per scenario: 5 or 20).
    model_file_count: usize,
}

/// Build a multi-file HF fixture with `n_models` quant variants. Mirrors the
/// `devon-hf-allunique` builder in `folder_delete_walking_skeleton.rs` but
/// parameterised over the file count so the 5-file and 20-file companion
/// cases share builder code.
fn build_n_model_fixture(n_models: usize) -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let hf_home = root.join(".cache").join("huggingface");
    let hub = hf_home.join("hub");
    let repo_dir = hub.join(REPO_DIR_NAME);
    let blobs_dir = repo_dir.join("blobs");
    let snap_dir = repo_dir.join("snapshots").join(REV_SHA);
    let refs_dir = repo_dir.join("refs");
    fs::create_dir_all(&blobs_dir).expect("blobs dir");
    fs::create_dir_all(&snap_dir).expect("snap dir");
    fs::create_dir_all(&refs_dir).expect("refs dir");

    // Seed `n_models` blob + snapshot-symlink pairs. Each blob hash is a
    // 64-char hex string keyed by index; each snapshot is named
    // `<repo>-Q<idx>.gguf` so the user-visible filenames are unique and
    // recognisable.
    for i in 0..n_models {
        let hex_digit = std::char::from_digit((i % 16) as u32, 16).unwrap();
        let blob_hash: String = std::iter::repeat(hex_digit).take(64).collect();
        // Make hashes distinct even past i=15 by mixing in a second digit.
        let second = std::char::from_digit(((i / 16) % 16) as u32, 16).unwrap();
        let blob_hash: String = blob_hash
            .chars()
            .enumerate()
            .map(|(pos, c)| if pos == 0 { second } else { c })
            .collect();
        let blob_path = blobs_dir.join(&blob_hash);
        write_sparse(&blob_path, PER_MODEL_BYTES);

        let snap_name = format!("Llama-3.2-1B-Instruct-Q{:02}.gguf", i);
        let snap_path = snap_dir.join(&snap_name);
        symlink(
            PathBuf::from("..")
                .join("..")
                .join("blobs")
                .join(&blob_hash),
            &snap_path,
        )
        .unwrap_or_else(|e| panic!("symlink {}: {e}", snap_name));
    }

    fs::write(refs_dir.join("main"), REV_SHA).expect("refs/main");

    Fixture {
        _temp: temp,
        hf_home,
        hub_root: hub,
        repo_dir,
        model_file_count: n_models,
    }
}

fn write_sparse(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    let file = fs::File::create(path).unwrap_or_else(|e| panic!("create {}: {e}", path.display()));
    file.set_len(size)
        .unwrap_or_else(|e| panic!("set_len({}) on {}: {e}", size, path.display()));
}

fn modeltap_headless(fix: &Fixture) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("log tempdir");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("modeltap bin");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("HF_HOME", &fix.hf_home)
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("MODELTAP_HEADLESS_FOLDER_PATH", REPO_PATH);
    (cmd, log_dir_temp, log_file)
}

fn read_jsonl_events(log_file: &Path) -> Vec<Value> {
    let content = fs::read_to_string(log_file).unwrap_or_default();
    content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

fn folder_delete_event(events: &[Value]) -> &Value {
    let v: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some("action.folder_delete"))
        .collect();
    assert_eq!(
        v.len(),
        1,
        "expected exactly 1 action.folder_delete event, got {}: {:#?}",
        v.len(),
        v
    );
    v[0]
}

/// Run a confirmed (Enter) folder-delete with the byte-exact path and capture
/// the JSONL `action.folder_delete` event's `keystroke_count`. Used by the
/// 20-file scenario AND its 5-file companion case to assert file-count
/// independence (D3 / K-FGD-2).
fn run_happy_path_and_capture_keystrokes(fix: &Fixture) -> (u64, Value) {
    let (mut cmd, _log_temp, log_file) = modeltap_headless(fix);
    let script = "<folder-delete>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", REPO_PATH)
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "confirm")
        .timeout(Duration::from_secs(60))
        .assert()
        .success();

    let events = read_jsonl_events(&log_file);
    let event = folder_delete_event(&events).clone();
    let keystrokes = event
        .get("keystroke_count")
        .and_then(|v| v.as_u64())
        .expect("action.folder_delete must carry keystroke_count u64");
    (keystrokes, event)
}

// ---------------------------------------------------------------------------
// M6.1 — K-FGD-2: keystroke count for a 20-file folder is bounded and
// independent of file_count. The 20-file run and the 5-file companion run
// MUST record the same keystroke_count, both <= 40.
// ---------------------------------------------------------------------------
#[test]
fn keystroke_count_for_20file_folder_is_bounded_and_file_count_independent() {
    // 20-file fixture: bartowski/Llama-3.2-1B-Instruct-GGUF with 20 quant
    // variants (per the M6 scenario text).
    let fix_20 = build_n_model_fixture(20);
    assert_eq!(fix_20.model_file_count, 20);

    let (keystrokes_20, event_20) = run_happy_path_and_capture_keystrokes(&fix_20);
    assert_eq!(
        event_20.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "20-file happy path must succeed, got {}",
        event_20
    );
    assert!(
        keystrokes_20 <= 40,
        "K-FGD-2: 20-file keystroke_count must be <= 40, got {} (event: {})",
        keystrokes_20,
        event_20
    );

    // 5-file companion case: same repo path, same typed-confirm flow, but
    // only 5 model files in the on-disk fixture. The keystroke_count MUST
    // be IDENTICAL to the 20-file case — the whole point of K-FGD-2 is that
    // the dialog UX is O(1) in file_count.
    let fix_5 = build_n_model_fixture(5);
    assert_eq!(fix_5.model_file_count, 5);

    let (keystrokes_5, event_5) = run_happy_path_and_capture_keystrokes(&fix_5);
    assert_eq!(
        event_5.get("outcome").and_then(|v| v.as_str()),
        Some("success"),
        "5-file companion happy path must succeed, got {}",
        event_5
    );
    assert!(
        keystrokes_5 <= 40,
        "K-FGD-2: 5-file keystroke_count must be <= 40, got {} (event: {})",
        keystrokes_5,
        event_5
    );

    // The defining K-FGD-2 invariant: keystroke_count does NOT scale with
    // the number of files in the folder. Byte-equality across the two runs
    // is the strongest possible expression of "independent of file_count".
    assert_eq!(
        keystrokes_20, keystrokes_5,
        "K-FGD-2: keystroke_count must be identical across the 20-file ({}) and 5-file ({}) cases — independence of file_count is the headline property of this feature",
        keystrokes_20, keystrokes_5
    );

    // Sanity: the count should reflect typing the 33-char REPO_PATH plus the
    // final Enter (1) — well below the 40-key budget but greater than 30.
    // This catches a regression where keystroke_count gets silently zeroed
    // or hard-coded to a constant <= 40 (Testing Theater: tautology).
    assert!(
        keystrokes_20 >= REPO_PATH.chars().count() as u64,
        "K-FGD-2: keystroke_count ({}) must be >= length of typed path ({}) — otherwise the counter is not actually counting typed characters",
        keystrokes_20,
        REPO_PATH.chars().count()
    );
}

// ---------------------------------------------------------------------------
// M6.2 — K-FGD-3: every aborted typed-confirmation produces zero filesystem
// mutations. Concrete inputs span the failure modes called out in the
// roadmap (D6): wrong prefix, wrong case, extra char, missing char, trailing
// slash (M2 covers trailing-slash explicitly; included here for completeness
// across the K-FGD-3 invariant space).
// ---------------------------------------------------------------------------
#[test]
fn every_aborted_typed_confirmation_yields_zero_filesystem_mutation() {
    let mismatched_inputs = [
        "wrong/prefix",                           // wrong author
        "BARTOWSKI/LLAMA-3.2-1B-INSTRUCT-GGUF",   // wrong case
        "bartowski/Llama-3.2-1B-Instruct-GGUF-X", // extra char (suffix)
        "bartowski/Llama-3.2-1B-Instruct-GGU",    // missing trailing char
        "bartowski/Llama-3.2-1B-Instruct-GGUF/",  // trailing slash
        "bartowski",                              // missing /<repo>
        "",                                       // empty
    ];

    // Use a 5-file fixture for speed — the K-FGD-3 invariant is about
    // filesystem byte-identity and outcomes_count, not file_count.
    for input in mismatched_inputs {
        let fix = build_n_model_fixture(5);
        let pre = DirManifest::snapshot(&fix.hub_root);

        let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
        let script = "<folder-delete>q";
        cmd.env("MODELTAP_HEADLESS_INPUT", script)
            .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", input)
            .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "enter")
            .timeout(Duration::from_secs(30))
            .assert()
            .success();

        // Byte-identical HF cache pre/post — the destructive plugin call
        // was never reached because the dialog's typed-confirm comparator
        // returned CancelMismatch.
        let post = DirManifest::snapshot(&fix.hub_root);
        assert_eq!(
            pre, post,
            "K-FGD-3: input {:?} != folder.path -> HF cache must be byte-identical pre/post",
            input
        );

        let events = read_jsonl_events(&log_file);
        let event = folder_delete_event(&events);
        assert_eq!(
            event.get("outcome").and_then(|v| v.as_str()),
            Some("cancelled_mismatch"),
            "K-FGD-3: input {:?} != folder.path -> outcome must be cancelled_mismatch, got {}",
            input,
            event
        );
        assert_eq!(
            event.get("outcomes_count").and_then(|v| v.as_u64()),
            Some(0),
            "K-FGD-3: input {:?} -> zero DeleteOutcomes produced",
            input
        );
        // No DeleteOutcome means files_removed is zero too. files_total is
        // zero on the cancel path because the plan was never built.
        assert_eq!(
            event.get("files_removed").and_then(|v| v.as_u64()),
            Some(0),
            "K-FGD-3: files_removed must be 0 on cancellation",
        );
    }
}

// ---------------------------------------------------------------------------
// Schema completeness: the JSONL action.folder_delete event MUST carry
// keystroke_count as a numeric field on every emission path (success,
// cancel, refusal). The M6 scenarios depend on this — a regression that
// drops the field on the cancel paths would let the @property test pass
// vacuously (no field to disagree about).
// ---------------------------------------------------------------------------
#[test]
fn keystroke_count_present_on_both_success_and_cancel_paths() {
    // Success path.
    let fix = build_n_model_fixture(5);
    let (mut cmd, _log_temp, log_file) = modeltap_headless(&fix);
    let script = "<folder-delete>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", REPO_PATH)
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "confirm")
        .timeout(Duration::from_secs(60))
        .assert()
        .success();
    let events = read_jsonl_events(&log_file);
    let event = folder_delete_event(&events);
    assert!(
        event.get("keystroke_count").is_some(),
        "schema: keystroke_count must be present on success path event: {}",
        event
    );
    assert!(
        event
            .get("keystroke_count")
            .and_then(|v| v.as_u64())
            .is_some(),
        "schema: keystroke_count must be numeric (u64), got: {}",
        event
    );

    // Cancel path (mismatch).
    let fix2 = build_n_model_fixture(5);
    let (mut cmd2, _log_temp2, log_file2) = modeltap_headless(&fix2);
    cmd2.env("MODELTAP_HEADLESS_INPUT", "<folder-delete>q")
        .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", "wrong/repo")
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "enter")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();
    let events2 = read_jsonl_events(&log_file2);
    let event2 = folder_delete_event(&events2);
    assert!(
        event2.get("keystroke_count").is_some(),
        "schema: keystroke_count must be present on cancel path event: {}",
        event2
    );

    // Cancel path (escape).
    let fix3 = build_n_model_fixture(5);
    let (mut cmd3, _log_temp3, log_file3) = modeltap_headless(&fix3);
    cmd3.env("MODELTAP_HEADLESS_INPUT", "<folder-delete>q")
        .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", REPO_PATH)
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "esc")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();
    let events3 = read_jsonl_events(&log_file3);
    let event3 = folder_delete_event(&events3);
    assert!(
        event3.get("keystroke_count").is_some(),
        "schema: keystroke_count must be present on esc-cancel path event: {}",
        event3
    );
}
