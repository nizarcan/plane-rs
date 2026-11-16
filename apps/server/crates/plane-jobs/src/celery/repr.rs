// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Celery's `argsrepr`/`kwargsrepr` renderer.
//!
//! Celery does **not** use Python's `repr()` for these headers: it uses
//! `celery.utils.saferepr.saferepr` (`celery/app/amqp.py:320-323`), which has
//! its own escaping and truncation rules. They agree with `repr()` for the
//! common case — printable strings, integers, booleans, `None`,
//! `UUID('…')`, dicts and lists — but differ where it matters for a
//! byte-comparison:
//!
//! * a string is wrapped in single quotes and only `'` is escaped;
//!   backslashes, newlines, tabs and control characters are emitted raw
//!   (`_format_chars`, `celery/utils/saferepr.py:141-146`);
//! * a string is truncated to the remaining budget with
//!   `celery.utils.text.truncate`, which cuts at the last space *inside* the
//!   budget and appends `...` (`text.py:89-93`);
//! * the 1024-character budget is shared by the whole value; when it runs out
//!   the renderer emits `, ...` and then every closing bracket still open
//!   (`_saferepr`, `saferepr.py:158-183`);
//! * containers deeper than `maxlevels=3` render as `{...}`, `[...]`,
//!   `(...)` (`reprstream`, `saferepr.py:255-257`);
//! * floats and ints use `str(value)` (`safe_t = (Number,)`,
//!   `saferepr.py:225-226`), so `1.0` stays `1.0` and `1e-05` stays `1e-05`.
//!
//! Those differences are informational only — the worker ignores both headers
//! — but the golden test compares them, and the differential harness
//! normalises them (`TASKS.md` §2.7).

use uuid::Uuid;

use crate::celery::args::TaskArg;

/// `app.conf.argsrepr_maxsize` / `kwargsrepr_maxsize` (both 1024).
pub const SAFEREPR_MAXLEN: i64 = 1024;

/// `reprstream`'s `maxlevels` default (`saferepr.py:197`).
const SAFEREPR_MAXLEVELS: usize = 3;

/// Render the positional argument tuple `args` the way
/// `saferepr(args, 1024)` does.
#[must_use]
pub fn saferepr_args(args: &[TaskArg]) -> String {
    let mut parts = Vec::with_capacity(args.len() * 2 + 2);
    parts.push(Part::literal("(", false));
    for (index, arg) in args.iter().enumerate() {
        if index > 0 {
            parts.push(Part::literal(", ", true));
        }
        parts.push(Part::Value(arg));
    }
    // `_chainlist` + `LIT_TUPLE_END_SV`: a one-element tuple renders `(x,)`.
    if args.len() == 1 {
        parts.push(Part::literal(",)", false));
    } else {
        parts.push(Part::literal(")", false));
    }
    render(parts, SAFEREPR_MAXLEN)
}

/// Render the keyword argument dict `kwargs` the way
/// `saferepr(kwargs, 1024)` does.
#[must_use]
pub fn saferepr_kwargs(kwargs: &[(&'static str, TaskArg)]) -> String {
    let mut parts = Vec::with_capacity(kwargs.len() * 4 + 2);
    parts.push(Part::literal("{", false));
    for (index, (key, value)) in kwargs.iter().enumerate() {
        if index > 0 {
            parts.push(Part::literal(", ", true));
        }
        parts.push(Part::Key(key));
        parts.push(Part::literal(": ", true));
        parts.push(Part::Value(value));
    }
    parts.push(Part::literal("}", false));
    render(parts, SAFEREPR_MAXLEN)
}

/// Render a single value (used by the repr golden test).
#[must_use]
pub fn saferepr_value(value: &TaskArg) -> String {
    render(vec![Part::Value(value)], SAFEREPR_MAXLEN)
}

/// Render a single value with an explicit budget (test-only).
#[cfg(test)]
pub(crate) fn saferepr_with_maxlen(value: &TaskArg, maxlen: i64) -> String {
    render(vec![Part::Value(value)], maxlen)
}

/// A renderable piece of the token stream, mirroring `reprstream`'s yields.
#[derive(Debug)]
enum Part<'a> {
    /// `_literal(value, truncate, direction)`. `direction` is derived from
    /// the bracket text; the renderer tracks the container depth from it.
    Literal { text: &'static str, truncate: bool },
    /// `_key(value)`: a dict key rendered through `saferepr`.
    Key(&'a str),
    /// `_quoted(value)`: a `str` rendered by `_format_chars`.
    Quoted(&'a str),
    /// A dict key materialised from a parsed JSON value (a `PyStr` may not be
    /// valid UTF-8 without loss).
    OwnedKey(String),
    /// A string value materialised from a parsed JSON value.
    OwnedQuoted(String),
    /// A scalar already rendered to text (`_safetext(truncate(token))`).
    Raw(String),
    /// A value that has not been expanded yet.
    Value(&'a TaskArg),
    /// A parsed JSON value that has not been expanded yet. `request.data` is
    /// such a value (`TaskArg::Json`); it must go through the same
    /// `maxlevels`/truncation stream as a `TaskArg` container.
    Json(&'a plane_serde::pyjson::Value),
}

impl<'a> Part<'a> {
    fn literal(text: &'static str, truncate: bool) -> Self {
        Self::Literal { text, truncate }
    }
}

/// Expand a value into parts. Containers emit their opening and closing
/// brackets plus separators; scalars become quoted/raw tokens.
fn push_value<'a>(parts: &mut Vec<Part<'a>>, value: &'a TaskArg) {
    match value {
        TaskArg::Null => parts.push(Part::Raw("None".to_string())),
        TaskArg::Bool(true) => parts.push(Part::Raw("True".to_string())),
        TaskArg::Bool(false) => parts.push(Part::Raw("False".to_string())),
        TaskArg::Int(number) => parts.push(Part::Raw(number.to_string())),
        TaskArg::Float(number) => parts.push(Part::Raw(py_float_str(*number))),
        TaskArg::Str(text) | TaskArg::JsonText(text) => parts.push(Part::Quoted(text)),
        TaskArg::Uuid(uuid) => parts.push(Part::Raw(uuid_repr(*uuid))),
        TaskArg::List(items) => {
            parts.push(Part::literal("[", false));
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    parts.push(Part::literal(", ", true));
                }
                parts.push(Part::Value(item));
            }
            parts.push(Part::literal("]", false));
        }
        TaskArg::Map(entries) => {
            parts.push(Part::literal("{", false));
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    parts.push(Part::literal(", ", true));
                }
                parts.push(Part::Key(key));
                parts.push(Part::literal(": ", true));
                parts.push(Part::Value(item));
            }
            parts.push(Part::literal("}", false));
        }
        TaskArg::Json(json) => parts.push(Part::Json(json)),
    }
}

/// Expand one level of a parsed JSON value. Nested containers stay as
/// [`Part::Json`] so `render` can apply `maxlevels` and the budget to them,
/// exactly as it does for a `TaskArg` container.
fn push_json<'a>(parts: &mut Vec<Part<'a>>, value: &'a plane_serde::pyjson::Value) {
    use plane_serde::pyjson::Value;
    match value {
        Value::Null => parts.push(Part::Raw("None".to_string())),
        Value::Bool(true) => parts.push(Part::Raw("True".to_string())),
        Value::Bool(false) => parts.push(Part::Raw("False".to_string())),
        Value::Int(number) => parts.push(Part::Raw(number.to_string())),
        Value::Float(number) => parts.push(Part::Raw(py_float_str(*number))),
        Value::Str(text) => {
            parts.push(Part::OwnedQuoted(text.to_string_lossy().into_owned()));
        }
        Value::List(items) => {
            parts.push(Part::literal("[", false));
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    parts.push(Part::literal(", ", true));
                }
                parts.push(Part::Json(item));
            }
            parts.push(Part::literal("]", false));
        }
        Value::Dict(dict) => {
            parts.push(Part::literal("{", false));
            for (index, (key, item)) in dict.iter().enumerate() {
                if index > 0 {
                    parts.push(Part::literal(", ", true));
                }
                parts.push(Part::OwnedKey(key.to_string_lossy().into_owned()));
                parts.push(Part::literal(": ", true));
                parts.push(Part::Json(item));
            }
            parts.push(Part::literal("}", false));
        }
    }
}

/// The token stream plus the renderer's budget.
///
/// Port of `_saferepr` + `reprstream` (`celery/utils/saferepr.py:158-269`):
/// a depth-first traversal where descending into a container splices the
/// child's parts in front of the rest of the current frame, exactly like
/// `chain([lit_start], val, [_dirty, lit_end], it)`. That is why a partially
/// rendered value can end in a mismatched closing bracket
/// (`{'a': {'b': , ...]}}`) — the bug is Celery's, and reproducing it keeps
/// the header bytes identical.
fn render(mut parts: Vec<Part<'_>>, maxlen: i64) -> String {
    let mut out = String::new();
    let mut budget = maxlen;
    let mut level: usize = 0;
    let mut index = 0;
    loop {
        if index >= parts.len() {
            break;
        }
        if budget <= 0 {
            out.push_str(", ...");
            // Only non-truncating literals survive: `_saferepr`'s dangling
            // bracket loop.
            for part in &parts[index..] {
                if let Part::Literal {
                    text,
                    truncate: false,
                } = part
                {
                    out.push_str(text);
                }
            }
            break;
        }
        match &parts[index] {
            Part::Literal { text, .. } => {
                if *text == "[" || *text == "{" || *text == "(" {
                    level += 1;
                } else if *text == "]" || *text == "}" || *text == ")" || *text == ",)" {
                    level = level.saturating_sub(1);
                }
                out.push_str(text);
                budget -= char_len(text);
                index += 1;
            }
            Part::Key(key) => {
                let text = format_chars(key, budget);
                budget -= char_len(&text);
                out.push_str(&text);
                index += 1;
            }
            Part::Quoted(text) => {
                let text = format_chars(text, budget);
                budget -= char_len(&text);
                out.push_str(&text);
                index += 1;
            }
            Part::OwnedKey(key) | Part::OwnedQuoted(key) => {
                let text = format_chars(key, budget);
                budget -= char_len(&text);
                out.push_str(&text);
                index += 1;
            }
            Part::Raw(text) => {
                let text = truncate(text, budget);
                budget -= char_len(&text);
                out.push_str(&text);
                index += 1;
            }
            Part::Value(value) => {
                if is_container(value) {
                    if level >= SAFEREPR_MAXLEVELS {
                        let text = collapsed(value);
                        out.push_str(text);
                        budget -= char_len(text);
                        index += 1;
                    } else {
                        // Splice the child in front of the rest of this frame.
                        let mut child = Vec::new();
                        push_value(&mut child, value);
                        parts.splice(index..=index, child);
                        // `index` now points at the child's opening bracket.
                    }
                } else {
                    let mut child = Vec::new();
                    push_value(&mut child, value);
                    parts.splice(index..=index, child);
                }
            }
            Part::Json(value) => {
                if is_json_container(value) {
                    if level >= SAFEREPR_MAXLEVELS {
                        let text = if matches!(value, plane_serde::pyjson::Value::List(_)) {
                            "[...]"
                        } else {
                            "{...}"
                        };
                        out.push_str(text);
                        budget -= char_len(text);
                        index += 1;
                    } else {
                        let mut child = Vec::new();
                        push_json(&mut child, value);
                        parts.splice(index..=index, child);
                    }
                } else {
                    let mut child = Vec::new();
                    push_json(&mut child, value);
                    parts.splice(index..=index, child);
                }
            }
        }
    }
    out
}

fn is_json_container(value: &plane_serde::pyjson::Value) -> bool {
    matches!(
        value,
        plane_serde::pyjson::Value::List(_) | plane_serde::pyjson::Value::Dict(_)
    )
}

fn is_container(value: &TaskArg) -> bool {
    match value {
        TaskArg::List(_) | TaskArg::Map(_) => true,
        TaskArg::Json(json) => {
            matches!(
                json,
                plane_serde::pyjson::Value::List(_) | plane_serde::pyjson::Value::Dict(_)
            )
        }
        _ => false,
    }
}

fn collapsed(value: &TaskArg) -> &'static str {
    match value {
        TaskArg::List(_) => "[...]",
        TaskArg::Map(_) => "{...}",
        TaskArg::Json(plane_serde::pyjson::Value::List(_)) => "[...]",
        TaskArg::Json(plane_serde::pyjson::Value::Dict(_)) => "{...}",
        _ => "",
    }
}

/// `_format_chars` (`saferepr.py:141-146`): `truncate` first, then escape `'`.
fn format_chars(value: &str, maxlen: i64) -> String {
    let truncated = truncate(value, maxlen);
    let mut out = String::with_capacity(truncated.len() + 2);
    out.push('\'');
    for character in truncated.chars() {
        if character == '\'' {
            out.push('\\');
        }
        out.push(character);
    }
    out.push('\'');
    out
}

/// `celery.utils.text.truncate` (`text.py:89-93`).
///
/// `len(s) >= maxlen` is checked with a bounded iterator: a multi-megabyte
/// page description must not be counted end to end just to fill a 1024-byte
/// budget.
fn truncate(value: &str, maxlen: i64) -> String {
    if maxlen > 0 {
        let limit = usize::try_from(maxlen).unwrap_or(usize::MAX);
        if limit > 0 && value.chars().nth(limit - 1).is_some() {
            let prefix: String = value.chars().take(limit).collect();
            let cut = match prefix.rfind(' ') {
                Some(position) => &prefix[..position],
                None => prefix.as_str(),
            };
            let mut out = String::with_capacity(cut.len() + 3);
            out.push_str(cut);
            out.push_str("...");
            return out;
        }
    }
    value.to_string()
}

fn char_len(value: &str) -> i64 {
    i64::try_from(value.chars().count()).unwrap_or(i64::MAX)
}

fn uuid_repr(uuid: Uuid) -> String {
    let mut out = String::with_capacity(40);
    out.push_str("UUID('");
    out.push_str(&uuid.hyphenated().to_string());
    out.push_str("')");
    out
}

/// `str(float)`: Python's `float.__repr__`.
///
/// Ported from `plane-serde/src/float.rs` because that module keeps its
/// formatter private. CPython uses the shortest round-tripping digits (ryu),
/// positional notation for a decimal point in `(-4, 16]` (the `decpt`
/// position counted from the digit string), a mandatory `.0`, and a signed
/// two-digit exponent (`1e-05`, `1e+16`).
fn py_float_str(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .to_string();
    }

    let negative = value.is_sign_negative();
    let abs = value.abs();
    let mut out = String::with_capacity(26);
    if negative {
        out.push('-');
    }
    if abs == 0.0 {
        out.push_str("0.0");
        return out;
    }

    let mut buffer = ryu::Buffer::new();
    let text = buffer.format_finite(abs);
    let (mantissa, exponent) = match text.split_once('e') {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().unwrap_or_default()),
        None => (text, 0),
    };
    let (integer_part, fraction_part) = match mantissa.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (mantissa, ""),
    };

    let mut digits: Vec<u8> = Vec::with_capacity(24);
    for byte in integer_part.bytes().chain(fraction_part.bytes()) {
        if !byte.is_ascii_digit() || digits.len() == 24 {
            // Not reachable for a finite ryu rendering; fall back to Rust's
            // shortest round-trip form rather than panicking.
            return format!("{value}");
        }
        digits.push(byte);
    }
    let mut decpt = i32::try_from(integer_part.len()).unwrap_or(0) + exponent;

    let mut start = 0;
    while start + 1 < digits.len() && digits[start] == b'0' {
        start += 1;
        decpt -= 1;
    }
    while digits.len() - start > 1 && digits[digits.len() - 1] == b'0' {
        digits.pop();
    }
    let digits = &digits[start..];

    if decpt <= -4 || decpt > 16 {
        out.push(char::from(digits[0]));
        if digits.len() > 1 {
            out.push('.');
            for byte in &digits[1..] {
                out.push(char::from(*byte));
            }
        }
        out.push('e');
        let exp = decpt - 1;
        if exp < 0 {
            out.push('-');
        } else {
            out.push('+');
        }
        let magnitude = exp.unsigned_abs();
        if magnitude >= 100 {
            out.push(char::from(b'0' + ((magnitude / 100) % 10) as u8));
            out.push(char::from(b'0' + ((magnitude / 10) % 10) as u8));
            out.push(char::from(b'0' + (magnitude % 10) as u8));
        } else {
            out.push(char::from(b'0' + (magnitude / 10) as u8));
            out.push(char::from(b'0' + (magnitude % 10) as u8));
        }
    } else if decpt <= 0 {
        out.push_str("0.");
        for _ in 0..decpt.unsigned_abs() {
            out.push('0');
        }
        for byte in digits {
            out.push(char::from(*byte));
        }
    } else if (decpt as usize) >= digits.len() {
        for byte in digits {
            out.push(char::from(*byte));
        }
        for _ in 0..(decpt as usize - digits.len()) {
            out.push('0');
        }
        out.push_str(".0");
    } else {
        let point = decpt as usize;
        for byte in &digits[..point] {
            out.push(char::from(*byte));
        }
        out.push('.');
        for byte in &digits[point..] {
            out.push(char::from(*byte));
        }
    }
    out
}
