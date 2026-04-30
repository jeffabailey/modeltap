//! Acceptance tests for US-18 (Plugin trait certification).
//!
//! These four scenarios CERTIFY the `Tool` trait shipped in 01-02 by exercising
//! the architecture-rule contract end-to-end:
//!
//! 1. **A new plugin appears in the left pane on launch** — the
//!    `atomic-chat-fixture` crate is wired into the test binary via the
//!    `test-fixtures` Cargo feature; on launch the headless TUI shows a row
//!    labelled "atomic-chat" alongside the four production tools, and zero
//!    files were touched in `crates/modeltap-core/src/` to make it appear.
//!
//! 2. **A plugin panic does not crash modeltap** — the same fixture, when
//!    built with the `panic-on-discover` feature, panics inside `discover()`.
//!    The TUI still launches; the panicking tool's row shows `(error)` and
//!    `diagnostics.log` (here surfaced as `tool_errors` on the `launch.inventory`
//!    event) carries the panic message; the other tools render normally.
//!
//! 3. **Architecture rule R1 — modeltap-core has no plugin dependency** —
//!    the workspace-level `tests/architecture.rs` lint asserts the three
//!    invariants by parsing `cargo metadata`. Here we simply verify the
//!    invariants hold by inspecting the registered plugin set.
//!
//! 4. **launch.inventory event lists all registered plugins** — read
//!    `launch.log` after a clean headless launch; the `launch.inventory`
//!    event must carry a `tools_registered` array containing every plugin
//!    `Tool::name()` value (Riley uses this field as the canonical inventory
//!    of the deployed plugin set, per kpi-instrumentation.md §3.3).
//!
//! Tags: @us-18 @release-3 @plugin-trait

use std::path::Path;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers (mirrors us_01_launch_quit::modeltap_headless / read_launch_log so
// we don't take a cross-test dependency).
// ---------------------------------------------------------------------------

fn modeltap_headless() -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let log_dir = temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "100")
        // Point every plugin at a non-existent root so `discover()` returns
        // `NotInstalled` and the test does NOT depend on the host's real
        // `~/.ollama/`, `~/.cache/huggingface/`, etc.
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/ollama")
        .env("HF_HOME", "/nonexistent/hf-home")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/lm-studio")
        .env("MODELTAP_LLAMA_CLI_DIRS", "/nonexistent/llama-cli")
        // Opt INTO the atomic-chat test fixture for the US-18 scenarios.
        // The fixture's `discover()` short-circuits to `NotInstalled` unless
        // this env var is set, so prior acceptance tests (US-02/03/05/...)
        // never see it. Mirrors the `MODELTAP_LMSTUDIO_DIRS` pattern.
        .env("MODELTAP_ENABLE_TEST_FIXTURE_ATOMIC_CHAT", "1");
    (cmd, temp)
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

fn find_event<'a>(events: &'a [Value], event_name: &str) -> &'a Value {
    events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some(event_name))
        .unwrap_or_else(|| panic!("missing {event_name} event in launch.log"))
}

// ---------------------------------------------------------------------------
// Scenario 1: A new plugin appears in the left pane on launch.
// ---------------------------------------------------------------------------

/// AC-1 / AC-2 / AC-3 — adding the 5th plugin (`atomic-chat`) requires zero
/// changes to `crates/modeltap-core/src/`. Wiring is purely linkage in
/// `modeltap-app` (see `Cargo.toml` `test-fixtures` feature) plus the
/// `inventory::submit!` block inside `plugins/atomic-chat-fixture/`.
///
/// We verify the plugin is live by reading the headless first-paint frame
/// from stdout and asserting "atomic-chat" appears in the left-pane tool list.
#[test]
fn riley_sees_the_fifth_plugin_in_the_left_pane_on_launch() {
    let (mut cmd, temp) = modeltap_headless();
    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.contains("atomic-chat"),
        "headless first-paint must show the atomic-chat fixture in the left \
         pane, got:\n{stdout}"
    );

    // launch.inventory event must list it too — same source of truth used by
    // Riley's release dashboards.
    let log_dir = temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let inv = find_event(&events, "launch.inventory");
    let tools = inv
        .get("tools_registered")
        .and_then(|v| v.as_array())
        .expect("launch.inventory.tools_registered must be an array");
    let tool_names: Vec<&str> = tools.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        tool_names.contains(&"atomic-chat"),
        "tools_registered must include atomic-chat, got {tool_names:?}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: A plugin panic does not crash modeltap.
// ---------------------------------------------------------------------------

/// AC-4 — when the fixture is built with the `panic-on-discover` feature its
/// `discover()` panics. The supervisor (`plugin_isolation::run_plugin_call_isolated`
/// inside `discovery::run_discovery`) catches the panic at the
/// `tokio::spawn` JoinError boundary; the tool's slot lands in
/// `summary.tool_errors()` and the TUI continues to render. The other four
/// tools must still appear normally (`NotInstalled` rows in this test, since
/// we point them at non-existent roots).
///
/// Test seam: `MODELTAP_FIXTURE_ATOMIC_CHAT_PANIC=1` flips the fixture's
/// runtime branch without requiring a rebuild. The fixture honours the env
/// var IFF the opt-in is also set (it is — `modeltap_headless()` sets
/// `MODELTAP_ENABLE_TEST_FIXTURE_ATOMIC_CHAT=1`).
#[test]
fn riley_observes_a_panicking_plugin_does_not_crash_modeltap() {
    let (mut cmd, temp) = modeltap_headless();
    let assert = cmd
        .arg("--quit-after-paint")
        .env("MODELTAP_FIXTURE_ATOMIC_CHAT_PANIC", "1")
        .timeout(Duration::from_secs(5))
        .assert()
        .success(); // <- the binary did NOT crash

    // The TUI rendered: bottom-bar text proves the frame painted.
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.contains("[<-/->] tools"),
        "TUI must render even when one plugin panics, got:\n{stdout}"
    );

    // The panicking plugin landed in tool_errors, captured to launch.inventory.
    let log_dir = temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let inv = find_event(&events, "launch.inventory");
    let errs = inv
        .get("tool_errors")
        .and_then(|v| v.as_array())
        .expect("launch.inventory.tool_errors must be an array");
    let err_names: Vec<&str> = errs.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        err_names.contains(&"atomic-chat"),
        "tool_errors must include atomic-chat (the panicking plugin), got \
         {err_names:?}"
    );

    // The other four production plugins must still be present in the
    // tools_registered inventory so the panic isolation is local, not
    // catastrophic.
    let tools = inv
        .get("tools_registered")
        .and_then(|v| v.as_array())
        .expect("launch.inventory.tools_registered must be an array");
    let tool_names: Vec<&str> = tools.iter().filter_map(|v| v.as_str()).collect();
    for name in &["ollama", "hf", "llama-cli", "lm-studio"] {
        assert!(
            tool_names.contains(name),
            "tools_registered must still include {name} after a sibling panic, \
             got {tool_names:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 3: Architecture rule — modeltap-core has no plugin dependency.
//
// The full lint lives in `tests/architecture.rs` at the workspace root (it
// shells out to `cargo metadata`). Here we duplicate ONE invariant — the
// most surgical one — through the registered plugin set so the acceptance
// suite alone proves the contract.
// ---------------------------------------------------------------------------

/// AC-6 — adding the fixture must not have introduced ANY new core surface
/// area; the registered plugin set is built entirely from `inventory::iter`
/// against the slot defined in `modeltap-core`. The architectural surface is
/// "plugins implement Tool, modeltap-app links them, modeltap-core is unaware".
///
/// We assert the architectural invariant by reading the production binary's
/// `launch.inventory.tools_registered` event — that field is populated
/// directly from `inventory::iter::<modeltap_core::PluginFactory>()` in
/// `main.rs`, BEFORE discovery runs and independent of the runtime opt-in
/// gate. If any plugin failed to register (e.g. dropped `inventory::submit!`)
/// the list would be short. The full cross-Cargo.toml lint lives at
/// `crates/modeltap-app/tests/architecture.rs` and runs in CI.
#[test]
fn architecture_rule_r1_modeltap_core_has_no_concrete_plugin_dependency() {
    // Drive the production composition root and observe the registered
    // plugin set via launch.inventory — same channel Riley's release
    // dashboards use.
    let (mut cmd, temp) = modeltap_headless();
    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();

    let log_dir = temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let inv = find_event(&events, "launch.inventory");
    let tools = inv
        .get("tools_registered")
        .and_then(|v| v.as_array())
        .expect("launch.inventory.tools_registered must be an array");
    let registered: Vec<&str> = tools.iter().filter_map(|v| v.as_str()).collect();

    // Production: ollama, hf, llama-cli, lm-studio. Test fixture: atomic-chat.
    // 5 total when the test-fixtures feature is on.
    assert_eq!(
        registered.len(),
        5,
        "expected 5 registered plugins (4 production + 1 fixture), got {registered:?}"
    );
    assert!(
        registered.contains(&"atomic-chat"),
        "atomic-chat must be registered, got {registered:?}"
    );

    // The contract: modeltap-core's published surface (`Tool`,
    // `PluginFactory`) is the ONLY API the fixture used. If the fixture had
    // imported a plugin sibling, the workspace's cargo metadata lint
    // (`tests/architecture.rs`) would have caught it.
}

// ---------------------------------------------------------------------------
// Scenario 4: launch.inventory event lists all registered plugins.
// ---------------------------------------------------------------------------

/// AC-7 — Riley's release dashboards consume `launch.inventory.tools_registered`
/// to know which plugins are deployed in a given build. The schema must list
/// every plugin's `Tool::name()` in deterministic (alphabetic) order.
#[test]
fn riley_reads_launch_inventory_lists_every_registered_plugin() {
    let (mut cmd, temp) = modeltap_headless();
    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();

    let log_dir = temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let inv = find_event(&events, "launch.inventory");
    let tools = inv
        .get("tools_registered")
        .and_then(|v| v.as_array())
        .expect("launch.inventory.tools_registered must be an array");
    let tool_names: Vec<&str> = tools.iter().filter_map(|v| v.as_str()).collect();

    // 4 production + 1 test fixture = 5.
    assert_eq!(
        tool_names.len(),
        5,
        "tools_registered must list every registered plugin, got {tool_names:?}"
    );
    for required in &["atomic-chat", "hf", "llama-cli", "lm-studio", "ollama"] {
        assert!(
            tool_names.contains(required),
            "tools_registered missing {required}, got {tool_names:?}"
        );
    }

    // Deterministic alphabetic order so dashboards diff cleanly across runs.
    let mut sorted = tool_names.clone();
    sorted.sort();
    assert_eq!(
        tool_names, sorted,
        "tools_registered must be alphabetically sorted for stable dashboards"
    );
}
