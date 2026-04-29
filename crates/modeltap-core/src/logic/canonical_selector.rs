//! Pure-domain canonical selector for the unify action (US-10, ADR-002,
//! ADR-003).
//!
//! Given a list of `CandidatePath` (one per tool that registers a model
//! identified by sha256), pick the path that the unify action will hardlink
//! the others to. Per ADR-003 there is no `~/.modeltap/store/` — the
//! canonical is always an existing tool-owned path. Per ADR-002 the canonical
//! must already have content matching the dedup-key sha256.
//!
//! ## Decision rules
//!
//! 1. Filter to candidates whose `exists == true`. A non-existent path
//!    cannot be the source of a hardlink.
//! 2. Among existing candidates, prefer the **largest** by `size_bytes`.
//!    The user has likely just verified its content; preserving it as the
//!    source minimises the bytes-at-risk.
//! 3. Tiebreaker on equal `size_bytes`: prefer Ollama-blob paths. Ollama's
//!    content-addressed blob store is the most predictable dedup substrate
//!    (the blob filename IS the sha256 of bytes — see
//!    `plugins/ollama/OLLAMA_BLOB_VERIFICATION.md`).
//! 4. Final tiebreaker: lexicographic path order, so the function is
//!    deterministic across runs.
//!
//! ## Purity contract
//!
//! No I/O. The orchestrator constructs `CandidatePath` entries by `stat`-ing
//! each path beforehand (using the `FsProbe` port); this module only ranks.

use std::path::PathBuf;

use serde::Serialize;

use crate::types::ToolId;

/// One candidate path (per tool) that the unify action could pick as the
/// canonical source. `size_bytes` and `exists` are filled in by the
/// orchestrator after `stat`-ing each path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidatePath {
    pub tool: ToolId,
    pub path: PathBuf,
    /// True if the file exists at `path`. False ⇒ excluded from selection.
    pub exists: bool,
    /// Apparent on-disk size (the tool-reported size). Used as the rank.
    pub size_bytes: u64,
    /// True if this candidate is an Ollama-blob path. Used as tiebreaker.
    /// Plugin-provided rather than parsed from the path because the rule is
    /// "the path the Ollama plugin owns", not a string match.
    pub is_ollama_blob: bool,
}

/// Pick the canonical path from a slice of candidates. Returns `None` when
/// no candidate exists on disk (the unify action is then not applicable).
///
/// See the module docstring for the full ranking rules. This function is
/// its own driving port — calling it directly in tests IS port-to-port
/// testing per the `nw-tdd-methodology` convention.
pub fn select_canonical(candidates: &[CandidatePath]) -> Option<&CandidatePath> {
    candidates.iter().filter(|c| c.exists).max_by(|a, b| {
        // Largest size_bytes first. `Ord::cmp` returns Greater when the
        // left is larger; we want the largest to be "max", so the natural
        // ordering works — but `max_by` needs a comparator that reflects
        // "a > b" by returning Greater. So compare a vs b directly.
        a.size_bytes
            .cmp(&b.size_bytes)
            // Prefer Ollama-blob paths (true > false).
            .then_with(|| a.is_ollama_blob.cmp(&b.is_ollama_blob))
            // Lexicographic — but inverted because `max_by` keeps the
            // greater. We want the *smaller* path lexicographically as
            // canonical for stability across runs ⇒ invert.
            .then_with(|| b.path.cmp(&a.path))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(
        tool: &'static str,
        path: &str,
        exists: bool,
        size_bytes: u64,
        is_ollama_blob: bool,
    ) -> CandidatePath {
        CandidatePath {
            tool: ToolId(tool),
            path: PathBuf::from(path),
            exists,
            size_bytes,
            is_ollama_blob,
        }
    }

    #[test]
    fn returns_none_when_no_candidates_exist_on_disk() {
        let cs = vec![
            cand("ollama", "/a", false, 10, true),
            cand("hf", "/b", false, 20, false),
        ];
        assert!(select_canonical(&cs).is_none());
    }

    #[test]
    fn returns_only_existing_when_others_missing() {
        let cs = vec![
            cand("ollama", "/a", false, 10, true),
            cand("hf", "/b", true, 5, false),
        ];
        let picked = select_canonical(&cs).unwrap();
        assert_eq!(picked.tool, ToolId("hf"));
    }

    #[test]
    fn prefers_largest_existing_candidate() {
        let cs = vec![
            cand("ollama", "/a", true, 10, true),
            cand("hf", "/b", true, 100, false),
            cand("llama-cli", "/c", true, 50, false),
        ];
        let picked = select_canonical(&cs).unwrap();
        assert_eq!(picked.tool, ToolId("hf"));
        assert_eq!(picked.size_bytes, 100);
    }

    #[test]
    fn tiebreaks_equal_size_in_favor_of_ollama_blob() {
        let cs = vec![
            cand("hf", "/b", true, 100, false),
            cand("ollama", "/a", true, 100, true),
            cand("llama-cli", "/c", true, 100, false),
        ];
        let picked = select_canonical(&cs).unwrap();
        assert_eq!(picked.tool, ToolId("ollama"));
    }

    #[test]
    fn final_tiebreak_is_deterministic_lexicographic() {
        // No Ollama blob, all equal size → smallest path wins.
        let cs = vec![
            cand("hf", "/zzz", true, 100, false),
            cand("llama-cli", "/aaa", true, 100, false),
            cand("lm-studio", "/mmm", true, 100, false),
        ];
        let picked = select_canonical(&cs).unwrap();
        assert_eq!(picked.path, PathBuf::from("/aaa"));
    }

    #[test]
    fn empty_input_returns_none() {
        let cs: Vec<CandidatePath> = vec![];
        assert!(select_canonical(&cs).is_none());
    }

    #[test]
    fn single_existing_candidate_is_picked() {
        let cs = vec![cand("ollama", "/only", true, 42, true)];
        let picked = select_canonical(&cs).unwrap();
        assert_eq!(picked.tool, ToolId("ollama"));
    }
}
