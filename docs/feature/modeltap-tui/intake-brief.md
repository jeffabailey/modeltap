# modeltap-tui — Intake Brief

**Captured:** 2026-04-28
**Wave entry point:** DISCUSS
**Source:** `/nw:new` wizard, user brief

## One-line description

A Rust TUI (built with [ratatui](https://ratatui.rs/)) that lets a user discover, inspect, and clean up locally-downloaded AI models across multiple local-AI tools, and de-duplicate them by linking one canonical copy across all tools.

## Problem

Each local-AI tool (Ollama, llama.cpp / `llama-cli`, Hugging Face cache, LM Studio, Jan, ...) downloads and stores its own copy of every model the user pulls. Running multiple tools wastes large amounts of disk space on duplicate model files, and there is no single place to see what is installed where, what overlaps, or what to delete.

## Target users

Local-AI power users on macOS and Linux who run more than one inference tool and want to:

- See all locally-downloaded models in one view, grouped by tool.
- Reclaim disk space by deleting models per tool.
- Use a single canonical copy of a model across multiple tools (UMR-equivalent).
- Add support for new tools without forking the project.

## Tools to support in v1

1. **Ollama**
2. **llama-cli** (llama.cpp)
3. **Hugging Face** (`hf` CLI / HF cache)
4. **LM Studio**

Architecture must make adding a 5th tool (e.g., **Atomic Chat**) a small, isolated change.

## Functional requirements (from user brief)

### F1 — Multi-tool model inventory
- Discover models on disk for each supported tool by reading the tool's on-disk layout (model directory, manifest, registry, etc.).
- Show every model with: name, size, tool(s) it is registered with, on-disk path(s).

### F2 — TUI layout (ratatui)
- **Left pane:** list of supported tools.
- **Right pane:** when a tool is selected, list the models that tool has registered.
- **Bottom bar:** all keyboard shortcuts always visible.
- A red icon next to a model that is **only usable in one tool** (i.e., not deduplicable / not linkable to others — e.g., format-locked).

### F3 — Hotkey: `u` (unify)
- Take a model the user already downloaded with one tool and make it available to all other compatible tools, **without** copying the file (hardlink or pointer/config update, whichever the target tool supports).
- Reference behaviour: <https://github.com/EvanZhouDev/umr>

### F4 — Hotkey: `z` (zap)
- Delete every model for a specific tool. Destructive; must confirm.
- Delete a single model for a specific tool. Destructive; must confirm.

### F5 — Plugin / extensibility architecture
- Adding a new tool = implementing a small trait (or equivalent) that exposes:
  - discovery (where does this tool keep its models?)
  - listing (enumerate registered models)
  - linking (how does this tool register an external model file? hardlink, manifest entry, symlink, config update, ...)
  - deletion (how do you remove a model and its registration cleanly?)
  - capability metadata (which formats does this tool accept? -- powers the red-icon "only-one-tool" indicator)
- New tools register themselves; the rest of the app should not need to change.

### F6 — Format / capability awareness
- Track per-tool accepted formats (e.g., GGUF, MLX, safetensors, Ollama blob layout, ...).
- A model is "deduplicable" when at least one other supported tool can consume the same on-disk file via linking.
- Models that no other supported tool can consume get the red icon in F2.

## Non-functional / scope flags (chosen in /nw:new)

- **Cross-platform from day one: macOS + Linux.** Discovery paths, hardlinking, and config-update strategies must work on both. Windows deferred.
- **Plugin extensibility is a v1 must-have**, not v2.

## Explicit non-goals (v1)

- **MLX format** — out of scope for v1; revisit after GGUF parity.
- **Windows support** — only windows subsystem for linux support.
- **Downloading new models from a registry** — `modeltap` manages already-downloaded models; tools (or `hf`) handle initial download.
- **Running inference** — view/manage only, no serving.
- **Mouse interaction** — keyboard-first; brief mentions "click the tool" but ratatui apps are typically keyboard-driven, so interpret as "select and activate" via keys (Enter / arrows).

## Open questions for DISCUSS to resolve

1. **Canonical store location** — does modeltap maintain its own store (like UMR's `umr add ./path`) or always point at the original tool's store? Probably both, with a default.

Answer: Just point at the original tool's store so we have once source of truth.

2. **Linking strategy per tool** — for each of {Ollama, llama-cli, HF, LM Studio}, document exactly how to register an external file (hardlink target dir? edit a manifest? edit a SQLite catalog?). DISCUSS may need light spike research per tool.


3. **What does "only-one-tool model" mean concretely?** — is it format-based (GGUF works in N tools, Ollama-blob only in Ollama) or layout-based (a tool that requires its own dir layout)?

Answer: format based, yes Ollama-blog is a good example.


4. **Confirmation UX for `z` (zap)** — modal? typed confirmation? undo window?

Answer: Typed confirmation is fine, no undo required.

5. **Concurrency** — what happens if a tool is running and holds a model file open while we try to relink/delete?

Tell the user to close the tool then let them retry.

6. **Identifying "the same model"** — by SHA256 of the file? by HF repo + quant? by name? Important because dedup correctness depends on it.

Answer: Pick the most durable one. If it's SHA256 then fine, otherwise use the HF repo + quant.

7. **State persistence** — does modeltap maintain an index/registry on disk (UMR does), and where? `~/.modeltap/`?

Answer: No, keep the tool's model directory as the source of truth.

## Reference: UMR (the JS tool we're matching/extending)

- Repo: <https://github.com/EvanZhouDev/umr>
- Behaviour we want to replicate:
  - `umr add hf <repo>` — register an HF-cache model in the unified registry.
  - `umr add ./file.gguf` — register a local file (clones into UMR store).
  - `umr link <client> <model-id>` — make the registered model usable by a client (Ollama / LM Studio / Jan).
  - Linking uses hardlinks and/or per-tool config updates so no duplicate bytes hit disk.
- Differences in `modeltap`:
  - Rust, not JS.
  - Interactive ratatui TUI, not pure CLI.
  - Plugin trait for tools.
  - Cleanup-first framing (zap), not just registration-first.
