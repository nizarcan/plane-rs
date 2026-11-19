// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The small part of PyYAML's `SafeDumper` the OpenAPI responses need.
//!
//! `drf_spectacular.renderers.OpenApiYamlRenderer` dumps the schema with
//! `yaml.safe_dump(..., allow_unicode=True)` plus a string representer that
//! forces `|` style for a string containing a newline. The two byte streams
//! this crate renders are:
//!
//! * `OPTIONS /api/schema/` — `SimpleMetadata`'s `{name, description, renders,
//!   parses}` dict (`rest_framework/metadata.py`), and
//! * `GET /api/schema/?version=…` — the committed schema with one scalar
//!   (`info.version`) replaced.
//!
//! Both are strings, so this module reproduces PyYAML's scalar style choice and
//! escaping, not the whole emitter. [`scalar`] is checked against
//! `deploy/openapi/version-scalars.json`, a corpus recorded from Python's
//! `yaml` through the same dumper (`tools/openapi/capture.py`).

/// PyYAML's `ESCAPE_REPLACEMENTS` (subset reachable with `allow_unicode=True`).
fn replacement(ch: char) -> Option<&'static str> {
    Some(match ch {
        '\0' => "\\0",
        '\x07' => "\\a",
        '\x08' => "\\b",
        '\t' => "\\t",
        '\n' => "\\n",
        '\x0b' => "\\v",
        '\x0c' => "\\f",
        '\r' => "\\r",
        '\x1b' => "\\e",
        '"' => "\\\"",
        '\\' => "\\\\",
        '\u{85}' => "\\N",
        '\u{2028}' => "\\L",
        '\u{2029}' => "\\P",
        _ => return None,
    })
}

/// Whether PyYAML treats `ch` as printable with `allow_unicode=True`
/// (`yaml/emitter.py`'s `analyze_scalar`): printable ASCII, NBSP, and the
/// non-surrogate BMP plus astral ranges.
fn printable(ch: char) -> bool {
    matches!(ch,
        '\t' | '\n' | '\r'
        | '\u{20}'..='\u{7e}'
        | '\u{85}'
        | '\u{a0}'..='\u{d7ff}'
        | '\u{e000}'..='\u{fffd}'
        | '\u{10000}'..='\u{10ffff}')
}

fn needs_double_quote(value: &str) -> bool {
    value.chars().any(|ch| {
        ch == '\t'
            || ch == '\r'
            || ch == '\u{85}'
            || ch == '\u{2028}'
            || ch == '\u{2029}'
            || !printable(ch)
    })
}

fn needs_single_quote(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    // `: ` and ` #` end a plain scalar; the values this module renders begin
    // with `0.0.1 (` or are a metadata string, so the remaining indicator
    // rules (`-` at the start, `?`, `:`, …) cannot apply.
    value.contains(": ") || value.contains(" #")
}

/// Whether `value` would be read back as a non-string under YAML 1.1's implicit
/// resolvers. PyYAML quotes those to keep them strings.
fn looks_like_other_type(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "" | "~"
            | "null"
            | "true"
            | "false"
            | "yes"
            | "no"
            | "on"
            | "off"
            | "y"
            | "n"
            | ".nan"
            | ".inf"
            | "+.inf"
            | "-.inf"
    ) {
        return true;
    }
    if lower.parse::<i64>().is_ok() || lower.parse::<f64>().is_ok() {
        return true;
    }
    // `2026-06-15` and the timestamp forms.
    let bytes = value.as_bytes();
    bytes.len() >= 8
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
}

/// Encode `value` as a YAML scalar.
///
/// `indent` is the column the scalar's first character starts at; a block
/// literal's continuation lines are indented by two more spaces, as PyYAML
/// emits them. The caller writes `key: ` (or `- `) before the result.
#[must_use]
pub fn scalar(value: &str, indent: usize) -> String {
    let has_newline = value.contains('\n');
    if !has_newline
        && !needs_double_quote(value)
        && !needs_single_quote(value)
        && !looks_like_other_type(value)
    {
        return value.to_owned();
    }
    if has_newline && !needs_double_quote(value) {
        // PyYAML's string representer forces `|` for a string with a newline.
        // The values rendered here never end with a newline (the API version is
        // wrapped in `0.0.1 (…)`), so the chomping indicator is `-`.
        let mut out = String::from("|-\n");
        let padding = " ".repeat(indent + 2);
        for (index, line) in value.split('\n').enumerate() {
            if index > 0 {
                out.push('\n');
            }
            if !line.is_empty() {
                out.push_str(&padding);
                out.push_str(line);
            }
        }
        return out;
    }
    if !needs_double_quote(value) {
        // Single-quoted style: only `'` is doubled.
        let mut out = String::with_capacity(value.len() + 2);
        out.push('\'');
        for ch in value.chars() {
            if ch == '\'' {
                out.push('\'');
            }
            out.push(ch);
        }
        out.push('\'');
        return out;
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if let Some(escape) = replacement(ch) {
            out.push_str(escape);
        } else if printable(ch) {
            out.push(ch);
        } else {
            let code = u32::from(ch);
            if code <= 0xff {
                out.push_str(&format!("\\x{code:02X}"));
            } else if code <= 0xffff {
                out.push_str(&format!("\\u{code:04X}"));
            } else {
                out.push_str(&format!("\\U{code:08X}"));
            }
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_values_stay_plain() {
        assert_eq!(scalar("Spectacular Api", 2), "Spectacular Api");
        assert_eq!(scalar("0.0.1 (1)", 2), "0.0.1 (1)");
        assert_eq!(scalar("0.0.1 (é)", 2), "0.0.1 (é)");
        assert_eq!(scalar("0.0.1 (100%)", 2), "0.0.1 (100%)");
    }

    #[test]
    fn indicators_and_resolvers_are_quoted() {
        assert_eq!(scalar("0.0.1 (a: b)", 2), "'0.0.1 (a: b)'");
        assert_eq!(scalar("0.0.1 (x #y)", 2), "'0.0.1 (x #y)'");
        assert_eq!(scalar("true", 2), "'true'");
        assert_eq!(scalar("123", 2), "'123'");
        assert_eq!(scalar("", 2), "''");
    }

    #[test]
    fn newlines_use_a_block_literal() {
        assert_eq!(scalar("0.0.1 (x\ny)", 2), "|-\n    0.0.1 (x\n    y)");
        assert_eq!(scalar("a\n\nb", 2), "|-\n    a\n\n    b");
    }

    #[test]
    fn tabs_use_double_quotes() {
        assert_eq!(scalar("0.0.1 (a\tb)", 2), "\"0.0.1 (a\\tb)\"");
        assert_eq!(scalar("a\tb\"c", 2), "\"a\\tb\\\"c\"");
        assert_eq!(scalar("a\tb\\c", 2), "\"a\\tb\\\\c\"");
    }
}
