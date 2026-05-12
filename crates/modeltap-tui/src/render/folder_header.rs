//! Folder-group header row renderer (US-05c.AC-2 / AC-3; F-FGD-1).
//!
//! Pure function: takes a `FolderGroup` + expand/collapse flag + classifier
//! counts and returns a ratatui `Line` for the folder header row. The header
//! is what the user sees in the right pane in place of the per-file rows
//! when the active tool is hf and the folder has ≥2 files. Single-file
//! folders are NOT rendered as a header — they collapse to the US-04 row
//! form (handled by the caller at step 01-05 wiring time).
//!
//! Format (US-05c.AC-2):
//!
//! ```text
//! [+] <author>/<repo>  N files, X GB (M unique, K shared)
//! [-] <author>/<repo>  N files, X GB (M unique, K shared)
//! ```
//!
//! - `[+]` when `expanded == false`; `[-]` when `expanded == true`.
//! - `N` is `folder.file_count()` (models + sidecars).
//! - `X` is `format_bytes(folder.total_bytes())`.
//! - `M` / `K` are the per-folder unique / shared counts the caller supplies
//!   (computed via `logic::folder_group::classify_unique_vs_shared` — the
//!   single classifier engine per AC-13 / D-FGD-4).
//!
//! Folder headers are cursor-targetable; sidecar child rows are not (AC-3).
//! That selection-routing lives in the right-pane caller (step 01-05); this
//! module only produces the row text.

use modeltap_core::types::FolderGroup;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::render::bytes::format_bytes;

/// Build the ratatui `Line` for a folder header row.
///
/// `expanded` controls the leading `[+]` / `[-]` glyph. `unique_count` and
/// `shared_count` come from the single-engine classifier at
/// `modeltap_core::logic::folder_group::classify_unique_vs_shared`; the
/// caller is responsible for ordering — this function does not look up the
/// classifier itself (pure render).
///
/// The line is `Style::default()` everywhere — the right pane applies any
/// selection / focus styling on top when the header is the cursor target.
pub fn render_folder_header_line<'a>(
    folder: &'a FolderGroup,
    expanded: bool,
    unique_count: usize,
    shared_count: usize,
) -> Line<'a> {
    let indicator = if expanded { "[-]" } else { "[+]" };
    let file_count = folder.file_count();
    let total_bytes = format_bytes(folder.total_bytes());
    let mut spans: Vec<Span<'a>> = Vec::with_capacity(4);
    spans.push(Span::styled(indicator.to_string(), Style::default()));
    spans.push(Span::raw(" "));
    spans.push(Span::raw(folder.path.clone()));
    spans.push(Span::raw(format!(
        "  {} files, {} ({} unique, {} shared)",
        file_count, total_bytes, unique_count, shared_count
    )));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::types::{
        DedupKey, DisplayLabel, Format, ModelMeta, ModelStatus, Sidecar, SidecarKind,
    };
    use modeltap_core::ToolId;
    use std::path::PathBuf;

    fn model(id: &str, size: u64) -> ModelMeta {
        ModelMeta {
            tool: ToolId("hf"),
            id_in_tool: id.to_string(),
            on_disk_path: PathBuf::from(format!("/cache/{id}")),
            size_bytes: size,
            format: Format::Gguf,
            dedup_key: DedupKey::Tentative(DisplayLabel::from(format!("{id}@{size}"))),
            display_label: DisplayLabel::from(id),
            status: ModelStatus::Healthy,
        }
    }

    fn folder_3files_1sidecar() -> FolderGroup {
        let models = vec![
            model("alice/foo/a.gguf", 1_000),
            model("alice/foo/b.gguf", 2_000),
            model("alice/foo/c.gguf", 3_000),
        ];
        let sidecars = vec![Sidecar {
            path: PathBuf::from("/cache/alice/foo/README.md"),
            size_bytes: 100,
            kind: SidecarKind::Readme,
        }];
        FolderGroup::new(
            "alice/foo".to_string(),
            PathBuf::from("/cache/hub/models--alice--foo"),
            ToolId("hf"),
            models,
            sidecars,
        )
        .expect("constructs")
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn collapsed_header_starts_with_plus_indicator() {
        let folder = folder_3files_1sidecar();
        let line = render_folder_header_line(&folder, false, 3, 0);
        let text = line_text(&line);
        assert!(text.starts_with("[+] "), "got: {:?}", text);
        assert!(text.contains("alice/foo"), "got: {:?}", text);
    }

    #[test]
    fn expanded_header_starts_with_minus_indicator() {
        let folder = folder_3files_1sidecar();
        let line = render_folder_header_line(&folder, true, 3, 0);
        let text = line_text(&line);
        assert!(text.starts_with("[-] "), "got: {:?}", text);
    }

    #[test]
    fn header_includes_file_count_and_unique_shared_split() {
        let folder = folder_3files_1sidecar();
        let line = render_folder_header_line(&folder, false, 3, 0);
        let text = line_text(&line);
        // 3 models + 1 sidecar = 4 files
        assert!(text.contains("4 files"), "got: {:?}", text);
        assert!(text.contains("(3 unique, 0 shared)"), "got: {:?}", text);
    }
}
