//! Encoding detection + decoding — Phase 3 replacement for the `ced` (detect)
//! + `iconv-lite` (decode) native deps used in `src/main/filesystem/encoding.ts`
//! and `markdown.ts`.
//!
//! Uses `chardetng` (the detector behind Firefox) for guessing and
//! `encoding_rs` for decoding, both pure-Rust. BOM detection mirrors
//! `guessEncoding`'s UTF-8 / UTF-16 LE/BE checks.
//!
//!   (new) → fs_read_text_auto   — read + auto-detect + decode to UTF-8
//!
//! The renderer's open flow (Phase 4) keeps the pure line-ending / trailing-
//! newline detection in JS; only the native-dep part lives here.

use chardetng::EncodingDetector;
use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadTextResult {
    /// File contents decoded to UTF-8 (BOM stripped).
    content: String,
    /// Canonical name of the detected encoding (e.g. "UTF-8", "Shift_JIS").
    encoding: String,
    /// Whether a byte-order mark was present (so the save path can re-add it).
    is_bom: bool,
}

/// Pick an encoding for `bytes`: BOM first, then chardetng when `auto_guess`,
/// else UTF-8. Returns the chosen encoding and whether a BOM was seen.
pub fn detect(bytes: &[u8], auto_guess: bool) -> (&'static Encoding, bool) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (UTF_8, true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return (UTF_16BE, true);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return (UTF_16LE, true);
    }
    if auto_guess {
        let mut detector = EncodingDetector::new();
        detector.feed(bytes, true);
        (detector.guess(None, true), false)
    } else {
        (UTF_8, false)
    }
}

#[tauri::command]
pub fn fs_read_text_auto(path: String, auto_guess: bool) -> Result<ReadTextResult, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let (encoding, is_bom) = detect(&bytes, auto_guess);
    // `decode` sniffs/strips a BOM and yields a UTF-8 Cow.
    let (content, _enc_used, _had_errors) = encoding.decode(&bytes);
    Ok(ReadTextResult {
        content: content.into_owned(),
        encoding: encoding.name().to_string(),
        is_bom,
    })
}
