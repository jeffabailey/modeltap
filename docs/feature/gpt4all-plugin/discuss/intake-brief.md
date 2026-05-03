# Intake Brief — GPT4All as 5th Production Plugin

## Summary

Add GPT4All as the 5th production plugin in modeltap (joining ollama, hf, lm-studio, atomic-chat) so Devon can see GPT4All-installed models in the left pane, dedup them against other tools' GGUFs via SHA256, and unify them into shared inodes.

## Context

modeltap's plugin contract is frozen at 6 methods on the `Tool` trait per ADR-001. Four production plugins exist plus one US-18 certification fixture. This is the first new plugin added since the cross-tool-model-unify feature shipped, so it inherits all the dedup/unify infrastructure for free.

The user explicitly chose GPT4All over alternatives (text-generation-webui, KoboldCpp, Msty, llamafile) because:
- Clean fixed default directories (vs. user-configured paths)
- Pure GGUF blobs (high overlap with HF cache + Ollama, big dedup wins)
- Mature user base
- Storage layout requires zero manifest parsing (unlike Ollama)

## Storage Layout (verified 2026-05-02)

GPT4All has two distinct default storage locations depending on which front-end the user installed:

### Python SDK (`gpt4all` Python package)

- All platforms: `~/.cache/gpt4all/`
- Source: `gpt4all-bindings/python/gpt4all/gpt4all.py` — `DEFAULT_MODEL_DIRECTORY = Path.home() / ".cache" / "gpt4all"`

### Desktop chat app (`gpt4all-chat`)

- macOS: `~/Library/Application Support/nomic-ai/gpt4all-chat/`
- Linux: `~/.local/share/nomic-ai/gpt4all-chat/`
- Windows (WSL out-of-scope per project constraint #6): `%APPDATA%\Local\nomic-ai\gpt4all-chat\`
- Source: `gpt4all-chat/src/mysettings.cpp` — `defaultLocalModelsPath()` via Qt `QStandardPaths::AppLocalDataLocation`.

### Both layouts

Each location is a flat directory of `*.gguf` files (e.g., `Meta-Llama-3-8B-Instruct.Q4_0.gguf`). No manifest sidecar, no metadata index. The file IS the model.

## Plugin Mapping

| `Tool` trait method | GPT4All implementation |
|---|---|
| `name()` | `"gpt4all"` |
| `discover(env)` | Walk both default dirs (any that exist) for `*.gguf` files; skip dotfiles. Each file → `DiscoveredModel` with `id_in_tool = filename`, `on_disk_path`, `size_bytes` from metadata, `format = Format::Gguf`, `display_label` from filename without extension. |
| `link(canonical, target)` | Hardlink `canonical.path` → `target.path` (same as ollama / hf / lm-studio impls). Cross-fs fallback per ADR-008. |
| `delete_one(model)` | Remove the single file. |
| `delete_all()` | Remove all `*.gguf` files in both default dirs. |
| Plugin status / install detection | Tool is "installed" when at least one of the two default dirs exists. |

## Env-Var Override

Mirror existing plugins' pattern:
- `MODELTAP_GPT4ALL_DIRS` — colon-separated path list overrides the defaults (used by acceptance tests with `tempfile::TempDir`).

## Acceptance Boundary (skeleton)

- Devon launches modeltap with GPT4All installed → left pane shows `gpt4all (N)` slot ordered alphabetically.
- Devon presses → to select gpt4all → right pane lists every `*.gguf` with size.
- Pre-existing dedup/unify flow works unchanged: pressing `u` on a gpt4all row that has a peer in HF cache or Ollama opens the unify dialog.
- Synthetic `[All Unified]` slot picks up gpt4all-rooted unifications without code change.

## Out of Scope (v1 of this plugin)

- LoRA / adapter files (only `.gguf` blobs in v1)
- Chat history / conversation files (irrelevant to model storage)
- GPT4All's downloads.json metadata cache (we don't need it; size from filesystem is sufficient)
- Network probing (no remote model catalog calls — discovery is local fs only)

## Risks / Open Questions

1. **Two default dirs, possibly both populated**: A user with both Python SDK and desktop app might have the same model in both locations on separate inodes. The dedup classifier already handles this — both will appear under `gpt4all` with `=` glyphs and unify will merge them. Confirmed: no special handling needed.

2. **Case-sensitive filenames**: GPT4All filenames mix case (`Meta-Llama-3-8B-Instruct.Q4_0.gguf`). modeltap's display_label normalization should handle this; existing helpers do.

3. **Absent directories**: Both defaults may not exist for non-GPT4All users. Discovery must return empty cleanly (not an error) — same behavior as existing plugins when their dir is absent.

## Existing Plugin Template

Use `plugins/atomic-chat/` as the template — it's the newest production plugin and already follows all current conventions (Cargo.toml structure, inventory::submit!, env-var override pattern, fixture conventions).
