// xtask::tag — pure tag-validation logic.
//
// Per DESIGN component-boundaries.md §2.2:
//   pub fn assert_tag_matches(tag: &str, version: &Version) -> Result<(), VersionError>;
//
// Pure function: takes a tag string and a Version, returns Result. No I/O.
//
// Implemented in DELIVER step 01-02 (Walking Skeleton, US-02 — TAG activity).

use crate::cargo_toml::{Version, VersionError};

/// Assert that `tag` equals `format!("v{version}")` exactly.
///
/// Case-sensitive byte-for-byte comparison. Any divergence — wrong patch/minor/
/// major, missing leading `v`, extra leading/trailing whitespace, mixed case —
/// yields `Err(VersionError::TagMismatch { tag, version })` whose Display reads
///
///   tag <tag> does not match workspace version <version>
///
/// per US-02.AC-5 / walking-skeleton.feature scenario "Validate-tag rejects a
/// tag that does not match the workspace version".
pub fn assert_tag_matches(tag: &str, version: &Version) -> Result<(), VersionError> {
    let expected = format!("v{version}");
    if tag == expected {
        return Ok(());
    }
    Err(VersionError::TagMismatch {
        tag: tag.to_owned(),
        version: version.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().expect("test fixture must be valid semver")
    }

    // -------------------------------------------------------------------------
    // Behavior 1: returns Ok when tag matches exactly.
    // -------------------------------------------------------------------------

    #[test]
    fn returns_ok_when_tag_equals_v_plus_version() {
        assert_tag_matches("v0.1.0", &v("0.1.0")).expect("matching tag should be Ok");
    }

    #[test]
    fn returns_ok_for_prerelease_tag_that_matches() {
        // Per walking-skeleton.feature outline: "v0.0.1-rc1" matches "0.0.1-rc1".
        assert_tag_matches("v0.0.1-rc1", &v("0.0.1-rc1"))
            .expect("matching prerelease tag should be Ok");
    }

    // -------------------------------------------------------------------------
    // Behavior 2: returns Err on version mismatch (parametrised).
    //
    // One #[test] per case rather than table-iteration, so the failing case is
    // immediately identifiable in test output. Each line is a separate input
    // variation of the same behavior; counted as ONE behavior (Mandate 5 in
    // CRAFTER.md, parametrise input variations).
    // -------------------------------------------------------------------------

    #[test]
    fn returns_err_when_patch_differs() {
        let err = assert_tag_matches("v0.1.1", &v("0.1.0")).expect_err("patch mismatch");
        assert!(matches!(err, VersionError::TagMismatch { .. }));
    }

    #[test]
    fn returns_err_when_minor_differs() {
        let err = assert_tag_matches("v0.2.0", &v("0.1.0")).expect_err("minor mismatch");
        assert!(matches!(err, VersionError::TagMismatch { .. }));
    }

    #[test]
    fn returns_err_when_major_differs() {
        let err = assert_tag_matches("v1.0.0", &v("0.1.0")).expect_err("major mismatch");
        assert!(matches!(err, VersionError::TagMismatch { .. }));
    }

    // -------------------------------------------------------------------------
    // Behavior 3: returns Err when leading `v` is missing.
    // -------------------------------------------------------------------------

    #[test]
    fn returns_err_when_leading_v_is_missing() {
        let err = assert_tag_matches("0.1.0", &v("0.1.0")).expect_err("missing v prefix");
        assert!(matches!(err, VersionError::TagMismatch { .. }));
    }

    // -------------------------------------------------------------------------
    // Behavior 4: returns Err when tag has stray whitespace.
    // -------------------------------------------------------------------------

    #[test]
    fn returns_err_when_tag_has_trailing_whitespace() {
        let err = assert_tag_matches("v0.1.0 ", &v("0.1.0")).expect_err("trailing whitespace");
        assert!(matches!(err, VersionError::TagMismatch { .. }));
    }

    // -------------------------------------------------------------------------
    // Behavior 5: error message format contains both tag and version.
    //
    // Required by US-02.AC-5 and the walking-skeleton scenario which asserts
    // the literal string "tag v0.2.0 does not match workspace version 0.1.0".
    // -------------------------------------------------------------------------

    #[test]
    fn error_message_contains_both_tag_and_version_with_required_phrasing() {
        let err = assert_tag_matches("v0.2.0", &v("0.1.0")).expect_err("mismatch");
        let msg = err.to_string();
        assert!(
            msg.contains("v0.2.0"),
            "error message must contain offending tag, got: {msg}"
        );
        assert!(
            msg.contains("0.1.0"),
            "error message must contain expected version, got: {msg}"
        );
        assert!(
            msg.contains("does not match workspace version"),
            "error message must use the AC-5 phrasing, got: {msg}"
        );
    }

    // -------------------------------------------------------------------------
    // Property: assert_tag_matches succeeds iff tag == format!("v{version}").
    //
    // The walking-skeleton.feature outline says exactly this. We sample over
    // arbitrary semver triples and a small lexicon of plausible-but-wrong
    // tag mutations (missing v, extra suffix, etc.) plus the canonical match.
    // -------------------------------------------------------------------------

    proptest::proptest! {
        #[test]
        fn iff_invariant_tag_equals_v_plus_version(
            major in 0u64..1000,
            minor in 0u64..1000,
            patch in 0u64..1000,
        ) {
            let version = Version(semver::Version::new(major, minor, patch));
            let canonical = format!("v{version}");

            // Must succeed for the canonical form.
            proptest::prop_assert!(assert_tag_matches(&canonical, &version).is_ok());

            // Must fail when the leading `v` is dropped.
            let no_v = format!("{version}");
            proptest::prop_assert!(assert_tag_matches(&no_v, &version).is_err());

            // Must fail with an unrelated suffix.
            let extra = format!("{canonical}-extra");
            proptest::prop_assert!(assert_tag_matches(&extra, &version).is_err());
        }
    }
}
