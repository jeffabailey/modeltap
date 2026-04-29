# ADR-009: Single-Model Delete — First-Class `Tool::delete_one` Method

## Status

Accepted (2026-04-28). Closes the F4 delta from intake-brief that DISCUSS US-05 does not currently capture.

## Context

DISCUSS US-05 ("Zap a tool's models with typed confirmation") describes deleting EVERY model for a specific tool. The intake-brief (post-DISCUSS update) added:

> F4 — Hotkey: `z` (zap)
> - Delete every model for a specific tool. Destructive; must confirm.
> - **Delete a single model for a specific tool. Destructive; must confirm.**

The bolded line is new. DISCUSS user-stories.md does not have a story for it. We need:

1. The `Tool` trait to expose a single-model-delete operation.
2. A clear UI hook (the journey doc already mentions `[d] delete-from-one` on the detail screen — see `journey-cleanup-and-unify-visual.md` step 3).
3. A back-edit recommendation for DISCUSS to add either US-05b or expand US-05.

## Decision

**The `Tool` trait exposes both `delete_one(model)` and `delete_all()` as separate methods. They are not unified into one variadic method. The TUI calls `delete_one` from the detail screen (`[d]`) and `delete_all` from the left-pane zap (`z`).**

Both share:

- Typed-name confirmation (per US-05 UX pattern).
- Pre-action running-tool detection (per US-17).
- Bytes-reclaimed accounting that distinguishes "unique to this tool" (file deleted) from "shared" (only registration removed).

```rust
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    // ... other methods ...

    async fn delete_one(
        &self,
        model: &ModelMeta,
        ctx: &DeleteCtx,
    ) -> Result<DeleteOutcome, DeleteError>;

    async fn delete_all(
        &self,
        ctx: &DeleteCtx,
    ) -> Result<Vec<DeleteOutcome>, DeleteError>;
}
```

## Alternatives considered

### A — One method `delete(targets: DeleteTargets)` with a sum type

```rust
pub enum DeleteTargets {
    One(ModelMeta),
    All,
}
async fn delete(&self, targets: DeleteTargets, ...) -> ...
```

**Pros:** smaller trait surface.

**Cons:**
- The plugin implementation has to re-dispatch internally: `match targets { One(m) => self.delete_one_internal(m), All => self.delete_all_internal() }`. No simplification — same code, more indirection.
- Pattern-matching obscures the contract. Two separate methods read clearer.
- Future variants (e.g., `Some(Vec<ModelMeta>)`) bloat the enum.

**Rejected** for clarity.

### B — `delete_all` calls `delete_one` in a loop in the orchestrator

```rust
// in modeltap-app:
for model in tool.list_models().await? {
    tool.delete_one(&model, &ctx).await?;
}
```

**Pros:** trait has only `delete_one`.

**Cons:**
- Some tools have batch-optimized delete (e.g., Ollama can `rm -rf manifests/` then garbage-collect blobs; doing 12 individual manifest unlinks is slower).
- The orchestrator's loop has to handle partial failure on its own. Better to give the plugin atomic-ish responsibility for the whole-tool case.

**Rejected** for plugin autonomy.

### C (CHOSEN) — Two methods on the trait

Both methods exist. Plugin authors can implement `delete_all` as a loop over `delete_one` if they want; the default impl on the trait could even do that:

```rust
async fn delete_all(&self, ctx: &DeleteCtx) -> Result<Vec<DeleteOutcome>, DeleteError> {
    let mut outcomes = Vec::new();
    for model in self.list_models(ctx).await? {
        outcomes.push(self.delete_one(&model, ctx).await?);
    }
    Ok(outcomes)
}
```

But for tools where a batch operation is faster (Ollama: `rm -rf` the manifests dir), the plugin can override.

## Consequences

### Positive

- Both UI flows have a clear, type-safe API.
- Plugins can choose batch optimization or default loop.
- F4's new requirement is satisfied with a small additive trait method.

### Negative

- US-05 in DISCUSS needs to be expanded OR a US-05b added. **Back-edit BE-7** in architecture-design.md §11 captures this.

## UI mapping

| Trigger | Confirmation | Method |
|---|---|---|
| Tool selected in left pane, user presses `z` | Type tool name | `delete_all` |
| Model row selected in right pane, user presses `d` (or detail screen) | Type model id (or short prefix) | `delete_one` |

For `delete_one`, the typed confirmation may be the model's short id (e.g., `mistral:7b-instruct-q4_K_M`) — typing the full id is tedious. Alternative: a `[y/n]` confirmation modal, since the blast radius is one model. **For v1: typed confirmation when the model is unique-to-this-tool (file will be deleted); `[y/n]` confirmation when shared (only registration removed, file preserved).** This matches the safety priority: stronger guard for irreversible actions.

This logic lives in `modeltap-tui::view::confirm_dialog` and is driven by `ZapOnePlan.also_in_other_tools`.

## Test scenarios

- `delete_one` on a unique-to-this-tool model: file deleted, `bytes_freed > 0`, registration removed.
- `delete_one` on a shared model: file preserved (other tools still hardlink to it), `bytes_freed == 0`, registration removed.
- `delete_all` on a tool with mixed unique+shared: file deletions for unique, registration-only for shared, total `bytes_reclaimed` matches sum of unique sizes.
- Plugin contract test parameterizes both methods.

## Recommended back-edit to DISCUSS user-stories

Add **US-05b: Zap a single model from one tool** (or expand US-05 with a "Solution variant: single-model" subsection). UAT scenarios:

1. Devon presses `d` on a unique-to-this-tool model; types model id; file is deleted; reclaims its bytes.
2. Devon presses `d` on a shared model; presses `y` confirmation; only registration removed; file preserved (other tools still see it).
3. Devon presses Esc on either flow; no destructive action.

This back-edit is BE-7 in `architecture-design.md` §11.
