# HF plugin — link() strategy spike (ADR-004 OQ-1)

**Step:** DELIVER 02-03
**Status:** RESOLVED (closes ADR-004 OQ-1)
**Consumer:** 03-02 (US-10 Unify) — implements `Tool::link` for HF using this plan

## Question

Can `link()` install an external file (e.g., a GGUF from another tool's
directory) into the HF cache by replacing the blob with a hardlink, and will
`huggingface-cli` / `transformers` still load it correctly?

## Conclusion

**YES, with one precondition: the canonical's content sha256 must match the
expected blob filename.**

HF's cache is content-addressed: each blob is stored at
`<hub>/blobs/<sha256-of-content>` and snapshot symlinks point at it. The
snapshot symlink target resolution depends only on the blob filename (which IS
the sha256 of the bytes). Therefore:

- If we hardlink the canonical (with content hash X) to
  `<hub>/blobs/X`, the snapshot symlink that originally pointed at
  `<hub>/blobs/X` will continue to resolve to a valid file with sha256 X
  — `huggingface-cli` and `transformers` see no difference.
- If the canonical's sha256 ≠ the snapshot's expected target, hardlinking
  the canonical to `<hub>/blobs/X` would produce a broken cache (the
  symlink still points to `<hub>/blobs/X`, but the file at that path has
  different content). DO NOT proceed in this case — fall back to copy or
  refuse.

## Numbered fs ops for `Tool::link(src: &Path, model: &ModelMeta)`

Preconditions:
- `model.tool == "hf"` and `model.dedup_key` is `DedupKey::Sha256(X)`
- `src` is a real file (not a symlink) on the same filesystem as the HF
  cache (cross-fs handled separately by US-19 in step 03-03)
- `<hub>/blobs/X` either does not exist OR is already a hardlink with the
  same inode as `src` (idempotent)

1. Compute `target = <hub>/blobs/<X>` where `<hub>` = `HF_HOME ?? ~/.cache/huggingface/hub`.
2. If `target` exists:
   a. If `same_inode(target, src)` → already linked, return `LinkOutcome::AlreadyLinked`.
   b. If `sha256(target) == X` → unrelated copy, hardlink-replace via
      `tempfile::NamedTempFile` + `fs::hard_link(src, tmp)` + `fs::rename(tmp, target)`.
      The atomic-rename ensures no readers see a half-formed file.
   c. Else → content mismatch. Refuse (return `LinkError::ContentMismatch`).
      This should be impossible given the precondition that X is sha256 of `src`'s
      content, but defensively check.
3. If `target` does not exist → call `fs::hard_link(src, target)` directly.
4. Snapshot symlinks under `<hub>/models--<org>--<repo>/snapshots/<rev>/<file>`
   that previously pointed at `target` continue to resolve correctly because
   the symlink target path is unchanged and the file at that path now has
   matching sha256.

Postconditions:
- `target` exists with `inode == src.inode()` (verified by `stat`).
- `sha256(target) == X` (held by content-addressing).
- All `models--*/snapshots/*/*` symlinks pointing at `target` continue to
  resolve to valid files; running `huggingface-cli repo cache scan` would
  list the cached entry as healthy.

## Open risks for 03-02

- **Cross-filesystem hardlink fails** with `EXDEV`. US-19 (03-03) defines the
  cross-fs fallback (refuse-default with skip/copy/cancel options).
- **Concurrent `huggingface-cli` writes** during link could create a race.
  Per intake Q5 (03-07), modeltap detects-and-prompts the user to close
  running tools and retry. No explicit lock taken.
- **Symlink rewriting** is NOT needed for blob-replacement (the symlink
  target path is unchanged). It would only be needed if we wanted to install
  a model with a NEW sha256 that doesn't match any existing blob — an
  out-of-scope variant.
- **Garbage collection of orphaned blobs** (snapshot symlinks pointing at
  blobs we replaced) is NOT the responsibility of `link()` — `huggingface-cli
  repo cache delete` handles cache hygiene. modeltap leaves the cache in a
  state HF tooling can reason about.

## Verification (deferred to 03-02 build week)

DELIVER 03-02 will run a quick spike before merging the unify implementation:
- Set up a test HF cache with one model.
- Replace the blob via the strategy above.
- Run `python -c 'from huggingface_hub import scan_cache_dir; scan_cache_dir().export_as_table()'`
  and confirm the entry shows as healthy.
- Run `transformers.AutoModel.from_pretrained(...)` against the modified cache
  and confirm load succeeds.

If verification fails, revert to copy-fallback for HF in v1 and revisit in v1.x.
