// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A `json.loads`-compatible parser.
//!
//! The parser mirrors CPython 3.12's C accelerator, `Modules/_json.c`
//! (`scan_once_unicode`, `_parse_object_unicode`, `_parse_array_unicode`,
//! `_match_number_unicode`, `scanstring_unicode`) as it is reached through
//! `json.loads` -> `JSONDecoder.decode` -> `raw_decode`. Every error message
//! and every position comes from that code, not from the pure-Python
//! fallback: the C scanner reports, for example, `Invalid \escape` at the
//! backslash and `Invalid \uXXXX escape` at the `u`, and it has no
//! trailing-comma message (a trailing comma is an ordinary `Expecting value`
//! or `Expecting property name enclosed in double quotes`).
//!
//! Two details are easy to get wrong:
//!
//! * Positions in messages are **code point** offsets into the decoded text,
//!   not byte offsets, and `lineno`/`colno` follow
//!   `json.decoder.JSONDecodeError.__init__` (`doc.count("\n", 0, pos) + 1`,
//!   `pos - doc.rfind("\n", 0, pos)`).
//! * Containers are parsed with an explicit stack rather than recursion. The
//!   C scanner allows 9997 nested containers before `Py_EnterRecursiveCall`
//!   fails (measured; `Py_C_RECURSION_LIMIT` is 10000 and the call chain to
//!   `parse_array` already used three levels), and a recursive Rust parser
//!   would overflow a worker thread's stack long before that.

use super::value::{BigInt, PyDict, PyInt, PyStr, Value};
use super::{INT_MAX_STR_DIGITS, LoadError};

/// How many nested containers CPython accepts before raising `RecursionError`.
///
/// Measured against the pinned interpreter through DRF's `JSONParser`:
/// 9997 nested arrays (or objects, or a mix) parse, 9998 raise. The value is
/// a property of `Py_C_RECURSION_LIMIT` (10000) and the depth of the call
/// chain into `_json.scan_once`, so it is re-measured by the golden corpus
/// (category `recursion`).
pub const MAX_NESTING: usize = 9997;

/// Parse a decoded request body the way `json.loads` does.
pub(crate) fn parse(doc: &str) -> Result<Value, LoadError> {
    // json/__init__.py:333-336: a leading U+FEFF in the *string* is refused
    // before any parsing. (DRF decodes bytes itself, so json.detect_encoding
    // never runs; a UTF-8 BOM decodes to U+FEFF and lands here.)
    if doc.starts_with('\u{feff}') {
        return Err(json_error(
            "Unexpected UTF-8 BOM (decode using utf-8-sig)",
            doc,
            0,
        ));
    }

    let mut parser = Parser {
        doc,
        bytes: doc.as_bytes(),
        pos: 0,
    };
    parser.skip_whitespace();
    if parser.pos == parser.bytes.len() {
        return Err(json_error("Expecting value", doc, parser.pos));
    }

    let mut stack: Vec<Frame> = Vec::new();

    'value: loop {
        let mut value = match parser.peek() {
            // scan_once_unicode raises StopIteration when idx >= length, which
            // raw_decode turns into "Expecting value" at that index.
            None => return Err(json_error("Expecting value", parser.doc, parser.pos)),
            Some(b'[') => {
                parser.pos += 1;
                parser.check_depth(stack.len(), "array")?;
                parser.skip_whitespace();
                if parser.peek() == Some(b']') {
                    parser.pos += 1;
                    Value::List(Vec::new())
                } else {
                    stack.push(Frame::Array(Vec::new()));
                    continue 'value;
                }
            }
            Some(b'{') => {
                parser.pos += 1;
                parser.check_depth(stack.len(), "object")?;
                parser.skip_whitespace();
                if parser.peek() == Some(b'}') {
                    parser.pos += 1;
                    Value::Dict(PyDict::new())
                } else {
                    let key = parser.object_key()?;
                    stack.push(Frame::Object(PyDict::new(), key));
                    continue 'value;
                }
            }
            Some(b'"') => Value::Str(parser.string()?),
            Some(b't') => parser.literal(b"true", Value::Bool(true))?,
            Some(b'f') => parser.literal(b"false", Value::Bool(false))?,
            Some(b'n') => parser.literal(b"null", Value::Null)?,
            Some(b'N') if parser.starts_with(b"NaN") => {
                return Err(strict_constant("NaN"));
            }
            Some(b'I') if parser.starts_with(b"Infinity") => {
                return Err(strict_constant("Infinity"));
            }
            Some(b'-') if parser.starts_with(b"-Infinity") => {
                return Err(strict_constant("-Infinity"));
            }
            _ => parser.number()?,
        };

        // Attach the finished value to its container, closing containers
        // until one expects another value.
        'attach: loop {
            let array = match stack.last_mut() {
                None => return parser.finish(value),
                Some(Frame::Array(items)) => {
                    items.push(value);
                    true
                }
                Some(Frame::Object(dict, key)) => {
                    dict.insert(std::mem::take(key), value);
                    false
                }
            };

            parser.skip_whitespace();
            match (array, parser.peek()) {
                (_, Some(b',')) => {
                    parser.pos += 1;
                    parser.skip_whitespace();
                    if !array {
                        let key = parser.object_key()?;
                        if let Some(Frame::Object(_, slot)) = stack.last_mut() {
                            *slot = key;
                        }
                    }
                    continue 'value;
                }
                (true, Some(b']')) => {
                    parser.pos += 1;
                    let Some(Frame::Array(items)) = stack.pop() else {
                        return Err(json_error("Expecting ',' delimiter", doc, parser.pos));
                    };
                    value = Value::List(items);
                    continue 'attach;
                }
                (false, Some(b'}')) => {
                    parser.pos += 1;
                    let Some(Frame::Object(dict, _)) = stack.pop() else {
                        return Err(json_error("Expecting ',' delimiter", doc, parser.pos));
                    };
                    value = Value::Dict(dict);
                    continue 'attach;
                }
                _ => return Err(json_error("Expecting ',' delimiter", doc, parser.pos)),
            }
        }
    }
}

/// One open container.
enum Frame {
    Array(Vec<Value>),
    Object(PyDict, PyStr),
}

struct Parser<'a> {
    doc: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn starts_with(&self, word: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(word)
    }

    fn skip_whitespace(&mut self) {
        // IS_WHITESPACE in Modules/_json.c: space, tab, line feed, carriage
        // return only. Vertical tab, form feed, NBSP, U+2028 and friends are
        // not JSON whitespace.
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    /// Entering a container: `_Py_EnterRecursiveCall` fails when the C
    /// recursion budget is exhausted.
    fn check_depth(&self, open: usize, kind: &str) -> Result<(), LoadError> {
        if open >= MAX_NESTING {
            return Err(LoadError::recursion(format!(
                "maximum recursion depth exceeded while decoding a JSON {kind} from a unicode string"
            )));
        }
        Ok(())
    }

    fn literal(&mut self, word: &[u8], value: Value) -> Result<Value, LoadError> {
        if self.starts_with(word) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(json_error("Expecting value", self.doc, self.pos))
        }
    }

    /// Read a key and its `:` separator. `self.pos` is on the first
    /// non-whitespace character after `{` or after `,`.
    fn object_key(&mut self) -> Result<PyStr, LoadError> {
        if self.peek() != Some(b'"') {
            return Err(json_error(
                "Expecting property name enclosed in double quotes",
                self.doc,
                self.pos,
            ));
        }
        let key = self.string()?;
        self.skip_whitespace();
        if self.peek() != Some(b':') {
            return Err(json_error("Expecting ':' delimiter", self.doc, self.pos));
        }
        self.pos += 1;
        self.skip_whitespace();
        Ok(key)
    }

    /// `_match_number_unicode`: a deliberately lenient scanner that consumes
    /// the longest number-shaped prefix and leaves anything else to the
    /// caller (`1.` becomes `1` and the `.` is `Extra data`, or `Expecting
    /// ',' delimiter` inside a container).
    fn number(&mut self) -> Result<Value, LoadError> {
        let start = self.pos;
        let end_idx = self.bytes.len() - 1;
        let mut idx = start;

        if self.bytes[idx] == b'-' {
            idx += 1;
            if idx > end_idx {
                return Err(json_error("Expecting value", self.doc, start));
            }
        }

        let first = self.bytes[idx];
        if (b'1'..=b'9').contains(&first) {
            idx += 1;
            while idx <= end_idx && self.bytes[idx].is_ascii_digit() {
                idx += 1;
            }
        } else if first == b'0' {
            idx += 1;
        } else {
            return Err(json_error("Expecting value", self.doc, start));
        }

        let mut is_float = false;
        if idx < end_idx && self.bytes[idx] == b'.' && self.bytes[idx + 1].is_ascii_digit() {
            is_float = true;
            idx += 2;
            while idx <= end_idx && self.bytes[idx].is_ascii_digit() {
                idx += 1;
            }
        }

        if idx < end_idx && matches!(self.bytes[idx], b'e' | b'E') {
            let exponent_start = idx;
            idx += 1;
            if idx < end_idx && matches!(self.bytes[idx], b'+' | b'-') {
                idx += 1;
            }
            while idx <= end_idx && self.bytes[idx].is_ascii_digit() {
                idx += 1;
            }
            if self.bytes[idx - 1].is_ascii_digit() {
                is_float = true;
            } else {
                idx = exponent_start;
            }
        }

        let text = &self.doc[start..idx];
        self.pos = idx;

        if is_float {
            // `float()` is not `parse_constant`: DRF's strict_constant only
            // sees the literal tokens NaN/Infinity/-Infinity, so an
            // overflowing lexeme such as 1e400 becomes inf here and only
            // fails later, when the renderer refuses to write it
            // (allow_nan=False).
            return match text.parse::<f64>() {
                Ok(value) => Ok(Value::Float(value)),
                Err(_) => Err(json_error("Expecting value", self.doc, start)),
            };
        }

        let digits = text.strip_prefix('-').unwrap_or(text);
        if digits.len() > INT_MAX_STR_DIGITS {
            // longobject.c raises this from PyLong_FromString; it is a plain
            // ValueError, so DRF adds "JSON parse error - " and no position.
            return Err(LoadError::json(format!(
                "Exceeds the limit ({INT_MAX_STR_DIGITS} digits) for integer string conversion: \
                 value has {} digits; use sys.set_int_max_str_digits() to increase the limit",
                digits.len()
            )));
        }
        if let Ok(value) = text.parse::<i64>() {
            return Ok(Value::Int(PyInt::Small(value)));
        }
        match BigInt::from_decimal(text.starts_with('-'), digits) {
            Some(value) => Ok(Value::Int(PyInt::Big(value))),
            // Only a run of zeros, which i64 already covered.
            None => Ok(Value::Int(PyInt::Small(0))),
        }
    }

    /// `scanstring_unicode`. `self.pos` is on the opening quote.
    fn string(&mut self) -> Result<PyStr, LoadError> {
        let quote = self.pos;
        self.pos += 1;
        let content_start = self.pos;

        // Fast path: no escapes and no control characters, which is almost
        // every string. The scan stops only on ASCII bytes, so `pos` stays on
        // a character boundary and the slice is valid UTF-8.
        loop {
            match self.peek() {
                Some(b'"') => {
                    let text = &self.doc[content_start..self.pos];
                    self.pos += 1;
                    return Ok(PyStr::new(text));
                }
                Some(b'\\') => break,
                Some(byte) if byte < 0x20 => {
                    return Err(json_error(
                        "Invalid control character at",
                        self.doc,
                        self.pos,
                    ));
                }
                Some(_) => self.pos += 1,
                None => {
                    return Err(json_error(
                        "Unterminated string starting at",
                        self.doc,
                        quote,
                    ));
                }
            }
        }

        let mut out = Vec::with_capacity((self.pos - content_start).saturating_mul(2).max(16));
        let mut run_start = content_start;
        loop {
            match self.peek() {
                Some(b'"') => {
                    out.extend_from_slice(&self.bytes[run_start..self.pos]);
                    self.pos += 1;
                    return match PyStr::from_wtf8(out) {
                        Ok(text) => Ok(text),
                        // Unreachable: only code points encoded above are
                        // appended.
                        Err(_) => Err(LoadError::json("invalid string".to_owned())),
                    };
                }
                Some(b'\\') => {
                    out.extend_from_slice(&self.bytes[run_start..self.pos]);
                    self.pos += 1;
                    self.escape(&mut out, quote)?;
                    run_start = self.pos;
                }
                Some(byte) if byte < 0x20 => {
                    return Err(json_error(
                        "Invalid control character at",
                        self.doc,
                        self.pos,
                    ));
                }
                // Copy plain runs in bulk: bytes below 0x80 cannot be part of
                // a multi-byte character, so `run_start..pos` stays on
                // character boundaries.
                Some(_) => self.pos += 1,
                None => {
                    return Err(json_error(
                        "Unterminated string starting at",
                        self.doc,
                        quote,
                    ));
                }
            }
        }
    }

    /// Decode one escape sequence; `self.pos` is just past the backslash.
    fn escape(&mut self, out: &mut Vec<u8>, quote: usize) -> Result<(), LoadError> {
        let backslash = self.pos - 1;
        let Some(byte) = self.peek() else {
            return Err(json_error(
                "Unterminated string starting at",
                self.doc,
                quote,
            ));
        };
        if byte != b'u' {
            let decoded = match byte {
                b'"' => b'"',
                b'\\' => b'\\',
                b'/' => b'/',
                b'b' => 0x08,
                b'f' => 0x0C,
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                // The C scanner reports the position of the backslash.
                _ => return Err(json_error("Invalid \\escape", self.doc, backslash)),
            };
            out.push(decoded);
            self.pos += 1;
            return Ok(());
        }

        let u_index = self.pos;
        self.pos += 1;
        let end = self.pos + 4;
        // The C scanner requires at least one character after the four hex
        // digits, so `"\u1234` (no closing quote) reports an invalid escape.
        if end >= self.bytes.len() {
            return Err(json_error("Invalid \\uXXXX escape", self.doc, u_index));
        }
        let Some(first) = self.hex4(self.pos) else {
            return Err(json_error("Invalid \\uXXXX escape", self.doc, u_index));
        };
        self.pos = end;
        let mut code_point = u32::from(first);

        if (0xD800..=0xDBFF).contains(&first)
            && end + 6 < self.bytes.len()
            && self.bytes[end] == b'\\'
            && self.bytes[end + 1] == b'u'
        {
            match self.hex4(end + 2) {
                Some(second) if (0xDC00..=0xDFFF).contains(&second) => {
                    code_point = 0x1_0000
                        + ((u32::from(first) - 0xD800) << 10)
                        + (u32::from(second) - 0xDC00);
                    self.pos = end + 6;
                }
                // Not a low surrogate: keep the high one on its own and parse
                // the second escape normally on the next iteration.
                Some(_) => {}
                None => {
                    return Err(json_error("Invalid \\uXXXX escape", self.doc, end + 1));
                }
            }
        }

        let mut buffer = [0u8; 4];
        let length = crate::wtf8::encode(code_point, &mut buffer);
        out.extend_from_slice(&buffer[..length]);
        Ok(())
    }

    fn hex4(&self, index: usize) -> Option<u16> {
        let mut value: u16 = 0;
        for offset in 0..4 {
            let digit = *self.bytes.get(index + offset)?;
            let nibble = match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'f' => digit - b'a' + 10,
                b'A'..=b'F' => digit - b'A' + 10,
                _ => return None,
            };
            value = (value << 4) | u16::from(nibble);
        }
        Some(value)
    }

    /// `JSONDecoder.decode`: skip trailing whitespace, then refuse anything
    /// left over.
    fn finish(&self, value: Value) -> Result<Value, LoadError> {
        let mut end = self.pos;
        while matches!(self.bytes.get(end), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            end += 1;
        }
        if end != self.bytes.len() {
            return Err(json_error("Extra data", self.doc, end));
        }
        Ok(value)
    }
}

/// `rest_framework.utils.json.strict_constant`, which DRF installs as
/// `parse_constant` while `STRICT_JSON` is on (the Plane default).
fn strict_constant(constant: &str) -> LoadError {
    LoadError::json(format!(
        "Out of range float values are not JSON compliant: '{constant}'"
    ))
}

/// A `json.JSONDecodeError`: `"{msg}: line {lineno} column {colno} (char {pos})"`
/// with `pos` counted in code points.
fn json_error(message: &str, doc: &str, byte_pos: usize) -> LoadError {
    let mut lineno = 1;
    let mut line_start = 0;
    let mut chars = 0;
    for (index, ch) in doc.char_indices() {
        if index >= byte_pos {
            break;
        }
        chars += 1;
        if ch == '\n' {
            lineno += 1;
            line_start = chars;
        }
    }
    let colno = chars - line_start + 1;
    LoadError::json(format!(
        "{message}: line {lineno} column {colno} (char {chars})"
    ))
}

#[cfg(test)]
mod tests {
    use super::{MAX_NESTING, parse};

    fn error(input: &str) -> String {
        match parse(input) {
            Ok(value) => panic!("{input:?} parsed as {value:?}"),
            Err(error) => error.message().to_owned(),
        }
    }

    #[test]
    fn messages_match_the_c_scanner() {
        let cases = [
            ("", "Expecting value: line 1 column 1 (char 0)"),
            (" ", "Expecting value: line 1 column 2 (char 1)"),
            (
                "{",
                "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)",
            ),
            ("[1,]", "Expecting value: line 1 column 4 (char 3)"),
            (
                "{\"a\":1,}",
                "Expecting property name enclosed in double quotes: line 1 column 8 (char 7)",
            ),
            (
                "{\"a\"}",
                "Expecting ':' delimiter: line 1 column 5 (char 4)",
            ),
            (
                "{\"a\":1 \"b\":2}",
                "Expecting ',' delimiter: line 1 column 8 (char 7)",
            ),
            (
                "\"abc",
                "Unterminated string starting at: line 1 column 1 (char 0)",
            ),
            (
                "\"a\nb\"",
                "Invalid control character at: line 1 column 3 (char 2)",
            ),
            ("\"\\q\"", "Invalid \\escape: line 1 column 2 (char 1)"),
            (
                "\"\\u12\"",
                "Invalid \\uXXXX escape: line 1 column 3 (char 2)",
            ),
            ("01", "Extra data: line 1 column 2 (char 1)"),
            ("tru", "Expecting value: line 1 column 1 (char 0)"),
            (
                "\n\n{\"a\":\n\n1\n\n,\n\n}",
                "Expecting property name enclosed in double quotes: line 9 column 1 (char 15)",
            ),
            ("[\n1,\n]", "Expecting value: line 3 column 1 (char 5)"),
            (
                "\u{feff}{}",
                "Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(error(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn positions_are_code_points() {
        // Four bytes of é before the error; CPython says char 8.
        assert_eq!(
            error("[\"ééé\", ]"),
            "Expecting value: line 1 column 9 (char 8)"
        );
        assert_eq!(
            error("{\"é\": , }"),
            "Expecting value: line 1 column 7 (char 6)"
        );
    }

    #[test]
    fn nesting_limit_matches_the_c_recursion_guard() {
        let ok = format!("{}{}", "[".repeat(MAX_NESTING), "]".repeat(MAX_NESTING));
        assert!(parse(&ok).is_ok());
        let too_deep = format!(
            "{}{}",
            "[".repeat(MAX_NESTING + 1),
            "]".repeat(MAX_NESTING + 1)
        );
        assert_eq!(
            error(&too_deep),
            "maximum recursion depth exceeded while decoding a JSON array from a unicode string"
        );
        let object = format!(
            "{}{}",
            "{\"a\":".repeat(MAX_NESTING + 1),
            "}".repeat(MAX_NESTING + 1)
        );
        assert_eq!(
            error(&object),
            "maximum recursion depth exceeded while decoding a JSON object from a unicode string"
        );
    }

    #[test]
    fn integers_are_arbitrary_precision() {
        let value = parse("123456789012345678901234567890").unwrap();
        assert_eq!(
            value.as_int().unwrap().to_decimal_string(),
            "123456789012345678901234567890"
        );
        assert_eq!(parse("-0").unwrap().as_i64(), Some(0));
    }

    #[test]
    fn lone_surrogates_survive() {
        let value = parse("\"\\ud800x\"").unwrap();
        let text = value.as_pystr().unwrap();
        assert!(text.has_surrogates());
        assert_eq!(text.chars().collect::<Vec<_>>(), [0xD800, u32::from(b'x')]);
    }
}
