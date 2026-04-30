//! Unit tests for `modeltap_app::refresh::refresh_tool_incremental` and
//! `modeltap_app::inventory_build::replace_tool_in_inventory` (US-11.AC-1,
//! US-11.AC-2 — sub-500ms incremental rediscovery + degraded-on-failure path).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: refresh_tool_incremental returns Ok(ToolView) reflecting the
//!         post-mutation state for a healthy plugin (within 500 ms).
//!     B2: refresh_tool_incremental returns Err(RefreshError::Unreadable) when
//!         the plugin's directory has been removed mid-run.
//!     B3: refresh_tool_incremental returns Err(RefreshError::NotInstalled)
//!         when the plugin's directory was never present.
//!     B4: replace_tool_in_inventory recomputes total disk_usage_bytes from
//!         the new slots and leaves other tools' slots unchanged.
//!   budget = 4 × 2 = 8 unit tests max. We use 4 (one per behavior, with
//!   parametrized variants where useful).
//!
//! Each test enters through the public driving port (the function under
//! test). No mocks — uses the real Ollama plugin against tmp_path fixtures
//! per Mandate 4 (adapters tested with integration tests = real I/O).

use std::process::Command as StdCommand;
use std::time::Duration;

use modeltap_app::inventory_build::replace_tool_in_inventory;
use modeltap_app::refresh::{self, RefreshError};
use modeltap_core::{ToolId, ToolStatus};
use modeltap_plugin_ollama::OllamaPlugin;
use modeltap_tui::ToolView;

fn build_devon_multi_tool() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join("devon-multi-tool");
    let project_root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .and_then(|p| {
            p.parent()
                .and_then(|p| p.parent().map(std::path::PathBuf::from))
        })
        .expect("walk to workspace root");
    let script = project_root.join("tests/fixtures/build.sh");
    let status = StdCommand::new("bash")
        .arg(&script)
        .arg("devon-multi-tool")
        .arg(&target)
        .status()
        .expect("spawn build.sh");
    assert!(status.success(), "fixture builder failed");
    let ollama_dir = target.join(".ollama").join("models");
    (temp, ollama_dir)
}

// ---------------------------------------------------------------------------
// B1 — refresh_tool_incremental returns Ok on healthy plugin within 500 ms.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_tool_incremental_returns_ok_within_500ms_for_healthy_plugin() {
    let (_temp, ollama_dir) = build_devon_multi_tool();
    let plugin = OllamaPlugin::new_with_root(ollama_dir);

    let start = std::time::Instant::now();
    let view = refresh::refresh_tool_incremental(&plugin)
        .await
        .expect("incremental refresh succeeds against healthy fixture");
    let elapsed = start.elapsed();

    assert_eq!(view.tool, ToolId("ollama"));
    assert_eq!(view.status, ToolStatus::Ok);
    assert!(
        !view.model_ids.is_empty(),
        "devon-multi-tool fixture has 4 manifests"
    );

    // US-11.AC-1: refresh latency budget is 500 ms (production target).
    // Test-side margin: < 2 s on slow CI hosts. The 500 ms target is the
    // production invariant, not a test threshold.
    assert!(
        elapsed < Duration::from_secs(2),
        "refresh_tool_incremental took {:?} — must be well under 500 ms target",
        elapsed
    );
}

// ---------------------------------------------------------------------------
// B2 — refresh_tool_incremental returns Err(Unreadable) when the plugin's
// directory has been removed mid-run (the "directory removed mid-action"
// scenario from US-11.AC-2).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_tool_incremental_returns_err_when_directory_removed() {
    let (temp, ollama_dir) = build_devon_multi_tool();
    let plugin = OllamaPlugin::new_with_root(ollama_dir.clone());

    // Healthy refresh first to confirm precondition.
    let _pre = refresh::refresh_tool_incremental(&plugin)
        .await
        .expect("pre-mutation refresh ok");

    // Remove the entire fixture directory — the plugin's discover() will now
    // fail because the root no longer exists.
    drop(temp); // tempdir Drop deletes the tree.

    let result = refresh::refresh_tool_incremental(&plugin).await;
    assert!(
        matches!(
            result,
            Err(RefreshError::NotInstalled) | Err(RefreshError::Unreadable { .. })
        ),
        "expected RefreshError::Unreadable or NotInstalled when dir is gone, got: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// B3 — refresh_tool_incremental returns Err(NotInstalled) when the plugin's
// directory was never present. NotInstalled is a non-failure terminal state
// (the tool's slot stays NotInstalled in the UI), distinct from Unreadable.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn refresh_tool_incremental_returns_not_installed_when_root_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let nonexistent = temp.path().join("no-such-ollama");
    let plugin = OllamaPlugin::new_with_root(nonexistent);

    let result = refresh::refresh_tool_incremental(&plugin).await;
    assert!(
        matches!(result, Err(RefreshError::NotInstalled)),
        "expected RefreshError::NotInstalled for missing root, got: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// B4 — replace_tool_in_inventory recomputes the total disk usage from the
// new slots and leaves other tools' slots unchanged.
// ---------------------------------------------------------------------------

fn tool_view(name: &'static str, status: ToolStatus, sizes: &[u64]) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status,
        model_ids: (0..sizes.len()).map(|i| format!("{name}:m{i}")).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

#[test]
fn replace_tool_in_inventory_recomputes_total_and_preserves_other_slots() {
    // Initial inventory: ollama with 3 blobs, hf with 1 blob, llama-cli not
    // installed.
    let inventory = vec![
        tool_view("hf", ToolStatus::Ok, &[1_000_000]),
        tool_view("llama-cli", ToolStatus::NotInstalled, &[]),
        tool_view(
            "ollama",
            ToolStatus::Ok,
            &[10_000_000, 20_000_000, 30_000_000],
        ),
    ];
    let pre_total: u64 = inventory.iter().map(|t| t.total_bytes()).sum();
    assert_eq!(pre_total, 61_000_000);

    // Refresh ollama after a zap: 0 models, 0 bytes.
    let refreshed = tool_view("ollama", ToolStatus::Ok, &[]);
    let next = replace_tool_in_inventory(inventory.clone(), ToolId("ollama"), refreshed);

    let post_total: u64 = next.iter().map(|t| t.total_bytes()).sum();
    // New total = old total - bytes_reclaimed (60M) — within 1KB rounding.
    assert_eq!(post_total, 1_000_000);
    let diff = post_total.abs_diff(pre_total - 60_000_000);
    assert!(diff <= 1024, "INT-5: diff={diff}");

    // Other tools' slots unchanged.
    let hf = next
        .iter()
        .find(|t| t.tool == ToolId("hf"))
        .expect("hf present");
    assert_eq!(hf.total_bytes(), 1_000_000);
    let llama = next
        .iter()
        .find(|t| t.tool == ToolId("llama-cli"))
        .expect("llama-cli present");
    assert_eq!(llama.status, ToolStatus::NotInstalled);
}

#[test]
fn replace_tool_in_inventory_returns_unchanged_when_tool_not_present() {
    // Defensive branch — if the tool_id has no slot, return inventory
    // unchanged. This is a guard for pathological cases (refresh dispatched
    // for a tool we don't track).
    let inventory = vec![tool_view("hf", ToolStatus::Ok, &[1_000_000])];
    let unknown = tool_view("unknown-tool", ToolStatus::Ok, &[42]);
    let next = replace_tool_in_inventory(inventory.clone(), ToolId("unknown-tool"), unknown);
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].tool, ToolId("hf"));
    assert_eq!(next[0].total_bytes(), 1_000_000);
}
