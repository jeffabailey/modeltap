//! Acceptance tests for US-U8: `[All Unified]` empty-state guidance.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u8`. AC-U8.1, AC-U8.2, AC-U8.3.
//!
//! Two empty-state branches in `render::all_unified`:
//!
//!   - `collect_unified_rows` empty AND `state.hash_state.is_complete()` →
//!     onboarding text inviting the user to find a "=" row and press [u].
//!   - `collect_unified_rows` empty AND `is_complete()` is false (jobs still
//!     pending OR no jobs queued) → "Hashing in progress" message instead of
//!     the onboarding text. AC-U8.2 (honest UI) — don't tell the user
//!     "no models" when we haven't finished checking.
//!
//! Step 06-01 lands these tests live (not `#[ignore]`d) — the production
//! branches are what flip them green.
//!
//! Tags: @us-u8 @cross-artifact

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Mirror of `us_u2::build_unique_only_fixture` — one tool with one unique
/// blob. Hashing this fixture to completion produces zero unified groups so
/// the right-pane `[All Unified]` view falls into the empty branch with
/// `hash_state.is_complete() == true`.
struct UniqueFixture {
    _temp: TempDir,
    ollama_dir: PathBuf,
}

fn build_unique_only_fixture() -> UniqueFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("create blobs");
    fs::write(
        blobs.join("sha256-3333333333333333333333333333333333333333333333333333333333333333"),
        vec![0xAAu8; 4096],
    )
    .expect("write unique blob");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("solo");
    fs::create_dir_all(&manifest_dir).expect("create manifest");
    let manifest = r#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:3333333333333333333333333333333333333333333333333333333333333333","size":412},"layers":[{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:3333333333333333333333333333333333333333333333333333333333333333","size":4096}]}"#;
    fs::write(manifest_dir.join("7b"), manifest).expect("write manifest");
    UniqueFixture {
        _temp: temp,
        ollama_dir,
    }
}

/// Build a `Command` configured for headless mode pointing at the unique
/// fixture's ollama dir; every other tool is wired to a non-existent path so
/// the only installed tool is ollama. The slot list is
/// `["Atomic Chat", "gpt4all", "hf", "lm-studio", "ollama", "[All Unified]"]`
/// — ollama is the only one with `is_installed() == true`.
fn modeltap_headless_unique(fix: &UniqueFixture) -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", &fix.ollama_dir)
        .env("HF_HOME", "/nonexistent/no-such-hf")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    (cmd, temp)
}

/// Build a `Command` configured for headless mode where EVERY tool dir is
/// non-existent. There are no real tool views with installed status, so the
/// hash pool has zero jobs queued (`hash_state.total == 0` →
/// `is_complete() == false`). Default selection lands on slot index 0 — and
/// because no real tool is installed, `new_with_default_selection` falls back
/// to index 0 which is the synthetic `[All Unified]` slot is appended last
/// AFTER the (existing) un-installed real tools, so we still need to navigate
/// to it. The slot order is alphabetical-by-ToolId then synthetic appended:
///   ["Atomic Chat", "gpt4all", "hf", "lm-studio", "ollama", "[All Unified]"]
/// → 5 `<right>` keystrokes from idx=0 to idx=5.
fn modeltap_headless_empty() -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("HF_HOME", "/nonexistent/no-such-hf")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
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

// ---------------------------------------------------------------------------
// AC-U8.1 + AC-U8.3: empty + hashing complete → onboarding guidance text.
// ---------------------------------------------------------------------------

#[test]
fn empty_state_with_hashing_complete_shows_onboarding_guidance() {
    let fix = build_unique_only_fixture();
    let (mut cmd, _log_temp) = modeltap_headless_unique(&fix);
    // Slot order with this fixture (alphabetical by ToolId, synthetic last):
    //   ["Atomic Chat", "gpt4all", "hf", "lm-studio", "ollama", "[All Unified]"]
    // Default selection: alphabetically-first INSTALLED tool. Only ollama is
    // installed (idx=4). One `<right>` keystroke advances to idx=5 ([All
    // Unified]). After `<hash-complete>` the single unique-blob job is
    // complete, so `is_complete() == true` AND `collect_unified_rows` is
    // empty (no duplicates → no unified groups).
    let script = "<hash-complete><right>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));

    // AC-U8.1 + AC-U8.3: the empty + hash-complete branch must surface the
    // onboarding text. We assert on a stable substring (`No models are
    // unified yet`) plus an actionable hint (`press [u]`) so a renderer
    // change that only ships half the message still flags as a regression.
    assert!(
        frame.contains("No models are unified yet"),
        "AC-U8.1: empty + hash-complete must show 'No models are unified yet' \
         guidance in the right pane; got frame:\n{}",
        frame
    );
    // The right pane wraps the long onboarding sentence at its inner width
    // (~80 cols), so `press` may sit at the end of one wrapped line and
    // `[u]` at the start of the next. Assert each substring independently
    // so the actionable-hint contract survives wrapping at any column.
    assert!(
        frame.contains("[u] to unify"),
        "AC-U8.3: onboarding text must include the actionable '[u] to unify' \
         hint so the user knows the next step; got frame:\n{}",
        frame
    );
    assert!(
        frame.contains("press"),
        "AC-U8.3: onboarding text must include the verb 'press' before the \
         '[u]' hint; got frame:\n{}",
        frame
    );
    // Honesty check: the hashing-in-progress message MUST NOT appear when we
    // have actually finished hashing — that would re-introduce the AC-U8.2
    // confusion case the spec explicitly forbids.
    assert!(
        !frame.contains("Hashing in progress"),
        "AC-U8.2 inverse: post-hash-complete frame must NOT show the \
         'Hashing in progress' message; got frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U8.2: empty + hashing in progress → "Hashing in progress" message.
// ---------------------------------------------------------------------------

#[test]
fn empty_state_while_hashing_in_progress_shows_hashing_message() {
    let (mut cmd, _log_temp) = modeltap_headless_empty();
    // No real tool installed → `hash_state.total == 0` and
    // `is_complete() == false` (the predicate requires `total > 0`). Slot
    // order is the five un-installed real tools followed by the synthetic
    // `[All Unified]` slot at idx=5. Default selection lands on slot idx=0
    // (no tool is `is_installed()` so the constructor falls back to 0).
    // Five `<right>` keystrokes advance from idx=0 to idx=5.
    //
    // No `<hash-complete>` — the empty install has zero hash jobs queued,
    // so `is_complete()` is permanently false. This is the deterministic
    // shape of "we have not finished checking" that AC-U8.2 forbids
    // collapsing into the "no models" message.
    let script = "<right><right><right><right><right>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));

    // Sanity: we are actually on the [All Unified] slot. The right-pane
    // title is unique to the synthetic-slot dispatch.
    assert!(
        frame.contains("[All Unified]"),
        "precondition: navigation must land on the [All Unified] slot so \
         the right pane dispatches to render::all_unified; got frame:\n{}",
        frame
    );
    // AC-U8.2: while hashing has NOT completed (here: no jobs queued, so
    // `is_complete()` is false), the right pane must show the
    // 'Hashing in progress' message — NOT the onboarding "No models are
    // unified yet" copy. The honest-UI rule: don't tell the user there
    // are no unified models when we have not finished checking.
    assert!(
        frame.contains("Hashing in progress"),
        "AC-U8.2: empty + hash-in-progress must show 'Hashing in progress' \
         message; got frame:\n{}",
        frame
    );
    assert!(
        !frame.contains("No models are unified yet"),
        "AC-U8.2: pre-hash-complete frame must NOT show the onboarding \
         'No models are unified yet' message — that would lie to the user \
         while we are still checking; got frame:\n{}",
        frame
    );
}
