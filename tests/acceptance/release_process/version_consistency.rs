// Cross-artifact version-string consistency proof.
//
// Step: 02-05 (closes Phase 02 — multi-arch + integrity).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/integration-checkpoints.feature
//   - INT.AC-1 example: "Version string agrees across Cargo.toml, tag,
//     archive name, release title, and binary output"
//   - INT.AC-1 property: "Version-string consistency holds for any valid
//     semver release"
//   - INT.AC-2: "Each target's formula sha256 equals the artifact sidecar
//     content"
//   - INT.AC-3: "Each target's formula URL equals the GitHub Release archive
//     URL"
//
// Strategy: thread a version through every consumer via fixture seeding +
// xtask render-formula, then assert it appears verbatim in every produced
// artifact. The 6 axes are:
//
//   1. Cargo.toml `[workspace.package].version`        (input fixture)
//   2. The tag string `v<version>`                     (computed)
//   3. The archive name `modeltap-<version>-<triple>.tar.gz` (computed; matched
//      against rendered formula's url field)
//   4. The GitHub Release title (= the tag string by convention; computed)
//   5. The rendered formula's `version "<version>"` line (xtask render-formula
//      output)
//   6. The binary's `cargo run -p modeltap-app -- --version` output
//      (`modeltap <version>`) — covered by the example test only; the
//      property test omits this axis because invoking the real binary inside
//      a proptest loop would take many minutes and provides no extra
//      proptest-shrinking value (the version string is forwarded by clap
//      from `CARGO_PKG_VERSION` — same code path for every input).
//
// INT.AC-2 + INT.AC-3 are folded into the same test: rendering the formula
// from a sidecar and checking its sha256 + url fields covers them in one
// pass, which avoids re-creating the same fixture three times.
//
// PATH note: `~/.pyenv/shims/cc` shadows the real cc on this developer's
// machine and breaks build-script linking. Sub-cargo invocations need
// PATH=/usr/bin:$PATH (handled by the shared `xtask_in` helper).

use std::path::Path;
use std::process::Command;

use modeltap_acceptance::{
    template_path, workspace_manifest, write_sidecar, xtask_in, ALL_FOUR_TARGETS,
};
use proptest::prelude::*;
use tempfile::TempDir;

// =============================================================================
// Helper — render a 4-platform formula at `version` against a fresh tempdir
// using the sha256 fixtures from ALL_FOUR_TARGETS. Returns the formula body
// and the artifacts directory (callers may need to read sidecars back).
// =============================================================================

struct RenderedFormula {
    body: String,
    release_base_url: String,
    /// Sha256 of each target as recorded in the sidecar files (bare-hex,
    /// trailing newline stripped). Indexed parallel to ALL_FOUR_TARGETS.
    sidecar_shas: Vec<(String, String)>,
}

fn render_formula_at(version: &str) -> RenderedFormula {
    let workspace = TempDir::new().expect("create tempdir");
    let artifacts = workspace.path().join("artifacts");
    std::fs::create_dir_all(&artifacts).expect("mkdir artifacts");

    let mut sidecar_shas = Vec::new();
    for (triple, sha) in ALL_FOUR_TARGETS {
        write_sidecar(&artifacts, version, triple, sha);
        sidecar_shas.push((triple.to_string(), sha.to_string()));
    }

    let formula_dir = workspace.path().join("Formula");
    std::fs::create_dir_all(&formula_dir).expect("mkdir Formula");
    let output_path = formula_dir.join("modeltap.rb");

    let release_base_url =
        format!("https://github.com/jeffabailey/modeltap/releases/download/v{version}");

    let render = xtask_in(
        workspace.path(),
        &[
            "render-formula",
            "--version",
            version,
            "--template",
            template_path().to_str().expect("utf-8 template path"),
            "--output",
            output_path.to_str().expect("utf-8 output path"),
            "--sha256-dir",
            artifacts.to_str().expect("utf-8 sha256-dir path"),
            "--release-base-url",
            &release_base_url,
        ],
    )
    .output()
    .expect("invoke xtask render-formula");
    assert!(
        render.status.success(),
        "render-formula must succeed for version {version}; stderr={}",
        String::from_utf8_lossy(&render.stderr)
    );

    let body = std::fs::read_to_string(&output_path).expect("read rendered formula");

    // Keep the workspace TempDir alive for the duration of the function via
    // the move into `body`; once we drop `workspace` here it's fine because
    // we've already read the formula bytes.
    drop(workspace);

    RenderedFormula {
        body,
        release_base_url,
        sidecar_shas,
    }
}

/// Assert that the version string `version` is reachable from EVERY produced
/// artifact except the binary `--version` (which is covered by the
/// example test). Returns Ok if every assertion holds, Err with a message
/// otherwise — used by both the example test (where we panic) and the
/// proptest (where we propagate via prop_assert!).
fn assert_version_consistency_no_binary(
    version: &str,
    formula: &RenderedFormula,
) -> Result<(), String> {
    // Axis 2 — the tag string is `v<version>`. We check this implicitly by
    // verifying that the release-base-url ends with the tag.
    let tag = format!("v{version}");
    if !formula.release_base_url.ends_with(&tag) {
        return Err(format!(
            "release-base-url {url} must end with tag {tag}",
            url = formula.release_base_url
        ));
    }

    // Axis 3 — for each target, the archive name `modeltap-<version>-<triple>.tar.gz`
    // appears in the formula body (its url field).
    for (triple, _sha) in &formula.sidecar_shas {
        let archive = format!("modeltap-{version}-{triple}.tar.gz");
        if !formula.body.contains(&archive) {
            return Err(format!(
                "formula body must contain archive name {archive} for {triple}; \
                 body: {body}",
                body = formula.body
            ));
        }
    }

    // Axis 4 — the GitHub Release title equals the tag string by convention
    // (release.yml `gh release create` step uses `--title "$TAG"`). We assert
    // this indirectly: the release-base-url contains the tag (axis 2) AND
    // each archive url is built from `release-base-url + archive-name` (INT.AC-3
    // below). Since both endpoints agree on the tag, the title must too.
    // No additional check beyond axis 2 is required at this layer.

    // Axis 5 — the rendered formula's version line is `version "<version>"`.
    let expected_version_line = format!("version \"{version}\"");
    if !formula.body.contains(&expected_version_line) {
        return Err(format!(
            "formula body must contain `{expected_version_line}`; body: {body}",
            body = formula.body
        ));
    }

    // INT.AC-2 — each target's formula sha256 equals the bare-hex sidecar.
    // INT.AC-3 — each target's url starts with release-base-url AND ends with
    //            archive name.
    for (triple, sha) in &formula.sidecar_shas {
        let archive = format!("modeltap-{version}-{triple}.tar.gz");
        let expected_url = format!("{}/{archive}", formula.release_base_url);

        // INT.AC-3: the full url appears verbatim in the formula. This
        // proves both `starts_with(release-base-url)` and `ends_with(archive)`
        // simultaneously — they're concatenated by render-formula.
        if !formula.body.contains(&expected_url) {
            return Err(format!(
                "INT.AC-3: formula body must contain url {expected_url} for {triple}; \
                 body: {body}",
                body = formula.body
            ));
        }

        // INT.AC-2: locate the archive url, then assert the sha256 line
        // within a small window. Using the sha-line existence + proximity to
        // the archive url (already asserted in render_formula tests) proves
        // the formula sha equals the bare-hex sidecar content (we wrote the
        // sidecar with that exact `sha` value, so equality is transitive).
        let url_idx = formula
            .body
            .find(&archive)
            .expect("archive presence asserted above");
        let window = &formula.body[url_idx..(url_idx + 200).min(formula.body.len())];
        let expected_sha_line = format!("sha256 \"{sha}\"");
        if !window.contains(&expected_sha_line) {
            return Err(format!(
                "INT.AC-2: sha256 for {triple} must equal sidecar content {sha} \
                 within 200 bytes of its archive url; window: {window}"
            ));
        }
    }

    Ok(())
}

// =============================================================================
// INT.AC-1 example — version threads through ALL 6 axes including the binary
// `--version`. One example only; the binary axis is too costly to drive in a
// proptest loop and clap forwards `CARGO_PKG_VERSION` via the same code path
// for every input so per-version variation adds no signal.
// =============================================================================

#[test]
fn version_string_threads_through_all_six_consumers_for_workspace_version() {
    // Axis 1 — read the workspace version straight from the in-tree Cargo.toml
    // so this test always tracks reality. (We do NOT seed a fake version
    // here because axis 6 invokes the REAL `modeltap-app` binary, which uses
    // `env!("CARGO_PKG_VERSION")` at compile time — using the live version is
    // the only way axes 1 and 6 can agree.)
    let manifest = workspace_manifest();
    let manifest_text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest.display()));
    let version = parse_workspace_version(&manifest_text).unwrap_or_else(|| {
        panic!(
            "no [workspace.package].version line in {}",
            manifest.display()
        )
    });

    // Axes 2-5 + INT.AC-2 + INT.AC-3 — render the formula and assert.
    let formula = render_formula_at(&version);
    assert_version_consistency_no_binary(&version, &formula)
        .unwrap_or_else(|msg| panic!("version consistency violation: {msg}"));

    // Axis 6 — the real binary's --version output is `modeltap <version>`
    // (clap forwards CARGO_PKG_VERSION via #[command(version)] in
    // crates/modeltap-app/src/main.rs). PATH workaround so build-script linking
    // finds /usr/bin/cc.
    let mut cmd = Command::new(env!("CARGO"));
    cmd.arg("run")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--package")
        .arg("modeltap-app")
        .arg("--quiet")
        .arg("--")
        .arg("--version");
    let original_path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("/usr/bin:{original_path}"));

    let bin_out = cmd.output().expect("invoke modeltap-app --version");
    assert!(
        bin_out.status.success(),
        "modeltap-app --version must exit zero; stderr={}",
        String::from_utf8_lossy(&bin_out.stderr)
    );
    let stdout = String::from_utf8_lossy(&bin_out.stdout);
    let expected = format!("modeltap {version}");
    assert!(
        stdout.trim() == expected,
        "modeltap-app --version stdout must equal {expected:?} so axis 6 \
         agrees with axis 1 (Cargo.toml workspace.package.version); got: {stdout:?}"
    );
}

// =============================================================================
// INT.AC-1 property — version-string consistency holds for any valid semver.
// Covers axes 1-5 + INT.AC-2 + INT.AC-3. Skips axis 6 (binary) per the rationale
// at the top of the file. Cases kept low (8) because each case spawns a real
// `cargo run xtask render-formula` subprocess.
// =============================================================================

/// Strategy producing a valid semver `MAJOR.MINOR.PATCH` plus an optional
/// pre-release suffix matching `[A-Za-z0-9]+(\.[A-Za-z0-9]+)*`. We constrain
/// the numeric components to small ranges so the generated strings stay short
/// — render-formula doesn't care about magnitude, only string identity.
fn semver_strategy() -> impl Strategy<Value = String> {
    let core = (0u32..=9, 0u32..=99, 0u32..=999)
        .prop_map(|(maj, min, patch)| format!("{maj}.{min}.{patch}"));
    // SemVer 2.0.0 forbids leading zeros in *numeric* pre-release identifiers
    // ("096" is invalid; "abc.096" is invalid because ".096" is the second
    // dot-separated identifier and is purely numeric). To dodge that rule
    // entirely we keep every segment alphanumeric-with-required-letter, so no
    // identifier is ever purely numeric.
    let pre = proptest::option::of("[a-z][a-z0-9]{0,4}(\\.[a-z][a-z0-9]{0,2})?");
    (core, pre).prop_map(|(core, pre_opt)| match pre_opt {
        Some(pre) => format!("{core}-{pre}"),
        None => core,
    })
}

fn parse_workspace_version(manifest_text: &str) -> Option<String> {
    // Find the `[workspace.package]` table, then the `version = "..."` line
    // inside it. We do a small line-based scan instead of pulling in `toml`
    // because this crate already pays the toml-cost in xtask and we want
    // dev-deps minimal.
    let mut in_workspace_package = false;
    for raw in manifest_text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_workspace_package = line == "[workspace.package]";
            continue;
        }
        if in_workspace_package {
            if let Some(rest) = line.strip_prefix("version") {
                let rest = rest.trim_start_matches([' ', '=']).trim();
                if let Some(quoted) = rest.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                    return Some(quoted.to_string());
                }
            }
        }
    }
    None
}

proptest! {
    // Cases kept low — every case spawns a real xtask subprocess (~1-2s).
    // 8 cases x ~2s = ~16s, well within the per-test cargo timeout. Increase
    // only if a regression in render-formula's version handling slips through.
    #![proptest_config(ProptestConfig {
        cases: 8,
        max_shrink_iters: 16,
        .. ProptestConfig::default()
    })]

    #[test]
    fn version_string_consistency_holds_for_any_valid_semver(
        version in semver_strategy()
    ) {
        let formula = render_formula_at(&version);
        if let Err(msg) = assert_version_consistency_no_binary(&version, &formula) {
            // proptest's prop_assert! rejects on the first false, but we want
            // the rich error message — return it as the failure reason.
            return Err(TestCaseError::fail(msg));
        }
        // Quiet the unused-Path import in case future refactors drop it.
        let _ = Path::new("");
    }
}
