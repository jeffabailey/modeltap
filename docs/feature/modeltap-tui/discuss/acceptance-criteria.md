# Acceptance Criteria — modeltap-tui

Consolidated AC index across all stories. Each AC is observable, testable, and traces back to a UAT scenario in `user-stories.md` or `journey-cleanup-and-unify.feature`.

## US-01: TUI launches and quits cleanly

| AC | Criterion | Source |
|---|---|---|
| US-01.AC-1 | Cold start to first paint completes in under 1 second on a 2020-or-later workstation | UAT US-01 launch |
| US-01.AC-2 | Pressing `q` exits with code 0 and restores terminal state | UAT US-01 quit-q |
| US-01.AC-3 | Pressing Ctrl+C exits with code 130 and restores terminal state | UAT US-01 quit-ctrl-c |
| US-01.AC-4 | Terminal narrower than 80 columns is detected and refused with a usage error (exit 2) | UAT US-01 narrow |
| US-01.AC-5 | No escape-sequence garbage left on screen after exit, in any of {Terminal.app, iTerm2, tmux, alacritty, xterm} | Cross-terminal manual UAT |

## US-02: Discover Ollama models

| AC | Criterion |
|---|---|
| US-02.AC-1 | All Ollama models in standard locations are discovered, with correct id, size, and on-disk path |
| US-02.AC-2 | Total disk usage reported equals sum of unique blob sizes (manifests sharing a blob count it once) |
| US-02.AC-3 | Missing Ollama directory is handled as "not installed", not as an error |
| US-02.AC-4 | Unreadable Ollama directory is handled as "error", not a crash |
| US-02.AC-5 | Discovery completes within 2 seconds for ≤200 models |

## US-03: Two-pane layout

| AC | Criterion |
|---|---|
| US-03.AC-1 | Left pane lists tools with model count and disk usage; selected tool is visibly highlighted |
| US-03.AC-2 | Right pane shows models for the currently-selected tool with id and size |
| US-03.AC-3 | Left/Right arrows switch tools; Up/Down arrows scroll within the right pane |
| US-03.AC-4 | Tab key cycles between left-pane focus and right-pane focus |
| US-03.AC-5 | Bottom bar shows current shortcuts at all times, occupying exactly one row |
| US-03.AC-6 | Unbound keys produce no destructive action and no error message |

## US-04: Show model size and registered tools on each row

| AC | Criterion |
|---|---|
| US-04.AC-1 | Every right-pane row displays one of {`o`, `*`, `!`, `?`} as its first character |
| US-04.AC-2 | `*`-marked rows display "also in: ..." with the other tool names |
| US-04.AC-3 | Indicator color: `o` neutral, `*` neutral, `!` red, `?` yellow (paired with the symbol — never color-only) |
| US-04.AC-4 | Indicator is computed from `model.compatible_tools` per the dedup-key strategy |

## US-05: Zap with typed confirmation

| AC | Criterion |
|---|---|
| US-05.AC-1 | Pressing `z` opens the confirmation dialog showing model count, total bytes, unique count, created date, shared count |
| US-05.AC-2 | User must type the exact tool name (case-sensitive) — anything else cancels |
| US-05.AC-3 | Esc cancels at any point with no destructive action |
| US-05.AC-4 | After successful zap, all unique model files are deleted; all shared registrations are removed but other tools' copies remain |
| US-05.AC-5 | Post-action message shows bytes reclaimed and bytes retained |

## US-06: Show last action and reclaimed bytes

| AC | Criterion |
|---|---|
| US-06.AC-1 | After every zap or unify, the right pane shows the last action and outcome |
| US-06.AC-2 | Successful actions show bytes reclaimed |
| US-06.AC-3 | Partial successes show how many targets succeeded and which failed and why |
| US-06.AC-4 | Summary bar's total disk usage refreshes within 500ms of action completion |

## US-07: Discover llama-cli models

| AC | Criterion |
|---|---|
| US-07.AC-1 | Default search paths `~/llms/`, `~/models/` are scanned recursively |
| US-07.AC-2 | Additional paths from `~/.modeltap/config.toml` are honored |
| US-07.AC-3 | Each `.gguf` file is parsed for header metadata (architecture, quantization) |
| US-07.AC-4 | Corrupt or unreadable files are listed with `[format: corrupt]` rather than skipped silently |
| US-07.AC-5 | Cross-platform: works on macOS and Linux with the same default paths |

## US-08: Bottom bar with shortcuts always visible

| AC | Criterion |
|---|---|
| US-08.AC-1 | Bottom bar occupies exactly one row |
| US-08.AC-2 | Shortcuts unavailable in the current context are visibly dimmed but not removed |
| US-08.AC-3 | Detail screens and dialogs replace the main bar with their own shortcut list |
| US-08.AC-4 | `?` opens a comprehensive help overlay; `?` or Esc closes it |
| US-08.AC-5 | Shortcuts shown in the bar match the actual key handler dispatch table (single source of truth) |

## US-09: Compatibility indicator engine

| AC | Criterion |
|---|---|
| US-09.AC-1 | Indicator computation runs per model during inventory build |
| US-09.AC-2 | Computation uses the plugin's declared accepted-formats list (capability metadata) |
| US-09.AC-3 | The set of indicators is exactly {`o`, `*`, `!`, `?`} — no others |
| US-09.AC-4 | Indicator is recomputed after any zap or unify |

## US-10: Unify across tools using hardlinks

| AC | Criterion |
|---|---|
| US-10.AC-1 | Unify dialog shows canonical path, hardlink target list, and disk reclaim before any action |
| US-10.AC-2 | After Enter, every target stats to the same inode as canonical |
| US-10.AC-3 | Each plugin's `link()` method handles its tool's specific registration |
| US-10.AC-4 | If any target fails (cross-fs, permissions), the partial-success path runs and per-target error is shown |
| US-10.AC-5 | Already-unified models are detected and the dialog is informational only |

## US-11: Updated totals after action

| AC | Criterion |
|---|---|
| US-11.AC-1 | After any zap/unify, summary bar refreshes within 500ms |
| US-11.AC-2 | Refresh failures degrade gracefully with `(refresh failed)` indicator and retry shortcut |
| US-11.AC-3 | New total = old total - bytes_reclaimed (within rounding) |

## US-12: Discover Hugging Face cache models

| AC | Criterion |
|---|---|
| US-12.AC-1 | `HF_HOME` env var (or default `~/.cache/huggingface/`) is read |
| US-12.AC-2 | All `models--<org>--<repo>` directories are enumerated; snapshot symlinks resolved to blobs |
| US-12.AC-3 | Model id = `<org>/<repo>` (path-style canonical form) |
| US-12.AC-4 | Format inferred from filename suffix |
| US-12.AC-5 | Broken symlinks are reported, not silent |

## US-13: Model detail screen

| AC | Criterion |
|---|---|
| US-13.AC-1 | Detail screen shows id, format, size, dedup key, and per-tool paths |
| US-13.AC-2 | Status is one of: UNIFIED, NOT UNIFIED, PARTIALLY UNIFIED, SINGLE TOOL |
| US-13.AC-3 | Reclaim estimate computed correctly per status |
| US-13.AC-4 | Esc returns to main view |

## US-14: Dry-run preview before unify

| AC | Criterion |
|---|---|
| US-14.AC-1 | `n` produces the same plan as Enter would, with no filesystem mutation |
| US-14.AC-2 | Dry-run output is clearly labeled `(dry-run)` to distinguish from real action |
| US-14.AC-3 | Cross-filesystem and permission issues are surfaced during dry-run |

## US-15: Discover LM Studio models

| AC | Criterion |
|---|---|
| US-15.AC-1 | Default paths `~/.cache/lm-studio/models/` and `~/.lmstudio/models/` are both checked |
| US-15.AC-2 | Configured override via `~/.modeltap/config.toml` is honored |
| US-15.AC-3 | Each file is parsed for format from filename suffix |
| US-15.AC-4 | "Not installed" is distinguished from "error" |

## US-16: Format-locked indicator (red `!`)

| AC | Criterion |
|---|---|
| US-16.AC-1 | `!` indicator rendered in red (paired with the symbol — never color-only) |
| US-16.AC-2 | [u] shortcut is dimmed/disabled on `!` models in detail screen |
| US-16.AC-3 | Empty/missing capability metadata produces `?` not `!` |
| US-16.AC-4 | WCAG contrast: red on default terminal background ≥ 4.5:1 for normal text |

## US-17: Detect running tools and warn

| AC | Criterion |
|---|---|
| US-17.AC-1 | Before unify or zap, detect_running_tools runs and results are shown in dialog |
| US-17.AC-2 | Warning is soft: Devon can proceed despite warning |
| US-17.AC-3 | If lsof / detection is unavailable, message says so explicitly |
| US-17.AC-4 | Detection completes within 500ms |

## US-18: Plugin trait — adding a tool requires no core changes

| AC | Criterion |
|---|---|
| US-18.AC-1 | Tool trait defined in modeltap-core with: name, discover, list_models, link, delete, accepted_formats |
| US-18.AC-2 | At least 4 plugins (Ollama, llama-cli, HF, LM Studio) exist as separate modules |
| US-18.AC-3 | Adding a 5th plugin requires zero changes to modeltap-core source files |
| US-18.AC-4 | Plugin panics are caught at the plugin boundary; one bad plugin does not crash the TUI |
| US-18.AC-5 | Trait is documented in `CONTRIBUTING.md` with a worked example |

## US-19: Hardlink fallback when cross-filesystem

| AC | Criterion |
|---|---|
| US-19.AC-1 | Filesystem check (using `stat` device IDs) runs per-target before linking |
| US-19.AC-2 | Cross-fs targets surface in dry-run and real-run with explicit options |
| US-19.AC-3 | Skip option leaves the target untouched |
| US-19.AC-4 | Copy option copies bytes (disk not reclaimed for that target) and reports in summary |
| US-19.AC-5 | No partial-state corruption on error |

## US-20: Cross-platform path discovery (macOS + Linux)

| AC | Criterion |
|---|---|
| US-20.AC-1 | Each plugin has per-OS path defaults (cfg! gated) |
| US-20.AC-2 | CI runs on both macOS and Linux runners; both must pass to merge |
| US-20.AC-3 | README states supported platforms explicitly |
| US-20.AC-4 | Windows: build may compile but binary refuses to run with a clear message |
| US-20.AC-5 | No path is hardcoded to "/" or Unix-only assumptions outside platform abstraction |

## Cross-Story / Integration ACs (from `shared-artifacts-registry.md`)

| AC | Criterion |
|---|---|
| INT-1 | `total.disk_usage` (summary bar) == sum of `tool.disk_usage` (left pane) at all times |
| INT-2 | A `*`-marked model resolves to the same dedup key in detail screen as in row indicator computation |
| INT-3 | After unify, every `hardlink_targets[i]` stats to same inode as `canonical_path` |
| INT-4 | After zap, `Plugin::list_models()` for the zapped tool returns `[]` |
| INT-5 | Post-action: `new total.disk_usage == old total.disk_usage - last_action.bytes_reclaimed` (within rounding) |
| INT-6 | `keyboard_shortcuts` displayed in bottom bar matches the actual key handler dispatch table |
| INT-7 | `tool.name` is identical across left pane, zap confirmation prompt, and post-action summary |

## Total: 20 stories, 100+ ACs, 7 cross-story integration ACs.
