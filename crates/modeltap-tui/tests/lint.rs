//! Architecture-lint: `keymap.rs` must NEVER contain a hardcoded
//! `"<author>/<repo>"`-shaped string literal (INT-FGD-7 / US-05c.AC-19).
//!
//! Rationale (D6, INT-FGD-7): the typed-input comparator for the
//! folder-confirm dialog reads `folder_group.path` (the canonical artifact)
//! and compares it byte-exact to the user's typed input. If `keymap.rs`
//! were to embed a literal `"bartowski/Llama-3.2..."` (or any other
//! `<word>/<word>`-shaped string), that literal could drift from the data
//! model and the dialog could silently accept the wrong path. The
//! single source of truth is `FolderGroup::path` — and only that field.
//!
//! This test greps `crates/modeltap-tui/src/keymap.rs` for any line that
//! looks like an HF repo path literal: a string that, after stripping all
//! whitespace and quotes, matches the regex `^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$`.
//! Strings that contain spaces or other separators (e.g. `"[<-/->] tools"`,
//! `"up/down models"`) are excluded — they are bar-label tokens, not
//! repo-path literals.

use std::path::PathBuf;

fn keymap_path() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join("src/keymap.rs")
}

/// Returns true iff `s` (already stripped of whitespace + surrounding quotes)
/// looks like an HF repo path literal: exactly one `/`, both sides non-empty
/// and made of identifier-safe characters.
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
        // Require at least one alphabetic char so single-symbol "tool" labels
        // like "[<-" / "->]" cannot slip through; otherwise restrict to the
        // identifier-safe charset that an HF repo path uses.
        let has_alpha = part.chars().any(|c| c.is_ascii_alphabetic());
        let all_safe = part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        has_alpha && all_safe
    }
    ident_safe(author) && ident_safe(repo)
}

/// Extract every double-quoted string literal from `source`. Naive — does
/// not handle escaped quotes inside literals — but that is acceptable here
/// because `keymap.rs` does not use escapes inside any of its literals.
fn extract_string_literals(source: &str) -> Vec<String> {
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
            out.push(lit);
        }
    }
    out
}

#[test]
fn keymap_rs_contains_no_repo_path_shaped_literal() {
    let path = keymap_path();
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));

    let mut violations: Vec<String> = Vec::new();
    for lit in extract_string_literals(&source) {
        if is_repo_path_shaped(&lit) {
            violations.push(lit);
        }
    }

    assert!(
        violations.is_empty(),
        "keymap.rs must NOT contain a `<author>/<repo>`-shaped literal \
         (INT-FGD-7): folder-confirm comparator MUST read folder_group.path \
         exclusively. Violations: {:?}",
        violations
    );
}
