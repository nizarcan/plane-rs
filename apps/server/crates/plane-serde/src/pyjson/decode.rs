// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Charset decoding for a request body, matching DRF's `JSONParser`.
//!
//! `rest_framework/parsers.py:84-91` decodes the body with
//! `codecs.getreader(encoding)(stream)` and then calls `json.load`. The
//! reader is a `codecs.StreamReader` with `errors='strict'`, which decodes the
//! whole body in one incremental step (`codecs.py:454-532`, `final=False`).
//! Two consequences shape this module:
//!
//! * A `UnicodeDecodeError` message is `'<codec>' codec can't decode byte
//!   0xXX in position N: <reason>`, or `... bytes in position N-M ...` when
//!   more than one byte is involved, and DRF prefixes the whole thing with
//!   `JSON parse error - `.
//! * An incomplete multi-byte sequence at the *end* of the body is silently
//!   dropped: the incremental decoder returns it as pending and the reader
//!   never passes `final=True`. `b'"\xc3'` therefore reaches `json.loads` as
//!   `'"'` and reports `Unterminated string starting at`, not a decode error.
//!   `b'123\xc3'` parses as `123`. (Measured; `codecs.StreamReader.read`.)
//!
//! Codecs Python knows but this module does not (cp437, koi8-r, the other
//! ISO-8859 parts, ...) are reported as an unsupported charset. Plane clients
//! send UTF-8, and every charset DRF's own documentation mentions (utf-8,
//! latin-1, ascii, utf-16, utf-32) is implemented.

use std::borrow::Cow;

use super::LoadError;

/// Result of decoding a body.
pub(crate) type Decoded<'a> = Cow<'a, str>;

/// The codecs this module implements, as `codecs.lookup()` resolves them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Codec {
    Utf8,
    Utf8Sig,
    Ascii,
    Latin1,
    Cp1252,
    Utf16,
    Utf16Le,
    Utf16Be,
    Utf32,
    Utf32Le,
    Utf32Be,
}

/// `codecs.lookup` without the codec tables: normalise the name the way
/// `encodings.normalize_encoding` does (lowercase, runs of punctuation become
/// one `_`) and match CPython 3.12's `encodings/aliases.py` entries for the
/// supported codecs.
fn lookup(encoding: &str) -> Option<Codec> {
    let mut normalised = String::with_capacity(encoding.len());
    let mut punctuation = false;
    for byte in encoding.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' {
            if punctuation && !normalised.is_empty() {
                normalised.push('_');
            }
            normalised.push(byte.to_ascii_lowercase() as char);
            punctuation = false;
        } else {
            punctuation = true;
        }
    }
    // `.` and `-` are separators too, except inside `iso_646.irv_1991`.
    let name = normalised.replace(['-', '.'], "_");
    Some(match name.as_str() {
        "utf_8" | "utf8" | "u8" | "utf" | "utf8_ucs2" | "utf8_ucs4" | "cp65001" => Codec::Utf8,
        "utf_8_sig" => Codec::Utf8Sig,
        "ascii" | "646" | "ansi_x3_4_1968" | "ansi_x3_4_1986" | "cp367" | "csascii" | "ibm367"
        | "iso646_us" | "iso_646_irv_1991" | "iso_ir_6" | "us" | "us_ascii" => Codec::Ascii,
        "latin_1" | "8859" | "cp819" | "csisolatin1" | "ibm819" | "iso8859" | "iso8859_1"
        | "iso_8859_1" | "iso_8859_1_1987" | "iso_ir_100" | "l1" | "latin" | "latin1" => {
            Codec::Latin1
        }
        "cp1252" | "1252" | "windows_1252" => Codec::Cp1252,
        "utf_16" | "u16" | "utf16" => Codec::Utf16,
        "utf_16_le" | "utf_16le" | "unicodelittleunmarked" => Codec::Utf16Le,
        "utf_16_be" | "utf_16be" | "unicodebigunmarked" => Codec::Utf16Be,
        "utf_32" | "u32" | "utf32" => Codec::Utf32,
        "utf_32_le" | "utf_32le" => Codec::Utf32Le,
        "utf_32_be" | "utf_32be" => Codec::Utf32Be,
        _ => return None,
    })
}

/// Decode a request body with the charset DRF resolved from the
/// `Content-Type` header (`settings.DEFAULT_CHARSET` when absent).
pub(crate) fn decode<'a>(bytes: &'a [u8], encoding: &str) -> Result<Decoded<'a>, LoadError> {
    let Some(codec) = lookup(encoding) else {
        return Err(LoadError::charset(encoding));
    };
    match codec {
        Codec::Utf8 => decode_utf8(bytes).map_err(LoadFailure::into_load_error),
        Codec::Utf8Sig => {
            let data = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
            decode_utf8(data).map_err(LoadFailure::into_load_error)
        }
        Codec::Ascii => decode_ascii(bytes).map_err(LoadFailure::into_load_error),
        Codec::Latin1 => Ok(Cow::Owned(decode_latin1(bytes))),
        Codec::Cp1252 => decode_cp1252(bytes).map_err(LoadFailure::into_load_error),
        Codec::Utf16 => {
            decode_utf16(bytes, Utf16Endian::Generic).map_err(LoadFailure::into_load_error)
        }
        Codec::Utf16Le => {
            decode_utf16(bytes, Utf16Endian::Little).map_err(LoadFailure::into_load_error)
        }
        Codec::Utf16Be => {
            decode_utf16(bytes, Utf16Endian::Big).map_err(LoadFailure::into_load_error)
        }
        Codec::Utf32 => {
            decode_utf32(bytes, Utf32Endian::Generic).map_err(LoadFailure::into_load_error)
        }
        Codec::Utf32Le => {
            decode_utf32(bytes, Utf32Endian::Little).map_err(LoadFailure::into_load_error)
        }
        Codec::Utf32Be => {
            decode_utf32(bytes, Utf32Endian::Big).map_err(LoadFailure::into_load_error)
        }
    }
}

/// A decoding failure: either a `UnicodeDecodeError` (which has a position)
/// or a plain `UnicodeError` such as the missing-BOM message.
enum LoadFailure {
    DecodeError {
        codec: &'static str,
        byte: u8,
        start: usize,
        end: usize,
        reason: &'static str,
    },
    Message(&'static str),
}

impl LoadFailure {
    fn into_load_error(self) -> LoadError {
        match self {
            LoadFailure::DecodeError {
                codec,
                byte,
                start,
                end,
                reason,
            } => LoadError::decode(if end == start + 1 {
                format!(
                    "'{codec}' codec can't decode byte 0x{byte:02x} in position {start}: {reason}"
                )
            } else {
                format!(
                    "'{codec}' codec can't decode bytes in position {start}-{}: {reason}",
                    end - 1
                )
            }),
            LoadFailure::Message(message) => LoadError::decode(message.to_owned()),
        }
    }
}

// ---------------------------------------------------------------------------
// UTF-8
// ---------------------------------------------------------------------------

/// Decode UTF-8 the way CPython's `utf-8` codec does with `final=False`.
fn decode_utf8(bytes: &[u8]) -> Result<Decoded<'_>, LoadFailure> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(Cow::Borrowed(text));
    }
    // Find the first real error, or the length of the valid prefix when the
    // only problem is an incomplete sequence at the end of the input.
    let prefix = scan_utf8(bytes)?;
    match std::str::from_utf8(&bytes[..prefix]) {
        Ok(text) => Ok(Cow::Borrowed(text)),
        // The scan validated the prefix, so this cannot happen.
        Err(_) => Err(LoadFailure::DecodeError {
            codec: "utf-8",
            byte: *bytes.get(prefix).unwrap_or(&0),
            start: prefix,
            end: prefix + 1,
            reason: "invalid start byte",
        }),
    }
}

fn scan_utf8(bytes: &[u8]) -> Result<usize, LoadFailure> {
    let mut index = 0;
    while index < bytes.len() {
        let first = bytes[index];
        if first < 0x80 {
            index += 1;
            continue;
        }
        // `Modules/_json.c`'s scanner accepts the same byte sequences as
        // CPython's UTF-8 decoder; the restricted ranges below are the ones
        // that exclude overlong forms, surrogates and code points above
        // U+10FFFF (`Objects/stringlib/codecs.h`, `utf8_decode`).
        // unicodeobject.c:4906-4919: a *truncated* surrogate sequence
        // (ED A0..BF at the very end) is held as pending by the incremental
        // decoder instead of raising, and StreamReader then drops it.
        if first == 0xED && index + 2 == bytes.len() && (0xA0..=0xBF).contains(&bytes[index + 1]) {
            return Ok(index);
        }
        let (continuations, second_low, second_high) = match first {
            0xC2..=0xDF => (1, 0x80, 0xBF),
            0xE0 => (2, 0xA0, 0xBF),
            0xE1..=0xEC => (2, 0x80, 0xBF),
            0xED => (2, 0x80, 0x9F),
            0xEE..=0xEF => (2, 0x80, 0xBF),
            0xF0 => (3, 0x90, 0xBF),
            0xF1..=0xF3 => (3, 0x80, 0xBF),
            0xF4 => (3, 0x80, 0x8F),
            _ => {
                return Err(LoadFailure::DecodeError {
                    codec: "utf-8",
                    byte: first,
                    start: index,
                    end: index + 1,
                    reason: "invalid start byte",
                });
            }
        };
        for offset in 1..=continuations {
            let Some(&byte) = bytes.get(index + offset) else {
                // Incomplete at end of input: the incremental decoder keeps
                // it pending and `StreamReader` drops it.
                return Ok(index);
            };
            let (low, high) = if offset == 1 {
                (second_low, second_high)
            } else {
                (0x80, 0xBF)
            };
            if byte < low || byte > high {
                return Err(LoadFailure::DecodeError {
                    codec: "utf-8",
                    byte: first,
                    start: index,
                    end: index + offset,
                    reason: "invalid continuation byte",
                });
            }
        }
        index += continuations + 1;
    }
    Ok(index)
}

// ---------------------------------------------------------------------------
// Single-byte codecs
// ---------------------------------------------------------------------------

fn decode_ascii(bytes: &[u8]) -> Result<Decoded<'_>, LoadFailure> {
    match bytes.iter().position(|&byte| byte >= 0x80) {
        None => Ok(Cow::Borrowed(
            // Every byte is ASCII.
            std::str::from_utf8(bytes).unwrap_or_default(),
        )),
        Some(index) => Err(LoadFailure::DecodeError {
            codec: "ascii",
            byte: bytes[index],
            start: index,
            end: index + 1,
            reason: "ordinal not in range(128)",
        }),
    }
}

fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

/// cp1252's 0x80..=0x9F block; the five unassigned bytes are `None`.
/// `encodings/cp1252.py` in the pinned CPython.
const CP1252: [Option<char>; 32] = [
    Some('\u{20AC}'),
    None,
    Some('\u{201A}'),
    Some('\u{0192}'),
    Some('\u{201E}'),
    Some('\u{2026}'),
    Some('\u{2020}'),
    Some('\u{2021}'),
    Some('\u{02C6}'),
    Some('\u{2030}'),
    Some('\u{0160}'),
    Some('\u{2039}'),
    Some('\u{0152}'),
    None,
    Some('\u{017D}'),
    None,
    None,
    Some('\u{2018}'),
    Some('\u{2019}'),
    Some('\u{201C}'),
    Some('\u{201D}'),
    Some('\u{2022}'),
    Some('\u{2013}'),
    Some('\u{2014}'),
    Some('\u{02DC}'),
    Some('\u{2122}'),
    Some('\u{0161}'),
    Some('\u{203A}'),
    Some('\u{0153}'),
    None,
    Some('\u{017E}'),
    Some('\u{0178}'),
];

fn decode_cp1252(bytes: &[u8]) -> Result<Decoded<'_>, LoadFailure> {
    let mut out = String::with_capacity(bytes.len());
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            0x80..=0x9F => match CP1252[usize::from(byte - 0x80)] {
                Some(ch) => out.push(ch),
                None => {
                    return Err(LoadFailure::DecodeError {
                        codec: "charmap",
                        byte,
                        start: index,
                        end: index + 1,
                        reason: "character maps to <undefined>",
                    });
                }
            },
            _ => out.push(char::from(byte)),
        }
    }
    Ok(Cow::Owned(out))
}

// ---------------------------------------------------------------------------
// UTF-16 and UTF-32
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Utf16Endian {
    Generic,
    Little,
    Big,
}

fn decode_utf16(bytes: &[u8], endian: Utf16Endian) -> Result<Decoded<'_>, LoadFailure> {
    // encodings/utf_16.py's incremental decoder decodes with the native byte
    // order first and only *then* complains about a missing BOM ("consumed >=
    // 2"), so a decode error outranks the BOM message. The pinned corpus is
    // generated on a little-endian host, which is what the native order is
    // here.
    let (little, mut index, generic) = match endian {
        Utf16Endian::Little => (true, 0, false),
        Utf16Endian::Big => (false, 0, false),
        Utf16Endian::Generic => {
            if bytes.len() < 2 {
                // One byte, or nothing: pending and dropped.
                return Ok(Cow::Owned(String::new()));
            }
            match bytes[..2] {
                [0xFF, 0xFE] => (true, 2, false),
                [0xFE, 0xFF] => (false, 2, false),
                _ => (true, 0, true),
            }
        }
    };
    let codec = if little { "utf-16-le" } else { "utf-16-be" };
    let mut out = String::with_capacity(bytes.len() / 2);
    // Bytes the incremental decoder consumed: a pending unit is not counted,
    // so `\x00\xd8` (a lone high surrogate at the end) consumes nothing and
    // still decodes to "".
    let mut consumed = index;
    while index + 1 < bytes.len() {
        let first = read_u16(bytes, index, little);
        let next = index + 2;
        if (0xD800..=0xDBFF).contains(&first) {
            if next + 1 >= bytes.len() {
                // A high surrogate as the last unit stays pending and is
                // dropped (measured: b'A\\x00\\x00\\xd8' decodes to "A").
                break;
            }
            let second = read_u16(bytes, next, little);
            if !(0xDC00..=0xDFFF).contains(&second) {
                return Err(LoadFailure::DecodeError {
                    codec,
                    byte: bytes[index],
                    start: index,
                    end: next,
                    reason: "illegal UTF-16 surrogate",
                });
            }
            index = next + 2;
            consumed = index;
            let code_point =
                0x1_0000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00);
            out.push(char::from_u32(code_point).unwrap_or('\u{FFFD}'));
        } else if (0xDC00..=0xDFFF).contains(&first) {
            return Err(LoadFailure::DecodeError {
                codec,
                byte: bytes[index],
                start: index,
                end: next,
                reason: "illegal encoding",
            });
        } else {
            index = next;
            consumed = index;
            out.push(char::from_u32(u32::from(first)).unwrap_or('\u{FFFD}'));
        }
    }
    if generic && consumed >= 2 {
        return Err(LoadFailure::Message(
            "UTF-16 stream does not start with BOM",
        ));
    }
    Ok(Cow::Owned(out))
}

fn read_u16(bytes: &[u8], index: usize, little: bool) -> u16 {
    let pair = [bytes[index], bytes[index + 1]];
    if little {
        u16::from_le_bytes(pair)
    } else {
        u16::from_be_bytes(pair)
    }
}

#[derive(Clone, Copy)]
enum Utf32Endian {
    Generic,
    Little,
    Big,
}

fn decode_utf32(bytes: &[u8], endian: Utf32Endian) -> Result<Decoded<'_>, LoadFailure> {
    // Same "decode, then complain about the BOM" order as UTF-16;
    // encodings/utf_32.py checks `consumed >= 4` after `utf_32_ex_decode`.
    let (little, mut index, generic) = match endian {
        Utf32Endian::Little => (true, 0, false),
        Utf32Endian::Big => (false, 0, false),
        Utf32Endian::Generic => {
            if bytes.len() < 4 {
                return Ok(Cow::Owned(String::new()));
            }
            match bytes[..4] {
                [0xFF, 0xFE, 0x00, 0x00] => (true, 4, false),
                [0x00, 0x00, 0xFE, 0xFF] => (false, 4, false),
                _ => (true, 0, true),
            }
        }
    };
    let codec = if little { "utf-32-le" } else { "utf-32-be" };
    let mut out = String::with_capacity(bytes.len() / 4);
    while index + 3 < bytes.len() {
        let unit = [
            bytes[index],
            bytes[index + 1],
            bytes[index + 2],
            bytes[index + 3],
        ];
        let value = if little {
            u32::from_le_bytes(unit)
        } else {
            u32::from_be_bytes(unit)
        };
        if value > 0x10_FFFF {
            return Err(LoadFailure::DecodeError {
                codec,
                byte: unit[0],
                start: index,
                end: index + 4,
                reason: "code point not in range(0x110000)",
            });
        }
        match char::from_u32(value) {
            Some(ch) => out.push(ch),
            None => {
                return Err(LoadFailure::DecodeError {
                    codec,
                    byte: unit[0],
                    start: index,
                    end: index + 4,
                    reason: "code point in surrogate code point range(0xd800, 0xe000)",
                });
            }
        }
        index += 4;
    }
    if generic && index >= 4 {
        return Err(LoadFailure::Message(
            "UTF-32 stream does not start with BOM",
        ));
    }
    Ok(Cow::Owned(out))
}

#[cfg(test)]
mod tests {
    use super::decode;
    use crate::pyjson::LoadErrorKind;

    fn message(bytes: &[u8], encoding: &str) -> String {
        match decode(bytes, encoding) {
            Ok(text) => format!("OK {text:?}"),
            Err(error) => {
                assert_ne!(error.kind(), LoadErrorKind::Json);
                error.message().to_owned()
            }
        }
    }

    #[test]
    fn utf8_error_messages_match_cpython() {
        assert_eq!(
            message(b"\xff", "utf-8"),
            "'utf-8' codec can't decode byte 0xff in position 0: invalid start byte"
        );
        assert_eq!(
            message(b"ab\x80", "UTF8"),
            "'utf-8' codec can't decode byte 0x80 in position 2: invalid start byte"
        );
        assert_eq!(
            message(b"\xc2A", "utf-8"),
            "'utf-8' codec can't decode byte 0xc2 in position 0: invalid continuation byte"
        );
        assert_eq!(
            message(b"\"\xe2\x82\"", "utf-8"),
            "'utf-8' codec can't decode bytes in position 1-2: invalid continuation byte"
        );
        assert_eq!(
            message(b"\xed\xa0\x80", "utf-8"),
            "'utf-8' codec can't decode byte 0xed in position 0: invalid continuation byte"
        );
    }

    #[test]
    fn utf8_drops_an_incomplete_tail() {
        // codecs.StreamReader never passes final=True, so the pending bytes
        // vanish instead of raising "unexpected end of data".
        assert_eq!(message(b"123\xc3", "utf-8"), "OK \"123\"");
        assert_eq!(message(b"\"\xf0\x9f\x98", "utf-8"), "OK \"\\\"\"");
        assert_eq!(message(b"\xc2", "utf-8"), "OK \"\"");
    }

    #[test]
    fn single_byte_codecs() {
        assert_eq!(message(b"caf\xe9", "latin-1"), "OK \"café\"");
        assert_eq!(
            message(b"\x81", "cp1252"),
            "'charmap' codec can't decode byte 0x81 in position 0: character maps to <undefined>"
        );
        assert_eq!(
            message(b"a\x80", "us-ascii"),
            "'ascii' codec can't decode byte 0x80 in position 1: ordinal not in range(128)"
        );
        assert_eq!(message(b"\x80", "cp1252"), "OK \"€\"");
    }

    #[test]
    fn utf16_and_utf32() {
        assert_eq!(
            message(b"{}", "utf-16"),
            "UTF-16 stream does not start with BOM"
        );
        assert_eq!(message(b"\xff\xfe{\x00}\x00", "utf-16"), "OK \"{}\"");
        assert_eq!(
            message(b"\x00\xd8A\x00", "utf-16-le"),
            "'utf-16-le' codec can't decode bytes in position 0-1: illegal UTF-16 surrogate"
        );
        assert_eq!(message(b"A\x00\x00\xd8", "utf-16-le"), "OK \"A\"");
        assert_eq!(
            message(b"\x00\x00\xd8\x00", "utf-32-be"),
            "'utf-32-be' codec can't decode bytes in position 0-3: code point in surrogate code point range(0xd800, 0xe000)"
        );
    }

    #[test]
    fn utf8_sig_strips_one_bom() {
        assert_eq!(message(b"\xef\xbb\xbf{}", "utf-8-sig"), "OK \"{}\"");
        assert_eq!(
            message(b"\xef\xbb\xbf\xff", "utf-8-sig"),
            "'utf-8' codec can't decode byte 0xff in position 0: invalid start byte"
        );
    }

    #[test]
    fn unknown_codecs_are_reported_the_way_drf_does() {
        assert_eq!(
            message(b"{}", "bz2"),
            "Unsupported charset \"bz2\" in request Content-Type header."
        );
    }
}
