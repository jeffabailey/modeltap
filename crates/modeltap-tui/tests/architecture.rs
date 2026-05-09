//! Architecture lint: bottom-bar shortcut strings must live ONLY in
//! `keymap.rs` (the SHORTCUT_TABLE entries) or `render/bottom_bar.rs` (the
//! pure render fn that walks SHORTCUT_TABLE) — no hardcoded duplicates
//! anywhere else in the render layer.
//!
//! Per US-08 dod: "no duplicated shortcut definitions anywhere in the
//! codebase." This test greps the render/* modules for the well-known
//! shortcut tokens and asserts they only appear in the allowed files. If a
//! contributor copies a shortcut string into a new render module, this test
//! fails immediately — preventing INT-6 drift.

use std::path::{Path, PathBuf};

/// The shortcut tokens that may appear ONLY in the canonical files.
const FORBIDDEN_TOKENS: &[&str] = &[
    "[u] unify",
    "[z] zap tool",
    "[?] help",
    "[d] delete-from-one",
    "[Esc] back",
    "[<-/->] tools",
    "[up/down] models",
    "[up/down] tools",
];

/// Files allowed to contain the shortcut tokens. Any other render-layer file
/// containing one of FORBIDDEN_TOKENS fails the lint.
const ALLOWED_FILES: &[&str] = &[
    // Single source of truth.
    "keymap.rs",
    // Pure render fn that walks SHORTCUT_TABLE.
    "render/bottom_bar.rs",
    // Help overlay — generated from SHORTCUT_TABLE too.
    "screens/help_overlay.rs",
];

fn crate_src_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("src")
}

fn is_allowed(path: &Path, src_root: &Path) -> bool {
    let rel = path.strip_prefix(src_root).unwrap_or(path);
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    ALLOWED_FILES.iter().any(|allowed| rel_str == *allowed)
}

fn walk_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rs_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

#[test]
fn no_hardcoded_shortcut_strings_outside_allowed_files() {
    let src = crate_src_root();
    let mut files = Vec::new();
    walk_rs_files(&src, &mut files);

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        if is_allowed(file, &src) {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(file) else {
            continue;
        };
        for token in FORBIDDEN_TOKENS {
            if contents.contains(token) {
                violations.push(format!(
                    "{}: contains forbidden hardcoded shortcut token {:?} \
                     — move to keymap::SHORTCUT_TABLE",
                    file.strip_prefix(&src).unwrap().display(),
                    token
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Architecture-lint violations (US-08 single source of truth):\n  - {}",
        violations.join("\n  - ")
    );
}
