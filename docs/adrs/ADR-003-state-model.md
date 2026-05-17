# ADR-003: State Model — Stateless Rediscovery, No Persistent Index

**Superseded by:** [ADR-015 — State Model: SQLite-Backed Cache With Pre-Mutate Revalidation](ADR-015-state-model-sqlite-cache.md) on 2026-05-17.

This ADR is preserved as the historical record of the v0.2.x state model. The `tool-model-info-sqlite-cache` feature reverses the stateless-rediscovery rule per explicit user direction; see ADR-015 for the new state model, including the cache-paint-only / filesystem-authoritative-on-mutate safety rule that addresses the cache-invalidation concerns that originally motivated this ADR.

## Status

Accepted (2026-04-28). Closes intake Q7 (and Q1 by implication). **Superseded by ADR-015 (2026-05-17).**

## Context

DISCUSS framed Q7 as a tradeoff: persistent JSON/SQLite index (faster, more state to manage) vs stateless rediscovery (simpler, slower per launch). The intake-brief override is unambiguous:

> "No, keep the tool's model directory as the source of truth."

This means modeltap holds no authoritative state on disk other than the user's `~/.modeltap/config.toml` (preferences) and `~/.modeltap/diagnostics.log` (output, not state).

## Decision

**modeltap is stateless across launches with respect to the model inventory.** Every launch:

1. Reads `~/.modeltap/config.toml` (if present) for user preferences.
2. Spawns parallel `discover()` calls per plugin against tool directories.
3. Builds the `Inventory` in memory.
4. Computes indicators in memory.
5. On exit, the `Inventory` and the SHA256 cache are dropped.

There is no `~/.modeltap/store/`, no `~/.modeltap/index.json`, no SQLite DB.

## Alternatives considered

### A — Persistent JSON/SQLite index

Maintain `~/.modeltap/index.json` with `{path → ContentHash, last_seen, size}` entries. Refresh in the background; serve UI from cache.

**Pros:** instant first paint with cached data; SHA256 reuse across launches.

**Cons:**
- Cache invalidation is hard. Tool-owned directories change without modeltap knowing — users `ollama pull`, `hf download`, edit symlinks. Stale cache shows the user a wrong picture; modeltap's whole value collapses.
- Adds a state file that itself must be migrated across versions, repaired when corrupt, locked against concurrent processes, etc.
- Conflicts with the user's explicit Q1 + Q7 directive: "tool's model directory is source of truth."
- Privacy — even an inventory cache contains a list of every model the user has, which is a data-exfiltration risk for an opt-in-telemetry-by-design tool (C5).

**Rejected** on user directive AND on cache-invalidation grounds.

### B — Persistent SHA256 cache only (not full inventory)

Keep `~/.modeltap/sha256-cache.toml` mapping `(path, mtime, size) → ContentHash`. Inventory is rebuilt every launch but hashes are reused.

**Pros:** the only expensive computation (SHA256 on multi-GB files) survives across launches.

**Cons:**
- Adds a state file (smaller surface than full index but still). User's Q7 answer is general: "no, keep the tool's model directory as the source of truth."
- Cache validity check (`mtime + size`) is reliable BUT does not protect against malicious / accidental file replacement that preserves mtime+size. For a safety-first tool, that's a concern.
- The user's library may grow over time; cache file grows. New maintenance burden.

**Rejected for v1.** May revisit as a v1.x opt-in optimization (`~/.modeltap/cache/sha256.toml` with explicit `--use-hash-cache` flag).

### C (CHOSEN) — Stateless

Per intake. Trades startup cost for simplicity and correctness.

## K3 implications and mitigations

K3 says first paint < 1 s. Stateless makes this harder. Mitigations (also documented in `architecture-design.md` §7):

1. **Render skeleton before discovery completes.** Tool names are known at compile time (registered plugins). Layout paints with "discovering..." rows in <150 ms. K3 is satisfied trivially even if discovery is slow.
2. **Parallel discovery via tokio.** Per-plugin `discover()` runs concurrently. Total wall-clock = max(plugin times), not sum.
3. **Discovery is metadata-only.** Plugins read manifests / walk directories / stat files. They do NOT read file contents. Discovery is O(N models), not O(N bytes).
4. **SHA256 is lazy.** Per ADR-002. The expensive operation never runs on the first-paint critical path.

Budget analysis in architecture-design.md §7: ~1.15 s to fully populated indicators. < 150 ms for first paint.

## Consequences

### Positive

- No persistence bugs. No cache invalidation. No migration. No corruption recovery. No locking.
- Inventory is always fresh — no "stale data" failure mode.
- Privacy: nothing about the user's library leaves volatile memory.
- Trivially testable — every test starts from a known empty state.
- Aligns with C5 (privacy by default).

### Negative

- Every launch pays full discovery cost. Acceptable per the budget analysis.
- Every launch pays full hashing cost when the user opens a detail screen. Mitigated by in-process cache during the session.
- Users with very large libraries (1000+ models, 200+ GB) may notice discovery latency. The first-paint skeleton makes this tolerable; if users complain, ADR-003 may be revisited with an opt-in cache (Alternative B).

## Enforcement

Test in `modeltap-app/tests/no_state_files.rs` asserts that after a full lifecycle (launch + zap + unify + exit), the only files modeltap created in `~/.modeltap/` are:

- `config.toml` (only if user created it; never auto-written)
- `diagnostics.log` (output, append-only)

NO other files. If a future commit adds `~/.modeltap/index.json`, this test fails.
