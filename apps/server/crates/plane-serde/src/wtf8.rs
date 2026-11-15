// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! WTF-8: UTF-8 that also permits lone surrogate code points.
//!
//! Python's `str` is a sequence of code points that may include the surrogate
//! range U+D800..=U+DFFF. `json.loads` produces such strings for a `\uXXXX`
//! escape that names a surrogate not followed by its partner
//! (`Lib/json/decoder.py` / `Modules/_json.c`, `scanstring_unicode`), and DRF
//! keeps them until the renderer tries to `.encode("utf-8")`.
//!
//! Rust's `String` cannot hold them, so [`crate::pyjson::PyStr`] stores WTF-8:
//! the standard UTF-8 encoding for every code point except surrogates, which
//! use the same three-byte form (`ED A0..BF 80..BF`) that UTF-8 reserves for
//! them. The encoding is injective for code point sequences, so byte equality
//! is Python string equality and a dict can hash the bytes.

/// Decode one code point of WTF-8 starting at byte `index`.
///
/// Returns the code point and its encoded length, or `None` if the bytes are
/// not WTF-8 (invalid lead, truncated sequence, bad continuation, overlong
/// form, or above U+10FFFF). Surrogates are accepted.
pub(crate) fn decode_at(bytes: &[u8], index: usize) -> Option<(u32, usize)> {
    let first = *bytes.get(index)?;
    if first < 0x80 {
        return Some((u32::from(first), 1));
    }
    let (continuations, mut code_point, minimum) = match first {
        0xC2..=0xDF => (1, u32::from(first & 0x1F), 0x80),
        0xE0..=0xEF => (2, u32::from(first & 0x0F), 0x800),
        0xF0..=0xF4 => (3, u32::from(first & 0x07), 0x1_0000),
        // 0x80..=0xC1 are continuation bytes or overlong leads, 0xF5..=0xFF
        // name code points above U+10FFFF.
        _ => return None,
    };
    for offset in 1..=continuations {
        let byte = *bytes.get(index + offset)?;
        if byte & 0xC0 != 0x80 {
            return None;
        }
        code_point = (code_point << 6) | u32::from(byte & 0x3F);
    }
    if code_point < minimum || code_point > 0x10FFFF {
        return None;
    }
    Some((code_point, continuations + 1))
}

/// Whether `bytes` is a complete WTF-8 sequence.
pub(crate) fn is_valid(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        match decode_at(bytes, index) {
            Some((_, length)) => index += length,
            None => return false,
        }
    }
    true
}

/// Encode one code point as WTF-8, returning its length. `buffer` must hold
/// four bytes; surrogates take three.
pub(crate) fn encode(code_point: u32, buffer: &mut [u8; 4]) -> usize {
    if code_point < 0x80 {
        buffer[0] = code_point as u8;
        1
    } else if code_point < 0x800 {
        buffer[0] = 0xC0 | (code_point >> 6) as u8;
        buffer[1] = 0x80 | (code_point & 0x3F) as u8;
        2
    } else if code_point < 0x1_0000 {
        buffer[0] = 0xE0 | (code_point >> 12) as u8;
        buffer[1] = 0x80 | ((code_point >> 6) & 0x3F) as u8;
        buffer[2] = 0x80 | (code_point & 0x3F) as u8;
        3
    } else {
        buffer[0] = 0xF0 | (code_point >> 18) as u8;
        buffer[1] = 0x80 | ((code_point >> 12) & 0x3F) as u8;
        buffer[2] = 0x80 | ((code_point >> 6) & 0x3F) as u8;
        buffer[3] = 0x80 | (code_point & 0x3F) as u8;
        4
    }
}

/// Iterator over the code points of a WTF-8 string.
#[derive(Clone, Debug)]
pub(crate) struct Chars<'a> {
    bytes: &'a [u8],
    index: usize,
}

impl<'a> Chars<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, index: 0 }
    }
}

impl Iterator for Chars<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let (code_point, length) = decode_at(self.bytes, self.index)?;
        self.index += length;
        Some(code_point)
    }
}

#[cfg(test)]
mod tests {
    use super::{Chars, encode, is_valid};

    fn to_wtf8(code_points: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        for &code_point in code_points {
            let mut buffer = [0u8; 4];
            let length = encode(code_point, &mut buffer);
            out.extend_from_slice(&buffer[..length]);
        }
        out
    }

    #[test]
    fn round_trips_surrogates_and_astral_characters() {
        let code_points = [
            0x0, 0x41, 0x7F, 0x80, 0x7FF, 0x800, 0xD7FF, 0xD800, 0xDBFF, 0xDC00, 0xDFFF, 0xE000,
            0xFFFF, 0x1_0000, 0x10_FFFF,
        ];
        let bytes = to_wtf8(&code_points);
        assert!(is_valid(&bytes));
        assert_eq!(Chars::new(&bytes).collect::<Vec<_>>(), code_points);
    }

    #[test]
    fn rejects_invalid_sequences() {
        for bytes in [
            &b"\x80"[..],
            b"\xC0\x80",
            b"\xC1\xBF",
            b"\xE0\x80\x80",
            b"\xED\xA0", // truncated surrogate
            b"\xF0\x80\x80\x80",
            b"\xF4\x90\x80\x80",
            b"\xF5\x80\x80\x80",
            b"\xFF",
            b"a\xC2",
        ] {
            assert!(!is_valid(bytes), "{bytes:?} should be invalid");
        }
    }

    #[test]
    fn astral_characters_match_utf8() {
        let mut buffer = [0u8; 4];
        let length = encode(0x1F680, &mut buffer);
        assert_eq!(&buffer[..length], "🚀".as_bytes());
    }
}
