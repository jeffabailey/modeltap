//! Minimal GGUF v3 header parser for `Tool::inspect_model` purposes.
//!
//! Reads just the metadata KV table — never touches tensor data. Shared by
//! [[plugins/lm-studio/src/inspect.rs]] and the future llama-cli inspect
//! override (step 03-02 part 4); the lm-studio half (this commit) lands the
//! parser, the llama-cli half reuses it without duplication.
//!
//! ## Why hand-rolled
//!
//! Per ADR-016 §"Implementation Guidance" — "use the `gguf` crate or hand-roll
//! a minimal parser — both acceptable." We hand-roll because the subset of
//! fields modeltap surfaces in the model-detail screen is small (general.*
//! and `<arch>.*` header KVs) and the parser is a few hundred lines.
//! Avoiding a new workspace dep keeps the dep-graph minimal and avoids
//! pulling in a parser that decompresses tensor data the inspect screen
//! never reads.
//!
//! ## Layout (GGUF v3 spec)
//!
//! ```text
//!   magic         : 4 bytes  ASCII "GGUF" (LE 0x46554747)
//!   version       : u32 LE   (we accept only `3`)
//!   tensor_count  : u64 LE   (read + discarded)
//!   kv_count      : u64 LE   (bounded ≤ 100_000)
//!   then kv_count entries, each:
//!     key_len     : u64 LE
//!     key_bytes   : key_len bytes (UTF-8)
//!     value_type  : u32 LE
//!     value       : depends on value_type
//! ```
//!
//! ## Safety contract
//!
//! The parser MUST NOT panic on malformed input. Every read is bounds-checked;
//! every length is validated against the remaining slice. Truncated, garbage,
//! or maliciously-crafted bytes return `Err(GgufParseError::*)`. The caller
//! (the plugin's `inspect_model`) translates that into
//! `Err(InspectError::FormatUnreadable)`.
//!
//! ## Object-Calisthenics scope
//!
//! Domain module — strict OC rules apply. The Cursor helper carries 2
//! instance fields (buf + pos); methods stay short. The parser tops out at
//! ~250 lines incl. tests-in-place.

use std::collections::BTreeMap;
use std::path::Path;

use thiserror::Error;

const MAGIC: &[u8; 4] = b"GGUF";
const SUPPORTED_VERSION: u32 = 3;

/// Maximum bytes we will read for the header. GGUF metadata-KV tables are
/// typically a few KB; cap at 16 MB to defend against a corrupt file that
/// claims a multi-GB header.
const MAX_HEADER_READ_BYTES: usize = 16 * 1024 * 1024;

/// Maximum kv_count we will iterate. Real-world GGUF files have ≤ ~200 KVs;
/// 100k is a comfortable upper bound that prevents a corrupt u64::MAX from
/// hanging the parser.
const MAX_KV_COUNT: u64 = 100_000;

/// Maximum length we will accept for a single string value. Real GGUF strings
/// (keys, value strings, tokenizer pieces) top out around 64 KB; 1 MB is the
/// hard ceiling.
const MAX_STRING_BYTES: usize = 1_000_000;

/// Maximum array length we will iterate when SKIPPING an unknown array value.
/// Picked at 100k so we don't waste time iterating a corrupt-claim u64::MAX
/// array of tokens.
const MAX_ARRAY_LEN: u64 = 100_000;

// GGUF value-type tags (per the GGUF v3 spec).
const TYPE_UINT8: u32 = 0;
const TYPE_INT8: u32 = 1;
const TYPE_UINT16: u32 = 2;
const TYPE_INT16: u32 = 3;
const TYPE_UINT32: u32 = 4;
const TYPE_INT32: u32 = 5;
const TYPE_FLOAT32: u32 = 6;
const TYPE_BOOL: u32 = 7;
const TYPE_STRING: u32 = 8;
const TYPE_ARRAY: u32 = 9;
const TYPE_UINT64: u32 = 10;
const TYPE_INT64: u32 = 11;
const TYPE_FLOAT64: u32 = 12;

/// Parsed GGUF header. Only what the per-model detail screen needs:
/// - `version` — the wire-format version (always `3` for now)
/// - `format` — human-readable label, e.g. `"GGUF v3"`
/// - `metadata_kv` — the header KV table projected to `String→String` so
///   the caller can `BTreeMap` it directly into `ModelDetail.metadata_kv`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufHeader {
    pub version: u32,
    pub format: String,
    pub metadata_kv: BTreeMap<String, String>,
}

/// Errors `parse_header` can return. Never panics; every malformation maps
/// to one of these variants.
#[derive(Debug, Error)]
pub enum GgufParseError {
    #[error("gguf io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("gguf bad magic: expected 'GGUF', got 0x{got:08x}")]
    BadMagic { got: u32 },

    #[error("gguf unsupported version: {got} (expected {SUPPORTED_VERSION})")]
    UnsupportedVersion { got: u32 },

    #[error("gguf malformed: {0}")]
    Malformed(&'static str),
}

/// Open `path` and parse the GGUF v3 header.
///
/// Reads at most `MAX_HEADER_READ_BYTES` bytes. Never panics. The returned
/// `metadata_kv` map carries every successfully-decoded KV — string values
/// verbatim, numeric values via `to_string()`. Array values are emitted as
/// their first element (when scalar) so the caller has something displayable;
/// nested arrays are skipped.
pub fn parse_header(path: &Path) -> Result<GgufHeader, GgufParseError> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(64 * 1024);
    let n = file
        .by_ref()
        .take(MAX_HEADER_READ_BYTES as u64)
        .read_to_end(&mut buf)?;
    let bytes = &buf[..n];
    parse_header_bytes(bytes)
}

/// Parse a GGUF v3 header from an in-memory byte slice. Same contract as
/// `parse_header`; exposed for fixture tests that build a synthetic header
/// without writing it to disk first.
pub fn parse_header_bytes(bytes: &[u8]) -> Result<GgufHeader, GgufParseError> {
    let mut cur = Cursor::new(bytes);
    let magic_bytes = cur.read_n(4)?;
    if magic_bytes != MAGIC.as_slice() {
        let got = u32::from_le_bytes([
            magic_bytes[0],
            magic_bytes[1],
            magic_bytes[2],
            magic_bytes[3],
        ]);
        return Err(GgufParseError::BadMagic { got });
    }
    let version = cur.read_u32()?;
    if version != SUPPORTED_VERSION {
        return Err(GgufParseError::UnsupportedVersion { got: version });
    }
    let _tensor_count = cur.read_u64()?;
    let kv_count = cur.read_u64()?;
    if kv_count > MAX_KV_COUNT {
        return Err(GgufParseError::Malformed("kv_count exceeds bound"));
    }

    let mut kv: BTreeMap<String, String> = BTreeMap::new();
    for _ in 0..kv_count {
        let key = cur.read_string()?;
        let value_type = cur.read_u32()?;
        match read_value_as_string(&mut cur, value_type)? {
            Some(value) => {
                kv.insert(key, value);
            }
            None => {
                // Unknown / nested-array — value was skipped, key omitted.
            }
        }
    }

    Ok(GgufHeader {
        version,
        format: format!("GGUF v{version}"),
        metadata_kv: kv,
    })
}

/// Read one value off the cursor and return its `String` projection. Returns
/// `Ok(None)` for values that have no scalar projection (nested arrays);
/// `Err` only for malformed input.
fn read_value_as_string(
    cur: &mut Cursor<'_>,
    value_type: u32,
) -> Result<Option<String>, GgufParseError> {
    match value_type {
        TYPE_UINT8 => Ok(Some(cur.read_u8()?.to_string())),
        TYPE_INT8 => Ok(Some((cur.read_u8()? as i8).to_string())),
        TYPE_UINT16 => Ok(Some(cur.read_u16()?.to_string())),
        TYPE_INT16 => Ok(Some((cur.read_u16()? as i16).to_string())),
        TYPE_UINT32 => Ok(Some(cur.read_u32()?.to_string())),
        TYPE_INT32 => Ok(Some((cur.read_u32()? as i32).to_string())),
        TYPE_FLOAT32 => Ok(Some(f32::from_bits(cur.read_u32()?).to_string())),
        TYPE_BOOL => Ok(Some((cur.read_u8()? != 0).to_string())),
        TYPE_STRING => Ok(Some(cur.read_string()?)),
        TYPE_UINT64 => Ok(Some(cur.read_u64()?.to_string())),
        TYPE_INT64 => Ok(Some((cur.read_u64()? as i64).to_string())),
        TYPE_FLOAT64 => Ok(Some(f64::from_bits(cur.read_u64()?).to_string())),
        TYPE_ARRAY => {
            // Emit the first scalar element so callers have something
            // renderable; skip the rest. Nested arrays return None.
            let inner_type = cur.read_u32()?;
            let len = cur.read_u64()?;
            if len > MAX_ARRAY_LEN {
                return Err(GgufParseError::Malformed("array len exceeds bound"));
            }
            if len == 0 {
                return Ok(None);
            }
            let first = read_value_as_string(cur, inner_type)?;
            for _ in 1..len {
                // We don't recurse for the SKIP path — only scalar inner
                // types are common for header arrays. Nested arrays are
                // rejected as malformed (we cannot determine the width
                // without reading every element).
                let _ = read_value_as_string(cur, inner_type)?;
            }
            Ok(first)
        }
        _ => Err(GgufParseError::Malformed("unknown value type")),
    }
}

// ---------------------------------------------------------------------------
// Bounds-checked cursor.
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn read_n(&mut self, n: usize) -> Result<&'a [u8], GgufParseError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(GgufParseError::Malformed("cursor overflow"))?;
        if end > self.buf.len() {
            return Err(GgufParseError::Malformed("truncated read"));
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn read_u8(&mut self) -> Result<u8, GgufParseError> {
        let b = self.read_n(1)?;
        Ok(b[0])
    }

    fn read_u16(&mut self) -> Result<u16, GgufParseError> {
        let b = self.read_n(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, GgufParseError> {
        let b = self.read_n(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_u64(&mut self) -> Result<u64, GgufParseError> {
        let b = self.read_n(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn read_string(&mut self) -> Result<String, GgufParseError> {
        let len = self.read_u64()?;
        if len > MAX_STRING_BYTES as u64 {
            return Err(GgufParseError::Malformed("string exceeds bound"));
        }
        let bytes = self.read_n(len as usize)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

// ---------------------------------------------------------------------------
// Tests — exercise the safety contract (no panics) and the projection.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic GGUF v3 header with the given metadata KVs. Each KV
    /// is `(key, type_tag, value_bytes)`. Used by both the parser tests below
    /// AND the acceptance-fixture builder (which re-exports the same helper
    /// shape — see `tests/src/fixtures/inspect_fixtures.rs::write_gguf_v3_header`).
    fn build_header(kvs: &[(&str, u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&SUPPORTED_VERSION.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        out.extend_from_slice(&(kvs.len() as u64).to_le_bytes());
        for (key, type_tag, value) in kvs {
            out.extend_from_slice(&(key.len() as u64).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&type_tag.to_le_bytes());
            out.extend_from_slice(value);
        }
        out
    }

    fn string_value(s: &str) -> Vec<u8> {
        let mut v = (s.len() as u64).to_le_bytes().to_vec();
        v.extend_from_slice(s.as_bytes());
        v
    }

    #[test]
    fn parses_string_and_uint32_kv_into_metadata_map() {
        let bytes = build_header(&[
            (
                "general.architecture",
                TYPE_STRING,
                string_value("llama"),
            ),
            (
                "general.quantization_version",
                TYPE_STRING,
                string_value("Q4_K_M"),
            ),
            (
                "llama.context_length",
                TYPE_UINT32,
                4096u32.to_le_bytes().to_vec(),
            ),
            (
                "llama.embedding_length",
                TYPE_UINT32,
                4096u32.to_le_bytes().to_vec(),
            ),
            (
                "tokenizer.ggml.model",
                TYPE_STRING,
                string_value("llama"),
            ),
        ]);
        let h = parse_header_bytes(&bytes).expect("header parses");
        assert_eq!(h.version, 3);
        assert_eq!(h.format, "GGUF v3");
        assert_eq!(
            h.metadata_kv.get("general.architecture").map(|s| s.as_str()),
            Some("llama")
        );
        assert_eq!(
            h.metadata_kv
                .get("general.quantization_version")
                .map(|s| s.as_str()),
            Some("Q4_K_M")
        );
        assert_eq!(
            h.metadata_kv.get("llama.context_length").map(|s| s.as_str()),
            Some("4096")
        );
    }

    #[test]
    fn bad_magic_returns_bad_magic_not_panic() {
        let bad = b"XXXX\x03\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        let err = parse_header_bytes(bad).expect_err("must error on bad magic");
        assert!(matches!(err, GgufParseError::BadMagic { .. }));
    }

    #[test]
    fn unsupported_version_returns_unsupported_version() {
        // Magic OK, version = 2 (unsupported).
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        let err = parse_header_bytes(&bytes).expect_err("must error on v2");
        assert!(matches!(err, GgufParseError::UnsupportedVersion { got: 2 }));
    }

    #[test]
    fn empty_input_returns_malformed_not_panic() {
        let err = parse_header_bytes(&[]).expect_err("empty must error");
        assert!(matches!(err, GgufParseError::Malformed(_)));
    }

    #[test]
    fn truncated_header_never_panics() {
        let full = build_header(&[(
            "general.architecture",
            TYPE_STRING,
            string_value("mistral"),
        )]);
        // For every truncation point up to full length, parse must return a
        // Result without panicking.
        for cut in 0..full.len() {
            let _ = parse_header_bytes(&full[..cut]);
        }
    }

    #[test]
    fn parse_header_from_file_reads_disk() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("test.gguf");
        let bytes = build_header(&[(
            "general.architecture",
            TYPE_STRING,
            string_value("phi3"),
        )]);
        std::fs::write(&path, &bytes).unwrap();
        let h = parse_header(&path).expect("parse from disk");
        assert_eq!(
            h.metadata_kv
                .get("general.architecture")
                .map(|s| s.as_str()),
            Some("phi3")
        );
    }

    #[test]
    fn random_bytes_never_panic() {
        // Linear congruential pseudo-random for determinism; same shape as
        // the archived loose-gguf parser's fuzz test.
        let mut seed: u64 = 0xDEAD_BEEF_CAFE_F00D;
        for _ in 0..200 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = ((seed >> 32) as usize) % 4096;
            let mut buf = vec![0u8; len];
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (seed.wrapping_add(i as u64) & 0xFF) as u8;
            }
            // Every random buf with the GGUF magic at the front must also
            // not panic — that's the corrupt-but-magic-OK case.
            if buf.len() >= 4 {
                buf[..4].copy_from_slice(MAGIC);
            }
            let _ = parse_header_bytes(&buf);
        }
    }
}
