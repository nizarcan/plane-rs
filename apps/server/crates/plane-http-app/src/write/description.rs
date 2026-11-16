// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! §2d of `plans/WORK_ITEMS.md`: the description validators the work-item (and
//! draft, and comment) serializers run.
//!
//! | Python | Here |
//! | --- | --- |
//! | `validate_html_content` (`plane/utils/content_validator.py:209-239`) | [`validate_html_content`] |
//! | `validate_binary_data` (`plane/utils/content_validator.py:29-68`) | [`validate_binary_data`] |
//! | `strip_tags` (`plane/utils/html_processor.py:28-31`) | [`strip_tags`] (re-export of `plane_db::hooks::html`) |
//!
//! ## The error bodies
//!
//! `IssueCreateSerializer.validate` raises
//! `serializers.ValidationError({"error": "html content is not valid"})` and
//! `ValidationError({"description_binary": "Invalid binary data"})`
//! (`plane/app/serializers/issue.py:145-157`). DRF's `as_serializer_error`
//! wraps every dict value in a one-element list, so the bodies are
//! `{"error": ["html content is not valid"]}` and
//! `{"description_binary": ["Invalid binary data"]}` — [`html_invalid_error`] /
//! [`binary_invalid_error`].
//!
//! ## What is reachable
//!
//! The HTML branch always is. The **binary** branch is unreachable through the
//! API: `Issue.description_binary` is a `BinaryField` with `editable=False`
//! (`plane/db/models/issue.py:140`), so DRF builds it as a read-only
//! `ModelField` and a client value never reaches `attrs` (verified against the
//! real serializer: sending `description_binary` yields no error and no
//! validated key). It is ported because the draft serializer has the identical
//! branch (`plane/app/serializers/draft.py:92-95`) and a generated field table
//! could make it reachable; do not call it on the issue path expecting Django
//! to return 400.
//!
//! ## HTML
//!
//! `assert_sanitized` is the serializer's step, complete: truthiness, the 10 MB
//! limit measured on the UTF-8 bytes (`len(html.encode("utf-8"))`), then
//! `nh3.clean` with Plane's tag/attribute/protocol tables (see [`super::html`]
//! and its corpus) — the sanitized string *replaces* `description_html`.

use plane_drf::view::ViewResponse;

pub use plane_db::hooks::html::strip_tags;

/// `MAX_SIZE` (`plane/utils/content_validator.py:15`).
pub const MAX_SIZE: usize = 10 * 1024 * 1024;

/// `SUSPICIOUS_BINARY_PATTERNS` (`content_validator.py:17-24`), in order.
const SUSPICIOUS_BINARY_PATTERNS: [&str; 6] = [
    "<html",
    "<!doctype",
    "<script",
    "javascript:",
    "data:",
    "<iframe",
];

/// `validate_html_content(html)`'s three outcomes
/// (`content_validator.py:211-243`).
///
/// The Python returns `(is_valid, error_message, clean_html)`: a falsy input is
/// `(True, None, None)` (the serializer's `if attrs["description_html"]` guard
/// makes that unreachable), an over-limit input is `(False, "HTML content
/// exceeds…", None)` and the serializer reports the generic message, and a
/// normal input is `(True, None, clean)` — the sanitized string *replaces* the
/// field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlValidation {
    /// Falsy input: nothing to do, keep the value as it is.
    Unchanged,
    /// Sanitize-and-replace.
    Sanitized(String),
    /// Over the 10 MB limit (or `nh3` raised, which cannot happen here):
    /// `{"error": ["html content is not valid"]}`.
    Invalid,
}

/// `validate_html_content` (`plane/utils/content_validator.py:209-243`).
#[must_use]
pub fn validate_html_content(html: &str) -> HtmlValidation {
    match validate_html_reason(html) {
        Ok(None) => HtmlValidation::Unchanged,
        Ok(Some(clean)) => HtmlValidation::Sanitized(clean),
        Err(_) => HtmlValidation::Invalid,
    }
}

/// [`validate_html_content`] with the reason the issue path discards.
///
/// `validate_html_content` returns `(is_valid, error_message, sanitized_html)`
/// and the issue serializer only keeps the truthiness of `is_valid`
/// ([`html_invalid_error`]); `PageBinaryUpdateSerializer.validate_description_html`
/// raises `ValidationError(error_message)` with the **message**, so the page
/// route needs it (`plane/app/serializers/page.py:209-221`):
///
/// * `Err(HTML_TOO_LARGE)` — over 10 MB of UTF-8;
/// * `Err(HTML_SANITIZE_FAILED)` — `nh3.clean` raised, which the Rust
///   sanitiser cannot (kept for completeness);
/// * `Ok(None)` — a falsy input, left as it is;
/// * `Ok(Some(clean))` — the sanitized HTML that replaces the field.
///
/// # Errors
///
/// The message DRF renders under `description_html`.
pub fn validate_html_reason(html: &str) -> Result<Option<String>, &'static str> {
    if html.is_empty() {
        return Ok(None);
    }
    // `len(html_content.encode("utf-8")) > MAX_SIZE`.
    if html.len() > MAX_SIZE {
        return Err(HTML_TOO_LARGE);
    }
    Ok(Some(super::html::sanitize(html)))
}

/// `"HTML content exceeds maximum size limit (10MB)"`
/// (`plane/utils/content_validator.py:215`).
pub const HTML_TOO_LARGE: &str = "HTML content exceeds maximum size limit (10MB)";

/// `"Failed to sanitize HTML"` (`plane/utils/content_validator.py:241`).
pub const HTML_SANITIZE_FAILED: &str = "Failed to sanitize HTML";

/// `{"error": ["html content is not valid"]}` with DRF's 400.
#[must_use]
pub fn html_invalid_error() -> ViewResponse {
    super::field_string_error("error", "html content is not valid")
}

/// `validate_binary_data(data)` for the string form the serializer sees.
///
/// `true` means valid. The Python accepts `bytes` too, but a DRF
/// `ModelField` hands over the raw body value, i.e. a `str`.
#[must_use]
pub fn validate_binary_data(data: &str) -> bool {
    // `if not data: return True, None`.
    if data.is_empty() {
        return true;
    }
    let Some(decoded) = base64_decode(data) else {
        return false;
    };
    // `if len(binary_data) > MAX_SIZE` / `if len(binary_data) < 4`.
    if decoded.len() > MAX_SIZE || decoded.len() < 4 {
        return false;
    }
    // `decoded_text = binary_data.decode("utf-8", errors="ignore")[:200]`;
    // patterns are ASCII, so an ASCII lowercase is the same test.
    let text = utf8_ignore_prefix(&decoded, 200).to_ascii_lowercase();
    !SUSPICIOUS_BINARY_PATTERNS
        .iter()
        .any(|pattern| text.contains(pattern))
}

/// `{"description_binary": ["Invalid binary data"]}` with DRF's 400.
#[must_use]
pub fn binary_invalid_error() -> ViewResponse {
    super::field_string_error("description_binary", "Invalid binary data")
}

/// The validated value of `PageBinaryUpdateSerializer.description_binary`
/// (`plane/app/serializers/page.py:180-198`).
///
/// Python's hook returns the **decoded bytes** when the input is truthy and
/// passes `validate_binary_data`, and returns the input unchanged when it is
/// falsy. A falsy `""` therefore reaches `instance.description_binary` as a
/// `str`, which psycopg sends as text into a `bytea` column — a `DataError`,
/// i.e. the family 500 (pinned by `write/pages-actions.yaml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageBinary {
    /// The decoded bytes, written to the column.
    Bytes(Vec<u8>),
    /// The falsy input, written as-is: the database rejects it (500).
    Blank(String),
}

/// `validate_description_binary(value)`
/// (`plane/app/serializers/page.py:180-198`) with the message DRF renders.
///
/// The Python decodes **before** calling `validate_binary_data`, whose own
/// first line is `if not data: return True, None` — so an input whose base64
/// form decodes to nothing (`"!!!"`, `"===="`, …) is valid and writes an empty
/// `bytea`, while a 1–3 byte decode is "too short". `base64.b64decode`'
/// leniency comes from [`base64_decode`].
///
/// # Errors
///
/// The message `ValidationError(message)` carries
/// (`rest_framework/exceptions.py:148-161` wraps it in a one-element list).
pub fn validate_page_binary(value: &str) -> Result<PageBinary, &'static str> {
    // `if not value: return value`.
    if value.is_empty() {
        return Ok(PageBinary::Blank(String::new()));
    }
    // `binary_data = base64.b64decode(value)` — any exception becomes the
    // generic message (`except Exception` / `raise ValidationError("Failed to
    // decode base64 data")`).
    let Some(decoded) = base64_decode(value) else {
        return Err(BINARY_DECODE_FAILED);
    };
    // `validate_binary_data(binary_data)`: `if not data: return True, None`.
    if decoded.is_empty() {
        return Ok(PageBinary::Bytes(decoded));
    }
    if let Some(reason) = binary_data_reason(&decoded) {
        return Err(reason);
    }
    Ok(PageBinary::Bytes(decoded))
}

/// The three refusals of `validate_binary_data(bytes)`
/// (`plane/utils/content_validator.py:29-68`), as the message the page
/// serializer prefixes with `"Invalid binary data: "`.
///
/// The empty input is **not** a refusal here: the Python checks it first and
/// returns valid.
#[must_use]
pub fn binary_data_reason(data: &[u8]) -> Option<&'static str> {
    if data.is_empty() {
        return None;
    }
    if data.len() > MAX_SIZE {
        return Some(BINARY_TOO_LARGE);
    }
    if data.len() < 4 {
        return Some(BINARY_TOO_SHORT);
    }
    // `binary_data.decode("utf-8", errors="ignore")[:200]`; the patterns are
    // ASCII, so an ASCII lowercase is the same test.
    let text = utf8_ignore_prefix(data, 200).to_ascii_lowercase();
    if SUSPICIOUS_BINARY_PATTERNS
        .iter()
        .any(|pattern| text.contains(pattern))
    {
        return Some(BINARY_SUSPICIOUS);
    }
    None
}

/// `"Failed to decode base64 data"` (`plane/app/serializers/page.py:197`).
pub const BINARY_DECODE_FAILED: &str = "Failed to decode base64 data";

/// `"Invalid binary data: Binary data exceeds maximum size limit (10MB)"`
/// (`plane/app/serializers/page.py:189` + `content_validator.py:51`).
pub const BINARY_TOO_LARGE: &str =
    "Invalid binary data: Binary data exceeds maximum size limit (10MB)";

/// `"Invalid binary data: Binary data too short to be valid document format"`.
pub const BINARY_TOO_SHORT: &str =
    "Invalid binary data: Binary data too short to be valid document format";

/// `"Invalid binary data: Binary data contains suspicious content patterns"`.
pub const BINARY_SUSPICIOUS: &str =
    "Invalid binary data: Binary data contains suspicious content patterns";

/// `base64.b64decode(text)` with its default `validate=False` leniency — a
/// structural port of CPython 3.12's `binascii.a2b_base64`
/// (`Modules/binascii.c`, non-strict mode):
///
/// * a str that is not pure ASCII raises
///   `ValueError: string argument should contain only ASCII characters`;
/// * every byte outside the base64 alphabet is skipped;
/// * `=` is not data: while the current quad holds fewer than two characters it
///   is skipped, otherwise it counts as padding, and when the quad plus its
///   padding reaches four characters decoding **stops** (the rest of the input,
///   padding or not, is ignored);
/// * a data character resets the padding count, so `"YW=Jj"` is `b"abc"`;
/// * at the end a non-empty partial quad is `binascii.Error` ("Incorrect
///   padding", or "…cannot be 1 more than a multiple of 4" for one character).
///
/// `None` is any of those errors. An empty result is `Some(vec![])`, which the
/// page hook accepts (its `validate_binary_data` starts with `if not data`).
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    // `ascii_buffer_converter`: `PyUnicode_IS_ASCII` or a `ValueError`.
    if !text.is_ascii() {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4 + 3);
    let mut quad_pos = 0usize;
    let mut pads = 0usize;
    let mut leftchar = 0u32;
    for &byte in bytes {
        if byte == b'=' {
            if quad_pos >= 2 {
                pads += 1;
                if quad_pos + pads >= 4 {
                    return Some(out);
                }
            }
            continue;
        }
        let Some(value) = base64_value(byte) else {
            continue;
        };
        pads = 0;
        match quad_pos {
            0 => {
                quad_pos = 1;
                leftchar = value;
            }
            1 => {
                quad_pos = 2;
                out.push(((leftchar << 2) | (value >> 4)) as u8);
                leftchar = value & 0x0f;
            }
            2 => {
                quad_pos = 3;
                out.push(((leftchar << 4) | (value >> 2)) as u8);
                leftchar = value & 0x03;
            }
            _ => {
                quad_pos = 0;
                out.push(((leftchar << 6) | value) as u8);
                leftchar = 0;
            }
        }
    }
    (quad_pos == 0).then_some(out)
}

/// `table_a2b_base64[byte]`, `None` for the entries that table marks `-1`.
fn base64_value(byte: u8) -> Option<u32> {
    Some(match byte {
        b'A'..=b'Z' => u32::from(byte - b'A'),
        b'a'..=b'z' => u32::from(byte - b'a') + 26,
        b'0'..=b'9' => u32::from(byte - b'0') + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    })
}

/// `bytes.decode("utf-8", errors="ignore")[:limit]` without decoding the rest.
///
/// Invalid sequences are skipped one byte at a time, which is what CPython's
/// UTF-8 decoder does for `errors="ignore"`.
fn utf8_ignore_prefix(bytes: &[u8], limit: usize) -> String {
    let mut out = String::with_capacity(limit * 4);
    let mut index = 0;
    let mut chars = 0;
    while index < bytes.len() && chars < limit {
        match std::str::from_utf8(&bytes[index..]) {
            Ok(text) => {
                for character in text.chars() {
                    if chars == limit {
                        break;
                    }
                    out.push(character);
                    chars += 1;
                }
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if let Ok(text) = std::str::from_utf8(&bytes[index..index + valid]) {
                    for character in text.chars() {
                        if chars == limit {
                            return out;
                        }
                        out.push(character);
                        chars += 1;
                    }
                }
                index += valid + 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from `plane.utils.content_validator` and
    /// `plane.utils.html_processor` (CPython 3.12.13, nh3 0.2.18) by
    /// `tools/descriptions/gen_description_corpus.py`.
    const GOLDEN: &str = include_str!("../../tests/golden/description-validators.json");

    #[test]
    fn validate_html_content_matches_nh3() {
        let parsed: serde_json::Value = serde_json::from_str(GOLDEN).expect("golden json");
        let cases = parsed["validate_html_content"].as_array().expect("cases");
        assert!(cases.len() > 30, "corpus shrank: {}", cases.len());
        for case in cases {
            let input = case["input"].as_str().expect("input");
            let expected = match case["clean"].as_str() {
                Some(clean) => HtmlValidation::Sanitized(clean.to_owned()),
                None => HtmlValidation::Unchanged,
            };
            assert_eq!(validate_html_content(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn validate_binary_data_matches_python() {
        let parsed: serde_json::Value = serde_json::from_str(GOLDEN).expect("golden json");
        let cases = parsed["validate_binary_data"].as_array().expect("cases");
        assert!(cases.len() > 15, "corpus shrank: {}", cases.len());
        for case in cases {
            let input = case["input"].as_str().expect("input");
            let expected = case["is_valid"].as_bool().expect("is_valid");
            assert_eq!(validate_binary_data(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn the_error_bodies_are_drf_lists() {
        for (response, expected) in [
            (
                html_invalid_error(),
                r#"{"error":["html content is not valid"]}"#,
            ),
            (
                binary_invalid_error(),
                r#"{"description_binary":["Invalid binary data"]}"#,
            ),
        ] {
            match response {
                ViewResponse::Raw { status, body, .. } => {
                    assert_eq!(status, 400);
                    assert_eq!(String::from_utf8(body).expect("utf8"), expected);
                }
                _ => panic!("expected a rendered body"),
            }
        }
    }

    #[test]
    fn the_html_limit_is_the_utf8_byte_length() {
        // 10 MB of ASCII is over the limit only by one byte; a 4-byte character
        // counts as four.
        let at_limit = "a".repeat(MAX_SIZE);
        assert!(matches!(
            validate_html_content(&at_limit),
            HtmlValidation::Sanitized(_)
        ));
        let over = "a".repeat(MAX_SIZE + 1);
        assert_eq!(validate_html_content(&over), HtmlValidation::Invalid);
        let unicode = "\u{1F600}".repeat(MAX_SIZE / 4 + 1);
        assert_eq!(validate_html_content(&unicode), HtmlValidation::Invalid);
    }

    /// The page serializer's hook, pinned by
    /// `PATCH …/pages/{id}/description/` responses on the dev stack (Django
    /// 5.2.17, nh3 0.2.18): `"!!!"` decodes to nothing and is **valid**, while
    /// a 1-byte decode is "too short".
    #[test]
    fn validate_page_binary_matches_the_serializer_hook() {
        assert_eq!(
            validate_page_binary(""),
            Ok(PageBinary::Blank(String::new()))
        );
        assert_eq!(validate_page_binary("!!!"), Ok(PageBinary::Bytes(vec![])));
        assert_eq!(validate_page_binary("===="), Ok(PageBinary::Bytes(vec![])));
        assert_eq!(
            validate_page_binary("aGVsbG8gd29ybGQ="),
            Ok(PageBinary::Bytes(b"hello world".to_vec()))
        );
        assert_eq!(validate_page_binary("YQ=="), Err(BINARY_TOO_SHORT));
        // `base64.b64decode` requires a multiple of four with at most two
        // trailing `=`.
        assert_eq!(validate_page_binary("12345"), Err(BINARY_DECODE_FAILED));
        assert_eq!(validate_page_binary("PHNjcmlwdD4="), Err(BINARY_SUSPICIOUS));
        // Case-insensitive.
        assert_eq!(validate_page_binary("PElGUkFNRT4="), Err(BINARY_SUSPICIOUS));
        // Only the first 200 decoded characters are inspected: 201 `a`s then
        // `<IFRAME>` is accepted.
        let padded = format!("{}PElGUkFNRT4=", "YWFh".repeat(67));
        assert_eq!(
            validate_page_binary(&padded),
            Ok(PageBinary::Bytes({
                let mut bytes = vec![b'a'; 201];
                bytes.extend_from_slice(b"<IFRAME>");
                bytes
            }))
        );
    }

    #[test]
    fn binary_data_reason_covers_the_three_refusals() {
        assert_eq!(binary_data_reason(&[]), None);
        assert_eq!(binary_data_reason(b"abc"), Some(BINARY_TOO_SHORT));
        assert_eq!(
            binary_data_reason(&vec![b'a'; MAX_SIZE + 1]),
            Some(BINARY_TOO_LARGE)
        );
        assert_eq!(binary_data_reason(b"<html><body>"), Some(BINARY_SUSPICIOUS));
        assert_eq!(binary_data_reason(b"%PDF-1.7"), None);
    }

    #[test]
    fn validate_html_reason_adds_the_message_the_page_route_renders() {
        assert_eq!(validate_html_reason(""), Ok(None));
        assert_eq!(
            validate_html_reason("<p>x</p>"),
            Ok(Some("<p>x</p>".to_owned()))
        );
        assert_eq!(
            validate_html_reason(&"a".repeat(MAX_SIZE + 1)),
            Err(HTML_TOO_LARGE)
        );
        // `nh3.clean` drops the element and its text; Django stores the same
        // bytes (checked through the dev stack's description PATCH).
        assert_eq!(
            validate_html_reason("<script>alert(1)</script><p>x</p>"),
            Ok(Some("<p>x</p>".to_owned()))
        );
    }
}
