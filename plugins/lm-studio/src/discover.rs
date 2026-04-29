//! Synchronous discovery walker for the LM Studio plugin.
//!
//! Walks each configured search path RECURSIVELY for `.gguf` files. LM Studio
//! stores models with a path-style layout: `<root>/<org>/<repo>/<file>.gguf`.
//! Each `.gguf` file becomes one `DiscoveredModel`. Per intake C3 / ADR-004
//! OQ-3, MLX is OUT OF SCOPE for v1, so the plugin's `accepted_formats()`
//! reports only `Format::Gguf` and we skip non-`.gguf` files here.
//!
//! Behavior contract (per US-15 acceptance criteria):
//! - Every configured root missing → `Err(DiscoverError::NotInstalled)` (AC-4).
//! - At least one root exists but unreadable → `Err(DiscoverError::Io)` (AC-4).
//! - At least one root exists with `.gguf` files → `Ok(Vec<DiscoveredModel>)`.
//! - File suffix `.gguf` → `Format::Gguf` (AC-3); other suffixes ignored.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

use modeltap_core::{DiscoverError, DiscoveredModel, DisplayLabel, Format, ModelStatus};
use walkdir::WalkDir;

/// Walk every configured search path and return one `DiscoveredModel` per
/// `.gguf` file. NEVER panics.
///
/// Returns `Err(DiscoverError::NotInstalled)` ONLY when EVERY configured
/// search path is missing (does not exist). If at least one path exists but
/// cannot be read (permission denied, etc.) → `Err(DiscoverError::Io)` so the
/// app surfaces "(error)" rather than the benign "(not installed)" — see
/// US-15 AC-4.
pub fn discover_in(roots: &[PathBuf]) -> Result<Vec<DiscoveredModel>, DiscoverError> {
    let any_exists = roots.iter().any(|p| p.exists());
    if !any_exists {
        return Err(DiscoverError::NotInstalled);
    }

    // If at least one configured root exists but the FIRST walkdir entry
    // surfaces an Io error (e.g., the root itself is permission-denied),
    // bubble it up as DiscoverError::Io so the app reports "(error)".
    let mut models = Vec::new();
    for root in roots {
        if !root.exists() {
            // One root missing isn't fatal — others may still hold models.
            continue;
        }
        walk_one_root(root, &mut models)?;
    }

    // Stable order so launch.inventory is deterministic.
    models.sort_by(|a, b| a.id_in_tool.cmp(&b.id_in_tool));
    Ok(models)
}

/// Walk a single root, appending one entry per discovered `.gguf` file.
///
/// Distinguishes "root unreadable" from "root readable but empty":
///
/// - If walkdir yielded ONLY the root directory entry itself (no children
///   read) AND surfaced at least one error, treat as unreadable → Io.
/// - If walkdir yielded children (even non-`.gguf` ones), the root was
///   readable; per-entry errors during the walk are logged + skipped.
///
/// This pattern lets `~/.cache/lm-studio/models/` with restrictive perms on
/// the ROOT surface as "(error)" while a normal empty/populated root keeps
/// the "(installed but no models)" or "models listed" semantics.
fn walk_one_root(root: &Path, out: &mut Vec<DiscoveredModel>) -> Result<(), DiscoverError> {
    let mut child_entries_seen = false;
    let mut first_err: Option<walkdir::Error> = None;

    for entry in WalkDir::new(root).follow_links(false) {
        match entry {
            Ok(e) => {
                // The very first Ok yielded by walkdir is the root itself
                // (depth 0). Everything at depth >= 1 is a "child entry"
                // which proves the root was readable.
                if e.depth() > 0 {
                    child_entries_seen = true;
                }
                if !e.file_type().is_file() {
                    continue;
                }
                let path = e.path();
                if !is_gguf(path) {
                    continue;
                }
                out.push(read_one_gguf(path));
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                } else {
                    tracing::debug!(target: "modeltap.lm_studio", "walkdir error: {e}");
                }
            }
        }
    }

    // Root was unreadable when:
    //   - we never observed any child entry (walkdir gave us the root + then
    //     errored trying to recurse), AND
    //   - at least one error was surfaced.
    if !child_entries_seen {
        if let Some(e) = first_err {
            let io = e
                .into_io_error()
                .unwrap_or_else(|| std::io::Error::other("walkdir failed to read root"));
            return Err(DiscoverError::Io(io));
        }
    }

    Ok(())
}

fn is_gguf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gguf"))
        .unwrap_or(false)
}

/// Build a `DiscoveredModel` from a `.gguf` path. The id_in_tool is
/// `<org>/<repo>/<filename>` — the org/repo segments are reconstructed from
/// the parent path under the search root, mirroring how LM Studio's UI shows
/// "<org>/<repo>". Falls back to the bare filename if the path doesn't have
/// the expected depth.
fn read_one_gguf(path: &Path) -> DiscoveredModel {
    let id_in_tool = compute_id(path);
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    DiscoveredModel {
        id_in_tool: id_in_tool.clone(),
        on_disk_path: path.to_path_buf(),
        size_bytes,
        format: Format::Gguf,
        display_label: DisplayLabel::from(id_in_tool),
        status: ModelStatus::Healthy,
    }
}

/// Derive an id from the LM Studio cache path. We take the LAST three
/// segments of the path (org/repo/filename) when available; otherwise fall
/// back to the bare filename. Examples:
///   `/cache/lm-studio/models/microsoft/phi-3-mini/phi-3-q4.gguf`
///     → `microsoft/phi-3-mini/phi-3-q4.gguf`
///   `/some/random/path/foo.gguf` → `foo.gguf`
fn compute_id(path: &Path) -> String {
    let comps: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    let n = comps.len();
    if n >= 3 {
        return format!("{}/{}/{}", comps[n - 3], comps[n - 2], comps[n - 1]);
    }
    path.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_gguf(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Minimal valid GGUF header bytes — enough that the file is non-empty
        // and has the correct magic, but the LM Studio plugin doesn't parse
        // headers (unlike llama-cli) so any non-empty .gguf works.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GGUF");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        std::fs::write(path, &bytes).unwrap();
    }

    #[test]
    fn discover_returns_healthy_models_for_valid_gguf_files() {
        // Behavior 3: walking a populated tree yields one entry per .gguf.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        write_gguf(&root.join("microsoft/phi-3-mini/phi-3-q4.gguf"));
        write_gguf(&root.join("TheBloke/Mistral-7B-GGUF/mistral.Q4_0.gguf"));
        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 2, "expected 2 gguf entries; got {:?}", models);
        for m in &models {
            assert_eq!(m.format, Format::Gguf, "every entry must be Format::Gguf");
            assert_eq!(m.status, ModelStatus::Healthy);
            assert!(
                m.size_bytes > 0,
                "size_bytes must reflect on-disk size; got 0 for {:?}",
                m.id_in_tool
            );
        }
    }

    #[test]
    fn discover_id_is_org_repo_filename_path_style() {
        // Behavior 3 (id derivation): `<org>/<repo>/<filename>` is the
        // canonical id form, mirroring the LM Studio UI's display.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        write_gguf(&root.join("microsoft/phi-3-mini/phi-3-mini-q4.gguf"));
        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 1);
        assert_eq!(
            models[0].id_in_tool, "microsoft/phi-3-mini/phi-3-mini-q4.gguf",
            "id_in_tool must be <org>/<repo>/<filename>"
        );
    }

    #[test]
    fn discover_returns_not_installed_when_all_roots_missing() {
        // Behavior 4 (AC-4): both default paths missing → NotInstalled.
        let err = discover_in(&[
            PathBuf::from("/nonexistent/.cache/lm-studio/models"),
            PathBuf::from("/nonexistent/.lmstudio/models"),
        ])
        .expect_err("missing roots must error");
        assert!(matches!(err, DiscoverError::NotInstalled));
    }

    #[test]
    fn discover_returns_empty_ok_when_root_exists_but_holds_no_gguf() {
        // Behavior 4 (positive): empty existing root → Ok(empty), NOT
        // NotInstalled. The tool IS installed; it just has no models.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        std::fs::create_dir_all(&root).unwrap();
        let models = discover_in(&[root]).expect("ok");
        assert!(models.is_empty());
    }

    #[test]
    fn discover_walks_recursively_through_org_repo_subdirs() {
        // Behavior 3 (recursion): the LM Studio path-derived layout requires
        // recursive walking — `/<root>/<org>/<repo>/<file>.gguf`.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        write_gguf(&root.join("a/b/c.gguf"));
        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 1, "must find deeply nested .gguf");
    }

    #[test]
    fn discover_ignores_non_gguf_files() {
        // Behavior 3 (filter): only `.gguf` files are picked up; .safetensors,
        // .bin, .mlx are ignored (intake C3 / ADR-004 OQ-3 — MLX is v1.x+).
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        write_gguf(&root.join("good.gguf"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("README.txt"), "ignore me").unwrap();
        std::fs::write(root.join("config.bin"), [0u8; 16]).unwrap();
        std::fs::write(root.join("model.mlx"), [0u8; 16]).unwrap();
        std::fs::write(root.join("model.safetensors"), [0u8; 16]).unwrap();
        let models = discover_in(&[root]).expect("ok");
        assert_eq!(
            models.len(),
            1,
            "only the .gguf must be picked up (MLX/safetensors/bin are out of scope v1)"
        );
    }

    #[test]
    fn discover_unions_models_from_multiple_roots() {
        // Behavior 3 (union): plugin must scan BOTH conventional paths and
        // union the results — proves the new+old fallback contract.
        let temp = tempfile::tempdir().unwrap();
        let new_root = temp.path().join(".cache/lm-studio/models");
        let old_root = temp.path().join(".lmstudio/models");
        write_gguf(&new_root.join("org-a/repo-a/new.gguf"));
        write_gguf(&old_root.join("org-b/repo-b/old.gguf"));
        let models = discover_in(&[new_root, old_root]).expect("ok");
        assert_eq!(models.len(), 2, "both roots must contribute");
        let ids: Vec<&str> = models.iter().map(|m| m.id_in_tool.as_str()).collect();
        assert!(ids.iter().any(|i| i.ends_with("new.gguf")));
        assert!(ids.iter().any(|i| i.ends_with("old.gguf")));
    }

    #[cfg(unix)]
    #[test]
    fn discover_unreadable_root_surfaces_io_error() {
        // Behavior 5 (AC-4): permission-denied directory → DiscoverError::Io,
        // distinct from NotInstalled (which is benign).
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).unwrap();

        let res = discover_in(std::slice::from_ref(&root));

        // Restore perms so TempDir cleanup works.
        let _ = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755));

        match res {
            Err(DiscoverError::Io(_)) => {}
            other => panic!(
                "expected DiscoverError::Io for an unreadable root; got {:?}",
                other
            ),
        }
    }
}
