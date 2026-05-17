# ADR-016: Tool Trait Extension — `inspect_tool()` and `inspect_model()` via Default-Method

## Status

Accepted (2026-05-17). Closes Q-INFO-1 from `docs/feature/tool-model-info-sqlite-cache/discuss/requirements.md`.

Pairs with ADR-015 (the cache that persists the values these methods return) and follows the pattern established by ADR-010 (which added `delete_folder` to the trait).

## Context

The `tool-model-info-sqlite-cache` feature delivers per-tool and per-model detail screens (US-21, US-22). Both require tool-native metadata that today's `Tool` trait does not expose:

- **Per-tool:** install path, detected version, configured search paths (default vs user-config), last error, plugin version. Some of this is already available via `discover()` (model count, disk usage); the rest requires a new entry point.
- **Per-model:** format version, quantisation, architecture, parameters, context length, plus a plugin-defined `BTreeMap<String, String>` of tool-relevant KVs (GGUF header excerpts, Ollama manifest fields, HF `config.json` excerpts).

Q-INFO-1 (per DISCUSS): should these be **required trait methods** or **default-body methods returning `Unsupported`**?

The trait is currently:
- 6 methods from ADR-001 (`discover`, `link`, `delete_all`, `delete_one`, `link_dry_run`, `name`)
- +1 method from ADR-010 (`delete_folder`, default-body returning `Err(DeleteError::Unsupported)`)

So this is the second extension; the precedent is set.

## Decision

**Add two default-body methods to `Tool`:**

```rust
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    // ... existing 6 + delete_folder methods unchanged ...

    /// Return tool-level details for the per-tool inspect screen (US-21).
    /// Default returns Err(InspectError::Unsupported); plugins may override.
    async fn inspect_tool(&self) -> Result<ToolDetail, InspectError> {
        Err(InspectError::Unsupported { tool: self.name() })
    }

    /// Return model-level details (including tool-native metadata KVs) for the
    /// per-model detail screen (US-22). Default returns Err(InspectError::Unsupported);
    /// plugins may override.
    async fn inspect_model(&self, id: &ModelId) -> Result<ModelDetail, InspectError> {
        let _ = id;
        Err(InspectError::Unsupported { tool: self.name() })
    }
}
```

**Plugin overrides (planned for Release 1):**

| Plugin | `inspect_tool` | `inspect_model` |
|---|---|---|
| ollama | Override (HTTP `/api/version` for detected_version; aggregates from `discover()` for the rest) | Override (parses `~/.ollama/models/manifests/<repo>/<tag>` JSON) |
| hf | Override (best-effort detection of HF CLI version; aggregates from `discover()` for paths) | Override (reads `config.json` if present in model dir) |
| llama-cli | Default `Unsupported` for `inspect_tool` (no canonical version source); override `inspect_model` (parses GGUF header via `gguf` crate or minimal hand-rolled parser) | Override (GGUF header parser) |
| lm-studio | Default `Unsupported` for `inspect_tool` (best-effort or none) | Override (GGUF header or `model.json` config) |
| atomic-chat | Default `Unsupported` for both | Default `Unsupported` |
| gpt4all | Default `Unsupported` for both | Default `Unsupported` |

Plugins that opt into `Unsupported` produce a graceful "(not detectable)" UX in the detail screens; the integration AC AC-21-3 ensures the field is rendered as "(not detectable)" rather than as an empty string or a false value.

### New error variant

```rust
// modeltap-core/src/domain/errors.rs

#[derive(Debug, Clone, thiserror::Error)]
pub enum InspectError {
    #[error("inspect not supported by tool {tool}")]
    Unsupported { tool: ToolId },

    #[error("plugin {tool} panicked during inspect: {message}")]
    PluginPanic { tool: ToolId, message: String },

    #[error("failed to read {path}: {source}")]
    FileReadable { path: PathBuf, source: io::Error },

    #[error("format unreadable at {path}: {detail}")]
    FormatUnreadable { path: PathBuf, detail: String },
}
```

`Unsupported` is the same shape as `DeleteError::Unsupported` from ADR-010. `PluginPanic` is caught at the orchestrator boundary (extension of parent US-18 panic-isolation invariant; new INT-INFO-8).

### New domain types

`modeltap-core::domain::inspect::{ToolDetail, ModelDetail, SearchPathEntry, SearchPathSource}` — pure data, no methods beyond Serde derives. Full shape in `docs/feature/tool-model-info-sqlite-cache/design/architecture-design.md` §5.2.

## Alternatives Considered

### A — Required trait methods (no default body)

```rust
async fn inspect_tool(&self) -> Result<ToolDetail, InspectError>;
async fn inspect_model(&self, id: &ModelId) -> Result<ModelDetail, InspectError>;
```

**Pros:**
- "Loudly" communicates that every plugin must consider inspection.
- Compile-time error if a contributor adds a plugin and forgets to implement.

**Cons:**
- **Breaks source compatibility for existing plugins.** Every plugin must add a stub `Err(Unsupported)` body — pure ceremony.
- **Penalizes contributor experience.** Riley persona (US-18 / K4) is the target audience for a 5th-plugin walkthrough. "Implement these 9 methods including 2 you don't actually need" is a worse onboarding than "implement these 6 methods and override anything else."
- **Inconsistent with ADR-010.** That ADR chose default-body for `delete_folder` on identical reasoning; reversing here would be incoherent.

**Rejected** on contributor experience and consistency with the precedent.

### B — Capability subtrait `Inspectable: Tool`

```rust
trait Inspectable: Tool {
    async fn inspect_tool(&self) -> Result<ToolDetail, InspectError>;
    async fn inspect_model(&self, id: &ModelId) -> Result<ModelDetail, InspectError>;
}
```

**Pros:**
- The `Tool` trait stays smaller.
- Capability-presence is encoded at the type level.

**Cons:**
- **Same object-safety friction as ADR-010's rejected Alternative B.** `&dyn Tool` cannot be downcast to `&dyn Inspectable` without `dyn Any` machinery the codebase doesn't have, or a duplicate registry indexed by capability.
- The orchestrator's call site becomes asymmetric: `delete_folder` goes through the trait method; `inspect_tool` would go through a downcast.
- Adding capability subtraits multiplies the dispatch surface; linear growth in capability count = quadratic growth in dispatch helpers.
- ADR-010 rejected this exact alternative for these exact reasons.

**Rejected** for the same reasons as ADR-010's Alternative B.

### C — Default-body trait methods (CHOSEN)

The decision above. Mirrors ADR-010.

**Pros:**
- **Source-compatible with all existing plugin implementations.** Atomic-chat, gpt4all, and any 3rd-party plugins compile with zero changes; they inherit `Unsupported`.
- **Object-safety preserved.** `Box<dyn Tool>` continues to work without downcast machinery.
- **Symmetric dispatch.** `modeltap-app::orchestration::execute_inspect_*` calls `tool.inspect_*()` the same way `execute_zap` calls `tool.delete_all()`.
- **Plugin contract test extends cleanly.** Each plugin's contract test asserts either "returns `Unsupported`" or "honors the inspect contract."
- **Consistent with ADR-010.** Contributors learn the pattern once.
- **Rust-idiomatic.** Default-body methods are the standard extension mechanism since trait stabilization.

**Cons:**
- **Trait grows from 7 methods to 9.** ADR-001's "FROZEN SURFACE" comment continues to be qualified ("frozen against breaking changes; extensions via default-body methods are permitted with an ADR").
- **Silent fall-through risk** if a future inspect-capable plugin misses the override. Mitigated by the plugin contract test that asserts each `T: Tool` either returns `Unsupported` OR honors the inspect contract — no third state.

## Consequences

### Positive

- US-18 / K4 contributor friction unchanged. Adding a 5th plugin is still "implement what you support; defaults handle the rest."
- All 6 existing plugins compile without source changes from this trait extension.
- Plugin authors who genuinely cannot introspect (e.g., a future plugin for a tool with an opaque binary format) opt out by doing nothing — the default body covers them.
- The contract test catches accidental opt-out for a plugin that should have overridden.

### Negative

- The trait surface grows. Documentation in `crates/modeltap-core/src/domain/tool.rs` MUST clearly note that `inspect_*` is the second optional capability (after `delete_folder`).
- Two more `Unsupported` variants in the error model; existing exhaustive matches must add arms (one-time surface change; compiler enforces).

### Neutral

- The choice does NOT affect ADR-001 dispatch model (still `Box<dyn Tool>`).
- The choice does NOT affect ADR-006 update loop (new `Msg::OpenToolDetail`, `Msg::OpenModelDetail` flow through the existing pipeline).
- The choice does NOT affect ADR-013 hash pool (independent concern; ADR-018 documents the seam).

## Enforcement

### Plugin contract test extension

`crates/modeltap-core/tests/inspect_contract.rs` (new) covers, for each `T: Tool`:

1. **`inspect_tool` on a non-inspect-capable plugin:** returns `Err(InspectError::Unsupported { tool })`. No filesystem mutation. No panic.
2. **`inspect_tool` on an inspect-capable plugin:** returns `Ok(ToolDetail { ... })` with the contract from `data-models.md` (every required field populated; `detected_version` may be `None`).
3. **`inspect_model` on a non-inspect-capable plugin:** returns `Err(InspectError::Unsupported { tool })`.
4. **`inspect_model` on an inspect-capable plugin against a real fixture:** returns `Ok(ModelDetail { ... })` with `metadata_kv` non-empty for the plugin's selected KV set.
5. **`inspect_model` on a corrupt/unreadable file:** returns `Err(InspectError::FormatUnreadable { path, detail })`. Detail screen renders "(introspection failed — see diagnostics.log)" per AC-22-7.
6. **Plugin panic during `inspect_*`:** caught at `tokio::task::spawn` boundary in `modeltap-app::orchestration::execute_inspect_*`; surfaced as `InspectError::PluginPanic`; detail screen renders "(inspection failed)" per AC-21-9 / INT-INFO-8.

### Architecture-lint unchanged

ADR-015's R7, R8, R9 cover the cache layer. This trait extension does not introduce new architecture-lint rules; the existing R1-R6 (parent) continue to enforce plugin isolation.

## Implementation guidance (for DELIVER)

This ADR specifies WHAT, not HOW. Software-crafter owns:

- The exact GGUF header parser implementation (use the `gguf` crate or hand-roll a minimal parser — both acceptable).
- The HTTP timeout for Ollama's `/api/version` lookup (recommend 500ms with graceful `None` fallback so a hung daemon doesn't block the detail screen).
- The selected-KV list per plugin (this is plugin-internal; the trait contract is just "a `BTreeMap<String, String>` of plugin-relevant KVs").
- The HF `config.json` parsing strategy (selective `serde_json::Value` extraction OR a strongly-typed struct subset).

The following are constraints, not implementations:

- The trait method signatures are fixed as in §"Decision".
- The default body MUST return `Err(InspectError::Unsupported { tool: self.name() })`.
- `inspect_tool` and `inspect_model` MUST be safe to call concurrently across plugins (the orchestrator may parallel-dispatch).
- A plugin panic in `inspect_*` MUST be caught at the orchestrator boundary; the detail screen MUST render gracefully with the existing data.

## Cross-references

- ADR-001 (Plugin Dispatch via `Box<dyn Tool>`) — establishes the object-safe trait pattern this ADR extends.
- ADR-010 (Folder-Group Delete — HF Capability via Default-Method) — establishes the default-body precedent.
- ADR-015 (State Model: SQLite-Backed Cache With Pre-Mutate Revalidation) — sister ADR; the cache persists what these methods return.
- ADR-017 (Schema Migration Strategy) — sister ADR; the `cache_models.metadata_kv_json` column stores `inspect_model`'s output across launches.
- US-21 and US-22 in `docs/feature/tool-model-info-sqlite-cache/discuss/user-stories.md` — drivers.
- `docs/feature/tool-model-info-sqlite-cache/design/architecture-design.md` §5.2 — full trait shape and type definitions.
