//! Per-model detail screen (US-13). Pure view-model + pure render fn.
//!
//! Pressing Enter on a model row in the main view opens this screen. It shows:
//!
//! - Model id, format, format-quant label, size on disk
//! - Dedup key (SHA-256 hex) — or "computing dedup key... N%" while a lazy
//!   hash is in flight (per ADR-002)
//! - Per-tool registration list ("<tool>: <full path>")
//! - Status header (one of UNIFIED / NOT UNIFIED / PARTIALLY UNIFIED /
//!   SINGLE TOOL) + reclaim-estimate narrative
//! - Bottom-bar shortcut line generated from `keymap::SHORTCUT_TABLE` via
//!   `render::bottom_bar::render_bottom_bar` (US-08 contract — single
//!   source of truth)
//!
//! For SINGLE TOOL models, [u] is dimmed with the annotation "single tool —
//! unify not applicable". For UNIFIED models, the screen reads "UNIFIED — 1
//! inode, N hardlinks" with reclaim 0 (already reclaimed).
//!
//! Per ADR-006 the view layer is pure: this module reads `&DetailScreenState`
//! and writes ratatui widgets into a `Frame`. No I/O, no mutation. The
//! orchestrator (in `modeltap-app`) computes the SHA-256 via the `Hasher`
//! port + `Sha256Cache` and dispatches `Msg::OpenDetail(...)`/progress
//! updates.

use std::collections::BTreeMap;
use std::time::SystemTime;

use modeltap_core::logic::unification_status::{
    compute_reclaim_estimate, compute_unification_status, DetailModelView, DetailRegistration,
    UnificationStatus,
};
use modeltap_core::{ContentHash, Format};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::render::bytes::format_bytes;

use crate::app_state::AppState;
use crate::render::bottom_bar::{render_bottom_bar, BarContext};
use crate::render::last_action;

/// US-22 / step 03-01 — payload for the new Metadata section. Carried on the
/// `DetailScreenState` when the model-detail orchestrator has resolved the
/// `ModelDetail.metadata_kv` for the screen's model. `None` means the
/// orchestrator hasn't dispatched `Msg::ModelDetailReady` yet (or the
/// composition root never wired the model-detail orchestrator at all — the
/// legacy US-13 path) — in which case the Metadata section is omitted.
///
/// The `kv` BTreeMap renders as aligned key-value pairs (AC-22-4); the
/// `source` + `introspected_at` populate the dim section header
/// "Metadata (from <source>, introspected <N> ago)". `kv` may contain the
/// single `_status` key carrying one of the sentinel strings
/// (`METADATA_UNSUPPORTED_SENTINEL` /
/// `open_tool_detail::INSPECT_PANIC_SENTINEL`) — that key is rendered as a
/// bare status line rather than a key-value pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataSection {
    /// Plugin-defined key-value pairs. Sorted by key (BTreeMap). When the
    /// map contains a single `_status` key, the renderer treats it as a
    /// status line (e.g., "(metadata unsupported for this tool)").
    pub kv: BTreeMap<String, String>,
    /// Free-form label of where the metadata came from — e.g., "ollama",
    /// "hf", "test-tool". Surfaces in the dim section header.
    pub source: String,
    /// When the metadata was last computed. `None` means "just now" /
    /// "unknown" — renders as "introspected just now".
    pub introspected_at: Option<SystemTime>,
}

/// Pure state for the detail screen. Constructed by `update()` from
/// `Msg::OpenDetail(...)`; mutated by `Msg::SetHashProgress(N)` when a lazy
/// hash is in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailScreenState {
    pub model: DetailModelView,
    pub registrations: Vec<DetailRegistration>,
    /// Final hash, when computed (cache hit OR completed compute). `None`
    /// while the hash is in flight; the screen renders the progress message.
    pub content_hash: Option<ContentHash>,
    /// Most-recent progress update from the Hasher port (0..=100). Renders
    /// "computing dedup key... N%" while `content_hash.is_none()`.
    pub hash_progress_pct: u8,
    /// US-22 / step 03-01 — optional Metadata section payload. `None` means
    /// the screen renders WITHOUT the Metadata section (legacy US-13 path
    /// plus tests that don't exercise model-detail orchestration). `Some(_)`
    ///   means the model-detail orchestrator dispatched the metadata for this
    ///   model_id and the renderer paints the Metadata section per AC-22-4.
    pub metadata: Option<MetadataSection>,
}

impl DetailScreenState {
    pub fn new(
        model: DetailModelView,
        registrations: Vec<DetailRegistration>,
        content_hash: Option<ContentHash>,
    ) -> Self {
        Self {
            model,
            registrations,
            content_hash,
            hash_progress_pct: if content_hash.is_some() { 100 } else { 0 },
            metadata: None,
        }
    }

    /// Set the most-recent progress percentage. The screen renders this
    /// while `content_hash.is_none()`.
    pub fn set_hash_progress(&mut self, percent: u8) {
        self.hash_progress_pct = percent.min(100);
    }

    /// Attach a Metadata section payload (US-22). Returns `self` for fluent
    /// chaining at construction sites.
    pub fn with_metadata(mut self, metadata: MetadataSection) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Derived status, computed on demand from the registrations. Pure.
    pub fn status(&self) -> UnificationStatus {
        compute_unification_status(&self.registrations)
    }

    /// Derived reclaim estimate in bytes. Pure.
    pub fn reclaim_bytes(&self) -> u64 {
        compute_reclaim_estimate(&self.status(), self.model.canonical_size_bytes)
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render the detail screen into `area`. Layout:
///
/// ```text
/// ┌─ Model: <id> ────────────────────────────────────┐
/// │ Format: GGUF [q4_K_M]                            │
/// │ Size:   4.4 GB                                   │
/// │ Dedup key: aaaaaaaa…aaaaaaaa  (or "computing…")  │
/// │                                                  │
/// │ Registrations:                                   │
/// │   hf:        /hub/.../model.safetensors          │
/// │   llama-cli: /llms/mistral.gguf                  │
/// │   ollama:    /ollama/blobs/sha256-…              │
/// │                                                  │
/// │ Status:    NOT UNIFIED — 3 separate copies (...) │
/// │ Reclaim:   If unified: would reclaim 8.8 GB      │
/// └──────────────────────────────────────────────────┘
/// (bottom bar generated from keymap::SHORTCUT_TABLE — US-08)
/// ```
pub fn render_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    detail: &DetailScreenState,
    app: &AppState,
) {
    // Vertical split: main detail panel (Min 1) | optional post-action banner
    // (2 rows when present) | bottom bar (1 row). The banner is reserved
    // only when `app.last_action.is_some()` so the regular detail layout is
    // unaffected when no action has fired (US-13 default).
    let banner_rows: u16 = if app.last_action.is_some() { 2 } else { 0 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(banner_rows),
            Constraint::Length(1),
        ])
        .split(area);

    let title = format!(" Model: {} ", detail.model.id);
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(chunks[0]);
    frame.render_widget(block, chunks[0]);

    let body_lines = build_body_lines(detail);
    let paragraph = Paragraph::new(body_lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);

    // Post-action banner (US-06): when the orchestrator has dispatched a
    // Msg::SetLastAction, the detail screen reserves 2 rows above the bottom
    // bar to render the structured banner. The banner is dismissed by any
    // navigation Msg (clear_last_action in update.rs).
    if let Some(action) = &app.last_action {
        last_action::render(frame, chunks[1], action);
    }

    // Bottom bar — generated from SHORTCUT_TABLE so the labels and dispatch
    // can never drift (US-08 AC-5 / INT-6 invariant).
    let ctx = BarContext::for_state(app);
    let bar = render_bottom_bar(&ctx, crate::render::colors::no_color_active());
    frame.render_widget(Paragraph::new(bar), chunks[2]);
}

/// Compose the body lines for the detail-screen panel. Pure.
fn build_body_lines(state: &DetailScreenState) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(format!(
        "Format: {}",
        format_format(state.model.format, state.model.format_quant.as_deref())
    )));
    lines.push(Line::from(format!(
        "Size:   {}",
        format_bytes(state.model.canonical_size_bytes)
    )));
    lines.push(Line::from(format!(
        "Dedup key: {}",
        format_dedup_key(state.content_hash, state.hash_progress_pct)
    )));
    lines.push(Line::from(""));
    lines.push(Line::from("Registrations:"));
    append_inode_groups(&mut lines, &state.registrations);
    lines.push(Line::from(""));

    let status = state.status();
    lines.push(Line::from(format!(
        "Status:    {}",
        format_status_header(&status, state.model.canonical_size_bytes)
    )));
    lines.push(Line::from(format!(
        "Reclaim:   {}",
        format_reclaim(&status, state.model.canonical_size_bytes)
    )));

    if matches!(status, UnificationStatus::SingleTool) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "single tool — unify not applicable",
            Style::default().add_modifier(Modifier::DIM),
        )));
    }

    // US-22 / step 03-01 — append the Metadata section when the model-detail
    // orchestrator has populated it. The header line is dim per AC-22-4; the
    // body renders one line per key-value pair with `:` alignment OR a single
    // status line when the map contains only the `_status` sentinel.
    if let Some(meta) = &state.metadata {
        lines.push(Line::from(""));
        append_metadata_section(&mut lines, meta);
    }
    lines
}

/// AC-22-4 — render the Metadata section. The header is a dim line of the
/// form "Metadata (from <source>, introspected <provenance>)"; the body is
/// either a single bare status line (when `kv` contains exactly one entry
/// with key `_status`, which is how the orchestrator surfaces the
/// "(metadata unsupported for this tool)" /
/// "(introspection failed -- see diagnostics.log)" sentinels) OR one
/// "  <key> : <value>" line per BTreeMap entry, padded so the `:` separator
/// aligns across all keys.
fn append_metadata_section(lines: &mut Vec<Line<'static>>, meta: &MetadataSection) {
    let provenance = format_metadata_provenance(meta.introspected_at);
    let header = format!(
        "Metadata (from {}, introspected {})",
        meta.source, provenance
    );
    lines.push(Line::from(Span::styled(
        header,
        Style::default().add_modifier(Modifier::DIM),
    )));

    // Sentinel mode: the orchestrator surfaces a single `_status` key when
    // the inspect call could not produce a real metadata map. Render that
    // value as a bare status line so the user sees the sentinel verbatim
    // (AC-22-7 + the US-22 default-Unsupported path).
    if meta.kv.len() == 1 {
        if let Some(status) = meta.kv.get("_status") {
            lines.push(Line::from(format!("  {}", status)));
            return;
        }
    }

    if meta.kv.is_empty() {
        // Defensive: an empty kv map with no `_status` key means the plugin
        // returned Ok with an empty metadata_kv. Render an empty-state line
        // rather than painting nothing — surfaces the degraded path during
        // triage without crashing.
        lines.push(Line::from("  (no metadata)"));
        return;
    }

    // Aligned key-value rendering: compute the longest key length so each
    // `:` lines up under the longest one. AC-22-4 mandates "aligned"; this
    // is the minimum interpretation that survives a future change to the
    // KV set.
    let max_key_len = meta.kv.keys().map(|k| k.len()).max().unwrap_or(0);
    for (key, value) in &meta.kv {
        lines.push(Line::from(format!(
            "  {key:<width$} : {value}",
            key = key,
            width = max_key_len,
            value = value
        )));
    }
}

/// Human-readable provenance string for the dim section header. Mirrors the
/// AC-22-2 / AC-22-8 contract: when `introspected_at` is `None` OR very
/// recent (within a few seconds of `SystemTime::now`), render "just now";
/// otherwise render a coarse "<N> <unit> ago" relative time. Keeps the
/// allocator-free string formatter local rather than pulling in `chrono`.
fn format_metadata_provenance(introspected_at: Option<SystemTime>) -> String {
    let Some(t) = introspected_at else {
        return "just now".to_string();
    };
    let now = SystemTime::now();
    let Ok(elapsed) = now.duration_since(t) else {
        // Clock went backwards or the stamp is in the future — treat as just
        // now so the display never panics on a weird wall-clock.
        return "just now".to_string();
    };
    let secs = elapsed.as_secs();
    if secs < 5 {
        return "just now".to_string();
    }
    if secs < 60 {
        return format!("{secs} seconds ago");
    }
    if secs < 3600 {
        return format!("{} minutes ago", secs / 60);
    }
    if secs < 86_400 {
        return format!("{} hours ago", secs / 3600);
    }
    format!("{} days ago", secs / 86_400)
}

/// Append the per-inode group view of `regs` to `lines` (US-U9 inode proof).
///
/// Grouping rules:
/// - When every registration has the SAME `Some(inode)` → render a single
///   "Shared inode N:" header followed by the indented paths (AC-U9.1).
/// - Otherwise → group registrations by `Some(inode)`; for each group render
///   an "Inode N:" header followed by the indented paths (AC-U9.2). Groups
///   are emitted in first-seen order so the layout is deterministic across
///   runs.
/// - Any registration whose inode is `None` (filesystem could not stat the
///   path) renders a single "inode: <not available on this filesystem>" line
///   labeled with the tool + path, so the user still sees the registration
///   exists but understands why no inode group is shown (AC-U9.4).
fn append_inode_groups(lines: &mut Vec<Line<'static>>, regs: &[DetailRegistration]) {
    let known: Vec<&DetailRegistration> = regs.iter().filter(|r| r.inode.is_some()).collect();
    let unknown: Vec<&DetailRegistration> = regs.iter().filter(|r| r.inode.is_none()).collect();

    // Detect the "all share one inode" branch — the unified-across-tools case.
    let single_shared_inode: Option<u64> = match known.first().and_then(|r| r.inode) {
        Some(first) if known.iter().all(|r| r.inode == Some(first)) && unknown.is_empty() => {
            Some(first)
        }
        _ => None,
    };

    if let Some(inode) = single_shared_inode {
        lines.push(Line::from(format!("  Shared inode {}:", inode)));
        for reg in &known {
            lines.push(Line::from(format!(
                "    {}: {}",
                reg.tool,
                reg.path.display()
            )));
        }
        return;
    }

    // Multi-inode (or mixed-known/unknown) view: emit one "Inode N:" group
    // per distinct known inode in first-seen order.
    let mut seen_inodes: Vec<u64> = Vec::new();
    for reg in &known {
        if let Some(ino) = reg.inode {
            if !seen_inodes.contains(&ino) {
                seen_inodes.push(ino);
            }
        }
    }
    for ino in &seen_inodes {
        lines.push(Line::from(format!("  Inode {}:", ino)));
        for reg in known.iter().filter(|r| r.inode == Some(*ino)) {
            lines.push(Line::from(format!(
                "    {}: {}",
                reg.tool,
                reg.path.display()
            )));
        }
    }

    // Any registration whose inode could not be determined renders the
    // informational text without crashing (AC-U9.4 graceful-degradation).
    for reg in &unknown {
        lines.push(Line::from(format!(
            "  {}: {} (inode: <not available on this filesystem>)",
            reg.tool,
            reg.path.display()
        )));
    }
}

// ---------------------------------------------------------------------------
// Formatters (pure, deterministic)
// ---------------------------------------------------------------------------

fn format_format(fmt: Format, quant: Option<&str>) -> String {
    let base = match fmt {
        Format::Gguf => "GGUF",
        Format::Safetensors => "Safetensors",
        Format::Bin => "Bin",
        Format::Awq => "AWQ",
        Format::Gptq => "GPTQ",
        Format::OllamaBlob => "OllamaBlob",
        Format::Mlx => "MLX",
        Format::Other => "Other",
    };
    match quant {
        Some(q) => format!("{base} [{q}]"),
        None => base.to_string(),
    }
}

/// Render the dedup-key field. When the hash is computed, show the full
/// 64-hex-char digest. While a hash is in flight, render the lazy-hash
/// progress UX per ADR-002 §"first-time hashing is slow on huge files".
fn format_dedup_key(hash: Option<ContentHash>, progress_pct: u8) -> String {
    match hash {
        Some(ContentHash(bytes)) => {
            let mut s = String::with_capacity(64);
            for b in bytes.iter() {
                s.push_str(&format!("{:02x}", b));
            }
            s
        }
        None => format!("computing dedup key... {}%", progress_pct.min(100)),
    }
}

/// Format the status header. The narrative text is part of the
/// master-acceptance contract (US-13.AC-2).
fn format_status_header(status: &UnificationStatus, canonical_size: u64) -> String {
    match status {
        UnificationStatus::SingleTool => "SINGLE TOOL — 1 path".to_string(),
        UnificationStatus::Unified { hardlink_count } => {
            format!("UNIFIED — 1 inode, {hardlink_count} hardlinks")
        }
        UnificationStatus::NotUnified { copy_count } => {
            let total = (*copy_count as u64).saturating_mul(canonical_size);
            format!(
                "NOT UNIFIED — {copy_count} separate copies ({} total)",
                format_bytes(total)
            )
        }
        UnificationStatus::PartiallyUnified {
            shared_count,
            total_count,
            ..
        } => format!(
            "PARTIALLY UNIFIED — {} of {} paths share inode",
            shared_count, total_count
        ),
    }
}

/// Format the reclaim narrative. UNIFIED reads "Reclaimed: M GB (already
/// reclaimed: M GB)" because the bytes WOULD have been duplicated had they
/// not been hardlinked.
fn format_reclaim(status: &UnificationStatus, canonical_size: u64) -> String {
    match status {
        UnificationStatus::SingleTool => "0 GB (no duplicates)".to_string(),
        UnificationStatus::Unified { hardlink_count } => {
            // "already reclaimed" = (N - 1) * size — the bytes saved by the
            // hardlinks. The "Reclaimed: M GB" form matches the master-
            // acceptance scenario "Already-unified model detail shows
            // hardlink count" → "Reclaimed: 8.8 GB".
            let bytes = ((*hardlink_count as u64).saturating_sub(1)).saturating_mul(canonical_size);
            format!("Reclaimed: {} (already reclaimed)", format_bytes(bytes))
        }
        UnificationStatus::NotUnified { .. } | UnificationStatus::PartiallyUnified { .. } => {
            let bytes = compute_reclaim_estimate(status, canonical_size);
            format!("If unified: would reclaim {}", format_bytes(bytes))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_dedup_key_renders_full_64_hex_when_hash_known() {
        let hash = ContentHash([0xAB; 32]);
        let s = format_dedup_key(Some(hash), 100);
        assert_eq!(s.len(), 64);
        assert!(s.starts_with("abab"));
    }

    #[test]
    fn format_dedup_key_renders_progress_when_hash_unknown() {
        let s = format_dedup_key(None, 73);
        assert_eq!(s, "computing dedup key... 73%");
    }

    #[test]
    fn format_status_header_for_not_unified_includes_total_bytes() {
        let s = format_status_header(
            &UnificationStatus::NotUnified { copy_count: 3 },
            4_400_000_000,
        );
        assert_eq!(s, "NOT UNIFIED — 3 separate copies (13.2 GB total)");
    }

    #[test]
    fn format_reclaim_for_not_unified_quotes_would_reclaim_phrase() {
        let s = format_reclaim(
            &UnificationStatus::NotUnified { copy_count: 3 },
            4_400_000_000,
        );
        assert_eq!(s, "If unified: would reclaim 8.8 GB");
    }

    // -----------------------------------------------------------------------
    // US-22 / step 03-01 — Metadata section render tests (AC-22-3 / AC-22-4).
    // -----------------------------------------------------------------------

    fn metadata_with_sentinel(status: &str) -> MetadataSection {
        let mut kv = BTreeMap::new();
        kv.insert("_status".to_string(), status.to_string());
        MetadataSection {
            kv,
            source: "test-tool".to_string(),
            introspected_at: None,
        }
    }

    fn metadata_with_kv() -> MetadataSection {
        let mut kv = BTreeMap::new();
        kv.insert("general.architecture".to_string(), "llama".to_string());
        kv.insert("llama.context_length".to_string(), "32768".to_string());
        MetadataSection {
            kv,
            source: "llama-cli".to_string(),
            introspected_at: None,
        }
    }

    /// RED_UNIT — AC-22-4: when the orchestrator surfaced the sentinel
    /// "(metadata unsupported for this tool)" (default-Unsupported path),
    /// the Metadata section renders that string verbatim as a bare status
    /// line.
    #[test]
    fn metadata_section_renders_unsupported_sentinel_when_status_only() {
        let meta = metadata_with_sentinel("(metadata unsupported for this tool)");
        let mut lines: Vec<Line<'static>> = Vec::new();
        append_metadata_section(&mut lines, &meta);
        let rendered: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("Metadata (from test-tool, introspected just now)"),
            "header must include source + provenance — got:\n{rendered}"
        );
        assert!(
            rendered.contains("(metadata unsupported for this tool)"),
            "sentinel must appear in body — got:\n{rendered}"
        );
    }

    /// RED_UNIT — AC-22-4: aligned key-value pairs in the Metadata section.
    /// The renderer pads each key to the longest key length so the `:`
    /// separator lines up across all rows.
    #[test]
    fn metadata_section_renders_aligned_key_value_pairs() {
        let meta = metadata_with_kv();
        let mut lines: Vec<Line<'static>> = Vec::new();
        append_metadata_section(&mut lines, &meta);
        let rendered: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("general.architecture : llama"),
            "kv row must align around colon — got:\n{rendered}"
        );
        assert!(
            rendered.contains("llama.context_length : 32768"),
            "kv row must align around colon — got:\n{rendered}"
        );
    }

    /// RED_UNIT — AC-22-3 retention: when `metadata` is `None` (legacy
    /// US-13 path), build_body_lines emits NO Metadata header line.
    #[test]
    fn build_body_lines_omits_metadata_section_when_payload_is_none() {
        let state = DetailScreenState::new(
            DetailModelView {
                id: "test-model".to_string(),
                canonical_size_bytes: 1024,
                format: Format::Gguf,
                format_quant: None,
                display_label: modeltap_core::DisplayLabel::from("test-model"),
                status: modeltap_core::ModelStatus::Healthy,
            },
            Vec::new(),
            None,
        );
        let lines = build_body_lines(&state);
        let rendered: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !rendered.contains("Metadata ("),
            "no Metadata section should render when state.metadata is None — got:\n{rendered}"
        );
    }

    /// RED_UNIT — AC-22-4: with metadata attached via with_metadata, the
    /// Metadata section appears in build_body_lines output. End-to-end
    /// integration of the per-line builder + the payload.
    #[test]
    fn build_body_lines_renders_metadata_section_when_payload_present() {
        let mut state = DetailScreenState::new(
            DetailModelView {
                id: "test-model".to_string(),
                canonical_size_bytes: 1024,
                format: Format::Gguf,
                format_quant: None,
                display_label: modeltap_core::DisplayLabel::from("test-model"),
                status: modeltap_core::ModelStatus::Healthy,
            },
            Vec::new(),
            None,
        );
        state = state.with_metadata(metadata_with_kv());
        let lines = build_body_lines(&state);
        let rendered: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("Metadata (from llama-cli"),
            "with metadata payload the section must render — got:\n{rendered}"
        );
    }
}
