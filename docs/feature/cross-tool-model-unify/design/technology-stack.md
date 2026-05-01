# Technology Stack — cross-tool-model-unify

**Verdict: zero new dependencies.**

Every facility this feature requires is already vendored in the workspace and used by v1. The maintainability priority demands not adding deps without a forcing function; there is none here.

## Dependencies relied on (all already in `Cargo.toml`)

| Crate | Version | License | Already used by | Used here for |
|---|---|---|---|---|
| `tokio` | workspace | MIT | `modeltap-app` (runtime, async I/O) | `spawn_blocking` for hash workers; `mpsc` for queue; existing event channel for `Msg::Hash*` |
| `tokio-util` | workspace | MIT | `modeltap-app` (already a dep for `CancellationToken`) | `CancellationToken` for cooperative shutdown |
| `sha2` | workspace | MIT/Apache-2.0 | `modeltap-app::sha256_cache::Sha2Hasher` | unchanged — workers call existing `Sha2Hasher` |
| `ratatui` | workspace | MIT | `modeltap-tui` | `[All Unified]` slot rendering; new dedup-column rendering |
| `serde` | workspace | MIT/Apache-2.0 | `modeltap-core` | derives on new types (`DedupGlyph`, `SyntheticSlot`, `DedupSummary`, `UnifiedRow`) |
| `tracing` | workspace | MIT | observability | hash-pool warn/error events |

## Verification

`tokio-util::sync::CancellationToken` requires the `tokio-util` crate with the `rt` feature. Confirm presence; if absent, this is a single-line Cargo.toml change in `modeltap-app` only — not a new top-level dependency, and is the simplest available cancellation primitive in the existing ecosystem. Rejected alternatives:

- `Arc<AtomicBool>` flag — works but loses the `cancellation.cancelled().await` ergonomics that simplify worker shutdown.
- `tokio::sync::Notify` — single-shot fan-out is awkward across N workers compared to the multi-await semantics of `CancellationToken`.

If `tokio-util` is not yet a dep (verify in DELIVER), add it with `features = ["rt"]`. License: MIT. Maintenance: actively maintained alongside tokio. No new license risk.

## What was considered and rejected

| Crate | Why considered | Why rejected |
|---|---|---|
| `rayon` | Easy parallel iterator over hash jobs | Introduces a second concurrency abstraction next to tokio; cancellation models conflict; maintainability cost > the small ergonomic benefit |
| `crossbeam-channel` | Faster than tokio mpsc for blocking workers | Workers are `spawn_blocking`, but the consuming end is the existing tokio mpsc that the event loop already drains; mixing channels would complicate shutdown |
| `blake3` | ~3x faster than SHA-256 | Changes the dedup-key contract; ADR-002 specifies SHA-256 for hardlink-safety reasoning. Out of scope. |
| `dashmap` | Lock-free shared cache | The existing `Arc<Mutex<HashMap>>` in `Sha256Cache` is fine for ≤4 concurrent writers; replacing it is unjustified maintenance churn |
| `notify` | Filesystem watching for live re-discovery | Stateless rediscovery (Q7) is the explicit policy; out of scope |

## License compliance

All listed crates pass the existing `cargo deny check` policy in v1's CI. No new dependencies = no new license review.

## Summary

This feature is implemented entirely within the existing tech stack. The only dependency action item for DELIVER is to verify `tokio-util` (already used elsewhere in the workspace) is depended on by `modeltap-app` with the `rt` feature; if not, add it.
