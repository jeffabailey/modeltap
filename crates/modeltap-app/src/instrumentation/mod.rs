//! Instrumentation facades — single point of emission for the JSONL events
//! the acceptance + KPI suites read out of `<log_dir>/launch.log`.
//!
//! Currently hosts only the launch-metrics facade (step 04-05, closes
//! Phase 04). Future work that adds more JSONL surfaces (e.g., per-action
//! audit lines) lands here so the line-shape stays consistent and reviewable
//! in one place.

pub mod launch_metrics;
