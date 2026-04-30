//! `LastAction` — pure data type for the post-action banner (US-06).
//!
//! After every mutating action (zap in WS scope; unify in 03-02; delete-from-
//! one in 03-06) the right pane renders a header + body describing the
//! outcome. This type holds the structured data; the render layer in
//! `modeltap-tui::render::last_action` formats it into ratatui lines.
//!
//! Per intake Q7, `LastAction` is in-memory only — lost on restart. No
//! persistent state.
//!
//! Per ADR-006 (Elm-style update), `AppState.last_action: Option<LastAction>`
//! is set by `Msg::SetLastAction(LastAction)` and cleared by any navigation
//! Msg.

use crate::types::ToolId;

/// Which mutating action produced this banner. Only `Zap` is reachable in
/// the WS slice; `Unify` lands in 03-02, `Delete` (single-model) in 03-06.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ActionVerb {
    Zap,
    Unify,
    Delete,
}

impl ActionVerb {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActionVerb::Zap => "zap",
            ActionVerb::Unify => "unify",
            ActionVerb::Delete => "delete",
        }
    }
}

/// What happened. `Success` for full success, `Partial` for partial-success
/// (some targets succeeded, some failed — used by 03-03 cross-fs unify),
/// `Failed` for full failure.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ActionStatus {
    Success,
    Partial {
        successes: u64,
        failures: Vec<TargetError>,
    },
    Failed,
}

impl ActionStatus {
    /// Short form for the header parenthetical: "success" / "failed" /
    /// "partial: N of M targets linked".
    pub fn header_label(&self) -> String {
        match self {
            ActionStatus::Success => "success".to_string(),
            ActionStatus::Failed => "failed".to_string(),
            ActionStatus::Partial {
                successes,
                failures,
            } => {
                let total = successes + failures.len() as u64;
                format!("partial: {} of {} targets linked", successes, total)
            }
        }
    }
}

/// One target's failure detail for partial-success messages. Used by 03-03
/// (cross-fs unify) to show "the failed target's path and reason" per the
/// master-acceptance scenario. WS slice never produces this.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TargetError {
    pub path: String,
    pub reason: String,
}

/// The structured post-action banner. Pure data; rendered by
/// `modeltap-tui::render::last_action::view_lines`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LastAction {
    pub verb: ActionVerb,
    /// Identifier of what was acted on. For zap, this is the ToolId string
    /// ("ollama", "llama-cli"). For unify/delete (single model), this is the
    /// model id.
    pub target: String,
    pub status: ActionStatus,
    /// Bytes freed by the action. For zap-success this is the sum of the
    /// removed-blob bytes (per `ZapOutcome.bytes_reclaimed` from
    /// `actions::zap`).
    pub bytes_reclaimed: u64,
    /// Bytes that stayed on disk because another tool still references them
    /// (shared blobs). For WS slice with no cross-tool sharing, this is 0;
    /// the schema is in place for 03-02 (unify) and 03-06 (delete-from-one).
    pub bytes_retained: u64,
    /// Optional extra string for the body line. Used by unify to render
    /// "1 inode, 3 hardlinks" per the US-06 scenario; None for zap.
    pub extra: Option<String>,
}

impl LastAction {
    /// Construct a success banner for a confirmed zap. `bytes_retained` is
    /// the sum of bytes that stayed on disk because another tool references
    /// them; pass 0 when no cross-tool sharing exists (WS slice).
    pub fn for_zap_success(tool_id: ToolId, bytes_reclaimed: u64, bytes_retained: u64) -> Self {
        Self {
            verb: ActionVerb::Zap,
            target: tool_id.0.to_string(),
            status: ActionStatus::Success,
            bytes_reclaimed,
            bytes_retained,
            extra: None,
        }
    }

    /// Construct a failure banner for a zap that errored before any work.
    /// `bytes_reclaimed == 0` and `bytes_retained == 0` by definition.
    pub fn for_zap_failed(tool_id: ToolId) -> Self {
        Self {
            verb: ActionVerb::Zap,
            target: tool_id.0.to_string(),
            status: ActionStatus::Failed,
            bytes_reclaimed: 0,
            bytes_retained: 0,
            extra: None,
        }
    }

    /// Construct a partial-success banner for a zap. Some targets succeeded,
    /// some failed. `bytes_reclaimed` is the bytes freed by the successes.
    pub fn for_zap_partial(
        tool_id: ToolId,
        bytes_reclaimed: u64,
        successes: u64,
        failures: Vec<TargetError>,
    ) -> Self {
        Self {
            verb: ActionVerb::Zap,
            target: tool_id.0.to_string(),
            status: ActionStatus::Partial {
                successes,
                failures,
            },
            bytes_reclaimed,
            bytes_retained: 0,
            extra: None,
        }
    }

    /// Construct a success banner for a confirmed unify (US-10). The
    /// `target` is the model's display label / id (NOT a tool id — unify
    /// acts on a single model across multiple tools). The `extra` line
    /// renders "1 inode, N hardlinks" per the US-06 scenario.
    pub fn for_unify_success(target: String, bytes_reclaimed: u64, hardlink_count: usize) -> Self {
        Self {
            verb: ActionVerb::Unify,
            target,
            status: ActionStatus::Success,
            bytes_reclaimed,
            bytes_retained: 0,
            extra: Some(format!("1 inode, {hardlink_count} hardlinks")),
        }
    }

    /// Construct an "already unified" banner. The unify dialog opened in
    /// AlreadyUnified mode and the user dismissed; we record an
    /// informational success-with-zero-reclaim banner.
    pub fn for_unify_already_unified(target: String, hardlink_count: usize) -> Self {
        Self {
            verb: ActionVerb::Unify,
            target,
            status: ActionStatus::Success,
            bytes_reclaimed: 0,
            bytes_retained: 0,
            extra: Some(format!(
                "already unified: 1 inode, {hardlink_count} hardlinks"
            )),
        }
    }

    /// Construct a partial-success banner for a unify. Used by 03-03 (cross-fs).
    pub fn for_unify_partial(
        target: String,
        bytes_reclaimed: u64,
        successes: u64,
        failures: Vec<TargetError>,
    ) -> Self {
        Self {
            verb: ActionVerb::Unify,
            target,
            status: ActionStatus::Partial {
                successes,
                failures,
            },
            bytes_reclaimed,
            bytes_retained: 0,
            extra: None,
        }
    }

    /// Construct a failure banner for a unify that errored before any work
    /// (or where every target failed).
    pub fn for_unify_failed(target: String) -> Self {
        Self {
            verb: ActionVerb::Unify,
            target,
            status: ActionStatus::Failed,
            bytes_reclaimed: 0,
            bytes_retained: 0,
            extra: None,
        }
    }
}
