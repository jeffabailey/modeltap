//! Pure data types exchanged across the modeltap-store public surface.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/design/data-models.md`
//! §"In-memory mirror types". These are typed row representations — no
//! methods beyond trivial accessors. Serde-derived for JSON-column round-trips
//! (search_paths, metadata_kv) and for general inspectability in tests.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use modeltap_core::types::ToolId;

/// Plugin-supplied stable identifier for a model within its tool. Newtype
/// would be ideal, but the rest of the codebase (DiscoveredModel, ModelMeta)
/// uses raw `String`; we follow suit so call sites do not double-convert.
pub type ModelId = String;

/// Row type for `cache_tools`. Pure data — no behavior.
///
/// Not serde-derived: row hydration goes through column-by-column SQL
/// reads (see `repo::tools::hydrate_tool`). The Serde derives on
/// `SearchPathEntry` below handle the one JSON column we DO round-trip.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedTool {
    pub tool_id: ToolId,
    pub install_path: PathBuf,
    pub detected_version: Option<String>,
    pub plugin_version: String,
    pub model_count: u64,
    pub disk_usage_bytes: u64,
    pub largest_model_id: Option<ModelId>,
    pub last_scan_at: SystemTime,
    pub last_scan_duration_ms: u64,
    pub last_error: Option<String>,
    pub last_error_at: Option<SystemTime>,
    pub search_paths: Vec<SearchPathEntry>,
}

/// An entry in `cache_tools.search_paths_json`. Source of the path matters
/// because the user-config tagging affects the model-detail provenance line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchPathEntry {
    pub path: PathBuf,
    pub source: SearchPathSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchPathSource {
    Default,
    UserConfig,
}

/// Row type for `cache_models`. Pure data — no behavior.
///
/// Not serde-derived: see the rationale on `CachedTool`. The `metadata_kv`
/// `BTreeMap` is serialized to/from the `metadata_kv_json` column at the
/// repository layer.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedModel {
    pub model_id: ModelId,
    pub tool_id: ToolId,
    pub display_name: String,
    pub format: Option<String>,
    pub quantisation: Option<String>,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub architecture: Option<String>,
    pub parameters_billions: Option<f64>,
    pub context_length: Option<u32>,
    pub dedup_group_id: Option<String>,
    pub metadata_kv: BTreeMap<String, String>,
    pub metadata_introspected_at: Option<SystemTime>,
    pub last_seen_at: SystemTime,
    pub last_validated_at: Option<SystemTime>,
}

/// Row type for `cache_model_files`. The `(mtime, size, inode, dev)` quad is
/// load-bearing for pre-mutate revalidation (ADR-015 §3).
///
/// Not exercised by the step 01-02 minimum CRUD surface — defined here for
/// the next step (`ModelFilesRepo`).
#[derive(Debug, Clone, PartialEq)]
pub struct CachedFile {
    pub model_id: ModelId,
    pub tool_id: ToolId,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime: SystemTime,
    pub inode: u64,
    pub dev: u64,
    pub last_stat_at: SystemTime,
}

/// Result of `Cache::verify_against_fs`. Returned in step 04 when the
/// revalidator lands; type declared here to keep all wire types in one place.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    Match,
    Drift { fresh: FileStat },
    Gone,
}

/// Subset of `Metadata` that the revalidator compares against the cached
/// quad. Distinct from `CachedFile` because it represents a fresh read,
/// not a stored row.
#[derive(Debug, Clone, PartialEq)]
pub struct FileStat {
    pub size_bytes: u64,
    pub mtime: SystemTime,
    pub inode: u64,
    pub dev: u64,
}
