# ADR-002: Dedup Key — SHA256 Primary, HF id+quant Display Fallback

## Status

Accepted (2026-04-28). Closes intake Q6 and DISCUSS deferred Q6.

## Context

modeltap's unify safety hinges on correctly answering "are these two on-disk files the same model?" Wrong answer ⇒ user thinks they're deduplicating but actually destroys distinct models. Right answer ⇒ user reclaims disk safely.

Candidates from intake:

- (a) **SHA256 of file content.** Most durable; content-addressable; every duplicate file has the same key by definition.
- (b) **HF repo+quant identifier.** Fast (no I/O); but tool-specific (only HF/Ollama have this metadata); two files with the same label may differ.
- (c) **Hybrid.** Both, with rules.

User's intake answer (Q6): "Pick the most durable one. SHA256 if fine, otherwise HF repo+quant."

## Decision

**Primary identity: `ContentHash([u8; 32])` — SHA-256 of the file content. Computed lazily (not at discovery time). Cached in process memory only (never persisted, per ADR-003).**

**Display fallback: `DisplayLabel(String)` — derived from filename, manifest, or HF metadata. Used as a tentative grouping key only when the SHA256 is not yet computed AND for human-readable UI labels.**

Encoded as the `DedupKey` enum:

```rust
pub enum DedupKey {
    Content(ContentHash),         // authoritative
    Tentative(DisplayLabel),      // pre-hash; may refine
}
```

## Alternatives considered

### A — SHA256 only

**Rejected only on UX grounds:** before SHA256 is computed, the UI has nothing to group by. We need a tentative key for the first paint. SHA256 IS the chosen primary identity; we just augment it with a tentative display key for the pre-hash window.

### B — HF id+quant only

**Pros:** zero I/O cost.

**Cons:**
- Not all tools have this metadata. llama-cli's `~/llms/foo.gguf` has only the filename — no manifest. HF id+quant is unparseable from a bare filename.
- Two files with the same label can have different content. A user who downloads `mistral-7b-q4_K_M.gguf` from HF and a different `mistral-7b-q4_K_M.gguf` from a fork will see them as "the same" under label-only dedup. Unify would corrupt one.
- Fails the safety priority (rank 1).

**Rejected** as primary. Kept as the *display* secondary.

### C — Hybrid with priority rules

The chosen approach is in fact a hybrid, but with a clear priority: SHA256 wins always when available. Tentative is only a placeholder. So this is what we picked, framed clearly.

### D — Filesystem inode

After unify, members share an inode. Tempting to use inode as the dedup key. Rejected because (a) inodes are filesystem-local — useless across mounts; (b) inodes don't exist before unify, so cannot help the discovery-time question.

## Cost analysis — SHA256 on real-world libraries

SHA256 throughput (RustCrypto `sha2`, single-thread):

| Hardware | Throughput | 4.4 GB GGUF | 50 GB GGUF |
|---|---|---|---|
| M2 Mac (NVMe) | ~1.2 GB/s | ~3.7 s | ~42 s |
| Linux NVMe SSD | ~800 MB/s | ~5.5 s | ~62 s |
| Linux SATA SSD | ~500 MB/s | ~9 s | ~100 s |
| Spinning rust | ~150 MB/s | ~30 s | ~5+ min |

**SHA256 on a typical 47 GB Ollama library = 40-90 s total if computed up-front.** That violates K3 entirely if eager.

**Mitigations applied:**

1. **Lazy.** SHA256 is computed only when the user opens a model's detail screen, presses `u`, or otherwise needs cross-tool identity confirmed. First paint and basic browsing do not require any hashing.
2. **Tentative grouping by display label.** US-04 row indicators (`*` / `o` / `!`) can be computed from label-based grouping for the first paint, with a small "(tentative)" marker. As the user clicks into models, hashes get computed and the indicator firms up.
3. **Process-local cache.** Once computed, store `(path, mtime, size) → ContentHash` in an in-memory `HashMap`. Invalidate on mtime/size change. Survives navigation within one session; lost on app exit (per ADR-003).
4. **Optional `--prefetch-hashes` flag.** Off by default; users with patience can opt in to background hashing on launch. Deferred to v1.x.

**For v1's UX, lazy is sufficient.** The detail-screen open is a deliberate user action; a 5-second wait with a progress indicator is acceptable. For a 50 GB single file the wait is uncomfortable; acceptable for v1, revisit if users complain (OQ-5 in architecture-design.md).

## Consequences

### Positive

- Safety: SHA256 is content-addressable. Two files with the same SHA256 are byte-identical. Unify can hardlink them with full confidence.
- Simplicity: one canonical identity; display label is purely a UX affordance.
- No persistent state: the cache is in-memory.

### Negative

- First-time hashing is slow on huge files. Documented; mitigated with progress UI.
- Pre-hash UI shows "(tentative)" markers that may refine when hashes complete. Slight user-experience seam to explain.

### Conservative-deletion rule

Per US-05 technical note: "if dedup-key is uncertain, treat as unique (preserves data)." This applies when:

- Two members have the same `Tentative(label)` key but their hashes have not yet been computed AND zap/delete is requested.

In that case, modeltap computes both SHA256s before proceeding (forced upgrade from Tentative to Content). If they differ, they were never duplicates and both files are preserved as unique.

## Enforcement

Property test in `modeltap-core/tests/dedup_properties.rs`:

- For all `(path, content)` pairs where `sha256(content_a) == sha256(content_b)` ⇒ `group_by_dedup_key` puts them in the same `DedupGroup`.
- For all pairs where the labels match but the contents differ ⇒ once both hashes are computed, they end up in DIFFERENT groups.

Plugin contract test asserts no plugin attempts to compute the SHA256 itself — that's exclusively the Hasher port's job (so the cache works).
