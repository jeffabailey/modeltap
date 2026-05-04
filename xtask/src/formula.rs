// xtask::formula — pure Tera-driven formula rendering.
//
// Per DESIGN component-boundaries.md §2.2 + data-models.md §1:
//   pub struct FormulaCtx { version, release_base_url, targets }
//   pub struct TargetEntry { triple, archive_name, sha256 }
//   pub fn render(template_text: &str, ctx: &FormulaCtx) -> Result<String, FormulaError>;
//
// Pure function: takes a template string + a context, returns the rendered
// formula text. The CLI dispatcher reads sidecars + writes output via
// fs_adapter — this module never touches disk.
//
// Implemented in DELIVER step 01-04 (Walking Skeleton, US-06 — TAP-BUMP).

use serde::Serialize;

use crate::cargo_toml::Version;

/// The four Homebrew DSL slots that a `TargetEntry` may populate.
///
/// The Tera template iterates `targets` and dispatches each entry into the
/// matching `on_macos`/`on_linux` × `on_arm`/`on_intel` block via this
/// enum (serialized as a snake-case string in the template's Tera context).
/// This makes block selection data-driven: adding a new supported triple is a
/// single match arm here + one template `{% if t.kind == "..." %}` block.
///
/// `from_triple` is the canonical mapping from rust target triples to slots;
/// the CLI dispatcher relies on it to decide whether a sidecar is "for a
/// supported target" and to enumerate the *expected* sidecar filenames so the
/// missing-sidecar error message can name them precisely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    MacArm,
    MacIntel,
    LinuxArm,
    LinuxIntel,
}

impl TargetKind {
    /// Map a rust target triple to its Homebrew DSL slot, or `None` if the
    /// triple is not one of the four supported targets (per ADR-012).
    pub fn from_triple(triple: &str) -> Option<Self> {
        match triple {
            "aarch64-apple-darwin" => Some(TargetKind::MacArm),
            "x86_64-apple-darwin" => Some(TargetKind::MacIntel),
            "aarch64-unknown-linux-gnu" => Some(TargetKind::LinuxArm),
            "x86_64-unknown-linux-gnu" => Some(TargetKind::LinuxIntel),
            _ => None,
        }
    }

    /// The rust target triple this slot corresponds to (inverse of
    /// `from_triple`). Used by the CLI dispatcher to enumerate the expected
    /// sidecar filenames when reporting which one is missing.
    pub fn triple(self) -> &'static str {
        match self {
            TargetKind::MacArm => "aarch64-apple-darwin",
            TargetKind::MacIntel => "x86_64-apple-darwin",
            TargetKind::LinuxArm => "aarch64-unknown-linux-gnu",
            TargetKind::LinuxIntel => "x86_64-unknown-linux-gnu",
        }
    }

    /// Iterate the four supported slots in declaration order. Used by the
    /// CLI to compute the full set of expected sidecar filenames for a
    /// release.
    pub fn all() -> [TargetKind; 4] {
        [
            TargetKind::MacArm,
            TargetKind::MacIntel,
            TargetKind::LinuxArm,
            TargetKind::LinuxIntel,
        ]
    }
}

/// Context passed to the Tera template. The walking-skeleton renders this with
/// exactly 1 entry in `targets`; R1 (post-WS step 02-04) renders 4 entries.
#[derive(Debug, Serialize)]
pub struct FormulaCtx {
    pub version: Version,
    pub release_base_url: String,
    pub targets: Vec<TargetEntry>,
}

/// Per-target archive identity. The Tera template iterates `targets` and
/// dispatches each entry into the matching `on_macos`/`on_linux` × `on_arm`/
/// `on_intel` Homebrew block via the `kind` discriminator (data-driven block
/// selection — adding a new triple is one TargetKind variant + one template
/// `{% if t.kind == "..." %}` block).
#[derive(Debug, Serialize)]
pub struct TargetEntry {
    /// Rust target triple, e.g., "aarch64-apple-darwin".
    pub triple: String,
    /// Homebrew DSL slot this entry populates. Derived from `triple` via
    /// `TargetKind::from_triple` at construction time so the template never
    /// has to re-classify.
    pub kind: TargetKind,
    /// Archive filename, e.g., "modeltap-0.2.0-aarch64-apple-darwin.tar.gz".
    pub archive_name: String,
    /// 64-hex-char lowercase sha256 from the artifact sidecar.
    pub sha256: String,
}

#[derive(Debug)]
pub enum FormulaError {
    /// Tera failed to parse the template text or render the context. Wraps the
    /// underlying tera::Error so the CLI can surface a sensible diagnostic.
    Tera(tera::Error),
    /// A sidecar file's content is not exactly 64 lowercase hex characters
    /// (the bare-hex sha256 format documented in data-models.md §4). Carries
    /// the offending sidecar filename so the maintainer can find the bad file.
    InvalidSidecar { filename: String },
}

impl std::fmt::Display for FormulaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormulaError::Tera(e) => write!(f, "formula template render failed: {e}"),
            FormulaError::InvalidSidecar { filename } => write!(
                f,
                "sidecar {filename} is not a bare 64-char lowercase hex sha256"
            ),
        }
    }
}

impl std::error::Error for FormulaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FormulaError::Tera(e) => Some(e),
            FormulaError::InvalidSidecar { .. } => None,
        }
    }
}

impl From<tera::Error> for FormulaError {
    fn from(e: tera::Error) -> Self {
        FormulaError::Tera(e)
    }
}

/// Render the Tera template against the formula context.
///
/// Algorithm:
/// 1. Build a one-shot `tera::Tera` instance with the supplied template text
///    registered under a fixed name (`modeltap.rb.tera`). One-shot avoids any
///    auto-discovery / glob-loading of unrelated templates.
/// 2. Build a `tera::Context` from `FormulaCtx` via `serde::Serialize`.
/// 3. Render and return the string.
///
/// Sidecar charset/length validation is the CLI dispatcher's responsibility
/// (see `is_valid_sha256`); by the time `FormulaCtx` is constructed, every
/// `sha256` field in `targets` is already known-good. This separation keeps
/// `render` a pure function over `(template, ctx) -> String` with no
/// per-string regex concerns.
pub fn render(template_text: &str, ctx: &FormulaCtx) -> Result<String, FormulaError> {
    const TEMPLATE_NAME: &str = "modeltap.rb.tera";
    let mut tera = tera::Tera::default();
    tera.add_raw_template(TEMPLATE_NAME, template_text)?;
    let tera_ctx = tera::Context::from_serialize(ctx)?;
    let rendered = tera.render(TEMPLATE_NAME, &tera_ctx)?;
    Ok(rendered)
}

/// Is `s` exactly 64 lowercase hex characters (the bare-hex sha256 format
/// documented in data-models.md §4)?
///
/// Used by the CLI dispatcher BEFORE constructing a `TargetEntry` so a malformed
/// sidecar fails fast with the offending filename, rather than silently producing
/// a malformed `sha256 "..."` line in the rendered formula.
///
/// Stdlib-only: a regex-crate dependency would be ~100KB of code for a check
/// that fits in three lines.
pub fn is_valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().expect("test fixture must be valid semver")
    }

    /// A minimal Tera template covering the substitutions exercised by the
    /// walking-skeleton acceptance test. We deliberately keep it tiny here
    /// (independent of the production `release/templates/modeltap.rb.tera`)
    /// so unit-test failures point at `render`, not the production template.
    const MINI_TEMPLATE: &str = r#"version "{{ version }}"
url "{{ release_base_url }}"
{%- for t in targets %}
{%- if t.triple == "x86_64-unknown-linux-gnu" %}
on_linux do
  on_intel do
    url "{{ release_base_url }}/{{ t.archive_name }}"
    sha256 "{{ t.sha256 }}"
  end
end
{%- endif %}
{%- endfor %}
"#;

    fn ws_ctx() -> FormulaCtx {
        FormulaCtx {
            version: v("0.0.1-rc1"),
            release_base_url:
                "https://github.com/jeffabailey/modeltap/releases/download/v0.0.1-rc1".to_owned(),
            targets: vec![TargetEntry {
                triple: "x86_64-unknown-linux-gnu".to_owned(),
                kind: TargetKind::LinuxIntel,
                archive_name: "modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz".to_owned(),
                sha256: "e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_owned(),
            }],
        }
    }

    // -------------------------------------------------------------------------
    // Behavior 1: render substitutes version, release_base_url, archive_name,
    // and sha256 into the on_linux/on_intel block (single-platform WS).
    // -------------------------------------------------------------------------

    #[test]
    fn render_substitutes_version_url_archive_and_sha256_for_single_target() {
        let out = render(MINI_TEMPLATE, &ws_ctx()).expect("render should succeed");

        assert!(
            out.contains("version \"0.0.1-rc1\""),
            "rendered formula must contain version field, got: {out}"
        );
        assert!(
            out.contains("modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz"),
            "rendered formula must contain archive name, got: {out}"
        );
        assert!(
            out.contains(
                "sha256 \"e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef\""
            ),
            "rendered formula must contain sha256 verbatim, got: {out}"
        );
        assert!(
            out.contains("on_linux") && out.contains("on_intel"),
            "rendered formula must contain the linux/intel block, got: {out}"
        );
    }

    // -------------------------------------------------------------------------
    // Behavior 2: render returns Err(FormulaError::Tera) on a malformed Tera
    // template. The dispatcher needs a typed error so it can surface a
    // diagnostic (rather than panic in the middle of a release run).
    // -------------------------------------------------------------------------

    #[test]
    fn render_returns_err_on_malformed_template() {
        // Unclosed `{%-` block tag — Tera's parser must reject this.
        let bad = "version \"{{ version }}\"\n{%- if foo \nbroken\n";
        let err = render(bad, &ws_ctx()).expect_err("malformed template must be Err");
        assert!(matches!(err, FormulaError::Tera(_)));
    }

    // -------------------------------------------------------------------------
    // Behavior 3: render returns Err when the template references a context
    // field that does not exist on FormulaCtx (e.g., a typo in the template).
    // -------------------------------------------------------------------------

    #[test]
    fn render_returns_err_on_missing_context_field() {
        // `nonexistent_field` is not a member of FormulaCtx. Tera's strict
        // mode (the default for unknown variables in expressions) fails.
        let template = "url \"{{ nonexistent_field }}\"\n";
        let err = render(template, &ws_ctx()).expect_err("missing field must be Err");
        assert!(matches!(err, FormulaError::Tera(_)));
    }

    // -------------------------------------------------------------------------
    // Behavior 4: when only one target is supplied, the other three Homebrew
    // blocks remain unpopulated. The MINI_TEMPLATE only emits the linux/intel
    // block, so we verify by checking that no other triple's archive name leaks
    // into the output.
    // -------------------------------------------------------------------------

    #[test]
    fn render_leaves_other_platform_blocks_empty_for_single_target_ws() {
        let out = render(MINI_TEMPLATE, &ws_ctx()).expect("render should succeed");
        for other_triple in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "aarch64-unknown-linux-gnu",
        ] {
            assert!(
                !out.contains(other_triple),
                "WS render must not emit {other_triple}, got: {out}"
            );
        }
    }

    // -------------------------------------------------------------------------
    // Behavior 5: is_valid_sha256 accepts exactly 64 lowercase hex chars.
    // -------------------------------------------------------------------------

    #[test]
    fn is_valid_sha256_accepts_canonical_64_lowercase_hex() {
        // Sample produced via `sha256sum` on a real archive; canonical form.
        let canonical = "e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert!(is_valid_sha256(canonical));
        // All-zeros and all-fs are also valid 64-hex strings.
        assert!(is_valid_sha256(&"0".repeat(64)));
        assert!(is_valid_sha256(&"f".repeat(64)));
    }

    // -------------------------------------------------------------------------
    // Behavior 6: is_valid_sha256 rejects malformed inputs (parametrised).
    //
    // One #[test] per case so the failing case is immediately identifiable in
    // test output. Each line is a separate input variation of the same behavior;
    // counted as ONE behavior (Mandate 5, parametrise input variations).
    // -------------------------------------------------------------------------

    #[test]
    fn is_valid_sha256_rejects_too_short() {
        // 63 chars
        assert!(!is_valid_sha256(&"a".repeat(63)));
    }

    #[test]
    fn is_valid_sha256_rejects_too_long() {
        // 65 chars
        assert!(!is_valid_sha256(&"a".repeat(65)));
    }

    #[test]
    fn is_valid_sha256_rejects_uppercase_and_non_hex_chars() {
        // 64 chars but uppercase A — sha256sum emits lowercase, so we reject.
        assert!(!is_valid_sha256(&format!("{}A", "a".repeat(63))));
        // 64 chars but contains a non-hex letter (G).
        assert!(!is_valid_sha256(&format!("{}g", "a".repeat(63))));
        // Empty string + whitespace are trivially invalid.
        assert!(!is_valid_sha256(""));
        assert!(!is_valid_sha256(&" ".repeat(64)));
        // Trailing newline (a common sidecar trap) — caller must trim() first.
        assert!(!is_valid_sha256(&format!("{}\n", "a".repeat(63))));
    }

    // -------------------------------------------------------------------------
    // Step 02-04 — Multi-arch (4-platform) coverage.
    // -------------------------------------------------------------------------

    /// A FormulaCtx populated with the four supported targets, each with a
    /// structurally-distinct sha256 so we can prove block↔sha256 wiring (not
    /// just "some sha256 was emitted four times").
    fn four_target_ctx() -> FormulaCtx {
        FormulaCtx {
            version: v("0.2.0"),
            release_base_url: "https://github.com/jeffabailey/modeltap/releases/download/v0.2.0"
                .to_owned(),
            targets: vec![
                TargetEntry {
                    triple: "aarch64-apple-darwin".to_owned(),
                    kind: TargetKind::MacArm,
                    archive_name: "modeltap-0.2.0-aarch64-apple-darwin.tar.gz".to_owned(),
                    sha256: "a".repeat(64),
                },
                TargetEntry {
                    triple: "x86_64-apple-darwin".to_owned(),
                    kind: TargetKind::MacIntel,
                    archive_name: "modeltap-0.2.0-x86_64-apple-darwin.tar.gz".to_owned(),
                    sha256: "b".repeat(64),
                },
                TargetEntry {
                    triple: "x86_64-unknown-linux-gnu".to_owned(),
                    kind: TargetKind::LinuxIntel,
                    archive_name: "modeltap-0.2.0-x86_64-unknown-linux-gnu.tar.gz".to_owned(),
                    sha256: "c".repeat(64),
                },
                TargetEntry {
                    triple: "aarch64-unknown-linux-gnu".to_owned(),
                    kind: TargetKind::LinuxArm,
                    archive_name: "modeltap-0.2.0-aarch64-unknown-linux-gnu.tar.gz".to_owned(),
                    sha256: "d".repeat(64),
                },
            ],
        }
    }

    // -------------------------------------------------------------------------
    // Behavior 7: TargetKind classifies the 4 supported rust target triples
    // into the 4 Homebrew DSL slots (mac_arm | mac_intel | linux_arm |
    // linux_intel). Data-driven block selection in the template depends on
    // this classification being total over the supported set.
    // -------------------------------------------------------------------------

    #[test]
    fn target_kind_classifies_all_four_supported_triples() {
        assert_eq!(
            TargetKind::from_triple("aarch64-apple-darwin"),
            Some(TargetKind::MacArm)
        );
        assert_eq!(
            TargetKind::from_triple("x86_64-apple-darwin"),
            Some(TargetKind::MacIntel)
        );
        assert_eq!(
            TargetKind::from_triple("aarch64-unknown-linux-gnu"),
            Some(TargetKind::LinuxArm)
        );
        assert_eq!(
            TargetKind::from_triple("x86_64-unknown-linux-gnu"),
            Some(TargetKind::LinuxIntel)
        );
    }

    // -------------------------------------------------------------------------
    // Behavior 8: TargetKind returns None for any unsupported triple. This
    // is the gate that lets the CLI dispatcher distinguish "supported but
    // missing" sidecars from "stray sidecar for an unsupported triple".
    // -------------------------------------------------------------------------

    #[test]
    fn target_kind_returns_none_for_unsupported_triples() {
        assert!(TargetKind::from_triple("riscv64gc-unknown-linux-gnu").is_none());
        assert!(TargetKind::from_triple("wasm32-unknown-unknown").is_none());
        assert!(TargetKind::from_triple("").is_none());
        assert!(TargetKind::from_triple("not-a-triple").is_none());
    }

    // -------------------------------------------------------------------------
    // Behavior 9: render() against the production template emits all 4
    // platform blocks and wires each sha256 to the matching triple's archive
    // URL (round-trip wiring).
    // -------------------------------------------------------------------------

    #[test]
    fn render_emits_all_four_platform_blocks_with_correct_sha256_wiring() {
        let template = include_str!("../../release/templates/modeltap.rb.tera");
        let out = render(template, &four_target_ctx()).expect("render should succeed");

        // All four DSL keywords appear.
        assert!(out.contains("on_macos"), "missing on_macos: {out}");
        assert!(out.contains("on_linux"), "missing on_linux: {out}");
        assert_eq!(
            out.matches("on_arm").count(),
            2,
            "expected 2 on_arm blocks, got: {out}"
        );
        assert_eq!(
            out.matches("on_intel").count(),
            2,
            "expected 2 on_intel blocks, got: {out}"
        );

        // For each target, the sha256 appears in proximity (within 200 chars)
        // of its archive URL — proving wire-up not just "all shas appear".
        let cases: [(&str, char); 4] = [
            ("aarch64-apple-darwin", 'a'),
            ("x86_64-apple-darwin", 'b'),
            ("x86_64-unknown-linux-gnu", 'c'),
            ("aarch64-unknown-linux-gnu", 'd'),
        ];
        for (triple, byte) in cases {
            let archive = format!("modeltap-0.2.0-{triple}.tar.gz");
            let idx = out
                .find(&archive)
                .unwrap_or_else(|| panic!("missing {triple} url in {out}"));
            let window = &out[idx..(idx + 200).min(out.len())];
            let expected_sha = std::iter::repeat_n(byte, 64).collect::<String>();
            assert!(
                window.contains(&format!("sha256 \"{expected_sha}\"")),
                "sha256 for {triple} not wired to its url; window: {window}"
            );
        }
    }

    // -------------------------------------------------------------------------
    // Behavior 10: round-trip property — every sha256 in the rendered formula
    // equals the verbatim sha256 from its TargetEntry input. We extract the
    // 64-hex digest set from the rendered text and assert equality with the
    // input set.
    // -------------------------------------------------------------------------

    // -------------------------------------------------------------------------
    // Mutation-coverage: TargetKind::triple emits the EXACT canonical triple
    // string for each variant. These pin the strings so the following mutants
    // are killed:
    //   - replace TargetKind::triple -> &'static str with ""
    //   - replace TargetKind::triple -> &'static str with "xyzzy"
    // -------------------------------------------------------------------------

    #[test]
    fn target_kind_triple_returns_canonical_string_for_each_variant() {
        assert_eq!(TargetKind::MacArm.triple(), "aarch64-apple-darwin");
        assert_eq!(TargetKind::MacIntel.triple(), "x86_64-apple-darwin");
        assert_eq!(TargetKind::LinuxArm.triple(), "aarch64-unknown-linux-gnu");
        assert_eq!(TargetKind::LinuxIntel.triple(), "x86_64-unknown-linux-gnu");
    }

    #[test]
    fn target_kind_triple_round_trips_through_from_triple() {
        for kind in TargetKind::all() {
            let triple = kind.triple();
            assert_eq!(
                TargetKind::from_triple(triple),
                Some(kind),
                "triple() must be the inverse of from_triple() for {kind:?}"
            );
        }
    }

    // -------------------------------------------------------------------------
    // Mutation-coverage: Display + Error::source for FormulaError.
    //
    // Pins the EXACT format text and the source-chain so the following mutants
    // are killed:
    //   - <impl Display for FormulaError>::fmt -> Ok(Default::default())
    //   - <impl Error for FormulaError>::source -> None
    // -------------------------------------------------------------------------

    #[test]
    fn display_for_invalid_sidecar_uses_exact_phrasing() {
        let err = FormulaError::InvalidSidecar {
            filename: "modeltap-0.2.0-x86_64-apple-darwin.tar.gz.sha256".to_owned(),
        };
        assert_eq!(
            format!("{err}"),
            "sidecar modeltap-0.2.0-x86_64-apple-darwin.tar.gz.sha256 is not a bare 64-char lowercase hex sha256"
        );
    }

    #[test]
    fn display_for_tera_starts_with_exact_prefix_and_includes_inner() {
        // Build a real tera::Error by triggering a parse failure.
        let mut tera = tera::Tera::default();
        let inner = tera
            .add_raw_template("bad", "{%- if foo \nbroken")
            .expect_err("parse must fail");
        let inner_msg = inner.to_string();
        let err = FormulaError::Tera(inner);
        let msg = format!("{err}");
        assert!(
            msg.starts_with("formula template render failed: "),
            "Tera Display must use exact prefix, got: {msg}"
        );
        assert!(
            msg.contains(&inner_msg),
            "Tera Display must include underlying tera error, got: {msg}"
        );
    }

    #[test]
    fn source_returns_inner_for_tera_only() {
        use std::error::Error as _;

        let mut tera = tera::Tera::default();
        let inner = tera
            .add_raw_template("bad", "{%- if foo \nbroken")
            .expect_err("parse must fail");
        let tera_err = FormulaError::Tera(inner);
        assert!(
            tera_err.source().is_some(),
            "Tera variant must expose its inner tera::Error via source()"
        );

        let invalid = FormulaError::InvalidSidecar {
            filename: "bad.sha256".to_owned(),
        };
        assert!(
            invalid.source().is_none(),
            "InvalidSidecar must not have a source"
        );
    }

    #[test]
    fn render_round_trip_preserves_every_sha256_verbatim() {
        let template = include_str!("../../release/templates/modeltap.rb.tera");
        let ctx = four_target_ctx();
        let out = render(template, &ctx).expect("render should succeed");

        // Extract every `sha256 "<64-hex>"` literal from the rendered text.
        // We do NOT depend on a regex crate; a hand-rolled scan is enough
        // because the literal shape is fixed by the template.
        let mut found: Vec<String> = Vec::new();
        for line in out.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("sha256 \"") {
                if let Some(hex) = rest.strip_suffix('"') {
                    found.push(hex.to_owned());
                }
            }
        }
        found.sort();

        let mut expected: Vec<String> = ctx.targets.iter().map(|t| t.sha256.clone()).collect();
        expected.sort();

        assert_eq!(
            found, expected,
            "rendered sha256 set must equal input sha256 set verbatim"
        );
    }
}
