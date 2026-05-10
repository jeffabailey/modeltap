//! Per-tool PNG icons rendered to the left of each tool name in the left pane.
//!
//! Terminals can't display bitmaps directly — `ratatui-image` 2.x emits the
//! escape sequences for the Kitty / iTerm2 / Sixel graphics protocols (and
//! falls back to half-block rasterization on unsupported terminals).
//!
//! # Two-layer design
//!
//! 1. [`asset_for`] is a pure function from a tool-id string to optional
//!    PNG bytes embedded at compile time via `include_bytes!`. No I/O, no
//!    state. Unit-tested below.
//! 2. [`with_icon`] looks up a pre-baked `Box<dyn Protocol>` from a thread-
//!    local cache and hands it to a closure for rendering. The cache is
//!    initialized at most once via [`try_init`] from the production
//!    interactive loop. Headless tests skip init → `with_icon` is a no-op
//!    and the left pane renders text-only (preserving snapshot stability).
//!
//! Per `crates/modeltap-tui/Cargo.toml`, this module is the sole consumer
//! of `ratatui-image` and `image` — keep that boundary tight. The Picker
//! is held by-value behind a `RefCell` because `new_protocol` requires
//! `&mut self`; we re-encode each protocol once at init and cache the
//! result, so render-time has zero Picker contention.

use std::cell::RefCell;
use std::collections::HashMap;

use ratatui::layout::Rect;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use ratatui_image::{Image, Resize};

/// Compile-time embed of every tool icon under `assets/`. Filenames are
/// canonical asset names, mapped to one or more `ToolId` strings via
/// [`asset_for`]. The path is relative to *this source file* — the
/// `../../../../assets/` walks up to repo root.
const HF_PNG: &[u8] = include_bytes!("../../../../assets/hf.png");
const LMSTUDIO_PNG: &[u8] = include_bytes!("../../../../assets/lmstudio.png");
const ATOMICCHAT_PNG: &[u8] = include_bytes!("../../../../assets/atomicchat.png");
const GPT4ALL_PNG: &[u8] = include_bytes!("../../../../assets/gpt4all.png");
const OLLAMA_PNG: &[u8] = include_bytes!("../../../../assets/ollama.png");

/// Pure resolver: tool-id string → embedded PNG bytes, if an asset exists.
///
/// Both forms of the Atomic Chat plugin (`"atomic-chat"` test fixture and
/// `"Atomic Chat"` production display name — see
/// `plugins/atomic-chat/src/lib.rs::TOOL_NAME`) map to the same asset so
/// either registration paints the same icon.
///
/// Tools without a matching asset return `None` — the left pane simply
/// omits the icon for that row but still reserves the icon column width.
// @lat: [[tui-icons#Two-layer design]]
pub fn asset_for(tool_id: &str) -> Option<&'static [u8]> {
    match tool_id {
        "hf" => Some(HF_PNG),
        "lm-studio" => Some(LMSTUDIO_PNG),
        "atomic-chat" | "Atomic Chat" => Some(ATOMICCHAT_PNG),
        "gpt4all" => Some(GPT4ALL_PNG),
        "ollama" => Some(OLLAMA_PNG),
        _ => None,
    }
}

/// Render-time icon size in terminal cells. Three columns wide × one row
/// tall keeps each row height-stable and leaves the surrounding text
/// layout intact. The pre-encoded Protocol is baked at this size; if the
/// row layout ever changes, regenerate at the new dimensions.
pub const ICON_RECT: Rect = Rect {
    x: 0,
    y: 0,
    width: 3,
    height: 1,
};

struct IconCache {
    /// Tool-id → pre-encoded Protocol ready for the stateless `Image`
    /// widget. Boxed because `Picker::new_protocol` returns
    /// `Box<dyn Protocol>`, and the trait isn't object-safe to clone.
    protocols: HashMap<&'static str, Box<dyn Protocol>>,
}

thread_local! {
    /// Optional because (a) test backends never call [`try_init`] and
    /// must render text-only, and (b) `Picker::from_termios` can fail on
    /// pipes / non-tty stdout — silent fallback is the right behavior.
    /// `RefCell` because rendering only borrows immutably and we never
    /// re-init mid-session.
    static ICONS: RefCell<Option<IconCache>> = const { RefCell::new(None) };
}

/// Initialize the icon cache by probing the terminal and pre-encoding
/// every embedded asset for the current graphics protocol. Called once
/// from `interactive::run` after raw mode is enabled.
///
/// Returns `Ok(())` on success or `Err` describing why icons will not be
/// available — callers (only `interactive.rs`) treat any error as
/// "render text-only" and continue. A second call is a no-op.
pub fn try_init() -> Result<(), Box<dyn std::error::Error>> {
    let already_initialized = ICONS.with(|cell| cell.borrow().is_some());
    if already_initialized {
        return Ok(());
    }

    let mut picker = Picker::from_termios()?;
    picker.guess_protocol();

    let mut protocols: HashMap<&'static str, Box<dyn Protocol>> = HashMap::new();
    for (tool_id, bytes) in EMBEDDED_ASSETS {
        match decode_and_encode(&mut picker, bytes) {
            Ok(proto) => {
                protocols.insert(tool_id, proto);
            }
            // Skip individual decode failures rather than failing the
            // whole init — one corrupt asset shouldn't blank out every
            // icon. Production has compile-embedded PNGs so this is
            // belt-and-braces, but the cost is two lines.
            Err(_) => continue,
        }
    }

    ICONS.with(|cell| *cell.borrow_mut() = Some(IconCache { protocols }));
    Ok(())
}

/// Hand the pre-encoded Protocol for `tool_id` to `f` for rendering, if
/// one exists. No-op when the cache is uninitialized (headless tests) or
/// when the tool has no matching asset (e.g. `"ollama"`).
///
/// We pass via callback so the Protocol stays inside the `RefCell` borrow
/// — handing back a raw `&dyn Protocol` would either require cloning the
/// boxed protocol (not object-safe for `Clone`) or leaking the borrow.
pub fn with_icon<F>(tool_id: &str, f: F)
where
    F: FnOnce(&dyn Protocol),
{
    ICONS.with(|cell| {
        if let Some(cache) = cell.borrow().as_ref() {
            if let Some(proto) = cache.protocols.get(tool_id) {
                f(&**proto);
            }
        }
    });
}

/// Render `tool_id`'s icon into `area` if available. Convenience wrapper
/// over [`with_icon`] + the stateless `Image` widget; returns `true` when
/// an icon was actually rendered so callers can decide whether to leave
/// the icon column blank or pad differently.
pub fn render_icon(frame: &mut ratatui::Frame<'_>, area: Rect, tool_id: &str) -> bool {
    let mut rendered = false;
    with_icon(tool_id, |proto| {
        let widget = Image::new(proto);
        frame.render_widget(widget, area);
        rendered = true;
    });
    rendered
}

const EMBEDDED_ASSETS: &[(&str, &[u8])] = &[
    ("hf", HF_PNG),
    ("lm-studio", LMSTUDIO_PNG),
    ("atomic-chat", ATOMICCHAT_PNG),
    ("Atomic Chat", ATOMICCHAT_PNG),
    ("gpt4all", GPT4ALL_PNG),
    ("ollama", OLLAMA_PNG),
];

fn decode_and_encode(
    picker: &mut Picker,
    bytes: &[u8],
) -> Result<Box<dyn Protocol>, Box<dyn std::error::Error>> {
    let dyn_img = image::load_from_memory(bytes)?;
    let proto = picker.new_protocol(dyn_img, ICON_RECT, Resize::Fit(None))?;
    Ok(proto)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_lookup_covers_every_plugin_with_a_matching_png() {
        // Every production plugin ToolId documented in
        // `plugins/*/src/lib.rs::TOOL_NAME` — locked in so renaming a
        // plugin without updating the icon table is a test failure
        // rather than a silently-missing icon.
        assert!(asset_for("hf").is_some(), "hf plugin must have an icon");
        assert!(
            asset_for("lm-studio").is_some(),
            "lm-studio plugin must have an icon"
        );
        assert!(
            asset_for("atomic-chat").is_some(),
            "atomic-chat fixture id must have an icon"
        );
        assert!(
            asset_for("Atomic Chat").is_some(),
            "production Atomic Chat display id must share the fixture's icon"
        );
        assert!(
            asset_for("gpt4all").is_some(),
            "gpt4all plugin must have an icon"
        );
        assert!(
            asset_for("ollama").is_some(),
            "ollama plugin must have an icon"
        );
    }

    #[test]
    fn unknown_tool_id_returns_none() {
        assert!(asset_for("does-not-exist").is_none());
        assert!(asset_for("").is_none());
    }

    #[test]
    fn atomic_chat_aliases_share_the_same_asset() {
        // Both ToolId forms must resolve to byte-identical asset content
        // so the same icon paints regardless of which registration the
        // composition root chose.
        let fixture = asset_for("atomic-chat").expect("fixture id");
        let prod = asset_for("Atomic Chat").expect("production id");
        assert!(
            std::ptr::eq(fixture.as_ptr(), prod.as_ptr()),
            "the two atomic-chat ToolIds must share the same embedded PNG"
        );
    }

    #[test]
    fn embedded_assets_are_non_empty_pngs() {
        // Defensive against a build-time path glitch silently embedding
        // an empty file. PNG magic is 8 bytes: 89 50 4E 47 0D 0A 1A 0A.
        const PNG_MAGIC: &[u8; 8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        for (id, bytes) in EMBEDDED_ASSETS {
            assert!(
                bytes.len() > 8,
                "{id} asset is suspiciously small ({} bytes)",
                bytes.len()
            );
            assert_eq!(
                &bytes[..8],
                PNG_MAGIC,
                "{id} asset is missing PNG magic bytes — wrong file embedded?",
            );
        }
    }

    #[test]
    fn with_icon_is_a_noop_when_cache_uninitialized() {
        // The thread-local starts as None in fresh test threads — this
        // is the headless-render contract: tests never see icons.
        let mut called = false;
        with_icon("hf", |_| called = true);
        assert!(
            !called,
            "with_icon must not invoke its callback before try_init() runs"
        );
    }
}
