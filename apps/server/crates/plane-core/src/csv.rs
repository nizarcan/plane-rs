// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python's `csv` module and Plane's CSV sanitiser, as far as Plane uses them.
//!
//! Two call sites exist in the app surface:
//!
//! * `ExportWorkspaceUserActivityEndpoint.generate_csv_from_rows`
//!   (`plane/app/views/workspace/base.py:343-349`) writes every row with
//!   `csv.writer(buffer, delimiter=",", quoting=csv.QUOTE_ALL)`;
//! * `plane/utils/csv_utils.py:10-26` runs `sanitize_csv_row` over each row
//!   first, prefixing a `'` to any **string** that starts with a
//!   formula-triggering character.
//!
//! The writer is byte-compatible with CPython 3.12's `_csv` for the one dialect
//! Plane asks for (`QUOTE_ALL`): every field is wrapped in `"`, an embedded `"`
//! is doubled, the line terminator is `\r\n`, and `None` is an **empty quoted
//! field** (`""`). Non-string values are `str()`-ed by the caller — `csv` itself
//! does that, and a datetime's `str()` is `isoformat(" ")`.
//!
//! The golden corpus `tests/golden/csv-writer.json` is recorded from the real
//! module by `tools/inventory/csv_corpus.py`; `tests/csv.rs` compares whole
//! documents, not field by field.

use std::borrow::Cow;

/// Characters that trigger formula evaluation in spreadsheet applications
/// (`plane/utils/csv_utils.py:7`).
pub const FORMULA_TRIGGERS: [char; 7] = ['=', '+', '-', '@', '\t', '\r', '\n'];

/// `sanitize_csv_value` (`plane/utils/csv_utils.py:10-21`).
///
/// `if isinstance(value, str) and value and value[0] in _CSV_FORMULA_TRIGGERS`
/// — only a string is touched, an empty string is left alone (so `value[0]`
/// never raises), and the test is on the first **character**.
#[must_use]
pub fn sanitize_csv_value(value: &str) -> Cow<'_, str> {
    match value.chars().next() {
        Some(first) if FORMULA_TRIGGERS.contains(&first) => Cow::Owned(format!("'{value}")),
        _ => Cow::Borrowed(value),
    }
}

/// `sanitize_csv_row` (`plane/utils/csv_utils.py:24-26`): the same function over
/// every field, keeping `None` as `None`.
#[must_use]
pub fn sanitize_csv_row<'a>(
    row: impl IntoIterator<Item = Option<&'a str>>,
) -> Vec<Option<Cow<'a, str>>> {
    row.into_iter()
        .map(|value| value.map(sanitize_csv_value))
        .collect()
}

/// One `csv.writer(...).writerow(row)` call with `delimiter=","` and
/// `quoting=csv.QUOTE_ALL`.
///
/// `None` is Python's `None` (an empty quoted field); every other field is the
/// text `str()` produced for the Python value. Appends the `\r\n` terminator.
pub fn write_row<'a>(out: &mut String, row: impl IntoIterator<Item = Option<&'a str>>) {
    let mut first = true;
    for field in row {
        if !first {
            out.push(',');
        }
        first = false;
        out.push('"');
        if let Some(text) = field {
            for ch in text.chars() {
                if ch == '"' {
                    out.push('"');
                }
                out.push(ch);
            }
        }
        out.push('"');
    }
    out.push('\r');
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_leading_trigger_character_is_prefixed() {
        for value in ["=1+2", "+1", "-1", "@x", "\tx", "\rx", "\nx"] {
            assert_eq!(sanitize_csv_value(value), format!("'{value}"));
        }
        for value in ["", "x=1", " =1", "'=1", "1+1"] {
            assert_eq!(sanitize_csv_value(value), value);
        }
    }

    #[test]
    fn a_row_is_quoted_and_terminated_with_crlf() {
        let mut out = String::new();
        write_row(&mut out, [Some("a"), None, Some("")]);
        assert_eq!(out, "\"a\",\"\",\"\"\r\n");
        out.clear();
        write_row(&mut out, [Some("he said \"hi\"")]);
        assert_eq!(out, "\"he said \"\"hi\"\"\"\r\n");
        out.clear();
        write_row(&mut out, [Some("a\nb"), Some("a\r\nb")]);
        assert_eq!(out, "\"a\nb\",\"a\r\nb\"\r\n");
        out.clear();
        write_row(&mut out, [Some("héllo → 世界 🚀")]);
        assert_eq!(out, "\"héllo → 世界 🚀\"\r\n");
    }
}
