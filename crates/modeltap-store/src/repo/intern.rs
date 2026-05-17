//! Tiny tool-id interner.
//!
//! `ToolId` wraps `&'static str`. Rusqlite returns owned `String`s for TEXT
//! columns; we need a stable `'static` projection without leaking a fresh
//! allocation every read. A bounded interner keyed by the small set of
//! registered plugin tool_ids (typically <10 entries) satisfies this with
//! constant memory across the process lifetime.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Look up (or insert) a `&'static str` for the given tool_id string.
/// First insert leaks a `Box<str>` once; subsequent calls return the same
/// pointer.
pub(crate) fn intern_tool_id(s: &str) -> &'static str {
    static TABLE: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = table.lock().expect("intern table poisoned");
    if let Some(existing) = guard.get(s) {
        return existing;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    guard.insert(s.to_string(), leaked);
    leaked
}
