//! ModelsRepo — `cache_models` reads and writes.
//!
//! Step 01-02 minimum: `write_models` (UPSERT a slice in a single
//! transaction) and `models_for_tool` (read all rows for one tool_id).
//! `models_for_tool` on an unknown tool returns an empty Vec.

use std::collections::BTreeMap;

use rusqlite::params;

use modeltap_core::types::ToolId;

use crate::error::CacheError;
use crate::open::Cache;
use crate::repo::tools::{format_iso8601_utc, parse_iso8601_utc};
use crate::types::CachedModel;

impl Cache {
    /// Write a batch of models for `tool_id` in one transaction. Each row
    /// upserts on (model_id, tool_id). Callers wanting an "atomic replace"
    /// (delete-then-insert) semantics will get a richer API in Phase 04;
    /// step 01-02 wants the minimum CRUD only.
    pub fn write_models(&self, tool_id: &ToolId, models: &[CachedModel]) -> Result<(), CacheError> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO cache_models (
                        model_id, tool_id, display_name, format, quantisation,
                        size_bytes, sha256, architecture, parameters_billions,
                        context_length, dedup_group_id, metadata_kv_json,
                        metadata_introspected_at, last_seen_at, last_validated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                    ON CONFLICT(model_id, tool_id) DO UPDATE SET
                        display_name = excluded.display_name,
                        format = excluded.format,
                        quantisation = excluded.quantisation,
                        size_bytes = excluded.size_bytes,
                        sha256 = excluded.sha256,
                        architecture = excluded.architecture,
                        parameters_billions = excluded.parameters_billions,
                        context_length = excluded.context_length,
                        dedup_group_id = excluded.dedup_group_id,
                        metadata_kv_json = excluded.metadata_kv_json,
                        metadata_introspected_at = excluded.metadata_introspected_at,
                        last_seen_at = excluded.last_seen_at,
                        last_validated_at = excluded.last_validated_at",
                )?;

                for model in models {
                    if model.tool_id != *tool_id {
                        return Err(CacheError::MalformedRow {
                            table: "cache_models",
                            detail: format!(
                                "model.tool_id {} does not match write_models target {}",
                                model.tool_id, tool_id
                            ),
                        });
                    }
                    let metadata_kv_json =
                        serde_json::to_string(&model.metadata_kv).map_err(|e| {
                            CacheError::MalformedRow {
                                table: "cache_models",
                                detail: format!("serialize metadata_kv_json: {e}"),
                            }
                        })?;
                    let metadata_introspected_at = model
                        .metadata_introspected_at
                        .as_ref()
                        .map(format_iso8601_utc)
                        .transpose()?;
                    let last_seen_at = format_iso8601_utc(&model.last_seen_at)?;
                    let last_validated_at = model
                        .last_validated_at
                        .as_ref()
                        .map(format_iso8601_utc)
                        .transpose()?;

                    stmt.execute(params![
                        model.model_id,
                        model.tool_id.0,
                        model.display_name,
                        model.format,
                        model.quantisation,
                        model.size_bytes as i64,
                        model.sha256,
                        model.architecture,
                        model.parameters_billions,
                        model.context_length,
                        model.dedup_group_id,
                        Some(metadata_kv_json),
                        metadata_introspected_at,
                        last_seen_at,
                        last_validated_at,
                    ])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Read all `cache_models` rows for one tool. Order is unspecified.
    pub fn models_for_tool(&self, tool_id: &ToolId) -> Result<Vec<CachedModel>, CacheError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT model_id, tool_id, display_name, format, quantisation,
                        size_bytes, sha256, architecture, parameters_billions,
                        context_length, dedup_group_id, metadata_kv_json,
                        metadata_introspected_at, last_seen_at, last_validated_at
                 FROM cache_models
                 WHERE tool_id = ?1",
            )?;
            let rows = stmt
                .query_map(params![tool_id.0], |row| {
                    Ok(RawModelRow {
                        model_id: row.get(0)?,
                        tool_id: row.get(1)?,
                        display_name: row.get(2)?,
                        format: row.get(3)?,
                        quantisation: row.get(4)?,
                        size_bytes: row.get(5)?,
                        sha256: row.get(6)?,
                        architecture: row.get(7)?,
                        parameters_billions: row.get(8)?,
                        context_length: row.get(9)?,
                        dedup_group_id: row.get(10)?,
                        metadata_kv_json: row.get(11)?,
                        metadata_introspected_at: row.get(12)?,
                        last_seen_at: row.get(13)?,
                        last_validated_at: row.get(14)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            rows.into_iter().map(hydrate_model).collect()
        })
    }
}

struct RawModelRow {
    model_id: String,
    tool_id: String,
    display_name: String,
    format: Option<String>,
    quantisation: Option<String>,
    size_bytes: i64,
    sha256: Option<String>,
    architecture: Option<String>,
    parameters_billions: Option<f64>,
    context_length: Option<i64>,
    dedup_group_id: Option<String>,
    metadata_kv_json: Option<String>,
    metadata_introspected_at: Option<String>,
    last_seen_at: String,
    last_validated_at: Option<String>,
}

fn hydrate_model(raw: RawModelRow) -> Result<CachedModel, CacheError> {
    let tool_id = ToolId(crate::repo::intern::intern_tool_id(&raw.tool_id));

    let metadata_kv: BTreeMap<String, String> = match raw.metadata_kv_json.as_deref() {
        Some(s) if !s.is_empty() => {
            serde_json::from_str(s).map_err(|e| CacheError::MalformedRow {
                table: "cache_models",
                detail: format!("parse metadata_kv_json: {e}"),
            })?
        }
        _ => BTreeMap::new(),
    };

    let metadata_introspected_at = raw
        .metadata_introspected_at
        .as_deref()
        .map(|s| parse_iso8601_utc(s, "cache_models.metadata_introspected_at"))
        .transpose()?;
    let last_seen_at = parse_iso8601_utc(&raw.last_seen_at, "cache_models.last_seen_at")?;
    let last_validated_at = raw
        .last_validated_at
        .as_deref()
        .map(|s| parse_iso8601_utc(s, "cache_models.last_validated_at"))
        .transpose()?;

    Ok(CachedModel {
        model_id: raw.model_id,
        tool_id,
        display_name: raw.display_name,
        format: raw.format,
        quantisation: raw.quantisation,
        size_bytes: raw.size_bytes as u64,
        sha256: raw.sha256,
        architecture: raw.architecture,
        parameters_billions: raw.parameters_billions,
        context_length: raw.context_length.map(|v| v as u32),
        dedup_group_id: raw.dedup_group_id,
        metadata_kv,
        metadata_introspected_at,
        last_seen_at,
        last_validated_at,
    })
}
