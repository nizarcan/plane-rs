// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python's string escaping for each dialect.
//!
//! CPython's `json.encoder` escapes `"`, `\`, the short forms `\b \f \n \r
//! \t`, and every other C0 control as `\u00XX` (lowercase hex). With
//! `ensure_ascii=True` (Django, kombu) every character outside `0x20..=0x7E`
//! is escaped as well, including DEL; with `ensure_ascii=False` (DRF) only
//! U+2028 and U+2029 are escaped (the renderer does a textual replace after
//! encoding).

use std::io;

use crate::Dialect;

/// Write a string fragment with the dialect's escaping applied. A fragment
/// is a run that contains no character `write_escaped_contents` handles
/// individually; non-ASCII and DEL still need the dialect rules.
pub(crate) fn write_fragment<W>(dialect: Dialect, writer: &mut W, fragment: &str) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    if dialect == Dialect::Drf {
        return write_drf_fragment(writer, fragment);
    }
    write_ascii_fragment(writer, fragment)
}

fn write_drf_fragment<W>(writer: &mut W, fragment: &str) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    // DRF's renderer does
    //   ret.replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")
    // over the encoded document, so those two characters are the only
    // non-ASCII ones that need escaping when ensure_ascii=False.
    let bytes = fragment.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == 0xE2 && bytes[index + 1] == 0x80 {
            let escaped: Option<&[u8]> = match bytes[index + 2] {
                0xA8 => Some(b"\\u2028"),
                0xA9 => Some(b"\\u2029"),
                _ => None,
            };
            if let Some(escaped) = escaped {
                if start < index {
                    writer.write_all(&bytes[start..index])?;
                }
                writer.write_all(escaped)?;
                index += 3;
                start = index;
                continue;
            }
        }
        index += 1;
    }
    if start < bytes.len() {
        writer.write_all(&bytes[start..])?;
    }
    Ok(())
}

fn write_ascii_fragment<W>(writer: &mut W, fragment: &str) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    let bytes = fragment.as_bytes();
    let mut start = 0;
    for (index, ch) in fragment.char_indices() {
        let code = ch as u32;
        if (0x20..=0x7E).contains(&code) {
            continue;
        }
        if start < index {
            writer.write_all(&bytes[start..index])?;
        }
        write_unicode_escape(writer, ch)?;
        start = index + ch.len_utf8();
    }
    if start < bytes.len() {
        writer.write_all(&bytes[start..])?;
    }
    Ok(())
}

/// Write one decoded character with the dialect's escaping rules.
pub(crate) fn write_escaped_char<W>(dialect: Dialect, writer: &mut W, ch: char) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    let code = ch as u32;
    let escaped: &[u8] = match ch {
        '"' => b"\\\"",
        '\\' => b"\\\\",
        '\u{8}' => b"\\b",
        '\u{c}' => b"\\f",
        '\n' => b"\\n",
        '\r' => b"\\r",
        '\t' => b"\\t",
        _ => {
            if code < 0x20 || (dialect.ensure_ascii() && !(0x20..=0x7E).contains(&code)) {
                return write_unicode_escape(writer, ch);
            }
            if dialect == Dialect::Drf && (code == 0x2028 || code == 0x2029) {
                return write_unicode_escape(writer, ch);
            }
            let mut utf8 = [0u8; 4];
            return writer.write_all(ch.encode_utf8(&mut utf8).as_bytes());
        }
    };
    writer.write_all(escaped)
}

/// Write `\uXXXX`, or a UTF-16 surrogate pair for astral characters.
fn write_unicode_escape<W>(writer: &mut W, ch: char) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    let code = ch as u32;
    if code <= 0xFFFF {
        write_u16_escape(writer, code as u16)
    } else {
        let value = code - 0x1_0000;
        let high = 0xD800 | (value >> 10);
        let low = 0xDC00 | (value & 0x3FF);
        write_u16_escape(writer, high as u16)?;
        write_u16_escape(writer, low as u16)
    }
}

fn write_u16_escape<W>(writer: &mut W, value: u16) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = [
        b'\\',
        b'u',
        HEX[((value >> 12) & 0xF) as usize],
        HEX[((value >> 8) & 0xF) as usize],
        HEX[((value >> 4) & 0xF) as usize],
        HEX[(value & 0xF) as usize],
    ];
    writer.write_all(&bytes)
}

/// Per-byte classification for the string scanner:
/// 0 = printable ASCII that no dialect escapes, 1 = a character Python's
/// escaping rules handle individually, 2 = non-ASCII or DEL, which the
/// dialect-specific fragment writer must inspect.
static KIND: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut index = 0;
    while index < 0x20 {
        table[index] = 1;
        index += 1;
    }
    table[b'"' as usize] = 1;
    table[b'\\' as usize] = 1;
    table[0x7F] = 2;
    let mut index = 0x80;
    while index < 0x100 {
        table[index] = 2;
        index += 1;
    }
    table
};

/// Write the body of a JSON string (without the surrounding quotes),
/// escaping the characters Python's `json` module escapes. This is the hot
/// path for every string value and object key: ASCII runs are copied in one
/// call, and only runs containing non-ASCII or DEL are inspected for the
/// dialect's `ensure_ascii` / U+2028 rules.
pub(crate) fn write_escaped_contents<W>(
    dialect: Dialect,
    writer: &mut W,
    value: &str,
) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    let bytes = value.as_bytes();
    let mut start = 0;
    // 0: plain ASCII run, 2: the run needs the fragment writer.
    let mut run_kind = 0u8;
    for (index, &byte) in bytes.iter().enumerate() {
        match KIND[byte as usize] {
            0 => {}
            1 => {
                if start < index {
                    write_run(dialect, writer, &value[start..index], run_kind)?;
                }
                write_escaped_char(dialect, writer, char::from(byte))?;
                start = index + 1;
                run_kind = 0;
            }
            _ => run_kind = 2,
        }
    }
    if start < bytes.len() {
        write_run(dialect, writer, &value[start..], run_kind)?;
    }
    Ok(())
}

fn write_run<W>(dialect: Dialect, writer: &mut W, run: &str, run_kind: u8) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    if run_kind == 0 {
        // Every byte is printable ASCII, so no dialect escapes anything in it.
        writer.write_all(run.as_bytes())
    } else {
        write_fragment(dialect, writer, run)
    }
}

/// Write a complete JSON string, including the surrounding quotes, with the
/// dialect's escaping rules.
pub(crate) fn write_json_string<W>(dialect: Dialect, writer: &mut W, value: &str) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    writer.write_all(b"\"")?;
    write_escaped_contents(dialect, writer, value)?;
    writer.write_all(b"\"")
}
