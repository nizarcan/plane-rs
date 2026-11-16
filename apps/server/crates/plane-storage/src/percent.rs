// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Percent-encoding and JSON string escaping exactly as the Python code does.
//!
//! Three different encoders appear on a presigned URL, and mixing them up is
//! the easiest way to produce a URL that looks right and fails verification:
//!
//! * `urllib.parse.quote(text, safe=...)` — used for the object key in the path
//!   (`safe="/~"`, `botocore/serialize.py:511-525`), the `Content-Disposition`
//!   filename (`safe="/"`, `plane/settings/storage.py:111`) and the
//!   `x-amz-copy-source` key. Uppercase hex, UTF-8 bytes, unreserved
//!   `A-Z a-z 0-9 _ . - ~` plus the `safe` characters are kept.
//! * `botocore.utils.percent_encode` — used for every query parameter value,
//!   with `SAFE_CHARS = "-._~"` (`botocore/utils.py:99`). The same bytes as
//!   `quote(value, safe="-._~")`.
//! * Python's `json.dumps(..., ensure_ascii=True)` string escaping for the POST
//!   policy document (`botocore/auth.py:807-846`): `"` and `\` escaped, the
//!   short control escapes, other C0 controls as `\u00XX`, non-ASCII as
//!   `\uXXXX` (lowercase hex) and astral characters as a UTF-16 surrogate pair.
//!
//! `urllib.parse.urlencode` also runs once inside botocore
//! (`botocore/awsrequest.py:375-386`), but the presign path immediately decodes
//! it again (`SigV4QueryAuth._modify_request_before_signing` → `parse_qs` →
//! `percent_encode_sequence`), so the final query string is
//! [`encode_component`] of the original value. The round trip is a no-op for
//! every byte sequence: `quote_plus` escapes `&`, `=`, `+`, `%` and space, and
//! `parse_qs` turns them back into the original characters. `quote` then
//! re-encodes them with `%20` for a space instead of `+`, which is what the
//! signed URL contains (`apps/api` tests pin this: a space in
//! `ResponseContentDisposition` appears as `%20`, and the `%` from
//! `quote(filename)` appears as `%25`).

/// `urllib.parse.quote(text, safe=extra_safe)` for `str` input.
///
/// `extra_safe` is a set of ASCII characters that are kept in addition to the
/// always-safe unreserved set. Non-ASCII characters are UTF-8 encoded and then
/// escaped byte by byte, with uppercase hex digits (`quote` uses `%XX` with
/// `string.hexdigits[:16].upper()`).
pub fn quote(text: &str, extra_safe: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for &byte in text.as_bytes() {
        if is_unreserved(byte) || extra_safe.as_bytes().contains(&byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    out
}

/// `botocore.utils.percent_encode(value)` — the query-string encoder.
pub fn encode_component(value: &str) -> String {
    quote(value, "-._~")
}

/// The object key path segment (`botocore/serialize.py:521`: `safe='/~'`).
pub fn encode_key(key: &str) -> String {
    quote(key, "/~")
}

/// `urllib.parse.quote(filename)` — Python's default `safe='/'`.
pub fn encode_filename(filename: &str) -> String {
    quote(filename, "/")
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'~')
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Append `text` escaped the way `json.dumps(text, ensure_ascii=True)` does.
pub fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                // CPython `py_encode_basestring_ascii` uses `\u%04x`, lowercase.
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x7f => out.push(c),
            c => {
                let code = c as u32;
                if code <= 0xffff {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    // UTF-16 surrogate pair, as `json.dumps` emits it.
                    let value = code - 0x1_0000;
                    let high = 0xd800 + (value >> 10);
                    let low = 0xdc00 + (value & 0x3ff);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_matches_python() {
        assert_eq!(quote("file name.png", "/~"), "file%20name.png");
        assert_eq!(quote("a/b~c", "/~"), "a/b~c");
        assert_eq!(quote("a/b c", ""), "a%2Fb%20c");
        assert_eq!(quote("é名前", "/~"), "%C3%A9%E5%90%8D%E5%89%8D");
        assert_eq!(
            quote("100% (1)+x#y.png", "/~"),
            "100%25%20%281%29%2Bx%23y.png"
        );
        assert_eq!(quote("a%2Fb", "/~"), "a%252Fb");
        assert_eq!(quote("'*!()", ""), "%27%2A%21%28%29");
        assert_eq!(quote("\u{1f389}", ""), "%F0%9F%8E%89");
        // Unicode whitespace and control characters are bytes, not characters.
        assert_eq!(quote("\u{00a0}", ""), "%C2%A0");
        assert_eq!(quote("\u{7f}", ""), "%7F");
    }

    #[test]
    fn component_keeps_reserved_characters_out() {
        assert_eq!(encode_component("a;b=c&d"), "a%3Bb%3Dc%26d");
        assert_eq!(encode_component("UTF-8''x"), "UTF-8%27%27x");
        assert_eq!(encode_component("a.b-c_d~"), "a.b-c_d~");
    }

    #[test]
    fn json_string_escapes_like_python() {
        let escape = |value: &str| {
            let mut out = String::new();
            push_json_string(&mut out, value);
            out
        };
        assert_eq!(escape("plain"), "\"plain\"");
        assert_eq!(escape("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(escape("a\nb\tc\rd"), "\"a\\nb\\tc\\rd\"");
        assert_eq!(
            escape("\u{08}\u{0c}\u{01}\u{1f}"),
            "\"\\b\\f\\u0001\\u001f\""
        );
        assert_eq!(escape("é名前"), "\"\\u00e9\\u540d\\u524d\"");
        assert_eq!(escape("🎉"), "\"\\ud83c\\udf89\"");
        // 0x7f *is* escaped by json.dumps (only 0x20..=0x7e pass through).
        assert_eq!(escape("\u{7f}"), "\"\\u007f\"");
        assert_eq!(escape("a/b"), "\"a/b\"");
    }
}
