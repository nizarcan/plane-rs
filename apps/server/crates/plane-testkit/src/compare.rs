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
            if a_text != b_text {
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
        let a_value = canonical_messages(a_groups.get(queue).unwrap_or(&empty));
        let b_value = canonical_messages(b_groups.get(queue).unwrap_or(&empty));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ValueMode;
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

    #[test]
    fn value_modes_are_constructible_from_the_config() {
        assert_eq!(ValueMode::default(), ValueMode::Volatile);
    }
}
