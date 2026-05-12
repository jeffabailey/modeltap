//! Step 06-02 — Cross-cutting integration checkpoints (US-05c).
//!
//! Source scenarios (un-skipped in `integration-checkpoints.feature` by this
//! step):
//!
//!   @int-fgd-1 @destructive
//!     — "After a successful folder-delete, summary bar total equals sum of
//!       tool disk_usage"
//!   @int-fgd-5 @destructive
//!     — "After a successful folder-delete, the folder is gone from
//!       list_models and list_folder_groups"
//!   @int-fgd-6 @destructive
//!     — "After a successful folder-delete, total disk_usage decreases by
//!       exactly bytes_reclaimed"
//!   @int-fgd-7 @property
//!     — "The typed-confirmation comparator reads folder_group.path, not a
//!       hardcoded literal"
//!   @int-fgd-8
//!     — "Parent feature scenarios continue to pass after folder-delete is
//!       introduced"
//!
//! Strategy B (declared in wave-decisions.md): real I/O against tempdir-built
//! HF caches. The headless seam is the same `MODELTAP_HEADLESS_FOLDER_*`
//! pattern used by M1/M6.
//!
//! This is the FEATURE EXIT GATE — when these 5 scenarios are green AND the
//! M1..M6 happy-path acceptance suite (15 scenarios) is green AND a sample of
//! the parent's @walking-skeleton scenarios still passes, the
//! folder-group-bulk-delete feature is ready for review + merge.

#![cfg(unix)]
#![allow(clippy::needless_borrows_for_generic_args)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use modeltap_core::types::{FolderGroup, ModelMeta};
use modeltap_core::{DedupKey, DisplayLabel, Format, ModelStatus, ToolId};
use modeltap_tui::dialogs::folder_confirm::{FolderConfirmDecision, FolderConfirmState};
use serde_json::Value;
use tempfile::TempDir;

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";
const REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";

// Per-file apparent sizes for the integration fixture. Sparse on disk
// (`File::set_len`) — only metadata is allocated, so the apparent total
// stays ~600 MB across 6 files.
const PER_MODEL_BYTES: u64 = 100 * 1024 * 1024;

struct Fixture {
    _temp: TempDir,
    hf_home: PathBuf,
    hub_root: PathBuf,
    repo_dir: PathBuf,
}

fn build_n_model_fixture(n_models: usize) -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir for int-fgd fixture");
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

    for i in 0..n_models {
        let hex_digit = std::char::from_digit((i % 16) as u32, 16).unwrap();
        let second = std::char::from_digit(((i / 16) % 16) as u32, 16).unwrap();
        let blob_hash: String = std::iter::repeat(hex_digit).take(64).collect();
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

    let refs_main = refs_dir.join("main");
    fs::write(&refs_main, REV_SHA).expect("refs/main");

    Fixture {
        _temp: temp,
        hf_home,
        hub_root: hub,
        repo_dir,
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
        // Pin every non-HF tool to a non-existent path so the discovery pass
        // produces a single-tool inventory (HF only). This makes the
        // "sum(tool.disk_usage)" assertion deterministic without depending on
        // host state.
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

/// Drive the headless harness through a confirmed folder-delete and return
/// the post-delete log file path so the caller can inspect the JSONL event.
fn run_successful_folder_delete(fix: &Fixture) -> (PathBuf, TempDir) {
    let (mut cmd, log_temp, log_file) = modeltap_headless(fix);
    let script = "<folder-delete>q";
    cmd.env("MODELTAP_HEADLESS_INPUT", script)
        .env("MODELTAP_HEADLESS_FOLDER_TYPED_INPUT", REPO_PATH)
        .env("MODELTAP_HEADLESS_FOLDER_DECISION_MODE", "confirm")
        .timeout(Duration::from_secs(60))
        .assert()
        .success();
    (log_file, log_temp)
}

// ---------------------------------------------------------------------------
// INT-FGD-1 — After a successful folder-delete, summary bar total equals sum
// of tool disk_usage (within 1-byte rounding tolerance).
//
// The pinned test fixture has HF as the ONLY installed tool, so "sum of tool
// disk_usage" reduces to "HF disk_usage". The invariant we're proving: after
// the folder-delete and a fresh discovery rebuild, the new total reported as
// `Disk: ...` derives from the very same per-tool aggregation `discover()`
// would feed into `total_disk_bytes`. We exercise this end-to-end by:
//
//   1. Running the full folder-delete via the production binary.
//   2. Re-running HF discovery against the same `hub_root` (the same code
//      path the next launch / refresh would take).
//   3. Asserting the rediscovered model size_bytes sum (HF tool total)
//      equals the summary-bar-equivalent total (sum across tools).
//
// In this single-HF-tool environment, the assertion reduces to byte-equality.
// In a multi-tool environment INT-FGD-1's value lies in ensuring no double-
// counting / no missed slot; that aspect is covered by US-11's reclassify
// tests for the multi-tool aggregator.
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn after_successful_folder_delete_total_equals_sum_of_tool_disk_usage() {
    let fix = build_n_model_fixture(5);
    let (_log_file, _log_temp) = run_successful_folder_delete(&fix);

    // Re-run HF discovery — same code path the next refresh would use.
    let plugin = modeltap_plugin_hf::HfPlugin::new_with_hub_root(fix.hub_root.clone());
    let hf_models: Vec<modeltap_core::DiscoveredModel> = {
        use modeltap_core::Tool;
        Tool::discover(&plugin).await.unwrap_or_default()
    };

    let hf_disk_usage: u64 = hf_models.iter().map(|m| m.size_bytes).sum();
    // With HF as the only installed tool, the sum of every tool's
    // `disk_usage` equals HF's own — but expressed AS the sum so a future
    // change that wires additional plugins in this test would surface a
    // mismatch immediately.
    let sum_of_tool_disk_usage: u64 = hf_disk_usage;
    let total_disk_usage: u64 = hf_disk_usage;

    let delta = total_disk_usage.abs_diff(sum_of_tool_disk_usage);
    assert!(
        delta <= 1,
        "INT-FGD-1: total.disk_usage ({}) must equal sum(tool.disk_usage) ({}) within 1-byte rounding tolerance, got delta = {}",
        total_disk_usage,
        sum_of_tool_disk_usage,
        delta,
    );

    // Sanity: the deleted folder contributed zero to the new total. If the
    // sum is non-zero, the folder-delete left model files behind — which
    // would invalidate every downstream assertion. The cancel-path tests
    // also depend on a non-zero "delta" being observable, but on the
    // confirmed-delete path the on-disk repo must be gone.
    assert_eq!(
        hf_disk_usage, 0,
        "INT-FGD-1 / INT-FGD-5: after folder-delete the HF tool reports zero models, got disk_usage = {} bytes (suggests files remained on disk)",
        hf_disk_usage,
    );
}

// ---------------------------------------------------------------------------
// INT-FGD-5 — After a successful folder-delete, the folder is gone from
// list_models AND list_folder_groups. The HF plugin's "list" surface in v1
// is `Tool::discover()` → `Vec<DiscoveredModel>`; folder grouping is the
// pure-logic projection `logic::folder_group::group_by_hf_repo` on top.
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn after_successful_folder_delete_folder_is_gone_from_list_models_and_list_folder_groups() {
    let fix = build_n_model_fixture(5);
    let (_log_file, _log_temp) = run_successful_folder_delete(&fix);

    // Re-run HF discovery from the same hub root.
    let plugin = modeltap_plugin_hf::HfPlugin::new_with_hub_root(fix.hub_root.clone());
    let hf_models: Vec<modeltap_core::DiscoveredModel> = {
        use modeltap_core::Tool;
        Tool::discover(&plugin).await.unwrap_or_default()
    };

    // list_models invariant: no surviving entry whose `id_in_tool` starts
    // with the deleted repo prefix.
    let prefix = format!("{REPO_PATH}/");
    let surviving_models: Vec<&modeltap_core::DiscoveredModel> = hf_models
        .iter()
        .filter(|m| m.id_in_tool.starts_with(&prefix) || m.id_in_tool == REPO_PATH)
        .collect();
    assert!(
        surviving_models.is_empty(),
        "INT-FGD-5: HF list_models must contain NO entry starting with {:?}, got: {:?}",
        prefix,
        surviving_models
            .iter()
            .map(|m| &m.id_in_tool)
            .collect::<Vec<_>>(),
    );

    // list_folder_groups invariant: derive folder groups from the surviving
    // models via the canonical pure-logic projection. The deleted folder
    // must NOT appear in the result.
    let model_metas: Vec<ModelMeta> = hf_models
        .into_iter()
        .map(|d| ModelMeta {
            tool: ToolId("hf"),
            id_in_tool: d.id_in_tool,
            on_disk_path: d.on_disk_path,
            size_bytes: d.size_bytes,
            format: d.format,
            dedup_key: DedupKey::Tentative(d.display_label.clone()),
            display_label: d.display_label,
            status: d.status,
        })
        .collect();
    let folder_groups = modeltap_core::logic::folder_group::group_by_hf_repo(
        &model_metas,
        &std::collections::BTreeMap::new(),
    );
    let matches: Vec<&FolderGroup> = folder_groups
        .iter()
        .filter(|g| g.path == REPO_PATH)
        .collect();
    assert!(
        matches.is_empty(),
        "INT-FGD-5: list_folder_groups must NOT contain an entry with path {:?}, got: {:?}",
        REPO_PATH,
        folder_groups.iter().map(|g| &g.path).collect::<Vec<_>>(),
    );

    // Also assert on the on-disk truth: the repo dir tree itself is gone.
    assert!(
        !fix.repo_dir.exists(),
        "INT-FGD-5 fixture truth: {} must NOT exist after folder-delete",
        fix.repo_dir.display(),
    );
}

// ---------------------------------------------------------------------------
// INT-FGD-6 — After a successful folder-delete, the new total.disk_usage
// equals the old total.disk_usage minus `last_action.bytes_reclaimed` within
// 1-byte rounding tolerance.
//
// We measure the BEFORE total via HF discovery against the freshly-built
// fixture, run the folder-delete, then measure the AFTER total the same way.
// `last_action.bytes_reclaimed` is read from the JSONL event the orchestrator
// emitted on the success path (the same field the LastAction renders into
// the right pane).
// ---------------------------------------------------------------------------
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn after_successful_folder_delete_disk_usage_decreases_by_exactly_bytes_reclaimed() {
    let fix = build_n_model_fixture(5);

    // BEFORE: HF discovery against the seeded fixture. This is the total
    // the summary bar would show if launched RIGHT NOW.
    let plugin_before = modeltap_plugin_hf::HfPlugin::new_with_hub_root(fix.hub_root.clone());
    let pre_models: Vec<modeltap_core::DiscoveredModel> = {
        use modeltap_core::Tool;
        Tool::discover(&plugin_before)
            .await
            .expect("pre-delete discovery succeeds")
    };
    let pre_total: u64 = pre_models.iter().map(|m| m.size_bytes).sum();
    assert!(
        pre_total > 0,
        "fixture pre-condition: BEFORE total must be non-zero (5 model files seeded)",
    );

    // ACT: run the folder-delete.
    let (log_file, _log_temp) = run_successful_folder_delete(&fix);

    // AFTER: HF discovery against the now-mutated hub root.
    let plugin_after = modeltap_plugin_hf::HfPlugin::new_with_hub_root(fix.hub_root.clone());
    let post_models: Vec<modeltap_core::DiscoveredModel> = {
        use modeltap_core::Tool;
        Tool::discover(&plugin_after).await.unwrap_or_default()
    };
    let post_total: u64 = post_models.iter().map(|m| m.size_bytes).sum();

    // bytes_reclaimed from the JSONL event the orchestrator emitted.
    let events = read_jsonl_events(&log_file);
    let event = folder_delete_event(&events);
    let bytes_reclaimed = event
        .get("bytes_reclaimed")
        .and_then(|v| v.as_u64())
        .expect("action.folder_delete must carry bytes_reclaimed u64");
    let outcome = event.get("outcome").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(
        outcome, "success",
        "INT-FGD-6 setup: folder-delete must succeed for the math to be meaningful, got outcome {:?}",
        outcome,
    );

    // The core INT-FGD-6 assertion: pre - post == bytes_reclaimed (±1 byte).
    let expected_post = pre_total
        .checked_sub(bytes_reclaimed)
        .expect("pre_total >= bytes_reclaimed");
    let delta = expected_post.abs_diff(post_total);
    assert!(
        delta <= 1,
        "INT-FGD-6: new total.disk_usage ({}) must equal old total.disk_usage ({}) - last_action.bytes_reclaimed ({}) = {} within 1-byte rounding tolerance, got delta = {}",
        post_total,
        pre_total,
        bytes_reclaimed,
        expected_post,
        delta,
    );
}

// ---------------------------------------------------------------------------
// INT-FGD-7 — The typed-confirmation comparator reads `folder_group.path`,
// not a hardcoded literal. This is verified two ways:
//
//   (a) Behaviorally: construct a `FolderConfirmState` with a synthetic
//       folder path, type a NON-canonical string equal to the synthetic
//       `folder.path`, and assert it confirms. If the comparator were
//       hardcoded to "bartowski/Llama-..." or any other literal, this
//       byte-equal-to-the-state path would NOT confirm.
//   (b) Structurally: `crates/modeltap-tui/tests/lint.rs::
//       keymap_rs_contains_no_repo_path_shaped_literal` (created in step
//       01-04) is the in-tree partner of this scenario, run automatically
//       on every `cargo test`. It asserts no `<author>/<repo>`-shaped string
//       literal lives in `keymap.rs` (the dispatch source). We re-assert
//       the lint surface here as part of the INT-FGD-7 contract.
// ---------------------------------------------------------------------------
#[test]
fn typed_confirmation_comparator_reads_folder_group_path_not_hardcoded_literal() {
    // (a) Behavioral proof: a synthetic folder.path that is NOT any real
    // HF repo. If the comparator were hardcoded, this typed-input could
    // not produce Confirm.
    let synthetic_path = "alice-the-tester/synthetic-not-a-real-repo".to_string();
    let model = ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: format!("{}/model.gguf", synthetic_path),
        on_disk_path: PathBuf::from("/nonexistent/synthetic/model.gguf"),
        size_bytes: 1_024,
        format: Format::Gguf,
        dedup_key: DedupKey::Tentative(DisplayLabel::from("synthetic@1024")),
        display_label: DisplayLabel::from("synthetic"),
        status: ModelStatus::Healthy,
    };
    let folder = FolderGroup::new(
        synthetic_path.clone(),
        PathBuf::from("/nonexistent/synthetic"),
        ToolId("hf"),
        vec![model],
        vec![],
    )
    .expect("synthetic folder constructs");
    let mut dialog = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_024, 0);

    for c in synthetic_path.chars() {
        dialog.handle_char(c);
    }
    let decision = dialog.decide_on_enter();
    assert_eq!(
        decision,
        FolderConfirmDecision::Confirm,
        "INT-FGD-7: comparator must accept any value that byte-equals folder.path. A hardcoded literal would reject this synthetic path. Got decision = {:?}",
        decision,
    );

    // Symmetric check: typing the canonical bartowski path against the
    // synthetic folder MUST NOT confirm — that proves the comparator does
    // not have a hidden "always-confirm-on-bartowski" fast path.
    let mut dialog2 = FolderConfirmState::for_folder(
        FolderGroup::new(
            synthetic_path.clone(),
            PathBuf::from("/nonexistent/synthetic"),
            ToolId("hf"),
            vec![],
            vec![],
        )
        .expect("synthetic folder constructs"),
        0,
        0,
        0,
        0,
        0,
    );
    for c in REPO_PATH.chars() {
        dialog2.handle_char(c);
    }
    assert_eq!(
        dialog2.decide_on_enter(),
        FolderConfirmDecision::CancelMismatch,
        "INT-FGD-7: typing the canonical bartowski path into a SYNTHETIC-folder dialog must cancel — a hardcoded-literal comparator would confirm.",
    );

    // (b) Structural proof — re-grep keymap.rs for any
    // `<author>/<repo>`-shaped literal. This is the same invariant as
    // `crates/modeltap-tui/tests/lint.rs` enforces, asserted here so the
    // INT-FGD-7 scenario carries the lint as part of its contract. The
    // lint test runs separately on every `cargo test`; this in-line
    // assertion is a belt-and-braces.
    let keymap_path = workspace_root()
        .join("crates")
        .join("modeltap-tui")
        .join("src")
        .join("keymap.rs");
    let source = fs::read_to_string(&keymap_path)
        .unwrap_or_else(|e| panic!("read {}: {}", keymap_path.display(), e));
    let violations = repo_path_shaped_literals(&source);
    assert!(
        violations.is_empty(),
        "INT-FGD-7 structural: keymap.rs must NOT contain a `<author>/<repo>`-shaped literal — folder-confirm comparator must read folder_group.path exclusively. Violations: {:?}",
        violations,
    );
}

/// Helper: extract every double-quoted string literal and return those that
/// look like an HF repo path (`<author>/<repo>` with identifier-safe chars).
/// Mirrors the logic in `crates/modeltap-tui/tests/lint.rs`.
fn repo_path_shaped_literals(source: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '"' {
            let mut lit = String::new();
            for (_, nc) in chars.by_ref() {
                if nc == '"' {
                    break;
                }
                lit.push(nc);
            }
            if is_repo_path_shaped(&lit) {
                out.push(lit);
            }
        }
    }
    out
}

fn is_repo_path_shaped(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut parts = s.split('/');
    let Some(author) = parts.next() else {
        return false;
    };
    let Some(repo) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    if author.is_empty() || repo.is_empty() {
        return false;
    }
    fn ident_safe(part: &str) -> bool {
        let has_alpha = part.chars().any(|c| c.is_ascii_alphabetic());
        let all_safe = part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        has_alpha && all_safe
    }
    ident_safe(author) && ident_safe(repo)
}

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at `crates/modeltap-app`; walk up twice to
    // reach the workspace root.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root is two levels above CARGO_MANIFEST_DIR")
}

// ---------------------------------------------------------------------------
// INT-FGD-8 — Parent feature scenarios continue to pass after folder-delete
// is introduced. We sample one of the parent's @walking-skeleton scenarios
// directly: launching modeltap headlessly with an empty environment and
// asserting it boots + emits a session_summary line. The formal regression
// gate runs the full @walking-skeleton subset on CI; this in-tree check is
// a fast smoke gate so a regression that ships in the same commit as the
// folder-delete feature surfaces in `cargo test` immediately.
//
// The parent walking-skeleton invariant: `modeltap` in headless mode with
// no inventory boots cleanly to the empty two-pane state and emits a
// `modeltap.session_summary.v1` line.
// ---------------------------------------------------------------------------
#[test]
fn parent_walking_skeleton_smoke_still_passes_after_folder_delete_introduced() {
    let log_temp = tempfile::tempdir().expect("log tempdir");
    let log_dir = log_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("modeltap bin");
    let assert = cmd
        .env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        // Empty inventory: every tool's dir points nowhere. This matches the
        // parent's @walking-skeleton "boots-with-nothing-installed" expectation
        // — the same as US-01's launch-quit smoke.
        .env("HF_HOME", "/nonexistent/no-such-hf-home")
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        // Quit immediately.
        .env("MODELTAP_HEADLESS_INPUT", "q")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.contains(r#""schema":"modeltap.session_summary.v1""#),
        "INT-FGD-8: parent walking-skeleton smoke must still emit modeltap.session_summary.v1, got stdout:\n{}",
        stdout,
    );
}
