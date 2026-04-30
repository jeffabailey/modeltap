//! GGUF header parser.
//!
//! Parses just enough of the GGUF v1/v2/v3 header to recover:
//! - the architecture string (`general.architecture`)
//! - the quantization label, derived from `general.file_type` (uint32)
//!
//! GGUF spec: <https://github.com/ggerganov/ggml/blob/master/docs/gguf.md>
//!
//! Layout:
//!
//! ```text
//!   magic         : 4 bytes  ASCII "GGUF" (LE 0x46554747)
//!   version       : u32 LE
//!   tensor_count  : u64 LE
//!   kv_count      : u64 LE
//!   then kv_count entries, each:
//!     key_len     : u64 LE
//!     key_bytes   : key_len bytes (UTF-8)
//!     value_type  : u32 LE       (see GGUF_TYPE_*)
//!     value       : depends on value_type
//! ```
//!
//! **Safety contract:** this parser MUST NOT panic on malformed input.
//! Every read is bounds-checked; every length is validated against the
//! remaining slice. Truncated, garbage, or maliciously-crafted bytes return
//! `Err(ParseError::*)`. The caller (the discovery walker) translates that
//! into a `DiscoveredModel` with `Format::Other` + `ModelStatus::Corrupt`.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon (per ADR-001 plugins live outside the core). Strict OC rules are
//! relaxed here — the parser exists to bridge real I/O.

#![allow(clippy::module_name_repetitions)]

use thiserror::Error;

const MAGIC: &[u8; 4] = b"GGUF";

// GGUF value type tags (from gguf.md):
const GGUF_TYPE_UINT8: u32 = 0;
const GGUF_TYPE_INT8: u32 = 1;
const GGUF_TYPE_UINT16: u32 = 2;
const GGUF_TYPE_INT16: u32 = 3;
const GGUF_TYPE_UINT32: u32 = 4;
const GGUF_TYPE_INT32: u32 = 5;
const GGUF_TYPE_FLOAT32: u32 = 6;
const GGUF_TYPE_BOOL: u32 = 7;
const GGUF_TYPE_STRING: u32 = 8;
const GGUF_TYPE_ARRAY: u32 = 9;
const GGUF_TYPE_UINT64: u32 = 10;
const GGUF_TYPE_INT64: u32 = 11;
const GGUF_TYPE_FLOAT64: u32 = 12;

const KEY_ARCHITECTURE: &str = "general.architecture";
const KEY_FILE_TYPE: &str = "general.file_type";

/// Parsed view of a GGUF header. Only the fields modeltap renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GgufHeader {
    /// e.g. "llama", "mistral", "qwen2". `None` if the header parses but no
    /// `general.architecture` key was found.
    pub architecture: Option<String>,
    /// Human-readable quantization label, e.g. "Q4_K_M", "Q4_0", "F16".
    /// Derived from `general.file_type` (uint32). `None` if absent.
    pub quantization: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("gguf header is corrupt or truncated")]
    Corrupt,
    #[error("gguf magic mismatch: expected GGUF")]
    BadMagic,
    #[error("gguf version unsupported: {0}")]
    UnsupportedVersion(u32),
}

/// Parse a GGUF header from a byte slice. Returns `Ok(GgufHeader)` for
/// well-formed input and `Err(ParseError)` for any malformation. NEVER panics.
pub fn parse_header(bytes: &[u8]) -> Result<GgufHeader, ParseError> {
    let mut cur = Cursor::new(bytes);
    let magic = cur.read_n(4)?;
    if magic != MAGIC.as_slice() {
        return Err(ParseError::BadMagic);
    }
    let version = cur.read_u32()?;
    // Be permissive on version: we know v1, v2, v3. Reject obvious garbage
    // (>=100) so a corrupt stream of FF FF FF FF doesn't get parsed as
    // u32::MAX kv entries.
    if !(1..=10).contains(&version) {
        return Err(ParseError::UnsupportedVersion(version));
    }
    let _tensor_count = cur.read_u64()?;
    let kv_count = cur.read_u64()?;
    // Bound kv_count so a corrupt header claiming kv_count = u64::MAX doesn't
    // blow up our loop. Real GGUF files have well under 1000 kv entries.
    if kv_count > 100_000 {
        return Err(ParseError::Corrupt);
    }

    let mut architecture: Option<String> = None;
    let mut file_type: Option<u32> = None;

    for _ in 0..kv_count {
        let key = cur.read_string()?;
        let value_type = cur.read_u32()?;
        match key.as_str() {
            KEY_ARCHITECTURE if value_type == GGUF_TYPE_STRING => {
                architecture = Some(cur.read_string()?);
            }
            KEY_FILE_TYPE if value_type == GGUF_TYPE_UINT32 => {
                file_type = Some(cur.read_u32()?);
            }
            // Some GGUF files store file_type as INT32 (older converters).
            KEY_FILE_TYPE if value_type == GGUF_TYPE_INT32 => {
                let v = cur.read_u32()?; // bit pattern is the same for non-negative
                file_type = Some(v);
            }
            // Skip every other key/value. We only need 2 metadata fields.
            _ => skip_value(&mut cur, value_type)?,
        }
    }

    let quantization = file_type.map(file_type_label);
    Ok(GgufHeader {
        architecture,
        quantization,
    })
}

/// Map a GGUF `general.file_type` integer to its human-readable name.
/// Source: <https://github.com/ggerganov/ggml/blob/master/docs/gguf.md#enum-llama_ftype>
/// Unknown values become `"FT_<n>"` so callers always get SOMETHING displayable.
pub fn file_type_label(ft: u32) -> String {
    match ft {
        0 => "F32".to_string(),
        1 => "F16".to_string(),
        2 => "Q4_0".to_string(),
        3 => "Q4_1".to_string(),
        7 => "Q8_0".to_string(),
        8 => "Q5_0".to_string(),
        9 => "Q5_1".to_string(),
        10 => "Q2_K".to_string(),
        11 => "Q3_K_S".to_string(),
        12 => "Q3_K_M".to_string(),
        13 => "Q3_K_L".to_string(),
        14 => "Q4_K_S".to_string(),
        15 => "Q4_K_M".to_string(),
        16 => "Q5_K_S".to_string(),
        17 => "Q5_K_M".to_string(),
        18 => "Q6_K".to_string(),
        // GGUF spec reserves additional codes; surface the raw value rather
        // than panic / drop. The display layer treats this as a still-valid
        // model with a non-standard quant.
        other => format!("FT_{}", other),
    }
}

// ---------------------------------------------------------------------------
// Skipping unknown / uninteresting values.
// ---------------------------------------------------------------------------

fn skip_value(cur: &mut Cursor<'_>, value_type: u32) -> Result<(), ParseError> {
    match value_type {
        GGUF_TYPE_UINT8 | GGUF_TYPE_INT8 | GGUF_TYPE_BOOL => cur.skip(1),
        GGUF_TYPE_UINT16 | GGUF_TYPE_INT16 => cur.skip(2),
        GGUF_TYPE_UINT32 | GGUF_TYPE_INT32 | GGUF_TYPE_FLOAT32 => cur.skip(4),
        GGUF_TYPE_UINT64 | GGUF_TYPE_INT64 | GGUF_TYPE_FLOAT64 => cur.skip(8),
        GGUF_TYPE_STRING => {
            let _ = cur.read_string()?;
            Ok(())
        }
        GGUF_TYPE_ARRAY => {
            let inner_type = cur.read_u32()?;
            let len = cur.read_u64()?;
            // Bound to avoid u64::MAX loops on corrupt input.
            if len > 10_000_000 {
                return Err(ParseError::Corrupt);
            }
            for _ in 0..len {
                skip_value(cur, inner_type)?;
            }
            Ok(())
        }
        // Unknown type tag — we cannot determine its width. Fail rather than
        // mis-skip and corrupt the parser state.
        _ => Err(ParseError::Corrupt),
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

    fn read_n(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        let end = self.pos.checked_add(n).ok_or(ParseError::Corrupt)?;
        if end > self.buf.len() {
            return Err(ParseError::Corrupt);
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn skip(&mut self, n: usize) -> Result<(), ParseError> {
        let end = self.pos.checked_add(n).ok_or(ParseError::Corrupt)?;
        if end > self.buf.len() {
            return Err(ParseError::Corrupt);
        }
        self.pos = end;
        Ok(())
    }

    fn read_u32(&mut self) -> Result<u32, ParseError> {
        let b = self.read_n(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_u64(&mut self) -> Result<u64, ParseError> {
        let b = self.read_n(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn read_string(&mut self) -> Result<String, ParseError> {
        let len = self.read_u64()?;
        // Bound string length: real GGUF keys / arch values are tiny.
        if len > 1_000_000 {
            return Err(ParseError::Corrupt);
        }
        let bytes = self.read_n(len as usize)?;
        // GGUF strings are UTF-8 by spec but we accept anything renderable
        // via `from_utf8_lossy`. The parser MUST NOT panic on bad UTF-8.
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid GGUF header in-memory — same shape as our
    /// fixture builder's `write_gguf` helper.
    fn build_header(arch: &str, file_type: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"GGUF");
        out.extend_from_slice(&3u32.to_le_bytes()); // version
        out.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        out.extend_from_slice(&2u64.to_le_bytes()); // kv_count
                                                    // KV[0]: general.architecture -> string(arch)
        let key = b"general.architecture";
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&GGUF_TYPE_STRING.to_le_bytes());
        out.extend_from_slice(&(arch.len() as u64).to_le_bytes());
        out.extend_from_slice(arch.as_bytes());
        // KV[1]: general.file_type -> uint32(file_type)
        let key = b"general.file_type";
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&GGUF_TYPE_UINT32.to_le_bytes());
        out.extend_from_slice(&file_type.to_le_bytes());
        out
    }

    #[test]
    fn valid_header_yields_arch_and_quant_label() {
        let bytes = build_header("llama", 15);
        let h = parse_header(&bytes).expect("must parse");
        assert_eq!(h.architecture.as_deref(), Some("llama"));
        assert_eq!(h.quantization.as_deref(), Some("Q4_K_M"));
    }

    #[test]
    fn truncated_header_returns_corrupt_not_panic() {
        let full = build_header("mistral", 2);
        // Truncate to half — must yield Err(Corrupt), never panic.
        for cut in 0..full.len() {
            let res = parse_header(&full[..cut]);
            assert!(
                res.is_err(),
                "truncated to {} bytes should error, got {:?}",
                cut,
                res
            );
        }
    }

    #[test]
    fn bad_magic_returns_bad_magic() {
        let bad = b"XXXX\x00\x00\x00\x01";
        let err = parse_header(bad).expect_err("bad magic must error");
        assert_eq!(err, ParseError::BadMagic);
    }

    #[test]
    fn empty_input_returns_corrupt() {
        let err = parse_header(&[]).expect_err("empty must error");
        assert_eq!(err, ParseError::Corrupt);
    }

    #[test]
    fn unknown_file_type_falls_back_to_ft_n_label() {
        let bytes = build_header("custom", 9999);
        let h = parse_header(&bytes).expect("parse");
        assert_eq!(h.quantization.as_deref(), Some("FT_9999"));
    }

    /// Hand-rolled "fuzz": 100 random byte vectors of varying lengths.
    /// Parser MUST NOT panic on any of them (regardless of whether it
    /// returns Ok or Err).
    #[test]
    fn random_bytes_never_panic() {
        // Linear congruential pseudo-random for determinism (no rand dep).
        // Same seed every run so a regression is reproducible.
        let mut seed: u64 = 0xDEAD_BEEF_CAFE_F00D;
        for _ in 0..100 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = ((seed >> 32) as usize) % 4096; // 0..4096 bytes
            let mut buf = vec![0u8; len];
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (seed.wrapping_add(i as u64) & 0xFF) as u8;
            }
            // Attempt to parse. The result is irrelevant; the assertion is
            // that we get back a Result without a panic / abort.
            let _ = parse_header(&buf);
        }
    }

    #[test]
    fn random_bytes_with_valid_magic_never_panic() {
        // Many corrupt files start "GGUF" by accident or because the user
        // truncated mid-download. Specifically test those.
        let mut seed: u64 = 0x1234_5678_ABCD_EF00;
        for _ in 0..100 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = (((seed >> 32) as usize) % 4092) + 4; // >=4 so we can splat magic
            let mut buf = vec![0u8; len];
            for (i, b) in buf.iter_mut().enumerate() {
                *b = (seed.wrapping_add(i as u64) & 0xFF) as u8;
            }
            buf[..4].copy_from_slice(b"GGUF");
            let _ = parse_header(&buf);
        }
    }

    #[test]
    fn header_with_intervening_unknown_kvs_still_extracts_arch_and_ft() {
        // Build a header with: arch (str), some_int (uint32), more_str (str), file_type (u32).
        let mut out = Vec::new();
        out.extend_from_slice(b"GGUF");
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&4u64.to_le_bytes()); // 4 KVs

        // KV[0]: arch.
        let key = b"general.architecture";
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&GGUF_TYPE_STRING.to_le_bytes());
        let v = b"llama";
        out.extend_from_slice(&(v.len() as u64).to_le_bytes());
        out.extend_from_slice(v);

        // KV[1]: some unknown uint32.
        let key = b"llama.context_length";
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&GGUF_TYPE_UINT32.to_le_bytes());
        out.extend_from_slice(&8192u32.to_le_bytes());

        // KV[2]: some unknown string.
        let key = b"general.name";
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&GGUF_TYPE_STRING.to_le_bytes());
        let v = b"Llama-3-8B-Instruct";
        out.extend_from_slice(&(v.len() as u64).to_le_bytes());
        out.extend_from_slice(v);

        // KV[3]: file_type.
        let key = b"general.file_type";
        out.extend_from_slice(&(key.len() as u64).to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&GGUF_TYPE_UINT32.to_le_bytes());
        out.extend_from_slice(&15u32.to_le_bytes());

        let h = parse_header(&out).expect("parse");
        assert_eq!(h.architecture.as_deref(), Some("llama"));
        assert_eq!(h.quantization.as_deref(), Some("Q4_K_M"));
    }
}
