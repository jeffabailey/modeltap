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
        }
    }

    /// Set the most-recent progress percentage. The screen renders this
    /// while `content_hash.is_none()`.
    pub fn set_hash_progress(&mut self, percent: u8) {
        self.hash_progress_pct = percent.min(100);
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
    for reg in &state.registrations {
        lines.push(Line::from(format!(
            "  {}: {}",
            reg.tool,
            reg.path.display()
        )));
    }
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
    lines
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
}
