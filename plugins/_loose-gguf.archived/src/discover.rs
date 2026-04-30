//! Synchronous discovery walker for the loose-gguf plugin.
//!
//! Walks each configured search path RECURSIVELY for `.gguf` files, parses
//! each header (lazily — only the first ~64 KiB are read), and emits one
//! `DiscoveredModel` per file. Corrupt / unreadable / truncated files are
//! still emitted, but with `Format::Other` and `ModelStatus::Corrupt` so the
//! TUI surfaces them as `[format: corrupt]` rather than silently dropping.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon (per ADR-001 plugins live outside the core). Strict OC rules are
//! relaxed here — the adapter exists to bridge real I/O.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use modeltap_core::{DiscoverError, DiscoveredModel, DisplayLabel, Format, ModelStatus};
use walkdir::WalkDir;

use crate::gguf_header::{parse_header, GgufHeader};

/// Walk every configured search path and return one `DiscoveredModel` per
/// `.gguf` file. NEVER panics on bad input — corrupt files become entries
/// with `ModelStatus::Corrupt`.
///
/// Returns `Err(DiscoverError::NotInstalled)` ONLY when EVERY configured
/// search path is missing/non-existent. If at least one path exists (even
/// if empty), we return `Ok(...)` so the app shows "0 models" instead of
/// "(not installed)".
pub fn discover_in(roots: &[PathBuf]) -> Result<Vec<DiscoveredModel>, DiscoverError> {
    let any_exists = roots.iter().any(|p| p.exists());
    if !any_exists {
        return Err(DiscoverError::NotInstalled);
    }

    let mut models = Vec::new();
    for root in roots {
        if !root.exists() {
            // One root missing isn't fatal — others may still hold models.
            continue;
        }
        walk_one_root(root, &mut models);
    }

    // Sort deterministically so launch.inventory is stable across runs.
    models.sort_by(|a, b| a.id_in_tool.cmp(&b.id_in_tool));
    Ok(models)
}

fn walk_one_root(root: &Path, out: &mut Vec<DiscoveredModel>) {
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(target: "modeltap.loose_gguf", "walkdir error: {e}");
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !is_gguf(path) {
            continue;
        }
        out.push(read_one_gguf(path));
    }
}

fn is_gguf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gguf"))
        .unwrap_or(false)
}

/// Build a `DiscoveredModel` from a `.gguf` path. Returns a Corrupt entry
/// on any I/O or parse failure rather than dropping the file. The id_in_tool
/// is the file stem (e.g. `llama-3-8b-q4_K_M`) so it survives even when the
/// header cannot be parsed.
fn read_one_gguf(path: &Path) -> DiscoveredModel {
    let id_in_tool = path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.display().to_string());
    let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    match read_header(path) {
        Ok(h) => {
            let display_label = compose_display_label(&id_in_tool, &h);
            DiscoveredModel {
                id_in_tool,
                on_disk_path: path.to_path_buf(),
                size_bytes,
                format: Format::Gguf,
                display_label: DisplayLabel::from(display_label),
                status: ModelStatus::Healthy,
            }
        }
        Err(reason) => {
            tracing::warn!(
                target: "modeltap.loose_gguf",
                "corrupt gguf {}: {}",
                path.display(),
                reason
            );
            DiscoveredModel {
                id_in_tool: id_in_tool.clone(),
                on_disk_path: path.to_path_buf(),
                size_bytes,
                // Per AC-4: corrupt entries surface as Format::Other so the
                // TUI renders `[format: corrupt]`. ModelStatus carries the
                // detailed reason for the diagnostics log.
                format: Format::Other,
                display_label: DisplayLabel::from(id_in_tool),
                status: ModelStatus::Corrupt { reason },
            }
        }
    }
}

/// Read the first chunk of `path` and parse a GGUF header from it. We read
/// up to 64 KiB; that's far more than any realistic GGUF metadata block but
/// small enough that discovery stays under the K3 budget on big trees.
fn read_header(path: &Path) -> Result<GgufHeader, String> {
    const MAX_HEAD: usize = 64 * 1024;
    let mut f = File::open(path).map_err(|e| format!("open: {e}"))?;
    let mut buf = vec![0u8; MAX_HEAD];
    let n = f.read(&mut buf).map_err(|e| format!("read: {e}"))?;
    buf.truncate(n);
    parse_header(&buf).map_err(|e| e.to_string())
}

/// Compose the displayed label. We want the quantization label visible when
/// known, even if the file stem already encodes it (cheap, deterministic,
/// and lets the TUI render a normalized form). e.g.
///   id="llama-3-8b-q4_K_M", quant="Q4_K_M" → "llama-3-8b-q4_K_M [Q4_K_M]"
///   id="custom",            quant=None     → "custom"
fn compose_display_label(id: &str, h: &GgufHeader) -> String {
    match (&h.architecture, &h.quantization) {
        (_, Some(q)) => format!("{} [{}]", id, q),
        (Some(arch), None) => format!("{} ({})", id, arch),
        (None, None) => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_gguf(path: &Path, arch: &str, file_type: u32) {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"GGUF");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&2u64.to_le_bytes());
        let key = b"general.architecture";
        bytes.extend_from_slice(&(key.len() as u64).to_le_bytes());
        bytes.extend_from_slice(key);
        bytes.extend_from_slice(&8u32.to_le_bytes()); // STRING
        bytes.extend_from_slice(&(arch.len() as u64).to_le_bytes());
        bytes.extend_from_slice(arch.as_bytes());
        let key = b"general.file_type";
        bytes.extend_from_slice(&(key.len() as u64).to_le_bytes());
        bytes.extend_from_slice(key);
        bytes.extend_from_slice(&4u32.to_le_bytes()); // UINT32
        bytes.extend_from_slice(&file_type.to_le_bytes());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &bytes).unwrap();
    }

    fn write_corrupt(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"XXXX\x00\x00\x00\x01").unwrap();
    }

    #[test]
    fn discover_returns_healthy_models_for_valid_gguf_files() {
        let temp = tempfile::tempdir().unwrap();
        let llms = temp.path().join("llms");
        write_gguf(&llms.join("a.gguf"), "llama", 15);
        write_gguf(&llms.join("b.gguf"), "mistral", 2);
        let models = discover_in(std::slice::from_ref(&llms)).expect("ok");
        assert_eq!(models.len(), 2, "expected 2 valid gguf entries");
        for m in &models {
            assert_eq!(m.format, Format::Gguf);
            assert_eq!(m.status, ModelStatus::Healthy);
        }
        let labels: Vec<&str> = models.iter().map(|m| m.display_label.0.as_str()).collect();
        assert!(
            labels.iter().any(|l| l.contains("Q4_K_M")),
            "expected one label to mention Q4_K_M, got {:?}",
            labels
        );
    }

    #[test]
    fn discover_corrupt_file_is_listed_with_format_corrupt() {
        let temp = tempfile::tempdir().unwrap();
        let llms = temp.path().join("llms");
        write_gguf(&llms.join("good.gguf"), "llama", 15);
        write_corrupt(&llms.join("bad.gguf"));
        let models = discover_in(&[llms]).expect("ok");
        assert_eq!(models.len(), 2, "both files should be listed");
        let bad = models
            .iter()
            .find(|m| m.id_in_tool == "bad")
            .expect("bad.gguf must be in result");
        assert_eq!(
            bad.format,
            Format::Other,
            "corrupt entries surface as Format::Other"
        );
        assert!(
            matches!(bad.status, ModelStatus::Corrupt { .. }),
            "corrupt entries have ModelStatus::Corrupt; got {:?}",
            bad.status
        );
    }

    #[test]
    fn discover_returns_not_installed_when_all_roots_missing() {
        let err = discover_in(&[
            PathBuf::from("/nonexistent/a"),
            PathBuf::from("/nonexistent/b"),
        ])
        .expect_err("missing roots must error");
        assert!(matches!(err, DiscoverError::NotInstalled));
    }

    #[test]
    fn discover_returns_empty_when_at_least_one_root_exists_but_holds_no_gguf() {
        let temp = tempfile::tempdir().unwrap();
        let empty = temp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let models = discover_in(&[empty]).expect("ok");
        assert!(models.is_empty());
    }

    #[test]
    fn discover_walks_recursively() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        write_gguf(&root.join("subdir/nested.gguf"), "llama", 15);
        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 1, "must find nested .gguf");
        assert_eq!(models[0].id_in_tool, "nested");
    }

    #[test]
    fn discover_ignores_non_gguf_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        write_gguf(&root.join("a.gguf"), "llama", 15);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("README.txt"), "ignore me").unwrap();
        std::fs::write(root.join("config.bin"), [0u8; 16]).unwrap();
        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 1, "only the .gguf must be picked up");
    }

    #[test]
    fn discover_does_not_panic_on_unreadable_file() {
        // A zero-byte file with .gguf extension cannot parse; must surface
        // as Corrupt without panicking.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("models");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("zero.gguf"), b"").unwrap();
        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].format, Format::Other);
    }
}
