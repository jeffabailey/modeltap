//! Acceptance tests for US-U7: `[All Unified]` pseudo-tool slot in left pane.
//!
//! Per `docs/feature/cross-tool-model-unify/distill/features/master-acceptance.feature`
//! tagged `@us-u7`. AC-U7.1..AC-U7.6 + AC-CONS-2.
//!
//! Wired green at step 04-03:
//!   - `domain::synthetic_slot::SyntheticSlot::AllUnified` exists.
//!   - `AppState::append_all_unified_slot` appends a synthetic slot at the
//!     end of `left_pane_slots` (called from `build_app_state` in main.rs).
//!   - `render::all_unified` renders the right-pane filtered view.
//!   - `render::left_pane::format_synthetic_row` derives the badge count from
//!     the SAME `collect_unified_rows` source as the right-pane footer
//!     (AC-CONS-2 single source of truth).
//!
//! Tags: @us-u7 @cross-artifact

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

fn modeltap_headless_at(ollama: &PathBuf, hf: &PathBuf) -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let log_dir = temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("log dir");
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
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
    (cmd, temp)
}

fn frame_text(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_pre_unified_two_tool(temp: &TempDir) -> (PathBuf, PathBuf) {
    // Two tools sharing one inode (already unified). After hashing the
    // single shared model produces glyph "#"; the [All Unified] count is 1.
    let root = temp.path().to_path_buf();
    let payload = vec![0x55u8; 4096];

    let ollama_dir = root.join(".ollama").join("models");
    let blobs = ollama_dir.join("blobs");
    fs::create_dir_all(&blobs).expect("blobs");
    let blob = "1212121212121212121212121212121212121212121212121212121212121212";
    let ollama_blob = blobs.join(format!("sha256-{}", blob));
    fs::write(&ollama_blob, &payload).expect("write ollama");
    let m = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("shared");
    fs::create_dir_all(&m).expect("manifest dir");
    let manifest = format!(
        r#"{{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{{"mediaType":"application/vnd.docker.container.image.v1+json","digest":"sha256:{blob}","size":412}},"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{blob}","size":4096}}]}}"#,
        blob = blob
    );
    fs::write(m.join("7b"), manifest).expect("manifest");

    let hf_home = root.join(".cache").join("huggingface");
    let repo = hf_home.join("hub").join("models--shared--Shared-7B");
    let hf_blobs = repo.join("blobs");
    let rev = "1212121212121212121212121212121212121212";
    let snap = repo.join("snapshots").join(rev);
    let refs = repo.join("refs");
    fs::create_dir_all(&hf_blobs).expect("hf blobs");
    fs::create_dir_all(&snap).expect("snap");
    fs::create_dir_all(&refs).expect("refs");
    let hf_blob_name = "abababababababababababababababababababababababababababababababab";
    let hf_blob = hf_blobs.join(hf_blob_name);
    // Pre-unified: hardlink to the ollama blob so they share one inode.
    fs::hard_link(&ollama_blob, &hf_blob).expect("hardlink hf to ollama");
    std::os::unix::fs::symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(hf_blob_name),
        snap.join("model.safetensors"),
    )
    .expect("snap symlink");
    fs::write(refs.join("main"), rev).expect("ref");

    (ollama_dir, hf_home)
}

/// Scrape the integer N out of `Unified: N models` in the right-pane footer.
/// Returns None when the footer is not present.
fn scrape_footer_unified_count(frame: &str) -> Option<u64> {
    for line in frame.lines() {
        // The footer reads:
        //   `Unified: N models | Total reclaimed by unification: ...`
        if let Some(rest) = line.split_once("Unified:").map(|(_, r)| r) {
            let trimmed = rest.trim_start();
            let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                if let Ok(n) = digits.parse::<u64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// Scrape the integer N out of `[All Unified] (N)` in the left pane.
/// Returns None when the badge has not been wired (or shows `(?)`).
fn scrape_badge_count(frame: &str) -> Option<u64> {
    for line in frame.lines() {
        if let Some(idx) = line.find("[All Unified]") {
            let after = &line[idx + "[All Unified]".len()..];
            // Look for `(N)` immediately after the slot label (single space).
            let after = after.trim_start();
            if let Some(stripped) = after.strip_prefix('(') {
                let digits: String = stripped
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !digits.is_empty() {
                    if let Ok(n) = digits.parse::<u64>() {
                        return Some(n);
                    }
                }
            }
        }
    }
    None
}

/// Count the number of right-pane rows whose body lines begin with the
/// All-Unified row format. We use the `<size>  N tools  saves <…>` shape as
/// the discriminator: every row in the All-Unified view contains the literal
/// substring " tools  saves " (two spaces around "tools" per `format_row`).
fn count_unified_rows(frame: &str) -> u64 {
    let mut n: u64 = 0;
    for line in frame.lines() {
        if line.contains(" tools  saves ") {
            n = n.saturating_add(1);
        }
    }
    n
}

// ---------------------------------------------------------------------------
// AC-U7.1: slot present in left pane below real tools.
// ---------------------------------------------------------------------------

#[test]
fn all_unified_slot_appears_in_left_pane_below_real_tools() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf) = build_pre_unified_two_tool(&temp);
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));
    assert!(
        frame.contains("[All Unified]"),
        "AC-U7.1: left pane must include '[All Unified]' slot, got:\n{}",
        frame
    );
    // AC-U7.1 ordering: the synthetic slot must appear AFTER every real tool
    // slot. We assert the slot label appears below at least one real tool
    // identifier (e.g. "ollama") in the rendered frame's line order.
    let lines: Vec<&str> = frame.lines().collect();
    let pos_all_unified = lines
        .iter()
        .position(|l| l.contains("[All Unified]"))
        .expect("[All Unified] line position");
    let pos_ollama = lines
        .iter()
        .position(|l| l.contains("ollama"))
        .expect("ollama row position");
    assert!(
        pos_all_unified > pos_ollama,
        "AC-U7.1: [All Unified] must appear BELOW the real tool rows; \
         all_unified at line {}, ollama at line {}, frame:\n{}",
        pos_all_unified,
        pos_ollama,
        frame
    );
}

// ---------------------------------------------------------------------------
// AC-U7.2 + AC-U7.6 + AC-CONS-2: badge count agrees with right-pane footer
// AND with the count of `#`-glyph rows. Single source of truth invariant.
// ---------------------------------------------------------------------------

#[test]
fn all_unified_badge_matches_summary_bar_unified_count() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf) = build_pre_unified_two_tool(&temp);
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    // Wait for hashing so the dedup classifier has the data it needs, then
    // navigate onto the [All Unified] slot to surface the right-pane footer.
    //
    // Slot order (alphabetical by ToolId): ["Atomic Chat", "hf",
    // "lm-studio", "ollama", "[All Unified]"]. Default selection lands on
    // the first INSTALLED tool. With this fixture, ollama and hf are
    // installed; "hf" is the alphabetically-first installed tool (idx=1).
    // Three `<right>` keystrokes advance from idx=1 to idx=4 ([All Unified]).
    let script = "<hash-complete><right><right><right>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(30))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));

    let badge = scrape_badge_count(&frame).unwrap_or_else(|| {
        panic!(
            "AC-U7.2 + AC-CONS-2: badge `[All Unified] (N)` must show a numeric \
             count after <hash-complete>; got frame:\n{}",
            frame
        )
    });
    let footer = scrape_footer_unified_count(&frame).unwrap_or_else(|| {
        panic!(
            "AC-U7.6: right-pane footer `Unified: N models | ...` must be \
             present after navigating to [All Unified]; got frame:\n{}",
            frame
        )
    });
    let row_count = count_unified_rows(&frame);

    assert_eq!(
        badge, footer,
        "AC-CONS-2: badge count ({}) must equal right-pane footer count \
         ({}); frame:\n{}",
        badge, footer, frame
    );
    assert_eq!(
        badge, row_count,
        "AC-CONS-2: badge count ({}) must equal the number of unified rows \
         ({}) rendered in the right pane; frame:\n{}",
        badge, row_count, frame
    );
    // Sanity: the pre-unified two-tool fixture has exactly one shared inode
    // → exactly one unified group.
    assert_eq!(
        badge, 1,
        "AC-U7.2: pre-unified two-tool fixture has one shared inode, \
         expected badge count 1; got {} in frame:\n{}",
        badge, frame
    );
}

// ---------------------------------------------------------------------------
// AC-U7.3: selecting [All Unified] filters right pane to # rows.
// ---------------------------------------------------------------------------

#[test]
fn selecting_all_unified_slot_filters_right_pane_to_hash_rows() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf) = build_pre_unified_two_tool(&temp);
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let script = "<hash-complete><right><right><right>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(30))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));

    // Right-pane title indicates dispatch to render::all_unified.
    assert!(
        frame.contains("[All Unified]"),
        "AC-U7.3: navigating onto the [All Unified] slot must dispatch the \
         right pane to render::all_unified (title `[All Unified]`); got \
         frame:\n{}",
        frame
    );
    // AC-U7.3 row-count invariant: every visible body row corresponds to a
    // `#`-glyph (already-unified) model. The header `Models in [All Unified]
    // (N)` advertises N rows; every row line carries " tools  saves ".
    let header_count = {
        // Scrape N out of `Models in [All Unified] (N)` — separate from the
        // left-pane badge so a regression in either does not silently mask
        // the other.
        let mut found: Option<u64> = None;
        for line in frame.lines() {
            if let Some(idx) = line.find("Models in [All Unified]") {
                let after = &line[idx + "Models in [All Unified]".len()..];
                let after = after.trim_start();
                if let Some(stripped) = after.strip_prefix('(') {
                    let digits: String = stripped
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if !digits.is_empty() {
                        if let Ok(n) = digits.parse::<u64>() {
                            found = Some(n);
                            break;
                        }
                    }
                }
            }
        }
        found
    }
    .unwrap_or_else(|| {
        panic!(
            "AC-U7.3: right-pane header `Models in [All Unified] (N)` must \
             be present; got frame:\n{}",
            frame
        )
    });
    let row_count = count_unified_rows(&frame);
    assert_eq!(
        header_count, row_count,
        "AC-U7.3: right-pane header advertises {} rows but {} body rows are \
         visible; frame:\n{}",
        header_count, row_count, frame
    );
    assert_eq!(
        row_count, 1,
        "AC-U7.3: pre-unified two-tool fixture has one shared inode → \
         exactly one row in the [All Unified] view; got {} in frame:\n{}",
        row_count, frame
    );
}

// ---------------------------------------------------------------------------
// AC-U7.4 + AC-U7.5: row format includes name, size, tool count, savings;
// footer aggregates totals.
// ---------------------------------------------------------------------------

#[test]
fn all_unified_view_row_format_and_footer_aggregates_totals() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (ollama, hf) = build_pre_unified_two_tool(&temp);
    let (mut cmd, _temp) = modeltap_headless_at(&ollama, &hf);
    let script = "<hash-complete><right><right><right>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(30))
        .assert()
        .success();
    let frame = frame_text(&String::from_utf8_lossy(&assert.get_output().stdout));

    // AC-U7.4: at least one body row contains the row format
    // `<name>  <size>  N tools  saves <bytes>`. The pre-unified fixture has
    // a 4096-byte payload shared by 2 tools → "2 tools" + "saves 4096 B".
    assert!(
        frame.contains("2 tools"),
        "AC-U7.4: row body must include 'N tools' (here N=2); got frame:\n{}",
        frame
    );
    assert!(
        frame.contains("saves "),
        "AC-U7.4: row body must include 'saves <bytes>'; got frame:\n{}",
        frame
    );
    // The shared payload size is 4096 bytes (sub-MB) → format_size renders
    // as "4096 B"; saves_bytes = (2-1) * 4096 = 4096 → also "4096 B".
    assert!(
        frame.contains("4096 B"),
        "AC-U7.4: shared 4096-byte payload must render as '4096 B' for both \
         size and saves columns; got frame:\n{}",
        frame
    );

    // AC-U7.5: footer aggregates totals — `Unified: N models | Total
    // reclaimed by unification: <SUM>`. SUM equals the sum of per-row
    // 'saves'. With one row and saves=4096, the total is also '4096 B'.
    let footer_count = scrape_footer_unified_count(&frame).unwrap_or_else(|| {
        panic!(
            "AC-U7.5: footer `Unified: N models | ...` must be present; \
             got frame:\n{}",
            frame
        )
    });
    assert_eq!(
        footer_count, 1,
        "AC-U7.5: footer should report 1 unified model for this fixture; \
         got {} in frame:\n{}",
        footer_count, frame
    );
    assert!(
        frame.contains("Total reclaimed by unification:"),
        "AC-U7.5: footer must include 'Total reclaimed by unification:'; \
         got frame:\n{}",
        frame
    );
    // The per-row saves sum equals the footer total. With one row at
    // 4096 B saves, the total reclaimed must be '4096 B'. Assert the
    // footer line carries that exact substring AFTER the marker so we do
    // not accidentally match the row's saves field.
    let footer_line = frame
        .lines()
        .find(|l| l.contains("Total reclaimed by unification:"))
        .expect("footer line");
    assert!(
        footer_line.contains("4096 B"),
        "AC-U7.5: footer total must equal sum of per-row saves (4096 B); \
         got footer line: {}",
        footer_line
    );
}
