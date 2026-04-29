# LM Studio plugin — path conventions + link() spike (ADR-004 OQ-2)

**Step:** DELIVER 02-04
**Status:** RESOLVED (closes ADR-004 OQ-2)
**Consumer:** 03-02 (US-10 Unify) — implements `Tool::link` for LM Studio

## Path conventions observed

| Convention | Path | LM Studio versions | Notes |
|---|---|---|---|
| New (recommended) | `~/.cache/lm-studio/models/<org>/<repo>/<file>.gguf` | 0.3.x and later (XDG-compliant) | Matches LM Studio's stated direction; current installs default here |
| Old (legacy) | `~/.lmstudio/models/<org>/<repo>/<file>.gguf` | 0.2.x and earlier | Some installs still here; LM Studio's docs note migration is opt-in |
| Custom | User-set via UI Preferences → Storage | any | Honored via `[plugins.lm-studio] search_paths` in `~/.modeltap/config.toml` |

modeltap checks **both** default paths in priority order (new first, old second), unions any user-configured paths, and lists models from all that exist. Cross-platform: macOS + Linux use the same conventions; WSL is Linux-equivalent. Native Windows is non-goal v1.

## Discovery semantics

- **NotInstalled**: neither default path exists, and no user-configured override is present. Surfaced as `(not installed)` in the left pane (US-02 AC-4 pattern). Benign state; not an error.
- **Error**: at least one configured path exists, but it cannot be read (permission denied, etc.). Surfaced as `(error)` in the left pane and listed in `launch.inventory.tool_errors`. This is the AC-4 distinction the LM Studio plugin enforces.
- **Healthy**: at least one path is readable; one entry per `.gguf` file under `<root>/<org>/<repo>/<filename>.gguf`. The model id is `<org>/<repo>/<filename>` (mirrors LM Studio's UI label).

## v1 format scope (intake C3 / ADR-004 OQ-3)

`accepted_formats()` returns `&[Format::Gguf]` only. **MLX is out of scope for v1** even though LM Studio supports it — the `Format` enum reserves `Mlx` for the v1.x expansion, but the plugin does not surface MLX files today. This is a deliberate scope boundary, not an oversight.

## Link strategy for `Tool::link` (deferred to 03-02)

LM Studio stores model files at predictable paths derived from `<org>/<repo>/<file>.gguf` — no symlink farm, no content-addressing. Therefore `link()` is **direct file replacement via hardlink**:

1. Compute target path: `<lm-studio-root>/<org>/<repo>/<filename>` (extract org/repo/filename from the `ModelMeta`).
2. If `target` exists and `same_inode(target, src)` → already linked; return `LinkOutcome::AlreadyLinked`.
3. If `target` exists and not same-inode → atomic-replace via temp hardlink + `fs::rename` (POSIX atomic, mirrors the helper in `modeltap-core::logic::link_helpers`).
4. If `target` does not exist → create parent dirs, then `fs::hard_link(src, target)`.

Postcondition: `target` exists with `inode == src.inode()`; LM Studio re-reads the file on the next model selection (closed-source — assumed; verify in the spike below).

## Verification spike (deferred to 03-02 build week)

DELIVER 03-02 will run a quick spike before committing to the link path:

1. Set up an LM Studio install with one model in `~/.cache/lm-studio/models/`.
2. Replace the `.gguf` via hardlink to a different inode (same content).
3. Restart LM Studio; select the model; confirm load succeeds.
4. Switch to another model and back; confirm re-read works.

If verification fails → revert to copy-fallback for LM Studio in v1; revisit in v1.x.

## Open risks for 03-02

- **LM Studio process keeping the file mmap'd** during link → Q5 / US-17 detect-and-prompt-then-retry covers this; ask the user to close LM Studio before linking.
- **Closed-source behavior**: LM Studio doesn't publish its file-handle semantics. Behavior may be version-dependent. Assume re-read on selection (consistent with how the UI lets users add models), but verify in the spike.
- **Custom storage location** set in LM Studio Preferences → Storage. modeltap honors this via `[plugins.lm-studio] search_paths`; user must configure modeltap manually if their LM Studio uses a non-default path.
- **MLX format** is out of scope v1 (intake C3 / ADR-004 OQ-3). When MLX support lands (v1.x), the `Format` enum gains `Mlx` and `accepted_formats()` adds it; link strategy is the same (file replacement).

## Cross-references

- ADR-001 — plugin registration via `inventory::submit!` (LM Studio is one of the four v1 plugins).
- ADR-004 — per-tool linking strategy (this document closes OQ-2).
- ADR-007 — error-handling layering (`thiserror` in plugin; `anyhow` only at the modeltap-app edge).
- intake C3 — MLX out-of-scope decision.
- US-15 acceptance criteria — `master-acceptance.feature` lines 612–634.
