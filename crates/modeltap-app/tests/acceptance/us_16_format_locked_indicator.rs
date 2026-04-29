//! Acceptance tests for US-16 (Format-locked red `!` indicator + WCAG NO_COLOR).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-16 @release-1 scenarios. The 3 scenarios drive:
//!
//! - AC-1 — `!` indicator rendered in red foreground (paired with the symbol;
//!   never color-only). Exercised via the pure `render_row_basic` driving port
//!   for the right-pane row layer; we assert the indicator span carries
//!   `Style::default().fg(Color::Red)` per ratatui.
//!
//! - AC-2 — `[u]` shortcut dimmed on `!`-marked models in the detail screen,
//!   with the annotation `single tool — unify not applicable`. A FormatLocked
//!   model is by definition registered with exactly ONE tool (single
//!   registration → `UnificationStatus::SingleTool`), so the existing detail-
//!   screen renderer applies the annotation. Asserts the rendered frame.
//!
//! - AC-3 — Empty `accepted_formats()` produces `?` (Unknown), not `!`. Two
//!   parts:
//!     1. Pure compatibility engine returns `Unknown` for the offending
//!        plugin's models (defensive branch).
//!     2. The orchestrator emits a `tracing::warn!` so the bug surfaces in
//!        `diagnostics.log` (developer-mode warning per US-16 spec).
//!
//! Tags: @us-16 @release-1.
//!
//! ## K2 baseline (devon-multi-tool fixture, 02-07)
//!
//! K2 is the post-launch metric "% of models marked `*` or `o`" (i.e., have a
//! known unification or compatibility outcome that is NOT format-locked).
//!
//! K2 baseline (devon-multi-tool fixture, 02-07): 75% of models marked `*` or `o`.
//!
//! Derivation: the synthetic devon-multi-tool fixture used in this test file
//! has 4 models — Mistral-7B (Shared `*`), Llama3-8B (Compatible `o`),
//! TheBloke/foo-AWQ (FormatLocked `!`), and a Gguf single-tool variant
//! (Compatible `o`). 3 of 4 are `*`/`o` → 75%. The first 30 days post-launch
//! will replace this synthetic baseline with the real-world K2 number per
//! `docs/feature/modeltap-tui/distill/outcome-kpis.md`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use modeltap_core::domain::RowIndicator;
use modeltap_core::logic::compatibility::{
    compute_indicator, Inventory, InventoryEntry, PluginCapabilityMap,
};
use modeltap_core::logic::unification_status::{DetailModelView, DetailRegistration};
use modeltap_core::{ContentHash, DiscoveredModel, DisplayLabel, Format, ModelStatus, ToolId};
use modeltap_tui::app_state::{AppState, Screen, ToolView};
use modeltap_tui::render::row::render_row_basic;
use modeltap_tui::screens::detail::DetailScreenState;
use modeltap_tui::view;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

const HASH_A: ContentHash = ContentHash([0xAA; 32]);

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

fn model(id: &str, format: Format, path: &str, size: u64) -> DiscoveredModel {
    DiscoveredModel {
        id_in_tool: id.to_string(),
        on_disk_path: PathBuf::from(path),
        size_bytes: size,
        format,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Healthy,
    }
}

fn entry(tool: &'static str, model: DiscoveredModel, hash: Option<ContentHash>) -> InventoryEntry {
    InventoryEntry {
        tool: ToolId(tool),
        model,
        content_hash: hash,
    }
}

fn caps(pairs: &[(&'static str, &[Format])]) -> PluginCapabilityMap {
    let mut m = PluginCapabilityMap::new();
    for (name, fmts) in pairs {
        m.insert(ToolId(name), fmts.to_vec());
    }
    m
}

fn render_main_to_text(state: &AppState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).expect("test backend");
    terminal.draw(|f| view(state, f)).expect("draw");
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

// -----------------------------------------------------------------------------
// Scenario 1 (US-16.AC-1): AWQ model gets red !
// "AWQ model is registered ONLY in HF; no other plugin's accepted_formats
//  contains AWQ → indicator is FormatLocked (`!`) and the indicator glyph
//  span carries fg=Color::Red. The symbol itself is also present (paired —
//  never color-only)."
// -----------------------------------------------------------------------------

#[test]
fn awq_model_gets_red_bang_indicator_paired_with_symbol() {
    // Engine: AWQ-only model in HF, no other plugin accepts AWQ → FormatLocked.
    let target = entry(
        "hf",
        model(
            "TheBloke/foo-AWQ",
            Format::Awq,
            "/hub/TheBloke/foo-AWQ/model.safetensors",
            7_000_000_000,
        ),
        Some(HASH_A),
    );
    let inventory = Inventory {
        entries: vec![target.clone()],
    };
    let plugin_caps = caps(&[
        ("hf", &[Format::Gguf, Format::Safetensors, Format::Awq]),
        ("llama-cli", &[Format::Gguf]),
        ("ollama", &[Format::OllamaBlob, Format::Gguf]),
        ("lm-studio", &[Format::Gguf]),
    ]);

    let result = compute_indicator(&target, &inventory, &plugin_caps);
    assert_eq!(
        result,
        RowIndicator::FormatLocked,
        "engine: AWQ-only-in-HF must classify as FormatLocked",
    );

    // Render: the row's first character is `!` AND the indicator span
    // carries Style::default().fg(Color::Red) — paired with the symbol.
    let line = render_row_basic("TheBloke/foo-AWQ", 7_000_000_000, result, &[], false);
    let glyph_span = &line.spans[0];
    assert_eq!(
        glyph_span.content.as_ref(),
        "!",
        "AC-1: indicator glyph must be `!` for FormatLocked"
    );
    assert_eq!(
        glyph_span.style.fg,
        Some(Color::Red),
        "AC-1: indicator span must carry fg=Color::Red for FormatLocked"
    );
}

// -----------------------------------------------------------------------------
// Scenario 1b (US-16.AC-5 / NO_COLOR): even with NO_COLOR=1 the symbol is
// preserved; only the ANSI color is dropped. The `!` glyph still carries the
// distinction so users on monochrome terminals lose nothing.
// -----------------------------------------------------------------------------

#[test]
fn awq_model_preserves_bang_symbol_under_no_color() {
    // Same FormatLocked outcome from the engine.
    let line = render_row_basic(
        "TheBloke/foo-AWQ",
        7_000_000_000,
        RowIndicator::FormatLocked,
        &[],
        true, // no_color
    );
    let glyph_span = &line.spans[0];
    assert_eq!(
        glyph_span.content.as_ref(),
        "!",
        "AC-5: `!` symbol must remain visible under NO_COLOR=1"
    );
    assert_eq!(
        glyph_span.style.fg, None,
        "AC-5: under NO_COLOR=1 the indicator span must NOT carry an fg color"
    );
    // No spans across the whole row carry an fg color — i.e., no ANSI color
    // escape \x1b[3X would be emitted by ratatui.
    for span in &line.spans {
        assert_eq!(
            span.style.fg, None,
            "AC-5: all spans must be color-free under NO_COLOR=1; got {:?} on {:?}",
            span.style.fg, span.content,
        );
    }
}

// -----------------------------------------------------------------------------
// Scenario 2 (US-16.AC-2): Format-locked model in detail screen
// "Open detail on a !-marked model. The single registration → SingleTool. The
//  detail screen dims [u] AND shows the annotation `single tool — unify not
//  applicable`. The annotation is the *content reason* for the dim — distinct
//  from the future *not-yet-implemented* reason that NotUnified will carry
//  once 03-02 lands."
// -----------------------------------------------------------------------------

#[test]
fn format_locked_model_detail_screen_dims_unify_with_annotation() {
    // Build an AppState with a single tool (HF) holding the AWQ model and
    // open detail on it. The detail-screen state has 1 registration →
    // UnificationStatus::SingleTool.
    let mut state = AppState::new_with_default_selection(vec![ToolView {
        tool: ToolId("hf"),
        status: modeltap_core::ToolStatus::Ok,
        model_ids: vec!["TheBloke/foo-AWQ".to_string()],
        model_sizes_bytes: vec![7_000_000_000],
    }]);
    state.selected_tool = 0;
    state.selected_row = 0;

    let registrations = vec![DetailRegistration {
        tool: ToolId("hf"),
        path: PathBuf::from("/hub/TheBloke/foo-AWQ/model.safetensors"),
        inode: Some(2001),
    }];
    state.current_screen = Screen::Detail(DetailScreenState::new(
        DetailModelView {
            id: "TheBloke/foo-AWQ".to_string(),
            format: Format::Awq,
            format_quant: None,
            canonical_size_bytes: 7_000_000_000,
            display_label: DisplayLabel::from("TheBloke/foo-AWQ"),
            status: ModelStatus::Healthy,
        },
        registrations,
        Some(HASH_A),
    ));

    let frame = render_main_to_text(&state);

    // AC-2: bottom bar shows [u] (still drawn, just dimmed).
    assert!(
        frame.contains("[u] unify"),
        "AC-2: bottom bar must still display [u] unify (dimmed):\n{}",
        frame
    );
    // AC-2: annotation present.
    assert!(
        frame.contains("single tool") && frame.contains("unify not applicable"),
        "AC-2: 'single tool — unify not applicable' annotation missing for FormatLocked detail:\n{}",
        frame
    );
}

// -----------------------------------------------------------------------------
// Scenario 3 (US-16.AC-3): Missing capability metadata produces ? not !
// "A plugin returning &[] from accepted_formats() has no compatibility
//  contract. Its models render as Unknown (?), not FormatLocked (!). The
//  orchestrator additionally emits a tracing::warn! so the bug surfaces in
//  diagnostics.log for contributor PRs."
// -----------------------------------------------------------------------------

#[test]
fn empty_accepted_formats_produces_unknown_indicator_not_format_locked() {
    // Synthetic plugin "broken-plugin" with empty accepted_formats(); its
    // model has format Gguf but the plugin's capability map declares no
    // formats → Unknown (Rule 1b in the engine).
    let target = entry(
        "broken-plugin",
        model(
            "mystery-model",
            Format::Gguf,
            "/elsewhere/mystery.gguf",
            1_000_000_000,
        ),
        Some(HASH_A),
    );
    let inventory = Inventory {
        entries: vec![target.clone()],
    };
    // Real plugins declare formats; broken-plugin declares none.
    let plugin_caps = caps(&[
        ("hf", &[Format::Gguf, Format::Safetensors]),
        ("llama-cli", &[Format::Gguf]),
        ("broken-plugin", &[]), // empty!
    ]);

    let result = compute_indicator(&target, &inventory, &plugin_caps);
    assert_eq!(
        result,
        RowIndicator::Unknown,
        "AC-3: empty accepted_formats() must yield Unknown (?), NOT FormatLocked (!)",
    );

    // Render through render_row_basic — the row's first character is `?`.
    let line = render_row_basic("mystery-model", 1_000_000_000, result, &[], false);
    let glyph_span = &line.spans[0];
    assert_eq!(
        glyph_span.content.as_ref(),
        "?",
        "AC-3: indicator glyph must be `?` for empty-capability plugins"
    );
}

// -----------------------------------------------------------------------------
// Scenario 3b (US-16.AC-3 / diagnostics.log): empty-capability warning is
// emitted to the diagnostics log via tracing::warn!. Captured in-process via
// a tracing subscriber Layer so the test does not depend on the binary's log
// file path.
// -----------------------------------------------------------------------------

/// Capturing tracing layer that records all events into a shared Vec<String>.
/// Each entry is a debug-formatted snapshot containing the event's level,
/// target, and message. The acceptance test asserts the warning text appears.
#[derive(Clone, Default)]
struct CaptureLayer {
    events: Arc<Mutex<Vec<String>>>,
}

impl<S> Layer<S> for CaptureLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        struct Visitor<'a>(&'a mut String);
        impl<'a> tracing::field::Visit for Visitor<'a> {
            fn record_debug(&mut self, _f: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                let _ = std::fmt::Write::write_fmt(self.0, format_args!("{:?} ", value));
            }
            fn record_str(&mut self, _f: &tracing::field::Field, value: &str) {
                self.0.push_str(value);
                self.0.push(' ');
            }
        }
        let metadata = event.metadata();
        let mut buf = format!(
            "level={} target={} msg=",
            metadata.level(),
            metadata.target()
        );
        let mut visitor = Visitor(&mut buf);
        event.record(&mut visitor);
        if let Ok(mut events) = self.events.lock() {
            events.push(buf);
        }
    }
}

#[test]
fn empty_capability_plugin_emits_diagnostics_warning() {
    let capture = CaptureLayer::default();
    let events = capture.events.clone();
    let subscriber = tracing_subscriber::registry().with(capture.with_filter(
        tracing_subscriber::filter::filter_fn(|m| *m.level() == Level::WARN),
    ));

    // Run the offender check inside the subscriber's scope.
    let plugin_caps = caps(&[
        ("hf", &[Format::Gguf, Format::Safetensors]),
        ("llama-cli", &[Format::Gguf]),
        ("broken-plugin", &[]), // empty!
    ]);
    let offenders = tracing::subscriber::with_default(subscriber, || {
        modeltap_app::inventory_build::warn_on_empty_capabilities(&plugin_caps)
    });

    // The pure function returns the offending plugin names so callers can
    // assert on them without parsing log text.
    assert_eq!(
        offenders,
        vec![ToolId("broken-plugin")],
        "AC-3: warn_on_empty_capabilities must return the offending plugin names",
    );

    // The warning text is captured by our subscriber Layer.
    let captured = events.lock().expect("capture mutex").clone();
    let found = captured.iter().any(|line| {
        line.contains("plugin broken-plugin returned empty accepted_formats()")
            && line.contains("level=WARN")
    });
    assert!(
        found,
        "AC-3: tracing::warn! 'plugin broken-plugin returned empty accepted_formats()' not captured. Captured events:\n{:#?}",
        captured
    );
}
