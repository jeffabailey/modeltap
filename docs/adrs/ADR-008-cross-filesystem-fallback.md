# ADR-008: Cross-Filesystem Fallback — Refuse-Default with Per-Target User Choice

## Status

Accepted (2026-04-28). Resolves US-19's "DESIGN to choose" line.

## Context

Hardlinks cannot cross filesystem boundaries. `std::fs::hard_link` returns `EXDEV` when the source and target are on different filesystems / mount points.

Realistic scenarios:

- User has `~/.ollama/` on `/` (system disk) and `/data/models/` on a separate `/data` mount (large secondary disk). One unify operation may target both.
- User has `~/.cache/huggingface/` on `/` and an external NAS mounted at `/mnt/nas/lm-studio-models/`. Even more cross-fs.
- User has everything on one disk. The common case. No fallback needed.

US-19 enumerates three options:

- **Skip** the cross-fs target (leave its copy alone).
- **Copy** the canonical to the cross-fs target (uses disk; achieves logical-deduplication via shared SHA256 but no inode sharing).
- **Cancel** the unify entirely.

What's the **default behavior** on unify when a cross-fs target is detected?

## Decision

**On detection of any cross-fs target, modeltap REFUSES to silently fall back. The unify dialog presents the three options explicitly per US-19 and requires the user to choose. There is no implicit default (no auto-skip, no auto-copy).**

The dry-run also surfaces cross-fs targets with the same options — so the user discovers cross-fs problems before any mutation.

If the user has only one cross-fs target and chooses "skip cross-fs," the operation proceeds for the same-fs targets. The "skip" decision is recorded in the `LastAction.detail` for transparency.

## Alternatives considered

### A — Default auto-copy (silent fallback)

**Pros:** maximally hands-off; users get "unified" semantics across mounts.

**Cons:**
- Defeats the whole "reclaim disk" purpose for that target. Worse, the user thinks they reclaimed disk and didn't.
- Quietly doubles the canonical's bytes. Surprising.
- Violates safety priority (rank 1).

**Rejected.**

### B — Default auto-skip

**Pros:** no surprise mutation.

**Cons:** the user pressed `u` expecting unification; silent skip means the result was different from what they intended. Surprise rather than safety.

**Rejected.** Better to require explicit choice.

### C — Default cancel-everything

**Pros:** zero risk.

**Cons:** disproportionate. If 5 of 6 targets are same-fs and 1 is cross-fs, the user almost certainly wants the 5 unified.

**Rejected.**

### D (CHOSEN) — Refuse-default, present three options

The user makes an informed choice per-target (or "all cross-fs same way"). No surprise.

## UI mapping

When the unify dialog detects ≥1 cross-fs target, the dialog shows:

```
+- Unify: mistralai/Mistral-7B-v0.3 ----------------------------------------+
|                                                                           |
| Canonical:    /Users/devon/llms/mistral-7b-q4.gguf  (largest existing)    |
|                                                                           |
| Same-filesystem targets (will hardlink, reclaiming 4.4 GB):               |
|   Ollama       /Users/devon/.ollama/models/blobs/sha256-8f3e...           |
|   Hugging Face /Users/devon/.cache/huggingface/hub/.../mistral-7b-q4.gguf |
|                                                                           |
| Cross-filesystem targets (CANNOT hardlink):                               |
|   LM Studio    /data/lm-studio/models/.../mistral-7b-q4.gguf  on /data    |
|                                                                           |
|   How to handle cross-fs target(s)?                                       |
|   [s] skip — leave that copy alone (no disk reclaim for it)               |
|   [c] copy — bytes copied, no reclaim for it but unified registration     |
|   [x] cancel — abort unify entirely                                       |
+---------------------------------------------------------------------------+
```

If no cross-fs targets exist, the dialog skips this section and shows the original simpler form from the journey doc.

## Consequences

### Positive

- User always knows what will happen before pressing Enter.
- US-19 AC satisfied: "no partial-state corruption on error" — choice is made up-front.
- Same code path for dry-run and real-run: the detection happens once when building the `UnifyPlan`, and per-target `UnifyAction` carries the choice.

### Negative

- Adds a UI step. For users on a one-disk laptop, this step never appears.

## Implementation notes

- `FsProbe::same_filesystem(a, b)` uses `std::fs::metadata().dev()` (via the `nix` crate on Unix) to compare device IDs.
- The `crosses_filesystem: bool` field on `UnifyTarget` is set during plan construction.
- The TUI's unify dialog branches on `plan.targets.iter().any(|t| t.crosses_filesystem)`.

## Test scenarios

`modeltap-core/tests/unify_plan_tests.rs` includes:

- All-same-fs plan ⇒ no cross-fs section in the rendered dialog.
- Some-cross-fs plan ⇒ cross-fs section listed with the three options.
- All-cross-fs plan ⇒ "all targets on different filesystems — unify cannot proceed" per US-19 example 3.
