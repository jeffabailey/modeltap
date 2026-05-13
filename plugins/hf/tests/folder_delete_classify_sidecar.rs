//! Unit-level coverage for `enumerate_sidecars` classification rules.
//!
//! These tests close the mutation-testing gap surfaced in
//! `docs/feature/folder-group-bulk-delete/deliver/phase5-mutation-results.md`:
//! the kill rate on `plugins/hf/src/folder_delete.rs` came in at 79.17%
//! production-only (below the 80% per-feature gate). The surviving mutants
//! clustered in `classify_sidecar` and `path_starts_with_subdir` — the
//! existing happy-path tests exercised paths inside `refs/`/`blobs/` but
//! never asserted classification for:
//!
//!   1. README.md / LICENSE / LICENSE.md  (each branch of the `||` chain),
//!   2. `*.urls` files (the second arm of the `.gguf.urls` || `.urls` chain),
//!   3. Files at the repo root with no special suffix (the `Other` arm),
//!   4. Files outside `repo_dir` entirely (`path_starts_with_subdir`
//!      returning `false` via `unwrap_or(false)`).
//!
//! `classify_sidecar` and `path_starts_with_subdir` are module-private; the
//! tests reach them through `enumerate_sidecars`, which is the public driving
//! port. This is port-to-port testing at the domain-function scope.

#![cfg(unix)]

use std::fs;
use std::path::Path;

use modeltap_core::types::SidecarKind;
use modeltap_plugin_hf::folder_delete::enumerate_sidecars;

// ---------------------------------------------------------------------------
// Test: every sidecar kind classified correctly via parametrized cases.
// ---------------------------------------------------------------------------
//
// One test, many cases — each case pins one classification branch:
//
//   * `README.md` / `LICENSE` / `LICENSE.md`   -> Readme
//   * `*.imatrix`                              -> Imatrix
//   * `*.gguf.urls` / `*.urls`                 -> Urls
//   * inside `refs/`                           -> HfInternal
//   * inside `blobs/`                          -> HfInternal
//   * root-level file with no special suffix   -> Other
//   * nested file with no special suffix       -> Other
//
// Each case lays down ONE file in a freshly-created repo dir and asserts the
// returned `SidecarKind`. Empty model_files list — we are exercising
// classification, not the model-exclusion filter.

#[test]
fn classify_sidecar_covers_all_kinds() {
    struct Case {
        /// Relative path under `repo_dir` for the file to drop.
        rel_path: &'static str,
        expected: SidecarKind,
    }
    let cases = [
        // --- Readme branch (each arm of the `||` chain) ---
        Case {
            rel_path: "snapshots/abc123/README.md",
            expected: SidecarKind::Readme,
        },
        Case {
            rel_path: "snapshots/abc123/LICENSE",
            expected: SidecarKind::Readme,
        },
        Case {
            rel_path: "snapshots/abc123/LICENSE.md",
            expected: SidecarKind::Readme,
        },
        // --- Imatrix branch ---
        Case {
            rel_path: "snapshots/abc123/imatrix.dat.imatrix",
            expected: SidecarKind::Imatrix,
        },
        // --- Urls branch — BOTH arms of `*.gguf.urls` || `*.urls` ---
        Case {
            rel_path: "snapshots/abc123/model.gguf.urls",
            expected: SidecarKind::Urls,
        },
        Case {
            rel_path: "snapshots/abc123/alt.urls",
            expected: SidecarKind::Urls,
        },
        // --- HfInternal: under refs/ ---
        Case {
            rel_path: "refs/main",
            expected: SidecarKind::HfInternal,
        },
        // --- HfInternal: under blobs/ ---
        Case {
            rel_path: "blobs/0123abcd",
            expected: SidecarKind::HfInternal,
        },
        // --- Other: top-level file in repo_dir with no special suffix ---
        Case {
            rel_path: "garbage.bin",
            expected: SidecarKind::Other,
        },
        // --- Other: nested file outside refs/blobs with no special suffix ---
        Case {
            rel_path: "snapshots/abc123/notes.txt",
            expected: SidecarKind::Other,
        },
    ];

    for case in &cases {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo_dir = temp.path().join("models--owner--repo");
        let file_path = repo_dir.join(case.rel_path);
        write_with_size(&file_path, 32);

        let sidecars = enumerate_sidecars(&repo_dir, &[]);

        let found = sidecars
            .iter()
            .find(|s| s.path == file_path)
            .unwrap_or_else(|| {
                panic!("enumerate_sidecars must include {file_path:?}; got {sidecars:?}",)
            });
        assert_eq!(
            found.kind, case.expected,
            "wrong SidecarKind for {:?}: expected {:?}, got {:?}",
            case.rel_path, case.expected, found.kind,
        );
    }
}

// ---------------------------------------------------------------------------
// Test: a file at the repo root (no leading `refs/` / `blobs/` component)
// classifies as `Other`, not `HfInternal`.
//
// Targets the `path_starts_with_subdir` mutant "replace body with `true`"
// (folder_delete.rs:125). If the predicate always returned true, the
// classifier would route every non-special-suffix file to `HfInternal`.
// ---------------------------------------------------------------------------

#[test]
fn classify_sidecar_root_level_file_is_other_not_hf_internal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp.path().join("models--owner--repo");
    let loose = repo_dir.join("loose_file.bin");
    write_with_size(&loose, 16);

    let sidecars = enumerate_sidecars(&repo_dir, &[]);

    let kind = sidecars
        .iter()
        .find(|s| s.path == loose)
        .map(|s| s.kind)
        .expect("loose root-level file must be enumerated");
    assert_eq!(
        kind,
        SidecarKind::Other,
        "root-level file must classify as Other, not HfInternal",
    );
}

// ---------------------------------------------------------------------------
// Test: a file under refs/ with no special suffix classifies as HfInternal.
//
// Pairs with the root-level test above to kill the `==` -> `!=` mutant on
// `path_starts_with_subdir` (folder_delete.rs:128). One assertion alone
// can't distinguish the two — flipping the operator inverts both branches
// in lockstep — but the PAIR of assertions falsifies any single-operator
// mutation.
// ---------------------------------------------------------------------------

#[test]
fn classify_sidecar_path_under_refs_is_hf_internal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp.path().join("models--owner--repo");
    let refs_entry = repo_dir.join("refs/main");
    write_with_size(&refs_entry, 40);

    let sidecars = enumerate_sidecars(&repo_dir, &[]);

    let kind = sidecars
        .iter()
        .find(|s| s.path == refs_entry)
        .map(|s| s.kind)
        .expect("refs/main must be enumerated");
    assert_eq!(
        kind,
        SidecarKind::HfInternal,
        "refs/main must classify as HfInternal",
    );
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn write_with_size(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let f = fs::File::create(path).unwrap();
    f.set_len(size).unwrap();
}
