# ADR-004: Per-Tool Linking Strategy (Q2 closure)

## Status

**Partially Accepted** (2026-04-28). The trait shape is locked; per-plugin linking specs are based on documentation review and require a verification spike during DELIVER for HF and LM Studio specifically.

## Context

Intake Q2 (carried into DISCUSS as a deferred question): for each of {Ollama, llama-cli, HF, LM Studio}, what does it mean to "register an external file" so that the tool will load it as one of its own models?

Per Q1 intake override: there is **no central modeltap store**. The "canonical" file for a unified model is one of the existing tool-owned copies — typically the largest, or the one chosen by user preference. The link operation replaces the OTHER tool-owned copies with hardlinks pointing at the canonical.

The `Tool::link()` method (defined in modeltap-core) is given:

- `canonical_src: &Path` — the path that already exists (in some other tool's directory) and should become the canonical inode.
- `model: &ModelMeta` — describes the model as it currently exists in THIS plugin's tool.

The plugin's job: replace `model.on_disk_path` with a hardlink to `canonical_src`, and update the tool's manifest/registry/config so the tool can still find and load the model.

## Decision

**Link semantics, by plugin:**

### Ollama plugin

**Tool layout:**
```
~/.ollama/models/
  manifests/registry.ollama.ai/library/<name>/<tag>   ← JSON manifest
  blobs/sha256-<hash>                                  ← content-addressed blob files
```

A manifest references blobs by their `sha256-<hash>` filename. The hash is the SHA256 of the blob content.

**Link operation:**

1. Compute the canonical's SHA256 (use the cached value).
2. Compute Ollama's expected blob filename: `~/.ollama/models/blobs/sha256-<hex>`.
3. If `canonical_src` and that path are byte-identical AND on the same filesystem:
   a. Atomically replace `~/.ollama/models/blobs/sha256-<hex>` with a hardlink to `canonical_src`. Use `fs::hard_link` to a sibling temp path then `fs::rename` over the target (POSIX atomic).
   b. Manifest does not need updating — it still references `sha256-<hex>` and the inode now points to the canonical.
4. If cross-fs: surface `LinkError::CrossFilesystem` per US-19.

**Verified from documentation:** Ollama's blob layout is documented at https://github.com/ollama/ollama/blob/main/docs/import.md. Blob filename = SHA256 of content. **No spike needed** for Ollama unless the manifest schema changes.

**Spike risk:** LOW. The blob format is stable; Ollama 0.x has used it for >2 years.

### llama-cli plugin

**Tool layout:**
```
<configured search paths>/*.gguf              ← loose files; no manifest
```

llama-cli does not have a registry. The "model" is just the file path the user passes to `llama-cli -m <path>`.

**Link operation:**

1. If `model.on_disk_path` and `canonical_src` are on the same filesystem:
   a. Hardlink-and-rename: create temp hardlink `<dir>/.modeltap.tmp.<n>` pointing at `canonical_src`, then `fs::rename` it over `model.on_disk_path`.
2. No manifest update needed.

**Verified.** llama-cli reads the `.gguf` file at runtime; if the file has the same content (which it does, by hardlink), it loads correctly. **No spike needed.**

**Spike risk:** NONE.

### Hugging Face cache plugin

**Tool layout:**
```
~/.cache/huggingface/hub/
  models--<org>--<repo>/
    blobs/<sha256>                            ← content blobs (named by HF's own hash)
    snapshots/<rev>/<file>                    ← symlinks → ../../blobs/<sha256>
    refs/<branch>                             ← text files containing rev
```

The user-facing path (e.g., `models--meta-llama--Llama-3-8B/snapshots/abc123/model.gguf`) is a SYMLINK whose target is `../../blobs/<sha256>`. The blob's filename is HF's own content hash (a sha256 hex, but they prefix variations).

**Link operation:**

1. Identify the blob path that the snapshot symlink points to: `blob_path = realpath(model.on_disk_path)`.
2. If `blob_path` is the same as `canonical_src`: already linked. No-op.
3. If `blob_path` and `canonical_src` are on the same filesystem AND have the same SHA256:
   a. Hardlink-and-rename `blob_path` → hardlink to `canonical_src`. The snapshot symlink still resolves through the (now-hardlinked) blob file. HF's own hash filename is preserved (so HF tooling continues to find it by name).

**Spike risk:** MEDIUM. **Verification needed in DELIVER:** confirm HF tooling tolerates the blob being a hardlink rather than the original download. It should — HF reads the file content, doesn't check inode — but a behavioral spike with `huggingface-cli` against a hardlinked blob is wise before US-10 ships.

**Open question OQ-1 in architecture-design.md** documents this.

### LM Studio plugin

**Tool layout:**
```
~/.cache/lm-studio/models/<org>/<repo>/<file>          ← OR
~/.lmstudio/models/<org>/<repo>/<file>                  ← (older convention)
```

Loose files in a tree. No manifest in the simple case.

**Link operation:**

1. Same as llama-cli: hardlink-and-rename `model.on_disk_path` to point at `canonical_src`'s inode.
2. No manifest update needed (no manifest exists).

**Spike risk:** MEDIUM. **Verification needed in DELIVER:** LM Studio is a closed-source GUI; its file-path conventions are stable but undocumented. We need a quick spike to confirm:

- Both default paths used in the wild (already documented).
- LM Studio re-reads the file on next model selection (yes per behavior).
- Whether LM Studio caches an in-memory file handle that would be invalidated by a `rename` mid-session — Q5 mitigation: detect-and-retry covers this.

**Open question OQ-2 in architecture-design.md** documents this.

## Common operations across all plugins

All four `link()` implementations share a pattern. We provide a shared helper in `modeltap-core::logic::link_helpers`:

```rust
pub async fn atomic_replace_with_hardlink(
    canonical_src: &Path,
    target: &Path,
    fs_probe: &dyn FsProbe,
) -> Result<(), LinkError> {
    if !fs_probe.same_filesystem(canonical_src, target).await? {
        return Err(LinkError::CrossFilesystem {
            canonical: canonical_src.to_path_buf(),
            target: target.to_path_buf(),
        });
    }
    let temp = target.with_extension(format!("modeltap-tmp-{}", std::process::id()));
    tokio::fs::hard_link(canonical_src, &temp).await?;
    tokio::fs::rename(&temp, target).await?;
    Ok(())
}
```

Each plugin's `link()` calls this helper for the file replacement step, and then performs any plugin-specific manifest/registry updates (Ollama: none; llama-cli: none; HF: none; LM Studio: none in the simple case).

## Alternatives considered

### A — Per-tool subprocess invocation (e.g., shell out to `ollama cp`)

**Rejected.** Tool CLIs are not stable APIs; their flags change across versions; many of them don't expose a "register an external file" command at all. Direct on-disk manipulation is the established UMR pattern.

### B — Symlinks instead of hardlinks

**Rejected.** Hardlinks are inode-equivalent (the file IS the file); symlinks add a layer of indirection that some tools don't follow correctly. The user explicitly asked for "hardlink or pointer/config update, whichever the target tool supports" — hardlinks are the most universally supported.

### C — Copy-and-delete-original

**Rejected.** That's not deduplication; it doubles the disk briefly and changes the canonical location. Loses the whole value of the unify operation.

## Consequences

### Positive

- The `Tool::link()` contract is small (one method, one operation).
- Three of four plugins have NO manifest update step — minimal complexity.
- Cross-fs failure is uniformly surfaced via `LinkError::CrossFilesystem`.

### Negative

- HF and LM Studio plugins have spike-risk on first implementation (OQ-1, OQ-2). DELIVER should run those spikes EARLY in their respective build weeks, before committing to the link path.
- Atomic replacement (`hard_link` then `rename`) requires write access to the directory containing the target. In permission-constrained setups, this may fail. Mitigation: `LinkError::PermissionDenied` is a first-class variant; UI explains.

## Spike checklist for DELIVER

Before the US-10 (Unify) story is implemented:

- [ ] Verify with a real HF cache: hardlinking a blob and confirming `huggingface-cli` and the `transformers` library load it.
- [ ] Verify with a real LM Studio install: hardlinking a model and confirming LM Studio loads it.
- [ ] Confirm Ollama's blob hash equals SHA256 of content (sanity check).
