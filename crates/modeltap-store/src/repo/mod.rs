//! Repository implementations for `Cache`.
//!
//! Each submodule extends `Cache` with `impl` blocks for one table's CRUD
//! surface. Splitting by table keeps file sizes small and matches the
//! component diagram in architecture-design.md §4.3.

pub(crate) mod intern;
pub(crate) mod models;
pub(crate) mod tools;
