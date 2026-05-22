//! Composition-root orchestrators — async coordinators that wire the pure
//! domain (`modeltap-core`) and the sync edges (`modeltap-store`,
//! plugin `Tool` trait) into a launch path.
//!
//! Currently hosts the warm-start orchestrator added in
//! tool-model-info-sqlite-cache step 01-04.

pub mod open_model_detail;
pub mod open_tool_detail;
pub mod warm_start;
