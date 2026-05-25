//! View-layer pure helpers. Contains string-shaping functions consumed by
//! the renderers in `render::*` but expressed as pure `fn(state) -> String`
//! so they can be unit-tested without a ratatui `Frame`.
//!
//! Step 05-03 (US-25): the `provenance` module holds
//! `format_provenance(now, last_scan_at) -> String` — the pure freshness-
//! suffix helper for the summary-bar provenance line. See CM-D §9 of
//! `docs/feature/tool-model-info-sqlite-cache/distill/acceptance-test-plan.md`
//! for the pure-function inventory entry this implements.

pub mod provenance;
