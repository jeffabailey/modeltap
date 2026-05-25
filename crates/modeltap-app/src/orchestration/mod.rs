//! Composition-root orchestrators — async coordinators that wire the pure
//! domain (`modeltap-core`) and the sync edges (`modeltap-store`,
//! plugin `Tool` trait) into a launch path.
//!
//! Currently hosts the warm-start orchestrator added in
//! tool-model-info-sqlite-cache step 01-04.

pub mod open_model_detail;
pub mod open_tool_detail;
pub mod reconcile;
// tool-model-info-sqlite-cache step 05-02 part 2/2: orchestrator-side K5
// gate. Wraps modeltap-store's `Cache::verify_against_fs` and is wired into
// every destructive entry point in `actions::*`. See
// lat.md/modeltap-store.md "Pre-mutate revalidator" section.
pub mod revalidate;
pub mod warm_start;
