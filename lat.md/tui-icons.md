# TUI Icons

Per-tool PNG icons render to the left of each tool name in the [[crates/modeltap-tui/src/render/left_pane.rs#render|left pane]] using terminal graphics protocols (Kitty, iTerm2, Sixel) with half-block fallback for unsupported terminals.

The icons identify tools at a glance — `hf`, `lm-studio`, `atomic-chat`, `gpt4all`, and `ollama` each ship a branded PNG under `assets/`. Tools without a matching asset leave the icon column blank but reserve the same width so names stay vertically aligned.

## Rationale

Three rendering strategies were considered before settling on terminal graphics protocols.

Unicode glyphs work in any terminal but provide poor brand recognition — every tool collapses to a generic emoji. ASCII-art conversion is universal but illegible at the 3-cell icon size required by a 30%-width left pane. Real PNG rendering via [[tui-icons#Ratatui-image dependency]] gives crisp branding in supported terminals (Ghostty, iTerm2, Kitty, WezTerm) and degrades gracefully via half-block elsewhere.

The user-visible cost is that Apple Terminal and tmux sessions show no icons, only text. The win is that the modal terminal on macOS development machines is iTerm2 / Ghostty, both of which render icons natively.

## Ratatui-image dependency

`ratatui-image = "2.0"` is pinned in `crates/modeltap-tui/Cargo.toml` because it is the only release line compatible with `ratatui = "0.28"` — the version this workspace pins via the workspace `Cargo.toml`.

Bumping to a newer ratatui-image (3.x onward) requires upgrading ratatui to 0.29 or 0.30, which is a workspace-wide migration touching every render module. Until that ratatui upgrade lands, do not silently tick this dependency.

The `image = "0.25"` peer dependency is bound to `["png"]` features only — modeltap ships no JPEG or WebP assets, and disabling default features shrinks the dependency graph by ~30 crates.

## Two-layer design

The [[crates/modeltap-tui/src/render/icons.rs]] module separates a pure resolver from a stateful cache so most of the icon logic can be unit-tested without a terminal.

[[crates/modeltap-tui/src/render/icons.rs#asset_for]] is a pure function from a tool-id string to optional embedded PNG bytes. Assets are bundled at compile time via `include_bytes!` so the binary is self-contained — no asset-path resolution at runtime, no missing-file failure mode.

[[crates/modeltap-tui/src/render/icons.rs#try_init]] builds a thread-local `IconCache` of pre-encoded `Box<dyn Protocol>` values, one per tool. The cache is `Option<_>` because two paths must succeed silently when icons are unavailable — see [[tui-icons#Headless test fallback]].

[[crates/modeltap-tui/src/render/icons.rs#with_icon]] hands the cached Protocol to a callback rather than returning a reference. The borrow stays inside the `RefCell` so callers cannot accidentally extend its lifetime, and the closure form keeps the rendering call site readable.

## Headless test fallback

`render::icons::try_init` is called only from [[crates/modeltap-app/src/interactive.rs#run]] — the production interactive event loop. The headless test backend never initializes the cache, so [[crates/modeltap-tui/src/render/icons.rs#with_icon]] is a no-op in tests and the left pane renders text-only.

This preserves snapshot stability: existing TestBackend assertions never see icon escape sequences, and the new icon column is always reserved-but-blank in tests.

The same fallback fires in production when [[crates/modeltap-tui/src/render/icons.rs#try_init|Picker::from_termios]] errors — pipes, unusual harnesses, or terminals lacking any supported graphics protocol all degrade to text-only without crashing.

## Left pane layout

[[crates/modeltap-tui/src/render/left_pane.rs#render]] now bypasses the ratatui `List` widget — the widget owned its own item layout and gave us no place to embed an `Image` per row.

The refactor renders the outer `Block` plus per-row text manually via `Buffer::set_string`, which is straightforward because each row is exactly one terminal line tall.

Each row is split via [[crates/modeltap-tui/src/render/left_pane.rs#split_row]] into a fixed-width icon area (3 cells) and a remaining text area separated by a 1-cell gap. The icon column is reserved for every row regardless of whether an icon is available, so tool names line up vertically across rows — a tool without an icon does not pull its neighbors leftward.

Selection highlighting (REVERSED + optional BOLD when the left pane has focus) applies only to the text area. Cell-level inversion does not compose with graphics-protocol output, so a clean icon next to an inverted text bar reads better than a partially-inverted bitmap.
