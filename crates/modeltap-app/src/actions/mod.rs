//! Action orchestrators — bridge from a `UpdateEffect::trigger_*` flag to a
//! plugin call + JSONL emission. Pure orchestration; the destructive work
//! lives behind `Tool::link` / `Tool::delete_one` / `Tool::delete_all`.
//!
//! Each action module owns its own JSONL-event payload mapping (e.g.,
//! `actions::zap` builds the `action.zap_all` envelope) and returns a
//! structured `*Outcome` the composition root surfaces in the UI.
//!
//! Per the kpi-instrumentation §"Privacy" rule: NO model names, NO paths,
//! NO usernames in any action JSONL event. The orchestrator is responsible
//! for redaction at this seam.

pub mod zap;
