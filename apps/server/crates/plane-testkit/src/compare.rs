// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The comparator: given two normalised results, produce a readable list of
//! differences.
//!
//! JSON is compared structurally with the wire key order and the exact number
//! lexemes kept (`serde_json`'s `preserve_order` and `arbitrary_precision`
//! features). D15 additionally checks that each side's raw body is byte-equal
//! to its own re-serialisation through a DRF-compatible compact formatter, so
//! structural equality of two Django-shaped bodies implies byte equality.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::celery::CeleryMessage;
use crate::db::{DbDiff, TableDiff};
use crate::normalize::{BodyValue, NormalizedBody};
use crate::redis::{RedisDiff, RedisEntry};
use crate::util::ellipsize;

const MAX_RENDERED_VALUE: usize = 100_000;

#[derive(Debug, Clone, Serialize)]
pub struct Difference {
    pub part: String,
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b: Option<String>,
}

impl Difference {
    pub fn new(
        part: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            part: part.into(),
            path: path.into(),
            message: message.into(),
            a: None,
            b: None,
        }
    }

    pub fn with_values(mut self, a: impl Into<String>, b: impl Into<String>) -> Self {
        self.a = Some(ellipsize(&a.into(), MAX_RENDERED_VALUE));
        self.b = Some(ellipsize(&b.into(), MAX_RENDERED_VALUE));
        self
    }

    /// A one-line header plus, when both sides have a value, a unified diff of
    /// the pretty-printed normalised text.
    pub fn render(&self) -> String {
        let mut out = format!("{} {}: {}", self.part, self.path, self.message);
        match (&self.a, &self.b) {
            (Some(a), Some(b)) => {
                out.push('\n');
                out.push_str(&unified_diff(a, b));
            }
            (Some(a), None) => {
                out.push_str("\nA only:\n");
                out.push_str(a);
                out.push('\n');
            }
            (None, Some(b)) => {
                out.push_str("\nB only:\n");
                out.push_str(b);
                out.push('\n');
            }
            (None, None) => out.push('\n'),
        }
        out
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonDiff {
    pub path: String,
    pub message: String,
}

/// A unified diff of two multi-line strings, as printed for every difference.
pub fn unified_diff(a: &str, b: &str) -> String {
    similar::TextDiff::from_lines(a, b)
        .unified_diff()
        .context_radius(3)
        .header("A", "B")
        .to_string()
}

// ---------------------------------------------------------------------------
// Status and headers
// ---------------------------------------------------------------------------

pub fn compare_status(a: u16, b: u16) -> Vec<Difference> {
    if a == b {
        Vec::new()
    } else {
        vec![
            Difference::new("status", "-", "status codes differ")
                .with_values(a.to_string(), b.to_string()),
        ]
    }
}

/// DECISIONS.md D18: a bodyless response's `Content-Length` is not part of the
/// contract. Django (uvicorn) sends `Content-Length: 0` on a 204; hyper omits
/// it, as RFC 9110 §8.6 requires, and Caddy strips it before any client sees
/// either form. The header is ignored only when both sides returned 204 and
/// the one value present is `0`, or when both returned 304 and only one side
/// sent the header. Everything else — a different value, a non-zero length on
/// a 204, or two different statuses — keeps its exact comparison.
pub const CONTENT_LENGTH_BODYLESS_RULE: &str = "content-length-bodyless";
pub const CONTENT_LENGTH_BODYLESS_WHY: &str = "DECISIONS.md D18: Django sends `Content-Length: 0` on a 204, hyper omits it, Caddy strips it; a 304 may omit the representation length.";

/// Compare headers with the response statuses known, applying D18.
///
/// Returns the differences and, when the bodyless `Content-Length` exception
/// fired, the rule name so the runner can list it as an applied rule.
pub fn compare_response_headers(
    a_status: u16,
    b_status: u16,
    a: &[(String, String)],
    b: &[(String, String)],
) -> (Vec<Difference>, Option<&'static str>) {
    if !content_length_is_bodyless(a_status, b_status, a, b) {
        return (compare_headers(a, b), None);
    }
    let without_content_length = |headers: &[(String, String)]| -> Vec<(String, String)> {
        headers
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("content-length"))
            .cloned()
            .collect()
    };
    (
        compare_headers(&without_content_length(a), &without_content_length(b)),
        Some(CONTENT_LENGTH_BODYLESS_RULE),
    )
}

/// The one `Content-Length` shape D18 allows: equal statuses of 204 or 304,
/// exactly one value across both sides, and (for a 204) that value is `0`.
fn content_length_is_bodyless(
    a_status: u16,
    b_status: u16,
    a: &[(String, String)],
    b: &[(String, String)],
) -> bool {
    if a_status != b_status || !matches!(a_status, 204 | 304) {
        return false;
    }
    let values = |headers: &[(String, String)]| -> Vec<String> {
        headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.trim().to_string())
            .collect()
    };
    let (a_values, b_values) = (values(a), values(b));
    // Exactly one side sent the header, once.
    if a_values.len() + b_values.len() != 1 {
        return false;
    }
    let present = a_values.first().or_else(|| b_values.first());
    a_status == 304 || present.is_some_and(|value| value == "0")
}

pub fn compare_headers(a: &[(String, String)], b: &[(String, String)]) -> Vec<Difference> {
    let a_map = header_map(a);
    let b_map = header_map(b);
    let names: BTreeSet<&String> = a_map.keys().chain(b_map.keys()).collect();
    let mut out = Vec::new();
    for name in names {
        match (a_map.get(name), b_map.get(name)) {
            (Some(a_values), Some(b_values)) => {
                if a_values.len() != b_values.len() {
                    out.push(
                        Difference::new(
                            "headers",
                            name,
                            format!(
                                "different number of values ({} on A, {} on B)",
                                a_values.len(),
                                b_values.len()
                            ),
                        )
                        .with_values(a_values.join("\n"), b_values.join("\n")),
                    );
                    continue;
                }
                for (index, (a_value, b_value)) in a_values.iter().zip(b_values.iter()).enumerate()
                {
                    if age_within_one_second(name, a_value, b_value) {
                        continue;
                    }
                    if presigned_urls_agree(a_value, b_value) {
                        continue;
                    }
                    if a_value != b_value {
                        out.push(
                            Difference::new(
                                "headers",
                                format!("{name}[{index}]"),
                                "header value differs",
                            )
                            .with_values(a_value.clone(), b_value.clone()),
                        );
                    }
                }
            }
            (Some(a_values), None) => out.push(
                Difference::new("headers", name, "header only present on A")
                    .with_values(a_values.join("\n"), ""),
            ),
            (None, Some(b_values)) => out.push(
                Difference::new("headers", name, "header only present on B")
                    .with_values("", b_values.join("\n")),
            ),
            (None, None) => {}
        }
    }
    out
}

/// Two canonical presigned URLs (DECISIONS.md D17) whose dates are at most two
/// seconds apart and whose remaining bytes are identical. Anything else —
/// a different path, parameter, credential scope or expiry — keeps its exact
/// comparison.
fn presigned_urls_agree(a: &str, b: &str) -> bool {
    crate::presign::equivalent(a, b) == Some(true)
}

/// `Age` is `int(now) - int(stored_at)` for a page-cache hit
/// (`FetchFromCacheMiddleware.process_request`). The two targets store the
/// page and observe the hit at their own wall-clock instants, so a second
/// boundary can legitimately put them one second apart. Every other header is
/// compared exactly, and so is `Age` when the two differ by more than one —
/// including when only one side sends it, which means one side hit the cache
/// and the other did not.
fn age_within_one_second(name: &str, a: &str, b: &str) -> bool {
    if !name.eq_ignore_ascii_case("age") {
        return false;
    }
    let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) else {
        return false;
    };
    (a - b).abs() <= 1
}

fn header_map(headers: &[(String, String)]) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, value) in headers {
        map.entry(name.to_ascii_lowercase())
            .or_default()
            .push(value.clone());
    }
    map
}

// ---------------------------------------------------------------------------
// Bodies
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormattingStatus {
    /// Not a JSON response.
    NotJson,
    Pass,
    Fail(String),
}

pub fn compare_bodies(a: &NormalizedBody, b: &NormalizedBody) -> Vec<Difference> {
    match (&a.value, &b.value) {
        (BodyValue::Json(a_json), BodyValue::Json(b_json)) => {
            let differences = structural_json_diffs(a_json, b_json);
            if differences.is_empty() {
                return Vec::new();
            }
            let summary = differences
                .iter()
                .take(5)
                .map(|difference| format!("{}: {}", difference.path, difference.message))
                .collect::<Vec<_>>()
                .join("; ");
            vec![
                Difference::new(
                    "body",
                    "json",
                    format!("{} structural difference(s): {summary}", differences.len()),
                )
                .with_values(a.pretty(), b.pretty()),
            ]
        }
        (BodyValue::Text(a_text), BodyValue::Text(b_text)) => {
            if a_text == b_text {
                Vec::new()
            } else {
                vec![
                    Difference::new("body", "text", "body text differs")
                        .with_values(a_text.clone(), b_text.clone()),
                ]
            }
        }
        (BodyValue::Binary(a_hex), BodyValue::Binary(b_hex)) => {
            if a_hex == b_hex {
                Vec::new()
            } else {
                vec![
                    Difference::new("body", "binary", "body bytes differ")
                        .with_values(a_hex.clone(), b_hex.clone()),
                ]
            }
        }
        _ => vec![
            Difference::new(
                "body",
                "type",
                "body representations differ (JSON vs text/binary)",
            )
            .with_values(a.pretty(), b.pretty()),
        ],
    }
}

/// D15: one side passing and the other failing the DRF byte rule is itself a
/// difference. Both sides failing is not: `JsonResponse` routes use Django's
/// spaced encoding on both backends.
pub fn compare_formatting(a: &FormattingStatus, b: &FormattingStatus) -> Vec<Difference> {
    match (a, b) {
        (FormattingStatus::Pass, FormattingStatus::Fail(error)) => vec![
            Difference::new(
                "body",
                "formatting",
                format!("A is DRF-compact, B is not: {error}"),
            )
            .with_values("DRF-compact", "not DRF-compact"),
        ],
        (FormattingStatus::Fail(error), FormattingStatus::Pass) => vec![
            Difference::new(
                "body",
                "formatting",
                format!("B is DRF-compact, A is not: {error}"),
            )
            .with_values("not DRF-compact", "DRF-compact"),
        ],
        _ => Vec::new(),
    }
}

/// Check that `raw` is byte-identical to the DRF-compatible compact
/// serialisation of its parsed value.
pub fn check_drf_formatting(raw: &[u8]) -> Result<(), String> {
    if raw.is_empty() {
        return Ok(());
    }
    let text = std::str::from_utf8(raw).map_err(|error| format!("not UTF-8: {error}"))?;
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("not valid JSON: {error}"))?;
    let reserialised = drf_compact(&value)?;
    if reserialised == text {
        Ok(())
    } else {
        let position = first_difference(text, &reserialised);
        Err(format!(
            "re-serialisation differs at byte {position}: {:?} vs {:?}",
            snippet(text, position),
            snippet(&reserialised, position)
        ))
    }
}

/// Compact JSON exactly as DRF's `JSONRenderer` writes it: `,`/`:` separators,
/// non-ASCII left raw, and U+2028/U+2029 escaped to six ASCII characters.
pub fn drf_compact(value: &Value) -> Result<String, String> {
    let text = serde_json::to_string(value).map_err(|error| error.to_string())?;
    Ok(text
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029"))
}

fn first_difference(a: &str, b: &str) -> usize {
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .position(|(x, y)| x != y)
        .unwrap_or_else(|| a.len().min(b.len()))
}

fn snippet(text: &str, position: usize) -> String {
    let start = position.saturating_sub(20);
    let bytes = text.as_bytes();
    let mut end = (position + 20).min(bytes.len());
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut start = start.min(end);
    while start < end && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..end].to_string()
}

/// Structural JSON comparison: key order matters, number lexemes matter.
pub fn structural_json_diffs(a: &Value, b: &Value) -> Vec<JsonDiff> {
    let mut out = Vec::new();
    walk_json("$", a, b, &mut out);
    out
}

fn walk_json(path: &str, a: &Value, b: &Value, out: &mut Vec<JsonDiff>) {
    match (a, b) {
        (Value::Object(a_map), Value::Object(b_map)) => {
            walk_object(path, a_map, b_map, out);
        }
        (Value::Array(a_items), Value::Array(b_items)) => {
            if a_items.len() != b_items.len() {
                out.push(JsonDiff {
                    path: path.to_string(),
                    message: format!(
                        "array length differs ({} on A, {} on B)",
                        a_items.len(),
                        b_items.len()
                    ),
                });
            }
            for (index, (a_item, b_item)) in a_items.iter().zip(b_items.iter()).enumerate() {
                walk_json(&format!("{path}[{index}]"), a_item, b_item, out);
            }
        }
        (Value::Number(a_number), Value::Number(b_number)) => {
            if a_number.to_string() != b_number.to_string() {
                out.push(JsonDiff {
                    path: path.to_string(),
                    message: format!("number lexeme differs ({a_number} vs {b_number})"),
                });
            }
        }
        (Value::String(a_text), Value::String(b_text)) => {
            if a_text != b_text && !presigned_urls_agree(a_text, b_text) {
                out.push(JsonDiff {
                    path: path.to_string(),
                    message: format!("string differs ({a_text:?} vs {b_text:?})"),
                });
            }
        }
        (Value::Bool(a_bool), Value::Bool(b_bool)) => {
            if a_bool != b_bool {
                out.push(JsonDiff {
                    path: path.to_string(),
                    message: format!("boolean differs ({a_bool} vs {b_bool})"),
                });
            }
        }
        (Value::Null, Value::Null) => {}
        _ => out.push(JsonDiff {
            path: path.to_string(),
            message: format!("type differs ({} vs {})", type_name(a), type_name(b)),
        }),
    }
}

fn walk_object(
    path: &str,
    a_map: &Map<String, Value>,
    b_map: &Map<String, Value>,
    out: &mut Vec<JsonDiff>,
) {
    let a_keys: Vec<&String> = a_map.keys().collect();
    let b_keys: Vec<&String> = b_map.keys().collect();
    let a_set: BTreeSet<&String> = a_keys.iter().copied().collect();
    let b_set: BTreeSet<&String> = b_keys.iter().copied().collect();
    if a_set == b_set && a_keys != b_keys {
        out.push(JsonDiff {
            path: path.to_string(),
            message: format!("key order differs ({:?} vs {:?})", a_keys, b_keys),
        });
    } else {
        for key in a_set.difference(&b_set) {
            out.push(JsonDiff {
                path: format!("{path}.{key}"),
                message: "key only present on A".to_string(),
            });
        }
        for key in b_set.difference(&a_set) {
            out.push(JsonDiff {
                path: format!("{path}.{key}"),
                message: "key only present on B".to_string(),
            });
        }
    }
    for (key, a_value) in a_map {
        if let Some(b_value) = b_map.get(key) {
            walk_json(&format!("{path}.{key}"), a_value, b_value, out);
        }
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// Compare step diffs as multisets per table: rows are sorted by their
/// canonical JSON, so row order from the heap does not matter.
pub fn compare_db(a: &DbDiff, b: &DbDiff) -> Vec<Difference> {
    let tables: BTreeSet<&String> = a.tables.keys().chain(b.tables.keys()).collect();
    let mut out = Vec::new();
    for table in tables {
        let a_value = canonical_table(a.tables.get(table));
        let b_value = canonical_table(b.tables.get(table));
        if a_value == b_value {
            continue;
        }
        let differences = structural_json_diffs(&a_value, &b_value);
        let summary = differences
            .iter()
            .take(5)
            .map(|difference| format!("{}: {}", difference.path, difference.message))
            .collect::<Vec<_>>()
            .join("; ");
        out.push(
            Difference::new(
                "db",
                table,
                format!(
                    "{} row change(s) across inserted/deleted/updated: {summary}",
                    differences.len()
                ),
            )
            .with_values(pretty(&a_value), pretty(&b_value)),
        );
    }
    out
}

fn canonical_table(table: Option<&TableDiff>) -> Value {
    let Some(table) = table else {
        return json!({});
    };
    let mut inserted = table.inserted.clone();
    let mut deleted = table.deleted.clone();
    let mut updated = table.updated.clone();
    inserted.sort_by_cached_key(canonical_key);
    deleted.sort_by_cached_key(canonical_key);
    updated.sort_by_cached_key(canonical_key);
    let mut object = Map::new();
    if !inserted.is_empty() {
        object.insert("inserted".to_string(), Value::Array(inserted));
    }
    if !deleted.is_empty() {
        object.insert("deleted".to_string(), Value::Array(deleted));
    }
    if !updated.is_empty() {
        object.insert("updated".to_string(), Value::Array(updated));
    }
    Value::Object(object)
}

fn canonical_key(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Celery
// ---------------------------------------------------------------------------

/// Compare drained Celery messages, grouped by queue and in delivery order.
pub fn compare_celery(a: &[CeleryMessage], b: &[CeleryMessage]) -> Vec<Difference> {
    let a_groups = queue_groups(a);
    let b_groups = queue_groups(b);
    let queues: BTreeSet<&String> = a_groups.keys().chain(b_groups.keys()).collect();
    let mut out = Vec::new();
    for queue in queues {
        let empty = Vec::new();
        let mut a_value = canonical_messages(a_groups.get(queue).unwrap_or(&empty));
        let mut b_value = canonical_messages(b_groups.get(queue).unwrap_or(&empty));
        reconcile_repr_headers(&mut a_value, &mut b_value);
        if a_value == b_value {
            continue;
        }
        let differences = structural_json_diffs(&a_value, &b_value);
        let summary = differences
            .iter()
            .take(5)
            .map(|difference| format!("{}: {}", difference.path, difference.message))
            .collect::<Vec<_>>()
            .join("; ");
        out.push(
            Difference::new(
                "celery",
                queue,
                format!("{} message difference(s): {summary}", differences.len()),
            )
            .with_values(pretty(&a_value), pretty(&b_value)),
        );
    }
    out
}

/// Celery's truncation marker: `_saferepr` writes `", ..."` once the
/// `argsrepr_maxsize`/`kwargsrepr_maxsize` budget (1024) is exhausted and then
/// every closing bracket that is still open (`celery/utils/saferepr.py:158-183`).
const TRUNCATION_MARKER: &str = ", ...";

/// The repr text before Celery's truncation marker.
///
/// A truncated repr ends with `", ..."` followed only by closing brackets; a
/// literal `", ..."` inside a quoted value is not a truncation (text follows
/// it). `None` means the value is not truncated.
fn repr_before_truncation(value: &str) -> Option<&str> {
    let marker = value.rfind(TRUNCATION_MARKER)?;
    let tail = &value[marker + TRUNCATION_MARKER.len()..];
    if tail
        .chars()
        .all(|character| matches!(character, ')' | ']' | '}'))
    {
        Some(&value[..marker])
    } else {
        None
    }
}

/// Drop the counters from the harness's placeholders (`<id:10>` -> `<id>`), so
/// two targets whose generated ids were numbered differently still line up:
/// the width of `<id:10>` vs `<id:4>` must not move the truncation point.
fn canonical_placeholders(value: &str) -> String {
    static PLACEHOLDER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let regex = PLACEHOLDER.get_or_init(|| {
        regex::Regex::new(r"<([A-Za-z0-9_-]+):[0-9]+>").expect("placeholder regex")
    });
    regex.replace_all(value, "<$1>").into_owned()
}

/// Compare the Python-repr headers of matched messages.
///
/// `argsrepr`/`kwargsrepr` are `saferepr(kwargs, 1024)`; when the budget runs
/// out the tail is cut and its position depends on how wide the harness's
/// placeholders came out on each side. An untruncated pair is compared
/// exactly (after the normal placeholder substitution). When either side is
/// truncated, the repr is compared up to the divergence only if that
/// divergence lies inside the **final** repr token — the one the cut produced:
/// a truncated repr can end in the middle of a value, and a complete
/// placeholder on one side is then compared against a raw random prefix on the
/// other. Everything before the final token is still compared byte for byte;
/// the truncated token's content is already compared by the message's own
/// `args`/`kwargs` body, which the structural comparison does in full.
fn reconcile_repr_headers(a_value: &mut Value, b_value: &mut Value) {
    let (Value::Array(a_messages), Value::Array(b_messages)) = (a_value, b_value) else {
        return;
    };
    for (a_message, b_message) in a_messages.iter_mut().zip(b_messages.iter_mut()) {
        for key in ["argsrepr", "kwargsrepr"] {
            let (Some(a_headers), Some(b_headers)) = (
                a_message.get_mut("headers").and_then(Value::as_object_mut),
                b_message.get_mut("headers").and_then(Value::as_object_mut),
            ) else {
                continue;
            };
            let (Some(a), Some(b)) = (a_headers.get_mut(key), b_headers.get_mut(key)) else {
                continue;
            };
            let (Some(a_text), Some(b_text)) = (a.as_str(), b.as_str()) else {
                continue;
            };
            let a_prefix = repr_before_truncation(a_text);
            let b_prefix = repr_before_truncation(b_text);
            if a_prefix.is_none() && b_prefix.is_none() {
                continue;
            }
            let a_prefix = canonical_placeholders(a_prefix.unwrap_or(a_text));
            let b_prefix = canonical_placeholders(b_prefix.unwrap_or(b_text));
            let common = common_prefix_chars(&a_prefix, &b_prefix);
            let shorter = a_prefix.chars().count().min(b_prefix.chars().count());
            // One prefix being a prefix of the other is the ordinary case: the
            // shorter side was cut earlier. Otherwise the first difference
            // must fall inside the final token, which is where a cut can land
            // in the middle of a (random) value.
            let inside_final_token =
                common >= final_token_start(&a_prefix).min(final_token_start(&b_prefix));
            if common != shorter && !inside_final_token {
                continue;
            }
            let kept: String = a_prefix.chars().take(common).collect();
            *a = Value::String(kept.clone());
            *b = Value::String(kept);
        }
    }
}

/// The number of leading characters two strings share.
fn common_prefix_chars(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .take_while(|(left, right)| left == right)
        .count()
}

/// The start (in characters) of the last top-level token of a Python repr.
///
/// Separators (`(`, `[`, `{`, `,`, `:`) count only when they are outside a
/// quoted string and at most one container deep, so a separator inside the
/// `current_instance` JSON string does not split it into tokens. A cut that
/// lands inside that final token is what makes the two sides' placeholders
/// diverge at the tail.
fn final_token_start(text: &str) -> usize {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0usize;
    for (index, character) in text.chars().enumerate() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' | '[' | '{' => {
                depth += 1;
                if depth == 1 {
                    start = index + 1;
                }
            }
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' | ':' if depth <= 1 => start = index + 1,
            _ => {}
        }
    }
    start
}

fn queue_groups(messages: &[CeleryMessage]) -> BTreeMap<String, Vec<&CeleryMessage>> {
    let mut groups: BTreeMap<String, Vec<&CeleryMessage>> = BTreeMap::new();
    for message in messages {
        groups
            .entry(message.queue.clone())
            .or_default()
            .push(message);
    }
    groups
}

fn canonical_messages(messages: &[&CeleryMessage]) -> Value {
    Value::Array(
        messages
            .iter()
            .map(|message| {
                let mut object = Map::new();
                object.insert("task".to_string(), json!(message.task));
                if !message.headers.is_empty() {
                    object.insert(
                        "headers".to_string(),
                        Value::Object(
                            message
                                .headers
                                .iter()
                                .map(|(key, value)| (key.clone(), value.clone()))
                                .collect(),
                        ),
                    );
                }
                if !message.properties.is_empty() {
                    object.insert(
                        "properties".to_string(),
                        Value::Object(
                            message
                                .properties
                                .iter()
                                .map(|(key, value)| (key.clone(), value.clone()))
                                .collect(),
                        ),
                    );
                }
                object.insert("args".to_string(), message.args.clone());
                object.insert("kwargs".to_string(), message.kwargs.clone());
                if let Some(extra) = &message.body_extra {
                    object.insert("body_extra".to_string(), extra.clone());
                }
                Value::Object(object)
            })
            .collect(),
    )
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "<invalid json>".to_string())
}

// ---------------------------------------------------------------------------
// Redis
// ---------------------------------------------------------------------------

/// TTLs are compared with a tolerance: a key written during one target's step
/// and re-read a few milliseconds later on the other side must not differ.
pub const TTL_TOLERANCE_MS: i64 = 2_000;

/// Compare the Redis side effects of one step.
///
/// `added`, `changed` and `removed` are treated as actions: the same key added
/// on both sides is compared by entry, one side adding a key the other does not
/// is a difference, and a key removed on both sides is equal (its before-state
/// was compared when it was written).
pub fn compare_redis(a: &RedisDiff, b: &RedisDiff) -> Vec<Difference> {
    let keys: BTreeSet<&Vec<u8>> = a
        .added
        .keys()
        .chain(a.changed.keys())
        .chain(a.removed.iter())
        .chain(b.added.keys())
        .chain(b.changed.keys())
        .chain(b.removed.iter())
        .collect();
    let mut out = Vec::new();
    for key in keys {
        let path = String::from_utf8_lossy(key).into_owned();
        let a_action = redis_action(a, key);
        let b_action = redis_action(b, key);
        if a_action != b_action {
            out.push(
                Difference::new("redis", path, "key changed differently").with_values(
                    a_action.unwrap_or("absent").to_string(),
                    b_action.unwrap_or("absent").to_string(),
                ),
            );
            continue;
        }
        let (Some(a_entry), Some(b_entry)) = (redis_entry(a, key), redis_entry(b, key)) else {
            // Both sides removed the key, or both sides left it alone.
            continue;
        };
        let mut differences = Vec::new();
        if a_entry.kind != b_entry.kind {
            differences.push(format!(
                "type differs ({} vs {})",
                a_entry.kind, b_entry.kind
            ));
        }
        // A pickled value's byte length depends on the *encoding* of its
        // contents: a generated UUID is pickled as a variable-length integer,
        // so two structurally equal cache entries can differ by a few bytes.
        // Compare the length only when the value itself was not compared (too
        // large for the snapshot caps); a list's element count is always
        // meaningful.
        let compare_size =
            a_entry.kind == "list" || (a_entry.value.is_none() && b_entry.value.is_none());
        if compare_size && a_entry.size != b_entry.size {
            differences.push(format!(
                "size differs ({} vs {})",
                a_entry.size, b_entry.size
            ));
        }
        if !ttl_matches(a_entry.ttl_ms, b_entry.ttl_ms) {
            differences.push(format!(
                "TTL differs beyond {TTL_TOLERANCE_MS} ms ({} ms vs {} ms)",
                a_entry.ttl_ms, b_entry.ttl_ms
            ));
        }
        if let (Some(a_value), Some(b_value)) = (&a_entry.value, &b_entry.value) {
            for difference in structural_json_diffs(a_value, b_value) {
                differences.push(format!("{}: {}", difference.path, difference.message));
            }
        } else if a_entry.value.is_some() != b_entry.value.is_some() {
            differences.push("one side compared the value, the other did not".to_string());
        }
        if differences.is_empty() {
            continue;
        }
        out.push(
            Difference::new(
                "redis",
                path,
                format!(
                    "{} value difference(s): {}",
                    differences.len(),
                    differences.join("; ")
                ),
            )
            .with_values(
                pretty(&redis_entry_json(a_entry)),
                pretty(&redis_entry_json(b_entry)),
            ),
        );
    }
    out
}

fn ttl_matches(a: i64, b: i64) -> bool {
    if a < 0 || b < 0 {
        return a == b;
    }
    (a - b).abs() <= TTL_TOLERANCE_MS
}

fn redis_action<'a>(diff: &'a RedisDiff, key: &Vec<u8>) -> Option<&'a str> {
    if diff.added.contains_key(key) {
        Some("added")
    } else if diff.changed.contains_key(key) {
        Some("changed")
    } else if diff.removed.contains(key) {
        Some("removed")
    } else {
        None
    }
}

fn redis_entry<'a>(diff: &'a RedisDiff, key: &Vec<u8>) -> Option<&'a RedisEntry> {
    diff.added.get(key).or_else(|| diff.changed.get(key))
}

fn redis_entry_json(entry: &RedisEntry) -> Value {
    let mut map = Map::new();
    map.insert("type".to_string(), json!(entry.kind));
    map.insert("size".to_string(), json!(entry.size));
    map.insert("ttl_ms".to_string(), json!(entry.ttl_ms));
    if let Some(value) = &entry.value {
        map.insert("value".to_string(), value.clone());
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::TableDiff;
    use crate::normalize::{BodyValue, NormalizedBody};

    fn body_json(value: Value) -> NormalizedBody {
        NormalizedBody {
            value: BodyValue::Json(value),
            modified: false,
        }
    }

    #[test]
    fn status_differences_are_detected() {
        assert!(compare_status(200, 200).is_empty());
        let differences = compare_status(200, 400);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].part, "status");
    }

    #[test]
    fn header_names_are_case_insensitive_but_values_are_not() {
        let a = vec![("Content-Type".to_string(), "application/json".to_string())];
        let b = vec![("content-type".to_string(), "application/json".to_string())];
        assert!(compare_headers(&a, &b).is_empty());

        let b = vec![("content-type".to_string(), "text/plain".to_string())];
        assert_eq!(compare_headers(&a, &b).len(), 1);

        let b = vec![
            ("content-type".to_string(), "application/json".to_string()),
            ("vary".to_string(), "Cookie".to_string()),
        ];
        assert_eq!(compare_headers(&a, &b).len(), 1);
    }

    #[test]
    fn multi_valued_headers_are_compared_in_order() {
        let a = vec![
            ("Set-Cookie".to_string(), "a=1".to_string()),
            ("Set-Cookie".to_string(), "b=2".to_string()),
        ];
        let b = vec![
            ("set-cookie".to_string(), "b=2".to_string()),
            ("set-cookie".to_string(), "a=1".to_string()),
        ];
        assert_eq!(compare_headers(&a, &b).len(), 2);
    }

    #[test]
    fn json_key_order_is_significant() {
        let a = body_json(serde_json::from_str(r#"{"a":1,"b":2}"#).expect("json"));
        let b = body_json(serde_json::from_str(r#"{"b":2,"a":1}"#).expect("json"));
        let differences = compare_bodies(&a, &b);
        assert_eq!(differences.len(), 1);
        assert!(differences[0].message.contains("key order"));
    }

    #[test]
    fn number_lexemes_are_significant() {
        let a = body_json(serde_json::from_str(r#"{"n":1.0}"#).expect("json"));
        let b = body_json(serde_json::from_str(r#"{"n":1}"#).expect("json"));
        let differences = compare_bodies(&a, &b);
        assert_eq!(differences.len(), 1);
        assert!(differences[0].message.contains("number lexeme"));
    }

    #[test]
    fn body_values_are_detected() {
        let a = body_json(json!({"name": "a"}));
        let b = body_json(json!({"name": "b"}));
        assert_eq!(compare_bodies(&a, &b).len(), 1);
        assert!(compare_bodies(&a, &a).is_empty());
    }

    #[test]
    fn drf_formatting_rule() {
        assert!(check_drf_formatting(br#"{"a":1,"b":[true,null]}"#).is_ok());
        assert!(check_drf_formatting(br#"{"a": 1}"#).is_err());
        assert!(check_drf_formatting(br#"{"a":"\u2028"}"#).is_ok());
        assert!(check_drf_formatting("{\"a\": \"\u{2028}\"}".as_bytes()).is_err());
        assert!(check_drf_formatting("{\"a\":\"héllo\"}".as_bytes()).is_ok());
        assert!(check_drf_formatting("{\"a\":\"h\\u00e9llo\"}".as_bytes()).is_err());
        assert!(check_drf_formatting(br#"{"a":1} "#).is_err());
        assert!(check_drf_formatting(b"").is_ok());
    }

    #[test]
    fn formatting_mismatch_is_a_difference_only_when_one_side_passes() {
        let pass = FormattingStatus::Pass;
        let fail = FormattingStatus::Fail("spaced".to_string());
        assert!(compare_formatting(&pass, &pass).is_empty());
        assert!(compare_formatting(&fail, &fail).is_empty());
        assert!(
            compare_formatting(&FormattingStatus::NotJson, &FormattingStatus::NotJson).is_empty()
        );
        assert_eq!(compare_formatting(&pass, &fail).len(), 1);
        assert_eq!(compare_formatting(&fail, &pass).len(), 1);
    }

    #[test]
    fn db_diffs_are_multisets() {
        let row_a = json!({"id": "a"});
        let row_b = json!({"id": "b"});
        let mut a = DbDiff::default();
        a.tables.insert(
            "public.t".to_string(),
            TableDiff {
                inserted: vec![row_a.clone(), row_b.clone()],
                ..Default::default()
            },
        );
        let mut b = DbDiff::default();
        b.tables.insert(
            "public.t".to_string(),
            TableDiff {
                inserted: vec![row_b, row_a],
                ..Default::default()
            },
        );
        assert!(compare_db(&a, &b).is_empty());

        let mut c = DbDiff::default();
        c.tables.insert(
            "public.t".to_string(),
            TableDiff {
                inserted: vec![json!({"id": "a"})],
                ..Default::default()
            },
        );
        assert_eq!(compare_db(&a, &c).len(), 1);
    }

    #[test]
    fn db_updates_are_detected_per_column() {
        let mut a = DbDiff::default();
        a.tables.insert(
            "public.t".to_string(),
            TableDiff {
                updated: vec![json!({"pk": ["a"], "changed": {"name": {"a": "x", "b": "y"}}})],
                ..Default::default()
            },
        );
        let mut b = a.clone();
        b.tables.get_mut("public.t").expect("table").updated[0]["changed"]["name"]
            .as_object_mut()
            .expect("changed")
            .insert("b".to_string(), json!("z"));
        assert_eq!(compare_db(&a, &b).len(), 1);
    }

    fn celery_message(task: &str, args: Value) -> CeleryMessage {
        CeleryMessage {
            queue: "celery".to_string(),
            task: Some(task.to_string()),
            headers: BTreeMap::new(),
            properties: BTreeMap::new(),
            args,
            kwargs: json!({}),
            body_extra: None,
        }
    }

    #[test]
    fn celery_task_and_args_are_compared() {
        let a = vec![celery_message("t", json!(["x"]))];
        assert!(compare_celery(&a, &a).is_empty());
        let b = vec![celery_message("other", json!(["x"]))];
        assert_eq!(compare_celery(&a, &b).len(), 1);
        let b = vec![celery_message("t", json!(["y"]))];
        assert_eq!(compare_celery(&a, &b).len(), 1);
        let b = vec![
            celery_message("t", json!(["x"])),
            celery_message("t", json!(["x"])),
        ];
        assert_eq!(compare_celery(&a, &b).len(), 1);
    }

    #[test]
    fn celery_header_differences_are_detected() {
        let mut a = celery_message("t", json!([]));
        a.headers.insert("retries".to_string(), json!(0));
        let mut b = a.clone();
        b.headers.insert("retries".to_string(), json!(1));
        assert_eq!(compare_celery(&[a], &[b]).len(), 1);
    }

    // -- Redis -------------------------------------------------------------

    fn redis_entry(kind: &str, size: usize, ttl_ms: i64, value: Value) -> RedisEntry {
        RedisEntry {
            kind: kind.to_string(),
            size,
            ttl_ms,
            value: Some(value),
        }
    }

    fn redis_diff(added: &[(&str, RedisEntry)]) -> RedisDiff {
        let mut diff = RedisDiff::default();
        for (key, entry) in added {
            diff.added.insert(key.as_bytes().to_vec(), entry.clone());
        }
        diff
    }

    #[test]
    fn redis_entries_of_both_sides_are_compared_field_by_field() {
        let entry = redis_entry("string", 3, 60_000, json!("bar"));
        let a = redis_diff(&[("k", entry.clone())]);
        let b = redis_diff(&[("k", entry.clone())]);
        assert!(compare_redis(&a, &b).is_empty());

        let other = redis_entry("string", 3, 60_000, json!("baz"));
        assert_eq!(compare_redis(&a, &redis_diff(&[("k", other)])).len(), 1);

        let other = redis_entry("list", 3, 60_000, json!(["bar"]));
        assert_eq!(compare_redis(&a, &redis_diff(&[("k", other)])).len(), 1);

        // A pickled value's byte length is an encoding detail; only compare it
        // when the value itself was not compared (too large for the caps).
        let a_large = RedisEntry {
            kind: "string".to_string(),
            size: 5_000,
            ttl_ms: 60_000,
            value: None,
        };
        let b_large = RedisEntry {
            size: 5_010,
            ..a_large.clone()
        };
        assert_eq!(
            compare_redis(
                &redis_diff(&[("k", a_large.clone())]),
                &redis_diff(&[("k", b_large)])
            )
            .len(),
            1
        );
        let a_large_value = RedisEntry {
            value: Some(json!({"same": true})),
            ..a_large.clone()
        };
        let b_large_value = RedisEntry {
            value: Some(json!({"same": true})),
            ..a_large
        };
        assert!(
            compare_redis(
                &redis_diff(&[("k", a_large_value)]),
                &redis_diff(&[("k", b_large_value)])
            )
            .is_empty()
        );
    }

    #[test]
    fn redis_ttls_have_a_tolerance_and_a_floor() {
        let entry = redis_entry("string", 1, 60_000, json!("x"));
        let a = redis_diff(&[("k", entry.clone())]);
        let mut close = entry.clone();
        close.ttl_ms = 58_900;
        assert!(compare_redis(&a, &redis_diff(&[("k", close)])).is_empty());

        let mut far = entry.clone();
        far.ttl_ms = 57_000;
        assert_eq!(compare_redis(&a, &redis_diff(&[("k", far)])).len(), 1);

        let mut persistent = entry;
        persistent.ttl_ms = -1;
        assert_eq!(
            compare_redis(&a, &redis_diff(&[("k", persistent)])).len(),
            1
        );
    }

    #[test]
    fn redis_extra_missing_and_removed_keys_are_differences() {
        let entry = redis_entry("string", 1, -1, json!("x"));
        let a = redis_diff(&[]);
        assert_eq!(
            compare_redis(&a, &redis_diff(&[("k", entry.clone())])).len(),
            1
        );

        let mut removed_a = RedisDiff::default();
        removed_a.removed.insert(b"k".to_vec());
        let mut removed_b = RedisDiff::default();
        removed_b.removed.insert(b"k".to_vec());
        assert!(compare_redis(&removed_a, &removed_b).is_empty());
        assert_eq!(compare_redis(&removed_a, &a).len(), 1);
    }
    #[test]
    fn repr_truncation_marker_is_celery_tail_only() {
        assert_eq!(repr_before_truncation("('a', ...}"), Some("('a'"));
        assert_eq!(repr_before_truncation("('a', ...}}"), Some("('a'"));
        assert_eq!(repr_before_truncation("('a, ... b')"), None);
        assert_eq!(repr_before_truncation("('a', 1)"), None);
    }

    #[test]
    fn placeholder_numbers_do_not_move_the_truncation_point() {
        assert_eq!(
            canonical_placeholders("<id:10>/<session:2>"),
            "<id>/<session>"
        );
        assert_eq!(
            repr_before_truncation("('x', <id:10>, ...}"),
            Some("('x', <id:10>")
        );
    }

    #[test]
    fn final_token_start_ignores_separators_inside_a_quoted_value() {
        // The JSON string's own `, ` and `: ` are not repr separators.
        let repr = "{'type': 'x', 'current_instance': '{\"id\": \"1\", \"name\": \"n\"}'}";
        assert_eq!(
            final_token_start(repr),
            repr.rfind(": '").expect("separator") + 1
        );
        // A list of uuids: the last element starts after `, `.
        let repr = "('3f2a1b4c-0000-4000-8000-000000000001', '3f2a1b4c-0000-40";
        assert_eq!(
            final_token_start(repr),
            repr.rfind(", '").expect("separator") + 1
        );
    }

    fn repr_message(repr: &str) -> CeleryMessage {
        let mut message = celery_message("t", json!([]));
        message
            .headers
            .insert("kwargsrepr".to_string(), json!(repr));
        message
    }

    #[test]
    fn a_truncated_repr_may_diverge_inside_its_final_value() {
        let prefix = format!(
            "{{'type': 'issue.activity.updated', 'current_instance': '{}",
            "x".repeat(900)
        );
        // One side has a complete placeholder where the other was cut in the
        // middle of the raw value the placeholder came from.
        let a = format!("{prefix}<id:1>tail of the value', ...}}");
        let b = format!("{prefix}3f2a1b4c-0000-4000-8000-abcdef012345', ...}}");
        assert!(
            compare_celery(&[repr_message(&a)], &[repr_message(&b)]).is_empty(),
            "a cut inside the final quoted value must not differ"
        );
    }

    #[test]
    fn a_truncated_repr_still_compares_everything_before_the_cut() {
        let tail = "x".repeat(900);
        let a = format!(
            "{{'type': 'issue.activity.updated', 'current_instance': '{tail}<id:1>tail', ...}}"
        );
        // The same cut, but the task name differs before the final token.
        let b = format!(
            "{{'type': 'intake.activity.created', 'current_instance': '{tail}3f2a1b4c-0000-4000-8000-abcdef012345', ...}}"
        );
        assert_eq!(
            compare_celery(&[repr_message(&a)], &[repr_message(&b)]).len(),
            1
        );
    }

    #[test]
    fn untruncated_reprs_are_compared_exactly() {
        let a = repr_message("{'epoch': <epoch>, 'x': 1}");
        let b = repr_message("{'epoch': <epoch>, 'x': 2}");
        assert_eq!(compare_celery(&[a], &[b]).len(), 1);
    }
}
