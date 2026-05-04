// xtask::cargo_toml — pure functions for Cargo.toml version reasoning.
//
// Per DESIGN component-boundaries.md §2.2:
//   pub fn parse_workspace_version(cargo_toml: &str) -> Result<Version, VersionError>;
//   pub fn assert_monotonic(current: &Version, proposed: &Version) -> Result<(), VersionError>;
//
// Pure functions: take strings/structs in, return Results out. No I/O.
// File reading is the caller's responsibility (the CLI dispatcher).

/// Newtype around `semver::Version` so the rest of xtask depends on a single
/// version type owned by this module.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub semver::Version);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

// Serialize as the canonical semver string ("0.2.0", "0.0.1-rc1") so the Tera
// template can reference `{{ version }}` directly. Implemented manually rather
// than enabling `semver/serde` to keep the dependency graph minimal.
impl serde::Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_str(&self.0)
    }
}

impl std::str::FromStr for Version {
    type Err = semver::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        semver::Version::parse(s).map(Version)
    }
}

#[derive(Debug)]
pub enum VersionError {
    MissingField,
    ParseFailed(semver::Error),
    NotMonotonic {
        current: Version,
        proposed: Version,
    },
    /// The git tag the maintainer pushed does not equal `format!("v{version}")`.
    /// Surfaced by `xtask::tag::assert_tag_matches` (DELIVER step 01-02).
    TagMismatch {
        tag: String,
        version: Version,
    },
}

impl std::fmt::Display for VersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VersionError::MissingField => {
                write!(f, "[workspace.package].version field is missing")
            }
            VersionError::ParseFailed(e) => {
                write!(f, "failed to parse semver version: {e}")
            }
            VersionError::NotMonotonic { current, proposed } => {
                write!(
                    f,
                    "proposed version {proposed} is not greater than current {current}"
                )
            }
            VersionError::TagMismatch { tag, version } => {
                // Exact phrasing required by US-02.AC-5 and walking-skeleton.feature
                // scenario "Validate-tag rejects a tag that does not match the
                // workspace version".
                write!(f, "tag {tag} does not match workspace version {version}")
            }
        }
    }
}

impl std::error::Error for VersionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VersionError::ParseFailed(e) => Some(e),
            _ => None,
        }
    }
}

/// Parse `[workspace.package].version` from Cargo.toml text.
///
/// Uses `toml_edit` (rather than the stricter `toml` crate) so unrelated parse
/// errors elsewhere in the file don't mask the simple "version field present?"
/// question. Comments and unusual formatting in Cargo.toml are tolerated.
pub fn parse_workspace_version(cargo_toml_text: &str) -> Result<Version, VersionError> {
    let doc: toml_edit::DocumentMut = cargo_toml_text
        .parse()
        .map_err(|_| VersionError::MissingField)?;

    let raw = doc
        .get("workspace")
        .and_then(|w| w.as_table())
        .and_then(|t| t.get("package"))
        .and_then(|p| p.as_table())
        .and_then(|t| t.get("version"))
        .and_then(|v| v.as_str())
        .ok_or(VersionError::MissingField)?;

    semver::Version::parse(raw)
        .map(Version)
        .map_err(VersionError::ParseFailed)
}

/// Assert that `proposed` is strictly greater than `current` per semver order.
///
/// Returns `Err(VersionError::NotMonotonic { ... })` whose Display includes
/// both versions in the form expected by the acceptance scenario:
/// `"proposed version <p> is not greater than current <c>"`.
pub fn assert_monotonic(current: &Version, proposed: &Version) -> Result<(), VersionError> {
    if proposed.0 > current.0 {
        Ok(())
    } else {
        Err(VersionError::NotMonotonic {
            current: current.clone(),
            proposed: proposed.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_CARGO_TOML: &str = r#"
[workspace]
resolver = "2"
members = ["a", "b"]

[workspace.package]
version = "0.2.0"
edition = "2021"
"#;

    const MISSING_VERSION_CARGO_TOML: &str = r#"
[workspace]
resolver = "2"
members = ["a"]

[workspace.package]
edition = "2021"
"#;

    const MALFORMED_VERSION_CARGO_TOML: &str = r#"
[workspace.package]
version = "not-a-semver"
"#;

    #[test]
    fn parse_workspace_version_returns_version_for_valid_cargo_toml() {
        let v = parse_workspace_version(VALID_CARGO_TOML).expect("should parse");
        assert_eq!(v.to_string(), "0.2.0");
    }

    #[test]
    fn parse_workspace_version_returns_missing_field_error_when_version_absent() {
        let err = parse_workspace_version(MISSING_VERSION_CARGO_TOML)
            .expect_err("should fail on missing field");
        assert!(matches!(err, VersionError::MissingField));
    }

    #[test]
    fn parse_workspace_version_returns_parse_failed_when_version_is_not_semver() {
        let err = parse_workspace_version(MALFORMED_VERSION_CARGO_TOML)
            .expect_err("should fail on malformed semver");
        assert!(matches!(err, VersionError::ParseFailed(_)));
    }

    fn v(s: &str) -> Version {
        s.parse().expect("test fixture must be valid semver")
    }

    #[test]
    fn assert_monotonic_ok_for_patch_minor_major_bumps() {
        // Patch bump
        assert_monotonic(&v("0.1.0"), &v("0.1.1")).expect("patch bump should be allowed");
        // Minor bump
        assert_monotonic(&v("0.1.0"), &v("0.2.0")).expect("minor bump should be allowed");
        // Major bump
        assert_monotonic(&v("0.1.0"), &v("1.0.0")).expect("major bump should be allowed");
    }

    #[test]
    fn assert_monotonic_err_when_proposed_equals_current() {
        let err = assert_monotonic(&v("0.2.0"), &v("0.2.0")).expect_err("equal should fail");
        assert!(matches!(err, VersionError::NotMonotonic { .. }));
    }

    #[test]
    fn assert_monotonic_err_when_proposed_less_than_current() {
        let err = assert_monotonic(&v("0.2.0"), &v("0.1.5")).expect_err("regression should fail");
        assert!(matches!(err, VersionError::NotMonotonic { .. }));
    }

    #[test]
    fn assert_monotonic_error_message_contains_both_versions() {
        // Exact wording per acceptance scenario in walking-skeleton.feature:
        // "proposed version 0.1.5 is not greater than current 0.2.0"
        let err = assert_monotonic(&v("0.2.0"), &v("0.1.5")).expect_err("regression should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("0.1.5"),
            "error message must contain proposed version, got: {msg}"
        );
        assert!(
            msg.contains("0.2.0"),
            "error message must contain current version, got: {msg}"
        );
        assert!(
            msg.contains("not greater than"),
            "error message must explain monotonicity violation, got: {msg}"
        );
    }

    // -------------------------------------------------------------------------
    // Mutation-coverage: Display + Error::source for VersionError.
    //
    // These pin the EXACT format text and the source-chain wiring so the
    // following mutations are killed:
    //   - <impl Display for VersionError>::fmt -> Ok(Default::default())
    //   - <impl Error for VersionError>::source -> None
    //   - delete match arm VersionError::ParseFailed(e) in source
    // -------------------------------------------------------------------------

    use std::error::Error as _;

    #[test]
    fn display_for_missing_field_is_exact_text() {
        let err = VersionError::MissingField;
        assert_eq!(
            format!("{err}"),
            "[workspace.package].version field is missing"
        );
    }

    #[test]
    fn display_for_parse_failed_includes_underlying_error() {
        let semver_err = semver::Version::parse("not-a-semver").expect_err("must fail");
        let inner_msg = semver_err.to_string();
        let err = VersionError::ParseFailed(semver_err);
        let msg = format!("{err}");
        assert!(
            msg.starts_with("failed to parse semver version: "),
            "ParseFailed Display must use exact prefix, got: {msg}"
        );
        assert!(
            msg.contains(&inner_msg),
            "ParseFailed Display must include underlying semver error, got: {msg}"
        );
    }

    #[test]
    fn display_for_not_monotonic_uses_exact_phrasing() {
        let err = VersionError::NotMonotonic {
            current: v("0.2.0"),
            proposed: v("0.1.5"),
        };
        assert_eq!(
            format!("{err}"),
            "proposed version 0.1.5 is not greater than current 0.2.0"
        );
    }

    #[test]
    fn display_for_tag_mismatch_uses_exact_phrasing() {
        let err = VersionError::TagMismatch {
            tag: "v0.1.5".to_owned(),
            version: v("0.2.0"),
        };
        assert_eq!(
            format!("{err}"),
            "tag v0.1.5 does not match workspace version 0.2.0"
        );
    }

    #[test]
    fn source_returns_inner_for_parse_failed_only() {
        // ParseFailed wraps a semver::Error — source MUST chain through.
        let semver_err = semver::Version::parse("not-a-semver").expect_err("must fail");
        let parse_failed = VersionError::ParseFailed(semver_err);
        assert!(
            parse_failed.source().is_some(),
            "ParseFailed must expose its inner semver error via source()"
        );
        // All other variants do NOT have a source.
        assert!(VersionError::MissingField.source().is_none());
        assert!(VersionError::NotMonotonic {
            current: v("0.2.0"),
            proposed: v("0.1.5"),
        }
        .source()
        .is_none());
        assert!(VersionError::TagMismatch {
            tag: "v0.1.5".to_owned(),
            version: v("0.2.0"),
        }
        .source()
        .is_none());
    }

    // proptest invariant: bumping forward is always allowed.
    // For any (a, b) with a < b, assert_monotonic(b, a) must return Ok
    // (here `b` is the proposed and `a` is the current — proposed > current).
    proptest::proptest! {
        #[test]
        fn forward_bump_always_allowed(
            major_a in 0u64..100,
            minor_a in 0u64..100,
            patch_a in 0u64..100,
            bump in 1u64..1000,
        ) {
            // Construct two versions where second is strictly greater.
            let a = Version(semver::Version::new(major_a, minor_a, patch_a));
            let b = Version(semver::Version::new(major_a, minor_a, patch_a + bump));
            // current = a, proposed = b, and b > a → must succeed.
            proptest::prop_assert!(assert_monotonic(&a, &b).is_ok());
        }
    }
}
