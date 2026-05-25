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

// ---------------------------------------------------------------------------
// US-20: cross-platform hygiene lint. Production source must not bake in
// absolute Unix paths (`/Users/...`, `/home/...`, `/etc/...`, etc.) —
// those would silently break on Windows and undermine the platform
// abstraction. The platform module itself is exempt, as is any line that
// lives under a `#[cfg(test)]` block (tests use synthetic absolute paths
// for fixture clarity).
// ---------------------------------------------------------------------------

/// Walk the workspace source tree and return every `*.rs` file under
/// `crates/` and `plugins/`. Skips `target/` and `tests/` directories
/// (tests are allowed to use absolute paths in fixtures).
fn workspace_source_files() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files = Vec::new();
    for top in ["crates", "plugins"] {
        collect_rs_files(&root.join(top), &mut files);
    }
    files
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.is_dir() {
            // Skip cargo build output and per-crate `tests/` directories
            // (integration tests are allowed to use absolute literal paths
            // in fixture builders).
            if matches!(name, "target" | "tests") {
                continue;
            }
            collect_rs_files(&path, out);
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// Forbidden absolute-path prefixes — Unix-only locations that have no
/// meaning on Windows. We intentionally do NOT flag the bare slash `"/"`
/// because path joins like `home.join(".cache")` are platform-portable;
/// only fully-qualified absolute path literals are an issue.
const FORBIDDEN_PREFIXES: &[&str] = &[
    "\"/Users/",
    "\"/home/",
    "\"/etc/",
    "\"/usr/",
    "\"/var/",
    "\"/proc/",
    "\"/sys/",
    "\"/opt/",
    "\"/tmp/",
];

/// Module path (relative-to-crate-root) that is exempt from the lint
/// because its job is to encode platform differences.
fn is_exempt_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    // The platform abstraction itself.
    s.ends_with("modeltap-app/src/platform.rs")
        // The architecture-lint test source itself (this file lists the
        // forbidden prefixes as string literals).
        || s.ends_with("modeltap-app/tests/architecture.rs")
        || s.ends_with("modeltap-tui/tests/architecture.rs")
}

/// Strip Rust line comments (`// ...`) so a comment containing an
/// absolute path is not falsely flagged. Approximate but adequate —
/// we don't track string-literal context across `//` because no
/// Rust string literal that we ship contains `//`.
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// Scan production code for hardcoded Unix absolute paths. Lines under a
/// `#[cfg(test)]` mod block are skipped via a depth counter, NOT a regex,
/// because `#[cfg(test)]` modules can be nested or inline. The counter
/// increments on each `#[cfg(test)]` attribute followed by a `mod ` or a
/// brace block, and decrements when the block's outer `}` is consumed.
///
/// This is approximate but correct for every case in this codebase: every
/// test-only literal lives inside a `#[cfg(test)] mod tests { ... }` block.
fn scan_for_hardcoded_unix_paths(path: &Path) -> Vec<(usize, String)> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut violations = Vec::new();
    let mut in_test_block = false;
    let mut test_block_depth: i32 = 0;
    let mut pending_cfg_test = false;

    for (lineno, raw) in content.lines().enumerate() {
        let line = strip_line_comment(raw).trim_start();

        // Track whether we just saw `#[cfg(test)]`.
        if line.starts_with("#[cfg(test)]") {
            pending_cfg_test = true;
            continue;
        }

        // If `mod tests {` (or any `mod <name> {`) follows a pending
        // `#[cfg(test)]`, enter a test block.
        if pending_cfg_test {
            if line.contains('{') {
                in_test_block = true;
                test_block_depth = count_braces(line);
            }
            pending_cfg_test = false;
            continue;
        }

        // Inside a test block: count braces to find the matching close.
        if in_test_block {
            test_block_depth += count_braces(line);
            if test_block_depth <= 0 {
                in_test_block = false;
                test_block_depth = 0;
            }
            continue;
        }

        // Production line — check for forbidden prefixes.
        for prefix in FORBIDDEN_PREFIXES {
            if line.contains(prefix) {
                violations.push((lineno + 1, raw.to_string()));
                break;
            }
        }
    }

    violations
}

/// Net brace delta: opens minus closes. Used to track when a
/// `#[cfg(test)]` mod block ends.
fn count_braces(line: &str) -> i32 {
    let mut delta: i32 = 0;
    for c in line.chars() {
        match c {
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// US-20 AC-5 — production source MUST NOT hardcode Unix absolute paths
/// outside the `platform.rs` module. Such literals would silently break on
/// Windows and undermine the platform abstraction installed in this step.
#[test]
fn no_hardcoded_unix_paths_outside_platform_module() {
    let mut all_violations: Vec<String> = Vec::new();
    for file in workspace_source_files() {
        if is_exempt_path(&file) {
            continue;
        }
        for (line, text) in scan_for_hardcoded_unix_paths(&file) {
            all_violations.push(format!("{}:{}: {}", file.display(), line, text.trim()));
        }
    }
    assert!(
        all_violations.is_empty(),
        "production source must not hardcode Unix absolute paths \
         (use platform.rs or join from $HOME). Offenders:\n{}",
        all_violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// tool-model-info-sqlite-cache — step 06-01.
//
// R7 + R8 + R9 architecture lints per
// `docs/feature/tool-model-info-sqlite-cache/design/component-boundaries.md`
// §R7-R9. These extend the parent R1-R6 lints above with the
// modeltap-store layering rules (R7 / R8) and the K5-extension safety lint
// (R9) which statically guarantees every destructive trait-call expression
// inside `src/orchestration/` AND `src/actions/` is preceded by a
// `revalidate::pre_mutate(...)` invocation in the same fn body.
//
// The K5 invariant — "no destructive action against stale cache data" —
// was wired into the four current call sites (unify, zap, delete_one,
// folder_delete) during step 05-02. R9 is the safety net for FUTURE call
// sites: a contributor adding a 5th destructive call without a guard fails
// CI immediately with a file:line pointer.
// ---------------------------------------------------------------------------

/// Set of plugin manifest paths under `plugins/` keyed by crate name (the
/// `pkg["name"]` field from cargo metadata). Used by R7 to drive its
/// "no crate other than `modeltap-app` may path-dep on `modeltap-store`"
/// assertion: we walk every workspace package's `dependencies[]` and report
/// the offender by name + the dep entry that violated.
fn collect_path_dep_consumers(metadata: &Value, target_crate: &str) -> BTreeSet<String> {
    let mut consumers = BTreeSet::new();
    for pkg in metadata["packages"].as_array().expect("packages array") {
        let pkg_name = pkg["name"].as_str().expect("pkg name");
        for dep in pkg["dependencies"].as_array().expect("deps array") {
            // Path-deps only — registry deps cannot violate the layering
            // rule (modeltap-store is a workspace member, not on crates.io).
            if dep.get("path").and_then(|v| v.as_str()).is_none() {
                continue;
            }
            if dep["name"].as_str() == Some(target_crate) {
                consumers.insert(pkg_name.to_string());
            }
        }
    }
    consumers
}

/// R7 — `modeltap-store` is a composition-root concern; ONLY `modeltap-app`
/// may path-depend on it. The TUI must not know SQLite exists; the core
/// must remain pure; plugins must not depend on a sibling layer crate. The
/// seam is "the app wires the cache to the rest of the system."
///
/// Failure mode: a future contributor (or a dependency drift) pulls
/// `modeltap-store` into, say, `modeltap-tui` for a debug helper. The lint
/// fires with a clear "<offender> path-depends on modeltap-store" message.
///
/// Note: the `modeltap-acceptance` test crate (under `tests/` at the
/// workspace root) legitimately depends on `modeltap-store` for its cache
/// introspection helpers (step 01-05 / 04-03 acceptance tests). It is
/// explicitly allow-listed below because it is publish = false and is not
/// part of any shipped binary.
#[test]
fn r7_only_app_depends_on_store() {
    let metadata = cargo_metadata();
    let consumers = collect_path_dep_consumers(&metadata, "modeltap-store");
    // Allow-list: the production composition root + the test-only
    // acceptance crate. Anything else is a violation.
    const ALLOWED: &[&str] = &["modeltap-app", "modeltap-acceptance"];
    let offenders: BTreeSet<String> = consumers
        .into_iter()
        .filter(|name| !ALLOWED.contains(&name.as_str()))
        .collect();
    assert!(
        offenders.is_empty(),
        "modeltap-store may only be path-depended on by {ALLOWED:?}, but these crates \
         depend on it too: {offenders:?} — violates component-boundaries.md §R7"
    );
}

/// R8 — `modeltap-store` MUST NOT depend on `tokio`, `ratatui`, or
/// `crossterm` (in either `[dependencies]` or `[dev-dependencies]`).
///
/// Rationale: the cache layer is sync rusqlite. Adding tokio creates two
/// concurrency models in the same crate (blocking calls vs an async
/// runtime). Adding ratatui / crossterm would couple a storage layer to a
/// rendering layer — the inverse of hexagonal layering. The async bridge
/// happens at the `modeltap-app` boundary via `spawn_blocking`; the TUI
/// bridge happens at the `modeltap-app` boundary via projection types.
///
/// We use `cargo metadata --no-deps` to list `modeltap-store`'s own
/// dependencies (which already covers both `[dependencies]` AND
/// `[dev-dependencies]` — cargo's metadata schema flattens them with a
/// `kind` discriminator). We assert each forbidden crate name is absent
/// from EITHER kind.
#[test]
fn r8_store_no_tokio_ratatui() {
    let metadata = cargo_metadata();
    const FORBIDDEN: &[&str] = &["tokio", "ratatui", "crossterm"];
    let mut offenders: Vec<String> = Vec::new();
    for pkg in metadata["packages"].as_array().expect("packages array") {
        if pkg["name"].as_str() != Some("modeltap-store") {
            continue;
        }
        for dep in pkg["dependencies"].as_array().expect("deps array") {
            let name = dep["name"].as_str().unwrap_or("?");
            if FORBIDDEN.contains(&name) {
                // `kind` is `null` for normal, `"dev"` for dev-dep,
                // `"build"` for build-dep. We forbid all three.
                let kind = dep.get("kind").and_then(|v| v.as_str()).unwrap_or("normal");
                offenders.push(format!("{name} (kind={kind})"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "modeltap-store must NOT depend on {FORBIDDEN:?} (per component-boundaries.md §R8), \
         but found: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// R9 — pre-mutate guard AST lint.
//
// The contract: every method-call expression in `crates/modeltap-app/src/
// orchestration/` OR `crates/modeltap-app/src/actions/` that targets one of
// the four destructive `Tool` trait methods (`link`, `delete_one`,
// `delete_all`, `delete_folder`) must be preceded — in the same fn body —
// by an invocation of `revalidate::pre_mutate(...)`. This is enforced
// statically so a future contributor adding a 5th call site without a
// guard cannot ship.
//
// Why both `orchestration/` AND `actions/`: the current call sites all
// live under `actions/` (unify, zap, delete_one, folder_delete) but the
// design's R9 wording uses "orchestration" as the umbrella for the
// composition-root coordinators. Linting both directories closes the
// "moved-but-not-renamed" loophole.
//
// Detection algorithm — per fn body:
//   1. Walk statements + nested expressions in source order.
//   2. Track whether `pre_mutate` has been called (or is name-imported).
//      `revalidate::pre_mutate(...)` and bare `pre_mutate(...)` (via `use
//      revalidate::pre_mutate;`) both count.
//   3. On encountering `<expr>.<DESTRUCTIVE>(...)` where the receiver is a
//      bare identifier or simple field access AND DESTRUCTIVE matches one
//      of the four trait method names, assert `pre_mutate` has already
//      fired. If not, record a violation with the file:line:method shape.
//
// False-positive safeguard: we skip method calls whose receiver is `self`
// — those are the `Tool` trait's own default-body implementations or test
// fixtures, not orchestration sites. The destructive method names are
// distinctive enough in modeltap that this filter is unnecessary in
// practice, but the safeguard documents intent.
// ---------------------------------------------------------------------------

/// Source files the R9 lint walks. Currently:
/// `crates/modeltap-app/src/orchestration/*.rs` and
/// `crates/modeltap-app/src/actions/*.rs`. Walked recursively so any future
/// sub-module additions are picked up automatically.
fn r9_source_files() -> Vec<PathBuf> {
    let app_src = workspace_root().join("crates/modeltap-app/src");
    let mut out = Vec::new();
    for sub in ["orchestration", "actions"] {
        collect_rs_files(&app_src.join(sub), &mut out);
    }
    out
}

/// The four destructive trait methods the K5 invariant gates. If a future
/// roadmap adds a 5th, append it here AND wire it through `pre_mutate` at
/// the call site. ADR-015 §"Enforcement" records the discipline.
const R9_DESTRUCTIVE_METHODS: &[&str] = &["link", "delete_one", "delete_all", "delete_folder"];

/// A single R9 violation — a destructive call with no preceding
/// `pre_mutate` in the same fn body. Reported as
/// `<file>:<line>: tool.<method>(...) has no preceding revalidate::pre_mutate`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct R9Violation {
    file: String,
    line: usize,
    method: String,
}

impl std::fmt::Display for R9Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: destructive call .{}() has no preceding revalidate::pre_mutate \
             in the same fn — violates component-boundaries.md §R9",
            self.file, self.line, self.method
        )
    }
}

/// Walk a parsed `syn::File` and accumulate every R9 violation found in
/// its fn / impl / async-fn bodies. The `file_label` is what
/// `R9Violation::file` reports — usually a relative path string for
/// readable diagnostics, but in unit tests it is a synthetic label.
///
/// Line numbers in `R9Violation` are derived from the destructive method's
/// `proc_macro2::Span` via `tokens_to_line_via_source` rather than
/// `Span::start()` — the latter requires the `span-locations` feature on
/// proc-macro2 which is not feature-stable in a regular dev-dep. Instead
/// we compute the line by tokenising the method ident's span back into a
/// `proc_macro2::Span` and counting newlines up to the byte offset in the
/// original source. The negative-R9 unit tests work without a source-text
/// argument because they only assert `line > 0` (the synthetic source is
/// only a few lines long; any positive line number is correct).
fn r9_walk_file(parsed: &syn::File, file_label: &str) -> Vec<R9Violation> {
    r9_walk_file_with_source(parsed, file_label, None)
}

/// Variant of [`r9_walk_file`] that takes the original source text so line
/// numbers in `R9Violation` can be accurate (the real file-walking lint
/// uses this). When `source` is `None`, line numbers default to `1` — the
/// synthetic-fixture unit tests are content with this because they assert
/// only `line > 0`.
fn r9_walk_file_with_source(
    parsed: &syn::File,
    file_label: &str,
    source: Option<&str>,
) -> Vec<R9Violation> {
    use syn::visit::Visit;

    /// Per-fn visitor: records `pre_mutate` calls and destructive method
    /// calls in source order; flags destructive calls that lack a
    /// preceding `pre_mutate`.
    struct FnBodyVisitor<'a> {
        file_label: &'a str,
        source: Option<&'a str>,
        pre_mutate_seen: bool,
        violations: Vec<R9Violation>,
    }

    impl<'ast, 'a> Visit<'ast> for FnBodyVisitor<'a> {
        fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
            // Match either `revalidate::pre_mutate(...)` (path call) OR
            // a bare `pre_mutate(...)` (via `use revalidate::pre_mutate;`).
            if let syn::Expr::Path(path_expr) = &*node.func {
                let segments: Vec<String> = path_expr
                    .path
                    .segments
                    .iter()
                    .map(|s| s.ident.to_string())
                    .collect();
                let last = segments.last().map(|s| s.as_str()).unwrap_or("");
                if last == "pre_mutate" {
                    self.pre_mutate_seen = true;
                }
            }
            syn::visit::visit_expr_call(self, node);
        }

        fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
            let method = node.method.to_string();
            // Treat the call as destructive ONLY when the method name
            // matches one of the four AND the receiver is not literally
            // `self` (which would be a trait-default body, not an
            // orchestration site).
            let is_destructive = R9_DESTRUCTIVE_METHODS.contains(&method.as_str())
                && !matches!(
                    &*node.receiver,
                    syn::Expr::Path(p) if p.path.is_ident("self")
                );
            if is_destructive && !self.pre_mutate_seen {
                let line = match self.source {
                    Some(src) => approx_line_from_source(src, &method),
                    None => 1,
                };
                self.violations.push(R9Violation {
                    file: self.file_label.to_string(),
                    line,
                    method,
                });
            }
            syn::visit::visit_expr_method_call(self, node);
        }
    }

    /// File-level visitor: enters each fn / method body with a fresh
    /// `FnBodyVisitor`, then folds the per-fn violations into the
    /// file-level accumulator. Resetting `pre_mutate_seen` per fn means a
    /// guard in fn A does not cover a destructive call in fn B — which is
    /// the correct semantics.
    struct FileVisitor<'a> {
        file_label: &'a str,
        source: Option<&'a str>,
        all_violations: Vec<R9Violation>,
    }

    impl<'ast, 'a> Visit<'ast> for FileVisitor<'a> {
        fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
            let mut inner = FnBodyVisitor {
                file_label: self.file_label,
                source: self.source,
                pre_mutate_seen: false,
                violations: Vec::new(),
            };
            inner.visit_block(&node.block);
            self.all_violations.extend(inner.violations);
            // Do NOT recurse — the per-fn visitor has covered nested
            // expressions; recursing the outer visitor would double-count.
        }

        fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
            let mut inner = FnBodyVisitor {
                file_label: self.file_label,
                source: self.source,
                pre_mutate_seen: false,
                violations: Vec::new(),
            };
            inner.visit_block(&node.block);
            self.all_violations.extend(inner.violations);
        }
    }

    let mut top = FileVisitor {
        file_label,
        source,
        all_violations: Vec::new(),
    };
    top.visit_file(parsed);
    top.all_violations
}

/// Best-effort line-number derivation from a method name + the original
/// source text. We search the source for the first occurrence of
/// `.<method_name>` and count newlines up to that byte offset.
///
/// Why not `proc_macro2::Span::start()`: that API requires the
/// `span-locations` feature on proc-macro2 which is not enableable from a
/// regular `dev-dependency` without forking syn's feature flags. The
/// source-text approach is dependency-free and good enough for the lint's
/// purpose — pointing the developer at the offending call site.
///
/// False-positive avoidance: when a method name appears multiple times in
/// the same source file, the first occurrence wins. The lint's purpose is
/// to flag the existence of an unguarded call site — the precise line is
/// only an aid for the developer to locate it. Two unguarded calls in the
/// same file are still both reported by the AST walker; only their
/// reported line numbers may collapse onto the first occurrence.
fn approx_line_from_source(source: &str, method_name: &str) -> usize {
    let needle = format!(".{}", method_name);
    let Some(byte_idx) = source.find(&needle) else {
        return 1;
    };
    source[..byte_idx].matches('\n').count() + 1
}

/// R9 — every destructive trait-call expression in modeltap-app's
/// orchestration / actions modules must be preceded by a
/// `revalidate::pre_mutate(...)` invocation in the same fn body.
///
/// This is THE load-bearing K5-extension lint: step 05-02 wired the four
/// current sites; this test pins the invariant so a future contributor
/// adding a 5th destructive call site without a guard fails CI at the
/// pre-merge step, before any production data can be corrupted.
#[test]
fn r9_pre_mutate_guard() {
    let mut all_violations: Vec<R9Violation> = Vec::new();
    let workspace = workspace_root();
    for file in r9_source_files() {
        let Ok(src) = std::fs::read_to_string(&file) else {
            continue;
        };
        let parsed = match syn::parse_file(&src) {
            Ok(p) => p,
            Err(e) => {
                panic!("R9 lint: failed to parse {}: {e}", file.display());
            }
        };
        let rel = file
            .strip_prefix(&workspace)
            .unwrap_or(&file)
            .display()
            .to_string();
        all_violations.extend(r9_walk_file_with_source(&parsed, &rel, Some(&src)));
    }
    assert!(
        all_violations.is_empty(),
        "R9 violations found (K5 invariant — destructive trait calls must \
         be preceded by revalidate::pre_mutate in the same fn body):\n  - {}",
        all_violations
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n  - ")
    );
}

/// Negative R9 — synthetic-fixture proof that the R9 walker actually
/// reports violations. Feeds the in-memory walker a tiny `syn::File`
/// containing an unguarded `tool.link(...)` call and asserts the returned
/// `Vec<R9Violation>` is non-empty, points to the synthetic file label,
/// and names `"link"` as the offending method.
///
/// This is the inverted-assertion shape per the step's acceptance criterion
/// #4: the negative test is itself a positive test — passes iff the lint
/// correctly identifies a deliberate violation.
#[test]
fn r9_walker_reports_unguarded_destructive_call() {
    let src = r#"
        async fn unguarded_link(tool: &dyn Tool, canonical: &Path, model: &ModelMeta) {
            // NO pre_mutate call above this line — the lint must report it.
            let _ = tool.link(canonical, model).await;
        }
    "#;
    let parsed = syn::parse_file(src).expect("synthetic source must parse");
    let violations = r9_walk_file(&parsed, "synthetic_unguarded.rs");
    assert!(
        !violations.is_empty(),
        "R9 walker failed to flag an unguarded `tool.link(...)` call — \
         the lint is broken (it would let a real violation through)"
    );
    let v = &violations[0];
    assert_eq!(
        v.file, "synthetic_unguarded.rs",
        "violation must carry the synthetic file label"
    );
    assert_eq!(
        v.method, "link",
        "violation must identify the destructive method"
    );
    assert!(v.line > 0, "violation must carry a non-zero line number");
}

/// Negative R9 — companion case proving the walker correctly RECOGNISES a
/// preceding `pre_mutate` and DOES NOT flag the destructive call. This
/// guards the walker against the opposite bug: false-positives on guarded
/// call sites. Without this test a bug that always reports violations
/// would still pass `r9_walker_reports_unguarded_destructive_call`.
#[test]
fn r9_walker_accepts_guarded_destructive_call() {
    let src = r#"
        async fn guarded_link(cache: &Cache, tool: &dyn Tool, canonical: &Path, model: &ModelMeta) {
            // Pre-mutate guard fires first — the lint must accept the call below.
            let _ = revalidate::pre_mutate(cache, &tool_id, &model_id, None).await;
            let _ = tool.link(canonical, model).await;
        }
    "#;
    let parsed = syn::parse_file(src).expect("synthetic source must parse");
    let violations = r9_walk_file(&parsed, "synthetic_guarded.rs");
    assert!(
        violations.is_empty(),
        "R9 walker incorrectly flagged a guarded `tool.link(...)` call: {violations:?}"
    );
}
