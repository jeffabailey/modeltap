//! Inspect domain types — pure data for the per-tool / per-model detail
//! screens (US-21, US-22) and the cache layer (US-23) that persists them.
//!
//! Per ADR-016 (Tool trait `inspect_tool`/`inspect_model` via default-method)
//! and `docs/feature/tool-model-info-sqlite-cache/design/data-models.md`
//! §"In-memory mirror types" + `architecture-design.md` §5.2.
//!
//! These are pure data: no I/O, no methods beyond constructors / trivial
//! conversions. The cache crate and the plugin crates exchange these values
//! across the `Tool` trait surface; the TUI renders them.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::Serialize;
use thiserror::Error;

use crate::types::ToolId;

// ---------------------------------------------------------------------------
// ModelId
// ---------------------------------------------------------------------------

/// Plugin-specified stable identifier for a single model within a tool.
/// Examples: `"mistral:7b-instruct-q4_K_M"` (Ollama), `"meta-llama/Llama-3-8B"`
/// (HF), `"Llama-3-8B-Q4_K_M.gguf"` (lm-studio / llama-cli).
///
/// Newtype around `String` — unlike `ToolId` (which wraps `&'static str` because
/// tool names are baked into plugin source), model IDs are computed at runtime
/// from filesystem state. Uniqueness is scoped to the owning `ToolId`; the
/// SQLite cache keys `cache_models` by `(model_id, tool_id)`.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct ModelId(pub String);

impl ModelId {
    /// Construct a `ModelId` from any value convertible into a `String`.
    pub fn from(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// View the inner identifier as a `&str` for read-only operations
    /// (logging, comparisons against `DiscoveredModel::id_in_tool`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// SearchPathEntry / SearchPathSource
// ---------------------------------------------------------------------------

/// One entry in a tool's search-path list. The `source` discriminates entries
/// the plugin discovered from its built-in defaults vs. entries the user added
/// via `~/.modeltap.toml` (per data-models.md `cache_tools.search_paths_json`).
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct SearchPathEntry {
    pub path: PathBuf,
    pub source: SearchPathSource,
}

/// Provenance of a search-path entry — rendered in the per-tool detail screen.
/// `Default` paths are intrinsic to the plugin (e.g., `~/.ollama/models`);
/// `UserConfig` paths come from the user's modeltap config.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
pub enum SearchPathSource {
    Default,
    UserConfig,
}

// ---------------------------------------------------------------------------
// ToolDetail
// ---------------------------------------------------------------------------

/// Tool-level details returned by `Tool::inspect_tool()` — the per-tool detail
/// screen reads this. Pure data; the plugin is responsible for populating
/// every field that it can determine, leaving the rest as `None`.
///
/// Field shape per `architecture-design.md` §5.2 and `data-models.md`
/// §"In-memory mirror types". Optional fields render as `"(not detectable)"`
/// in the TUI per AC-21-3.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDetail {
    pub tool_id: ToolId,
    pub install_path: PathBuf,
    /// Tool version reported by the tool itself (e.g., `ollama --version`
    /// or `/api/version`). `None` when the plugin cannot detect it; renders
    /// as `"(not detectable)"`.
    pub detected_version: Option<String>,
    /// Crate version of the modeltap plugin (e.g., `"modeltap-plugin-ollama 0.2.6"`).
    /// Always populated.
    pub plugin_version: String,
    /// Where the plugin looks for models. Default entries first, user-config
    /// entries appended.
    pub search_paths: Vec<SearchPathEntry>,
    pub model_count: usize,
    pub disk_usage_bytes: u64,
    pub largest_model: Option<ModelId>,
    pub last_scan_at: Option<SystemTime>,
    pub last_scan_duration_ms: Option<u64>,
    pub last_error: Option<String>,
    pub last_error_at: Option<SystemTime>,
}

// ---------------------------------------------------------------------------
// ModelDetail
// ---------------------------------------------------------------------------

/// Model-level details returned by `Tool::inspect_model()` — the per-model
/// detail screen reads this. The `metadata_kv` is plugin-defined and holds
/// tool-relevant header fields (GGUF KVs, Ollama manifest fields, HF
/// `config.json` excerpts).
///
/// Field shape per `architecture-design.md` §5.2 and `data-models.md`
/// §"In-memory mirror types". Optional fields render as `"(not detectable)"`
/// in the TUI per AC-22-3.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ModelDetail {
    pub model_id: ModelId,
    /// Human-readable format label (e.g., `"GGUF v3"`, `"Ollama manifest v2"`,
    /// `"safetensors v2"`). `None` when the plugin cannot determine it.
    pub format: Option<String>,
    /// Quantisation tag if known (e.g., `"Q4_K_M"`).
    pub quantisation: Option<String>,
    pub architecture: Option<String>,
    /// Parameter count in billions (e.g., `7.24`).
    pub parameters: Option<f64>,
    pub context_length: Option<u32>,
    /// Plugin-selected tool-relevant KVs. Sorted (BTreeMap) for deterministic
    /// rendering and JSON serialisation.
    pub metadata_kv: BTreeMap<String, String>,
    pub introspected_at: Option<SystemTime>,
}

// ---------------------------------------------------------------------------
// InspectError
// ---------------------------------------------------------------------------

/// Errors returned by `Tool::inspect_tool` and `Tool::inspect_model`.
///
/// Four variants (per ADR-016 §"New error variant" and
/// `architecture-design.md` §5.2):
/// - `Unsupported`: the plugin opted out of inspection. Returned by the
///   trait's default body and by plugins that genuinely cannot introspect.
///   The TUI renders this as `"(not detectable)"`.
/// - `PluginPanic`: the plugin panicked inside `inspect_*`; caught at the
///   orchestrator boundary (INT-INFO-8) and surfaced as this variant.
/// - `FileReadable`: an I/O error opening / reading a file the plugin
///   expected to find (e.g., the manifest is missing or unreadable).
///   The name matches the canonical spelling in ADR-016 §"New error variant".
/// - `FormatUnreadable`: the file was readable but its format could not be
///   parsed (corrupt header, unknown schema version).
#[derive(Debug, Error)]
pub enum InspectError {
    #[error("inspect not supported by tool {tool}")]
    Unsupported { tool: ToolId },

    #[error("plugin {tool} panicked during inspect: {message}")]
    PluginPanic { tool: ToolId, message: String },

    #[error("failed to read {path}: {source}", path = path.display())]
    FileReadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("format unreadable at {path}: {detail}", path = path.display())]
    FormatUnreadable { path: PathBuf, detail: String },
}
