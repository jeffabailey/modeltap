# ADR-013: Background SHA256 Hash Pool

## Status

Accepted (2026-04-30). Closes the "background hashing" decision left open in ADR-002 §"Mitigations applied" point 4 ("Optional `--prefetch-hashes` flag... deferred to v1.x") and required by NFR-1/NFR-2/NFR-3 of the cross-tool-model-unify feature.

## Context

modeltap v1 hashes lazily — SHA256 is computed only when the user opens a Detail screen or initiates a unify (per ADR-002). The cross-tool-model-unify feature needs to populate the dedup-able-bytes value in the summary bar at first paint and the per-row dedup glyph (`?/~/-/=/#`); both are derived from SHA256 hashes. Computing on demand defeats the goal — Devon never sees `Dedup-able > 0` without manually opening every model.

The feature requires hashing every discovered model after first paint, with progress visible in the status line and UI responsiveness preserved. ADR-002 already established that SHA256 takes 40-90 s on a typical 47 GB Ollama library if computed up front on a single thread; that violates K3 only if eager-blocking, but is acceptable in the background as long as:

- First paint completes within K3 (<1 s) before any hashing starts (NFR-1).
- Key handlers stay responsive (<100 ms) during hashing (NFR-3).
- Hashing of a typical install completes within 60 s p95 (NFR-2).
- Quit shuts the workers down cleanly within 500 ms (AC-U1.5).
- No persistent state survives the launch (NFR-4 / ADR-003).

## Decision

**Spawn a fixed pool of `min(num_cpus, 4)` `tokio::spawn_blocking` workers AFTER first paint completes. Workers consume `HashJob` items from a bounded `tokio::sync::mpsc` queue, compute SHA256 via the existing `Sha2Hasher`, populate the existing `Sha256Cache`, and post `Msg::HashComputed { model_id, hash }` to the existing TUI event channel on completion. A separate throttle task posts `Msg::HashProgressTick` at 250 ms cadence based on lock-free `AtomicU64` counters. Cooperative shutdown uses `tokio_util::sync::CancellationToken`; the composition root signals the token on `Msg::Quit` and awaits join with a 200 ms timeout.**

Implementation lives in `modeltap-app::hash_pool` (new module, app crate only).

## Alternatives considered

### A — Sequential single-worker

Spawn one worker that processes the queue serially.

- **Pros:** simplest possible; no concurrency to reason about.
- **Cons:** a 50 GB library takes ~80 s on warm SSD (ADR-002 cost table); violates NFR-2 (60 s p95). Maya-class users on slower SSDs would never finish hashing in a single session.
- **Rejected.**

### B — Unbounded concurrency (one task per file)

Spawn one `spawn_blocking` per discovered file at startup.

- **Pros:** simplest "fan-out everything" pattern.
- **Cons:** OS thread storm (each `spawn_blocking` consumes a blocking-pool thread). Disk I/O contention on a single SSD makes 5+ concurrent reads counterproductive. Memory peak from in-flight 64 KiB chunks scales with N.
- **Rejected** because it offers no throughput advantage over a 4-worker pool while wasting threads and risking IO thrash.

### C — `rayon` thread pool

Use `rayon::par_iter` over the job list with a custom thread pool sized to `min(num_cpus, 4)`.

- **Pros:** ergonomic parallel iterator; widely understood.
- **Cons:** introduces a second concurrency abstraction next to tokio (the workspace is tokio-pervasive). Two cancellation models (rayon's `Scope::spawn_broadcast` vs tokio's `CancellationToken`). The maintainability cost (ADR-001 priority) outweighs the small ergonomic gain.
- **Rejected.**

### D — Eager pre-paint hashing

Compute hashes BEFORE first paint, reverting ADR-002's lazy stance.

- **Pros:** simpler state model (no `Pending`/`Hashing` glyphs needed).
- **Cons:** violates NFR-1 / K3 catastrophically — first paint becomes 40-90 s on a typical library. Unacceptable.
- **Rejected.**

### E (CHOSEN) — Bounded post-paint pool with throttled progress

The decision above. Maps directly onto NFR constraints.

- **Pros:**
  - Reuses the existing `Sha256Cache` and `Sha2Hasher` (no new code paths in core).
  - Reuses the existing TUI event channel (no second integration point in `headless.rs`/`interactive.rs`).
  - 4 workers × ~200 MB/s effective per-worker IO (with contention) ≈ 800 MB/s aggregate, hitting the ADR-002 measured throughput on warm SSD; 50 GB / 800 MB/s ≈ 62.5 s — at the NFR-2 ceiling.
  - `spawn_blocking` is the correct tokio primitive for CPU-bound work; it never starves the async runtime.
  - `CancellationToken` provides clean multi-await shutdown semantics across all workers.
  - Throttle task isolates progress UI updates from completion events, preventing a redraw storm when 4 workers simultaneously complete (e.g., 4 small files at the same instant).
- **Cons:**
  - Per-platform thread-count assumption: `min(num_cpus, 4)` may be wrong for users with a single-core VM (use 1) or 32-core workstations on a slow USB drive (4 still IO-bound). Mitigation: undocumented `MODELTAP_HASH_WORKERS` env var as escape hatch.
  - Adds the `tokio-util` crate to `modeltap-app` if not already present (single-line Cargo.toml change; same Tokio license).

## Consequences

### Positive

- NFR-1 (first paint <1 s): hashing starts AFTER first paint by construction.
- NFR-2 (60 s p95 typical): 4-worker pool meets the budget on warm SSD.
- NFR-3 (UI <100 ms): workers run on the blocking pool; the async runtime is not starved; key handlers continue to dispatch.
- NFR-4 (no persistent state): cache is in-process only (already true via ADR-002 / ADR-003).
- AC-U1.5 (clean quit <500 ms): cancellation + 200 ms join timeout.

### Negative

- A new module (`hash_pool`) lives in `modeltap-app`. Maintenance surface grows by ~150 lines.
- The `Pending`/`Hashing` glyph states (`?` / `~`) are user-visible; require explanation in the help screen.
- File-changed-mid-hash can produce a stale cache entry keyed at start-time `(path, mtime, size)`; on the next launch the new mtime forces a re-hash. Tolerable; conservative-when-uncertain (BR-3) means the orchestrator's pre-link verification still catches any corner case before destructive work.
- Worker panics are isolated (the `JoinSet` surfaces them as `JoinError`), but the affected file's row stays `Pending` rather than auto-retrying. Acceptable; the user can quit-and-relaunch or open Detail (which still triggers lazy hash via the existing path).

### Neutral

- The choice does NOT affect ADR-002's primary identity decision (still SHA-256 of file content).
- The choice does NOT affect ADR-003's no-persistent-state rule (cache stays in process memory only).

## Enforcement

- Architecture-lint test in `tests/architecture.rs` already enforces `modeltap-core` does not depend on `tokio`. The hash pool is in `modeltap-app`; the rule continues to hold.
- AC-U1.5 acceptance scenario in DISTILL drives a quit-during-hashing test that asserts process exit within 500 ms.
- AC-U1.3 acceptance scenario asserts `j` keypress responds within 100 ms while hashing is in progress.

## Migration trigger

If user reports flag this on slow HDDs as still-too-slow, expose `MODELTAP_HASH_WORKERS` documented in `--help`. If users complain about the lack of cross-launch persistence, re-evaluate ADR-003. v1.x of this feature does not need either.
