//! Architecture rule R1 (per `docs/feature/modeltap-tui/devops/ci-pipeline.md`
//! §4 + ADR-001 §"Enforcement"):
//!
//!   1. `modeltap-core` MUST NOT path-depend on any plugin crate.
//!   2. Plugin crates MUST NOT path-depend on each other.
//!   3. `modeltap-tui` MUST NOT path-depend on any plugin crate.
//!
//! These three invariants make the plugin model honest. If any one breaks the
//! 5th-plugin contract from US-18 fails: a contributor adding "Atomic Chat"
//! could no longer rely on `inventory::submit!` alone — they would have to
//! amend `modeltap-core` or a sibling plugin's Cargo.toml. ADR-001 promises
//! that never happens.
//!
//! ## How the lint works
//!
//! We invoke `cargo metadata --format-version 1 --no-deps` from the workspace
//! root and walk the resulting `packages[]` array. For each crate of interest
//! we filter `dependencies[]` down to entries whose `path` resolves under the
//! `plugins/` directory — those are the only "plugin" deps that matter. The
//! workspace's outer Cargo.toml lists the four production plugins plus the
//! atomic-chat fixture; the lint tolerates none of them appearing in the
//! forbidden positions.
//!
//! `--no-deps` is critical: it makes `cargo metadata` return ONLY workspace
//! members, so we don't get noise from third-party crates.io packages.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// Resolve the workspace root. `CARGO_MANIFEST_DIR` points at the
/// modeltap-app crate's manifest; the workspace root is two levels up
/// (`crates/modeltap-app` -> repo root).
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(&manifest_dir)
        .to_path_buf()
}

/// Run `cargo metadata --format-version 1 --no-deps` from the workspace root
/// and return the parsed JSON document.
fn cargo_metadata() -> Value {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(workspace_root())
        .output()
        .expect("invoke cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("parse cargo metadata JSON")
}

/// Compute the absolute path to the `plugins/` directory.
fn plugins_dir() -> PathBuf {
    workspace_root().join("plugins")
}

/// Path-canonicalize, tolerating non-existent paths by falling back to the
/// raw path. We are matching prefixes, not opening files.
fn canon(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// `true` IFF the given dependency path lies under the workspace `plugins/`
/// directory.
fn is_plugin_dep(dep_path: &str) -> bool {
    let dep = canon(Path::new(dep_path));
    let plugins = canon(&plugins_dir());
    dep.starts_with(&plugins)
}

/// Names of plugin crates that depend on a sibling plugin crate via path-dep.
/// Returns the offending pairs as `("source crate", "sibling plugin crate")`.
fn collect_plugin_inter_deps(metadata: &Value) -> Vec<(String, String)> {
    let mut violations = Vec::new();
    let plugins = canon(&plugins_dir());
    for pkg in metadata["packages"].as_array().expect("packages array") {
        let manifest_path = pkg["manifest_path"].as_str().expect("manifest_path");
        let manifest = canon(Path::new(manifest_path));
        // Skip non-plugin packages — only plugin manifests live under plugins/.
        if !manifest.starts_with(&plugins) {
            continue;
        }
        let pkg_name = pkg["name"].as_str().expect("pkg name").to_string();
        for dep in pkg["dependencies"].as_array().expect("deps array") {
            // Path-deps only — registry deps cannot violate the rule.
            let Some(dep_path) = dep.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            if is_plugin_dep(dep_path) {
                let dep_name = dep["name"].as_str().unwrap_or("?").to_string();
                violations.push((pkg_name.clone(), dep_name));
            }
        }
    }
    violations
}

/// Names of plugin crates depended on (path-dep) by the named non-plugin crate.
fn collect_plugin_deps_of(metadata: &Value, crate_name: &str) -> BTreeSet<String> {
    let mut violations = BTreeSet::new();
    for pkg in metadata["packages"].as_array().expect("packages array") {
        if pkg["name"].as_str() != Some(crate_name) {
            continue;
        }
        for dep in pkg["dependencies"].as_array().expect("deps array") {
            let Some(dep_path) = dep.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            if is_plugin_dep(dep_path) {
                let dep_name = dep["name"].as_str().unwrap_or("?").to_string();
                violations.insert(dep_name);
            }
        }
    }
    violations
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// R1.1 — `modeltap-core` MUST NOT path-depend on any plugin crate.
///
/// This is the cardinal rule of ADR-001: the trait lives in the core, the
/// adapters (plugins) live outside. If core depended on a plugin we'd have
/// a cycle — and the 5th plugin contract would silently die because adding
/// a new plugin would force a core edit.
#[test]
fn r1_no_plugin_deps_in_core() {
    let metadata = cargo_metadata();
    let offenders = collect_plugin_deps_of(&metadata, "modeltap-core");
    assert!(
        offenders.is_empty(),
        "modeltap-core depends on plugin crate(s): {offenders:?} \
         — violates ADR-001 §Enforcement (R1.1)"
    );
}

/// R1.2 — Plugin crates MUST NOT path-depend on each other.
///
/// Plugins are siblings: a change in `hf` must never compile-break `ollama`.
/// The shared port lives in `modeltap-core`; two plugins that need to talk
/// must do so through `modeltap-core` types, never directly.
#[test]
fn r1_plugins_no_inter_deps() {
    let metadata = cargo_metadata();
    let offenders = collect_plugin_inter_deps(&metadata);
    assert!(
        offenders.is_empty(),
        "plugin crate(s) depend on sibling plugin(s): {offenders:?} \
         — violates ADR-001 §Enforcement (R1.2)"
    );
}

/// R1.3 — `modeltap-tui` MUST NOT path-depend on any plugin crate.
///
/// The render layer is plugin-agnostic — it consumes `ToolView` from
/// `modeltap-core` only. If TUI knew about a concrete plugin (e.g.
/// `modeltap-plugin-ollama`) the bottom-bar / left-pane code could special-case
/// rendering per tool, breaking the uniform contract. ADR-006 + US-08 forbid it.
#[test]
fn r1_tui_no_concrete_plugins() {
    let metadata = cargo_metadata();
    let offenders = collect_plugin_deps_of(&metadata, "modeltap-tui");
    assert!(
        offenders.is_empty(),
        "modeltap-tui depends on plugin crate(s): {offenders:?} \
         — violates ADR-001 §Enforcement (R1.3) / ADR-006"
    );
}
