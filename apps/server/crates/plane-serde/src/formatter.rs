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
//! encoding), and DEL is written raw.
//!
//! The writers also report how many *characters* of the output document they
//! produced. DRF's renderer ends with `ret.encode("utf-8")`, and a Python
//! `str` may contain a lone surrogate, which the encoder refuses with
//! `UnicodeEncodeError: 'utf-8' codec can't encode character '\ud800' in
//! position N: surrogates not allowed` where N is a character offset in the
//! whole document. Reproducing N exactly (and the plural/range form CPython
//! uses for a run of surrogates) needs that count.
//!
//! Strings are taken as WTF-8 bytes so the lone surrogates `json.loads` can
//! produce survive the trip.

use std::io;

use crate::Dialect;
use crate::wtf8;

/// A run of code points UTF-8 cannot encode (only produced for
/// [`Dialect::Drf`], whose encoder does not escape them).
#[derive(Debug)]
pub(crate) struct SurrogateError {
    /// The first code point of the run.
    pub(crate) code_point: u32,
    /// How many consecutive surrogate code points the run contains.
    pub(crate) run: usize,
    /// Character offset of the first one in the rendered document.
    pub(crate) position: usize,
}

impl SurrogateError {
    /// The exact `str(UnicodeEncodeError)` CPython produces.
    pub(crate) fn message(&self) -> String {
        let code_point = format!("\\u{:04x}", self.code_point);
        if self.run == 1 {
            format!(
                "'utf-8' codec can't encode character '{code_point}' in position {}: \
                 surrogates not allowed",
                self.position
            )
        } else {
            format!(
                "'utf-8' codec can't encode characters in position {}-{}: \
                 surrogates not allowed",
                self.position,
                self.position + self.run - 1
            )
        }
    }
}

/// A failure while writing a string.
#[derive(Debug)]
pub(crate) enum WriteError {
    Io(io::Error),
    Surrogate(SurrogateError),
}

impl WriteError {
    /// Map to the crate error type; a surrogate becomes the exact CPython
    /// message.
    pub(crate) fn into_crate_error(self) -> crate::Error {
        match self {
            WriteError::Io(error) => crate::Error::Io(error),
            WriteError::Surrogate(error) => crate::Error::Custom(error.message()),
        }
    }

    fn into_io(self) -> io::Error {
        match self {
            WriteError::Io(error) => error,
            // The transcode path rejects unpaired surrogates while parsing,
            // so it never reaches this.
            WriteError::Surrogate(error) => io::Error::other(error.message()),
        }
    }
}

impl From<io::Error> for WriteError {
    fn from(error: io::Error) -> Self {
        WriteError::Io(error)
    }
}

/// Write a complete JSON string, including the surrounding quotes.
pub(crate) fn write_json_string<W>(dialect: Dialect, writer: &mut W, value: &str) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    write_json_string_counted(dialect, writer, value, 0)
        .map(|_| ())
        .map_err(WriteError::into_io)
}

/// Write a complete JSON string and return the document's character count
/// after it.
pub(crate) fn write_json_string_counted<W>(
    dialect: Dialect,
    writer: &mut W,
    value: &str,
    chars: usize,
) -> Result<usize, WriteError>
where
    W: ?Sized + io::Write,
{
    write_json_wtf8_string_counted(dialect, writer, value.as_bytes(), chars)
}

/// Write a complete JSON string from WTF-8 bytes.
pub(crate) fn write_json_wtf8_string_counted<W>(
    dialect: Dialect,
    writer: &mut W,
    value: &[u8],
    chars: usize,
) -> Result<usize, WriteError>
where
    W: ?Sized + io::Write,
{
    writer.write_all(b"\"")?;
    let chars = write_wtf8_contents(dialect, writer, value, chars + 1)?;
    writer.write_all(b"\"")?;
    Ok(chars + 1)
}

/// Write the body of a JSON string (without quotes) and return the document's
/// character count after it.
pub(crate) fn write_escaped_contents_counted<W>(
    dialect: Dialect,
    writer: &mut W,
    value: &str,
    chars: usize,
) -> Result<usize, WriteError>
where
    W: ?Sized + io::Write,
{
    write_wtf8_contents(dialect, writer, value.as_bytes(), chars)
}

/// Write a run of text (no surrounding quotes) with the dialect's escaping.
pub(crate) fn write_fragment<W>(dialect: Dialect, writer: &mut W, fragment: &str) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    write_wtf8_contents(dialect, writer, fragment.as_bytes(), 0)
        .map(|_| ())
        .map_err(WriteError::into_io)
}

/// Write one decoded character with the dialect's escaping rules.
pub(crate) fn write_escaped_char<W>(dialect: Dialect, writer: &mut W, ch: char) -> io::Result<()>
where
    W: ?Sized + io::Write,
{
    let mut buffer = [0u8; 4];
    write_wtf8_contents(dialect, writer, ch.encode_utf8(&mut buffer).as_bytes(), 0)
        .map(|_| ())
        .map_err(WriteError::into_io)
}

/// The escaping core: `bytes` is WTF-8, `chars` is the number of output
/// characters written so far in the document.
fn write_wtf8_contents<W>(
    dialect: Dialect,
    writer: &mut W,
    bytes: &[u8],
    mut chars: usize,
) -> Result<usize, WriteError>
where
    W: ?Sized + io::Write,
{
    let mut index = 0;
    // Start of the pending ASCII run: printable ASCII that no dialect
    // escapes is copied in one `write_all`.
    let mut run_start = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if (0x20..0x7F).contains(&byte) && byte != b'"' && byte != b'\\' {
            index += 1;
            continue;
        }
        if run_start < index {
            writer.write_all(&bytes[run_start..index])?;
            chars += index - run_start;
        }
        if byte < 0x80 {
            chars += write_ascii(dialect, writer, byte)?;
            index += 1;
        } else {
            let Some((code_point, length)) = wtf8::decode_at(bytes, index) else {
                // Callers only pass validated WTF-8; treat a stray byte as
                // U+FFFD rather than panicking on a request path.
                writer.write_all("\u{FFFD}".as_bytes())?;
                chars += 1;
                index += 1;
                run_start = index;
                continue;
            };
            chars += write_non_ascii(dialect, writer, code_point, length, &bytes[index..], chars)?;
            index += length;
        }
        run_start = index;
    }
    if run_start < bytes.len() {
        writer.write_all(&bytes[run_start..])?;
        chars += bytes.len() - run_start;
    }
    Ok(chars)
}

/// Write one ASCII character that needs escaping, returning its output length.
fn write_ascii<W>(dialect: Dialect, writer: &mut W, byte: u8) -> Result<usize, WriteError>
where
    W: ?Sized + io::Write,
{
    let escaped: &[u8] = match byte {
        b'"' => b"\\\"",
        b'\\' => b"\\\\",
        0x08 => b"\\b",
        0x0C => b"\\f",
        b'\n' => b"\\n",
        b'\r' => b"\\r",
        b'\t' => b"\\t",
        0x7F if dialect == Dialect::Drf => {
            // ensure_ascii=False leaves DEL alone.
            writer.write_all(&[0x7F])?;
            return Ok(1);
        }
        _ => {
            write_u16_escape(writer, u16::from(byte))?;
            return Ok(6);
        }
    };
    writer.write_all(escaped)?;
    Ok(2)
}

/// Write one non-ASCII code point, returning its output length.
fn write_non_ascii<W>(
    dialect: Dialect,
    writer: &mut W,
    code_point: u32,
    length: usize,
    rest: &[u8],
    chars: usize,
) -> Result<usize, WriteError>
where
    W: ?Sized + io::Write,
{
    match dialect {
        Dialect::Drf => {
            if code_point == 0x2028 || code_point == 0x2029 {
                // rest[0..3] is the three-byte UTF-8 form.
                write_u16_escape(writer, code_point as u16)?;
                return Ok(6);
            }
            if (0xD800..=0xDFFF).contains(&code_point) {
                return Err(WriteError::Surrogate(SurrogateError {
                    code_point,
                    run: surrogate_run(rest),
                    position: chars,
                }));
            }
            writer.write_all(&rest[..length])?;
            Ok(1)
        }
        Dialect::Django | Dialect::Kombu => {
            if code_point <= 0xFFFF {
                write_u16_escape(writer, code_point as u16)?;
                return Ok(6);
            }
            let value = code_point - 0x1_0000;
            write_u16_escape(writer, (0xD800 | (value >> 10)) as u16)?;
            write_u16_escape(writer, (0xDC00 | (value & 0x3FF)) as u16)?;
            Ok(12)
        }
    }
}

/// How many consecutive surrogate code points start at `rest`.
fn surrogate_run(rest: &[u8]) -> usize {
    let mut run = 1;
    let mut offset = wtf8::decode_at(rest, 0).map_or(1, |(_, length)| length);
    while let Some((code_point, length)) = wtf8::decode_at(rest, offset) {
        if !(0xD800..=0xDFFF).contains(&code_point) {
            break;
        }
        run += 1;
        offset += length;
    }
    run
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

#[cfg(test)]
mod tests {
    use super::{WriteError, write_json_string_counted, write_json_wtf8_string_counted};
    use crate::Dialect;

    fn string(dialect: Dialect, text: &str) -> String {
        let mut out = Vec::new();
        let chars = write_json_string_counted(dialect, &mut out, text, 0).expect("escapes");
        let rendered = String::from_utf8(out).expect("utf-8 output");
        assert_eq!(
            chars,
            rendered.chars().count(),
            "character count for {text:?}"
        );
        rendered
    }

    #[test]
    fn drf_escaping() {
        assert_eq!(
            string(Dialect::Drf, "a\"b\\c\n\u{1}\u{7f}é😀"),
            "\"a\\\"b\\\\c\\n\\u0001\u{7f}é😀\""
        );
        assert_eq!(
            string(Dialect::Drf, "\u{2028}\u{2029}"),
            "\"\\u2028\\u2029\""
        );
    }

    #[test]
    fn ensure_ascii_escaping() {
        assert_eq!(
            string(Dialect::Django, "\u{7f}é😀\u{2028}"),
            "\"\\u007f\\u00e9\\ud83d\\ude00\\u2028\""
        );
    }

    #[test]
    fn counts_output_characters() {
        let mut out = Vec::new();
        // 1 quote + 1 "a" + 6 for \u0001 + 12 for the astral surrogate pair
        // + 1 quote = 21.
        let chars = write_json_string_counted(Dialect::Django, &mut out, "a\u{1}😀", 0).unwrap();
        assert_eq!(chars, 21);
        assert_eq!(chars, String::from_utf8(out).unwrap().chars().count());
    }

    #[test]
    fn lone_surrogates_fail_only_for_drf() {
        // One high surrogate followed by another, as `json.loads` produces
        // for "\ud800\ud800".
        let wtf8 = [0xED, 0xA0, 0x80, 0xED, 0xA0, 0x80];

        let mut out = Vec::new();
        let chars = write_json_wtf8_string_counted(Dialect::Django, &mut out, &wtf8, 0).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "\"\\ud800\\ud800\"");
        assert_eq!(chars, 14);

        let mut out = Vec::new();
        match write_json_wtf8_string_counted(Dialect::Drf, &mut out, &wtf8, 0) {
            Ok(_) => panic!("a lone surrogate must fail for drf"),
            Err(WriteError::Surrogate(error)) => {
                assert_eq!(error.code_point, 0xD800);
                assert_eq!(error.run, 2);
                assert_eq!(error.position, 1);
                assert_eq!(
                    error.message(),
                    "'utf-8' codec can't encode characters in position 1-2: surrogates not allowed"
                );
            }
            Err(_) => panic!("unexpected error"),
        }
    }
}
