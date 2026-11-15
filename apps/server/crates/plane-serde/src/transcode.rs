// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Streaming re-encoder for Postgres `json`/`jsonb` text.
//!
//! The input is parsed with CPython's `json.loads` semantics (strict control
//! characters, `\uXXXX` escapes including surrogate pairs, JSON number
//! grammar) and written straight into the output buffer through the dialect's
//! escaping and number formatting. Object member values are buffered per
//! level only to reproduce Python's duplicate-key rule (first position, last
//! value), which objects from `json` columns can need; `jsonb` output never
//! has duplicate keys.

use std::collections::HashMap;

use crate::float::PyFloatRepr;
use crate::formatter::{write_escaped_char, write_fragment, write_json_string};
use crate::{Dialect, Error};

pub(crate) fn transcode(dialect: Dialect, json_text: &str, out: &mut Vec<u8>) -> Result<(), Error> {
    let mut transcoder = Transcoder {
        bytes: json_text.as_bytes(),
        pos: 0,
        dialect,
        depth: 0,
        scratch: Vec::new(),
    };
    transcoder.skip_whitespace();
    if transcoder.pos == transcoder.bytes.len() {
        return Err(syntax("expecting value"));
    }
    transcoder.value(out)?;
    transcoder.skip_whitespace();
    if transcoder.pos != transcoder.bytes.len() {
        return Err(syntax("extra data"));
    }
    Ok(())
}

fn syntax(message: impl Into<String>) -> Error {
    Error::Syntax(message.into())
}

struct Member {
    key: String,
    value_start: usize,
    value_end: usize,
}

struct Transcoder<'a> {
    bytes: &'a [u8],
    pos: usize,
    dialect: Dialect,
    /// Nesting depth; objects use it to pick a scratch buffer, so parent and
    /// child object members never share one.
    depth: usize,
    scratch: Vec<Vec<u8>>,
}

impl Transcoder<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(byte) = self.peek() {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect_byte(&mut self, expected: u8) -> Result<(), Error> {
        if self.peek() == Some(expected) {
            self.pos += 1;
            Ok(())
        } else {
            Err(syntax(format!("expected '{}'", char::from(expected))))
        }
    }

    fn value(&mut self, dst: &mut Vec<u8>) -> Result<(), Error> {
        match self.peek() {
            Some(b'{') => self.object(dst),
            Some(b'[') => self.array(dst),
            Some(b'"') => self.string_value(dst),
            Some(b'-' | b'0'..=b'9') => self.number(dst),
            Some(b't') => {
                self.literal(b"true")?;
                dst.extend_from_slice(b"true");
                Ok(())
            }
            Some(b'f') => {
                self.literal(b"false")?;
                dst.extend_from_slice(b"false");
                Ok(())
            }
            Some(b'n') => {
                self.literal(b"null")?;
                dst.extend_from_slice(b"null");
                Ok(())
            }
            _ => Err(syntax("expecting value")),
        }
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), Error> {
        if self.bytes[self.pos..].starts_with(literal) {
            self.pos += literal.len();
            Ok(())
        } else {
            Err(syntax("expecting value"))
        }
    }

    fn array(&mut self, dst: &mut Vec<u8>) -> Result<(), Error> {
        self.expect_byte(b'[')?;
        self.depth += 1;
        dst.push(b'[');
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            dst.push(b']');
            self.depth -= 1;
            return Ok(());
        }
        let separator = self.dialect.item_separator();
        let mut first = true;
        loop {
            self.skip_whitespace();
            if !first {
                dst.extend_from_slice(separator);
            }
            self.value(dst)?;
            first = false;
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(syntax("expecting ',' delimiter")),
            }
        }
        dst.push(b']');
        self.depth -= 1;
        Ok(())
    }

    fn object(&mut self, dst: &mut Vec<u8>) -> Result<(), Error> {
        self.expect_byte(b'{')?;
        self.depth += 1;
        let slot = self.depth;
        if self.scratch.len() <= slot {
            self.scratch.resize_with(slot + 1, Vec::new);
        }
        let mut buffer = std::mem::take(&mut self.scratch[slot]);
        buffer.clear();
        let result = self.object_members(dst, &mut buffer);
        self.scratch[slot] = buffer;
        self.depth -= 1;
        result
    }

    fn object_members(&mut self, dst: &mut Vec<u8>, buffer: &mut Vec<u8>) -> Result<(), Error> {
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            dst.extend_from_slice(b"{}");
            return Ok(());
        }

        let mut members: Vec<Member> = Vec::new();
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(syntax("expecting property name enclosed in double quotes"));
            }
            let mut key = String::new();
            self.string_text(&mut key)?;

            self.skip_whitespace();
            self.expect_byte(b':')?;
            self.skip_whitespace();

            let value_start = buffer.len();
            self.value(buffer)?;
            let value_end = buffer.len();
            members.push(Member {
                key,
                value_start,
                value_end,
            });

            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(syntax("expecting ',' delimiter")),
            }
        }

        // Python's dict: a duplicate key keeps its first position and its last
        // value. Neither is known until the object ends, hence the buffering.
        let mut first_index: HashMap<&str, usize> = HashMap::with_capacity(members.len());
        let mut last_index: HashMap<&str, usize> = HashMap::with_capacity(members.len());
        for (index, member) in members.iter().enumerate() {
            first_index.entry(member.key.as_str()).or_insert(index);
            last_index.insert(member.key.as_str(), index);
        }

        let separator = self.dialect.item_separator();
        let mut first = true;
        dst.push(b'{');
        for (index, member) in members.iter().enumerate() {
            if first_index[member.key.as_str()] != index {
                continue;
            }
            let value = &members[last_index[member.key.as_str()]];
            if !first {
                dst.extend_from_slice(separator);
            }
            first = false;
            write_json_string(self.dialect, dst, &member.key)?;
            dst.extend_from_slice(self.dialect.key_separator());
            dst.extend_from_slice(&buffer[value.value_start..value.value_end]);
        }
        dst.push(b'}');
        Ok(())
    }

    /// Parse a string value, writing the quoted, dialect-escaped result.
    fn string_value(&mut self, dst: &mut Vec<u8>) -> Result<(), Error> {
        self.expect_byte(b'"')?;
        dst.push(b'"');
        loop {
            let Some(byte) = self.peek() else {
                return Err(syntax("unterminated string"));
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    dst.push(b'"');
                    return Ok(());
                }
                b'\\' => {
                    self.pos += 1;
                    let decoded = self.escape_sequence()?;
                    write_escaped_char(self.dialect, dst, decoded)?;
                }
                0x00..=0x1F => {
                    return Err(syntax("invalid control character in string"));
                }
                _ => {
                    let start = self.pos;
                    while let Some(byte) = self.peek() {
                        if byte == b'"' || byte == b'\\' || byte < 0x20 {
                            break;
                        }
                        self.pos += 1;
                    }
                    // The run starts and ends on ASCII boundaries, so it is a
                    // valid `str` slice.
                    let run = std::str::from_utf8(&self.bytes[start..self.pos])
                        .map_err(|_| syntax("invalid utf-8 in string"))?;
                    write_fragment(self.dialect, dst, run)?;
                }
            }
        }
    }

    /// Parse a string into decoded text (used for object keys, which have to
    /// be compared the way Python compares them).
    fn string_text(&mut self, out: &mut String) -> Result<(), Error> {
        self.expect_byte(b'"')?;
        loop {
            let Some(byte) = self.peek() else {
                return Err(syntax("unterminated string"));
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    return Ok(());
                }
                b'\\' => {
                    self.pos += 1;
                    let decoded = self.escape_sequence()?;
                    out.push(decoded);
                }
                0x00..=0x1F => {
                    return Err(syntax("invalid control character in string"));
                }
                _ => {
                    let start = self.pos;
                    while let Some(byte) = self.peek() {
                        if byte == b'"' || byte == b'\\' || byte < 0x20 {
                            break;
                        }
                        self.pos += 1;
                    }
                    let run = std::str::from_utf8(&self.bytes[start..self.pos])
                        .map_err(|_| syntax("invalid utf-8 in string"))?;
                    out.push_str(run);
                }
            }
        }
    }

    /// Decode one escape sequence; `self.pos` points just after the backslash.
    fn escape_sequence(&mut self) -> Result<char, Error> {
        let Some(byte) = self.peek() else {
            return Err(syntax("unterminated string"));
        };
        self.pos += 1;
        let decoded = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let first = self.hex4()?;
                if (0xD800..=0xDBFF).contains(&first) {
                    if self.peek() != Some(b'\\') {
                        return Err(syntax("unpaired surrogate"));
                    }
                    self.pos += 1;
                    self.expect_byte(b'u')?;
                    let second = self.hex4()?;
                    if !(0xDC00..=0xDFFF).contains(&second) {
                        return Err(syntax("unpaired surrogate"));
                    }
                    let combined = 0x1_0000
                        + ((u32::from(first) - 0xD800) << 10)
                        + (u32::from(second) - 0xDC00);
                    char::from_u32(combined).ok_or_else(|| syntax("invalid code point"))?
                } else if (0xDC00..=0xDFFF).contains(&first) {
                    return Err(syntax("unpaired surrogate"));
                } else {
                    char::from_u32(u32::from(first)).ok_or_else(|| syntax("invalid code point"))?
                }
            }
            _ => return Err(syntax("invalid \\escape")),
        };
        Ok(decoded)
    }

    fn hex4(&mut self) -> Result<u16, Error> {
        if self.pos + 4 > self.bytes.len() {
            return Err(syntax("truncated \\uXXXX escape"));
        }
        let mut value: u16 = 0;
        for _ in 0..4 {
            let digit = self.bytes[self.pos];
            let nibble = match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'f' => digit - b'a' + 10,
                b'A'..=b'F' => digit - b'A' + 10,
                _ => return Err(syntax("invalid \\uXXXX escape")),
            };
            value = (value << 4) | u16::from(nibble);
            self.pos += 1;
        }
        Ok(value)
    }

    fn number(&mut self, dst: &mut Vec<u8>) -> Result<(), Error> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(syntax("invalid number")),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(syntax("invalid number"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(syntax("invalid number"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }

        let lexeme = &self.bytes[start..self.pos];
        if !is_float {
            // Python's int() has a default 4300-digit string-conversion limit,
            // which json.loads inherits.
            let digits = usize::from(lexeme.first() == Some(&b'-'));
            if lexeme.len() - digits > 4300 {
                return Err(syntax(
                    "exceeds the limit (4300 digits) for integer string conversion",
                ));
            }
            // int("-0") is 0.
            if lexeme == b"-0" {
                dst.push(b'0');
            } else {
                dst.extend_from_slice(lexeme);
            }
            return Ok(());
        }

        let text = std::str::from_utf8(lexeme).map_err(|_| syntax("invalid number"))?;
        let value = text.parse::<f64>().map_err(|_| syntax("invalid number"))?;
        if value.is_finite() {
            match PyFloatRepr::new(value) {
                Some(repr) => {
                    dst.extend_from_slice(repr.as_str().as_bytes());
                    Ok(())
                }
                None => Err(Error::NonFinite(value)),
            }
        } else if self.dialect.allow_nan() {
            if value.is_sign_negative() {
                dst.extend_from_slice(b"-Infinity");
            } else {
                dst.extend_from_slice(b"Infinity");
            }
            Ok(())
        } else {
            Err(Error::NonFinite(value))
        }
    }
}
