# JTBD Job Stories — tool-model-info-sqlite-cache

This feature couples two changes — **per-tool/per-model inspection** (user-visible) and a **SQLite-backed persistence layer** (architectural enabler). JTBD discovery treats them as one motivator-and-enabler pair: the inspection feature is the surface, the cache is the mechanism that makes the surface fast enough to be useful, and the cache also unlocks several latent secondary jobs around freshness, audit, and cross-launch trust.

Persona is shared with the parent: **Devon Park** (multi-tool local-AI power user, macOS/Linux). No new persona introduced — this is a brownfield extension Devon would consume in the same session as his existing zap/unify workflow.

## Job inventory

Jobs are listed in suspected priority order (refined in `jtbd-opportunity-scores.md`). Each is captured in the canonical situation/motivation/outcome format; the three job dimensions and the four forces analysis appear inline so the next phase can score them without re-deriving.

---

## J1 — Verify a model is what I think it is

**When** I am about to run a model with a specific tool and want to confirm its actual on-disk metadata (size, format, quantisation, source repo, SHA256) before I either rely on it for work or unify/delete it,
**I want to** open a per-model detail view that shows all available metadata gathered from the tool's own files,
**so I can** stop second-guessing whether the model on disk matches the model in my head.

### Dimensions

- **Functional:** Look up authoritative metadata (size, format, quant level, dedup key, paths across tools, registration status) for one model without leaving modeltap.
- **Emotional:** Feel certain. The model in the right pane has a name, a size, an indicator — that's *enough to navigate* but not *enough to trust*. The detail view exists to convert navigation into certainty.
- **Social:** Be the colleague who can answer "is that the Q4_K_M or the Q5_K_M?" definitively in a chat thread, not the one who replies "checking..."

### Forces

- **Push:** Existing US-13 (model detail screen) shows paths + dedup key + unify-reclaim estimate, but stops there. To answer "what quantisation is this exactly?" Devon currently leaves the TUI for `gguf-dump`, `huggingface-cli scan-cache`, or `file`. Context-switch tax.
- **Pull:** A single detail view that surfaces what tool-specific introspection already returns (Ollama manifest fields, GGUF header KVs, HF `manifest.json`/`config.json` excerpts). Devon stays in flow.
- **Anxiety:** Detail view that takes 5+ seconds to render because every open triggers a fresh SHA256 + GGUF header parse breaks the flow worse than leaving the TUI. ALSO: showing wrong metadata (stale cache) is worse than not showing it.
- **Habit:** Devon already runs `gguf-dump` and `huggingface-cli scan-cache` from a second terminal pane. The detail view must be obviously faster *and* trustworthy or he'll keep the second pane open out of habit.

---

## J2 — Audit a tool's health and inventory cost at a glance

**When** I notice a tool's row in the left pane is taking up a lot of disk or something looks off (model count surprisingly high, "(error)" annotation, suspected unreachable directory),
**I want to** open a per-tool detail view that shows the tool's discovery root, last-scan time, model count, total bytes, plugin version, last-error if any, and what's been written to its directory since the last modeltap launch,
**so I can** diagnose *one specific tool* without re-running discovery globally or grepping `~/.modeltap/diagnostics.log`.

### Dimensions

- **Functional:** Show per-tool diagnostics: install path, version (if detectable), model count, total bytes, last successful scan time, last error, scan duration, configured search paths.
- **Emotional:** Feel oriented when something seems off. The left pane shows "Ollama 12" but no "wait — 12 since when?" Detail answers that.
- **Social:** Be the user who files good bug reports: "Ollama discovery returned 0 models; last successful scan was 3 days ago; here's the diagnostic excerpt" beats "modeltap is broken."

### Forces

- **Push:** "(error)" annotation per US-02 currently routes the user to a separate file (`~/.modeltap/diagnostics.log`). Detail view that surfaces the same data in-TUI removes a navigation step.
- **Pull:** A small per-tool dashboard (install path, version, model count, disk usage, last-scan-time, last-error) that Devon can pull up with one keystroke from the left pane.
- **Anxiety:** "Version" detection is fragile per tool (Ollama exposes a HTTP endpoint when running; llama-cli is a static binary; LM Studio's version is in an Electron about-box). False or missing version data feels less competent than no version data.
- **Habit:** Devon currently does `ls -la ~/.ollama/models/`, `ollama --version`, `du -sh ~/.cache/huggingface/`. A tool detail view replaces ~3 commands.

---

## J3 — Trust modeltap's first-paint inventory across launches

**When** I run `modeltap` for the second, third, hundredth time and I expect the inventory I just saw 5 minutes ago to still be there,
**I want to** see the inventory paint instantly from a cached state, with a clear indication that a freshness check is running in the background,
**so I can** start navigating immediately instead of waiting 1+ seconds every launch for parallel discovery to repopulate state I already had.

### Dimensions

- **Functional:** Render the inventory from a persisted snapshot at launch (sub-100ms paint), then reconcile with a background re-discovery pass and update the view if anything changed.
- **Emotional:** Feel that modeltap *remembers* between sessions. The current stateless-rediscovery model has a "fresh start every time" feel that is correct for hard-correctness but wrong for a tool the user opens dozens of times a day.
- **Social:** Be the user who recommends modeltap because "it pops open instantly" — not "it's good, just give it a second on launch."

### Forces

- **Push:** Stateless rediscovery (ADR-003) achieves K3's <1 s first-paint via the skeleton-first trick (paint a "discovering..." placeholder, fill in real data within ~1.15 s). The placeholder is honest but it means *every launch* shows an empty-ish UI for ~1 second. For a tool opened multiple times per day, the cumulative effect is "this tool always feels like it's loading."
- **Pull:** First paint from cached data is sub-100ms. Devon's hands are on home row before the inventory finishes loading instead of after.
- **Anxiety:** Stale cache shows the user a wrong picture and erodes K5 (safety). If the cache says "Mistral exists" but Devon `ollama rm`'d it 10 minutes ago in another terminal, a unify dialog built from cache will fail at the filesystem layer — embarrassing at best, dangerous at worst (acting on stale data).
- **Habit:** Devon expects modeltap to be stateless (ADR-003 is documented, he reads docs). Changing this is a contract change with him. Cache must be visibly opt-out and the "as of X seconds ago" provenance must be unmissable.

---

## J4 — Diagnose why a model I expected isn't showing up

**When** I `ollama pull` (or `hf download`) a model in a separate terminal and then alt-tab to modeltap and it isn't there yet,
**I want to** trigger a manual refresh for that one tool (or globally) without restarting modeltap,
**so I can** stop wondering whether modeltap missed it, whether the tool failed silently, or whether I should reach for `ps aux | grep modeltap` and Ctrl-C.

### Dimensions

- **Functional:** A `[r]` keypress that re-runs discovery against the currently-selected tool (or all tools) and updates the inventory. Show "refreshing..." spinner + "as of <timestamp>" pre-refresh, "as of just now" post-refresh.
- **Emotional:** Feel that modeltap is honest about staleness. "I see what you see, when you saw it last; press r if you've changed something."
- **Social:** Internal — Devon doesn't share this with anyone, but it kills a personal frustration of "did the download finish? did modeltap miss it? do I quit and reopen?"

### Forces

- **Push:** Current behaviour: download a model, alt-tab to modeltap, see stale inventory, quit + relaunch. ~3-second penalty per refresh. Cumulative across a download session.
- **Pull:** One-keystroke refresh that finishes in <1 s for the selected tool.
- **Anxiety:** Refresh during in-flight action (unify dialog open) would be confusing. Refresh must be a no-op (or queued) when not in the main view.
- **Habit:** Devon has been quitting + relaunching since the tool shipped. Discovering `[r]` requires the shortcut to be in the bottom bar (US-08 covers this).

---

## J5 — Compare dedup-confidence across launches

**When** I run unify on a multi-GB model, I want SHA256 confidence; currently SHA256 is computed lazily once per session and dropped on exit, so a re-launch costs me the whole hashing pass again,
**I want to** have SHA256 hashes persisted across launches (invalidated by mtime/size change),
**so I can** see authoritative dedup grouping immediately on every launch instead of waiting through a re-hash of files I already hashed last week.

### Dimensions

- **Functional:** Persist `(path, mtime, size) → SHA256` mappings in the cache; on subsequent launches, reuse hashes for unchanged files; rehash only files whose mtime+size differ.
- **Emotional:** Feel that effort is preserved. Hashing a 30-GB library is a real wall-clock cost; throwing it away on quit is wasteful.
- **Social:** Internal — Devon's value is "modeltap got faster the more I use it," which he'd notice but not necessarily articulate.

### Forces

- **Push:** Per ADR-003 alternative B (rejected for v1), SHA256 is the expensive operation. Re-hashing 30+ GB on every launch when nothing has changed is gratuitous. Currently this only bites users who open detail views or run unify; ADR-013 added a background hash pool which spreads the cost but doesn't eliminate it.
- **Pull:** Cache survives launches → second-launch unify dialog is instant.
- **Anxiety:** mtime-preserving file replacement (e.g., `cp --preserve=timestamps` of a swapped file) would let stale hashes through. For a safety-critical operation (unify decides what gets hardlinked to what), this is a real concern. ADR-003 rejected this alternative partly on these grounds.
- **Habit:** Users don't think about SHA256 caching; they just feel "first unify of the day is slow."

---

## J6 — Recover from a corrupted or unreadable cache

**When** my filesystem fills up mid-write, my SSD has a bad block, or I rsync'd my home directory and corrupted the cache file,
**I want to** have modeltap detect the corrupted cache on launch, log it, drop the cache, and proceed with stateless rediscovery as if the cache had never existed,
**so I can** never have a "modeltap won't start" failure mode caused by its own persistence layer.

### Dimensions

- **Functional:** Open SQLite with integrity checks; on `SQLITE_CORRUPT` or schema-version mismatch with no migration path, rename the broken cache to `cache.sqlite.corrupt-<timestamp>`, log to `diagnostics.log`, and proceed with empty cache.
- **Emotional:** Feel that the cache is a *performance optimization*, never a *dependency*. Devon should never feel "modeltap broke because of its own database."
- **Social:** Be the user who can describe a clean failure mode in a bug report ("cache corrupted, modeltap recovered automatically, here's the log").

### Forces

- **Push:** Any persistence layer introduces a new failure mode. Without explicit recovery, "cache corrupted" becomes "modeltap won't launch" — worse than the current stateless behaviour.
- **Pull:** Automatic recovery + log = cache becomes pure upside, never downside.
- **Anxiety:** Silent recovery hides real problems (disk failure). Recovery MUST log clearly and surface "I dropped a corrupted cache; see log" on the next launch.
- **Habit:** ADR-003 went out of its way to avoid this whole class of failure ("No persistence bugs. No cache invalidation. No migration. No corruption recovery."). Reintroducing the class requires handling it explicitly and visibly.

---

## J7 — Run multiple modeltap processes without corruption

**When** I have two terminal panes open, both running modeltap (because I forgot I left one open, or I'm comparing two states),
**I want to** know both processes can safely share or independently read the cache without one corrupting the other,
**so I can** keep my existing multi-pane workflow without fear of having to learn yet another "don't do that" rule.

### Dimensions

- **Functional:** SQLite WAL mode + busy-timeout permits concurrent readers and serialised writers. Per intake Q5, detect-and-prompt-then-retry for file-mutating actions. Cache reads/writes must follow the same discipline.
- **Emotional:** Feel that modeltap respects the user's existing terminal habits. No "you must close all other instances" warning.
- **Social:** Internal — but a "this app is single-instance only" message would feel un-Unix-y.

### Forces

- **Push:** A naive SQLite open (`PRAGMA journal_mode=DELETE`) would let process A's commit hose process B's reader mid-transaction.
- **Pull:** WAL mode + busy timeout is the standard answer; both processes work.
- **Anxiety:** A write from one process could invalidate read assumptions in the other ("I just saw 12 Ollama models; now it's 11"). For now: each process sees the cache state at the time of its own background-refresh; that's acceptable.
- **Habit:** Multi-pane terminal usage is universal for Devon's persona.

---

## Job summary

Six jobs surfaced (J1-J6 plus the operational J7). J1, J2, J3 are the strongest user-visible jobs. J4 (manual refresh) is a low-effort multiplier on J3. J5 (SHA256 persistence) is an architecture-bonus that touches K1/K5 via dedup correctness. J6 (corruption recovery) is a hidden-quality guardrail. J7 is operational hygiene.

Opportunity scoring in `jtbd-opportunity-scores.md`. Forces summary in `jtbd-four-forces.md`. Each job translates to one or more user stories in `user-stories.md` (US-21..US-27).
