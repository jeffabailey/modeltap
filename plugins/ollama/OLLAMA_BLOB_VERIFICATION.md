# Ollama plugin — blob hash verification (ADR-004 OQ-3)

**Step:** DELIVER 03-02
**Status:** RESOLVED (closes ADR-004 OQ-3)
**Consumer:** plugins/ollama/src/link.rs `verify_sha256_match(...)`

## Question

Does Ollama's blob filename (`sha256-<hash>`) equal the SHA256 of the file's
content bytes?

## Conclusion

**YES.** Ollama uses content-addressing for its blobs: each blob is stored at
`~/.ollama/models/blobs/sha256-<hash>` where `<hash>` is the lowercase hex
SHA256 of the file's raw bytes. This is verifiable directly:

- Read `~/.ollama/models/blobs/sha256-abc123...`
- Compute SHA256 of the file's content
- The computed hash equals `abc123...`

Manifests (under `<root>/manifests/<registry>/<repo>/<tag>`) reference layers
by `digest: sha256:<hash>`, where `<hash>` is the same value as the blob
filename suffix. No second indirection layer exists between manifest and
blob — the manifest's `digest` field IS the blob's content hash.

## Implication for `Tool::link`

When linking an external file (the canonical) into Ollama's blob store at
`<hub>/blobs/sha256-<X>`, modeltap MUST first verify that
`sha256(canonical) == X`. If equal, the hardlink replacement preserves the
manifest's resolution (manifests reference blobs by sha256 filename;
same hash = same effective content). If mismatch, refuse with
`LinkError::ContentMismatch` — this should be impossible given the
dedup-key precondition, but defensive checks are mandatory per ADR-008's
"no partial-state corruption" rule.

## Implementation

`plugins/ollama/src/link.rs::verify_sha256_match` parses the `<X>` from
the target filename (`sha256-<X>`), streams `canonical_src` through SHA-256
(via the `sha2` crate, mirroring the `Sha2Hasher` in
`crates/modeltap-app/src/sha256_cache.rs`), and compares the computed
hex hash to the parsed `<X>`. If they don't match,
`Err(LinkError::ContentMismatch { target, expected, actual })` is returned
before any filesystem mutation.

The check runs BEFORE the temp-hardlink + rename, so a mismatch leaves the
existing blob untouched. Postcondition on success: the blob's inode is now
the canonical's inode, the blob's content equals what every manifest
already references, and `huggingface-cli`-style content-address invariants
hold.

## Idempotency

If the target already shares the canonical's inode (verified via `metadata`
+ `os::unix::fs::MetadataExt::ino`), `link()` short-circuits with
`LinkResult::AlreadyLinked` — the sha256 check is skipped because we
already know the file is the canonical (same inode = same bytes).

## Open risks

None for the verification step itself. Cross-filesystem (`EXDEV`) is a
separate concern handled by `LinkError::CrossFilesystem`; the [s/c/x]
fallback dialog lands in step 03-03.

## Verification (mechanical, deterministic)

The check is a simple byte-by-byte SHA-256 streaming hash. No edge cases:

| canonical bytes | target name | result |
|---|---|---|
| sha256(bytes) == X | `sha256-X` | OK, atomic-replace |
| sha256(bytes) != X | `sha256-X` | `Err(ContentMismatch { expected: X, actual: ... })` |
| target name not parseable | any | `Err(MalformedMeta { ... })` |

## Cross-references

- ADR-002 — content-addressed dedup (sha256 is primary identity).
- ADR-004 OQ-3 — this spike closes the question.
- ADR-008 — no partial-state corruption rule.
- `plugins/hf/LINKING.md` — parallel spike for HF (also content-addressed).
- `crates/modeltap-app/src/sha256_cache.rs` — `Sha2Hasher` reference impl.
