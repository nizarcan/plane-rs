// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/filters/converters.py`, ported.
//!
//! `LegacyToRichFiltersConverter` turns the `filters` dict a saved view stored
//! before the rich-filter rewrite into the JSON tree `ComplexFilterBackend`
//! accepts, and is used by the one-off data migration
//! (`filter_migrations.py`). It is not on a request path, but it is part of the
//! filter contract: a view saved with legacy filters must keep working after
//! the migration, so the conversion has to produce exactly the tree the Python
//! produces.
//!
//! Differences are rejected rather than guessed: an unknown key is skipped
//! (non-strict mode, like the migration's default), and a value that fails
//! validation is dropped instead of being passed through.

use serde_json::{Map, Value as Json};

/// `DEFAULT_FIELD_MAPPINGS`.
pub const FIELD_MAPPINGS: &[(&str, &str)] = &[
    ("state", "state_id"),
    ("labels", "label_id"),
    ("cycle", "cycle_id"),
    ("module", "module_id"),
    ("assignees", "assignee_id"),
    ("mentions", "mention_id"),
    ("created_by", "created_by_id"),
    ("state_group", "state_group"),
    ("priority", "priority"),
    ("project", "project_id"),
    ("start_date", "start_date"),
    ("target_date", "target_date"),
];

/// `DEFAULT_UUID_FIELDS`.
pub const UUID_FIELDS: &[&str] = &[
    "state_id",
    "label_id",
    "cycle_id",
    "module_id",
    "assignee_id",
    "mention_id",
    "created_by_id",
    "project_id",
];

/// `DEFAULT_VALID_CHOICES`.
pub const VALID_CHOICES: &[(&str, &[&str])] = &[
    (
        "state_group",
        &["backlog", "unstarted", "started", "completed", "cancelled"],
    ),
    ("priority", &["urgent", "high", "medium", "low", "none"]),
];

/// `DEFAULT_DATE_FIELDS`.
pub const DATE_FIELDS: &[&str] = &["start_date", "target_date"];

fn mapping_for(legacy_key: &str) -> Option<&'static str> {
    FIELD_MAPPINGS
        .iter()
        .find(|(key, _)| *key == legacy_key)
        .map(|(_, value)| *value)
}

fn valid_value(field: &str, value: &str) -> bool {
    if UUID_FIELDS.contains(&field) {
        return crate::issue::filter::parse_uuid(value).is_some();
    }
    if let Some((_, choices)) = VALID_CHOICES.iter().find(|(name, _)| *name == field) {
        return choices.contains(&value);
    }
    if DATE_FIELDS.contains(&field) {
        return parse_flexible_date(value).is_some();
    }
    true
}

/// The subset of `dateutil.parser.parse` the converter needs: an ISO date, an
/// ISO datetime, or `YYYY/MM/DD`. `dateutil` accepts far more; a saved view
/// with a more exotic value is dropped, which is what non-strict mode does with
/// a value it cannot validate.
fn parse_flexible_date(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").is_ok() {
        return Some(trimmed.to_owned());
    }
    if chrono::NaiveDate::parse_from_str(trimmed, "%Y/%m/%d").is_ok() {
        return Some(trimmed.to_owned());
    }
    if chrono::DateTime::parse_from_rfc3339(trimmed).is_ok() {
        return Some(trimmed.to_owned());
    }
    if chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S").is_ok() {
        return Some(trimmed.to_owned());
    }
    None
}

/// `\d+_(weeks|months)$`.
fn relative_pattern(value: &str) -> bool {
    let Some((digits, unit)) = value.rsplit_once('_') else {
        return false;
    };
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && matches!(unit, "weeks" | "months")
}

/// `_convert_date_value`. Returns `None` when the Python returns `{}`.
fn convert_date_value(field: &str, values: &[String]) -> Option<(&'static str, Json)> {
    for value in values {
        if let Some((first, _)) = value.split_once(';')
            && relative_pattern(first)
        {
            return None;
        }
    }
    if values.len() > 2 {
        return None;
    }
    let mut exact: Vec<String> = Vec::new();
    let mut after: Vec<String> = Vec::new();
    let mut before: Vec<String> = Vec::new();
    for value in values {
        match value.split_once(';') {
            None => {
                if let Some(parsed) = parse_flexible_date(value) {
                    exact.push(parsed);
                }
            }
            Some((date_part, direction)) => {
                let Some(parsed) = parse_flexible_date(date_part) else {
                    continue;
                };
                match direction {
                    "after" => after.push(parsed),
                    "before" => before.push(parsed),
                    _ => {}
                }
            }
        }
    }
    let _ = field;
    if after.len() == 1 && before.len() == 1 && exact.is_empty() {
        let start = after[0].clone().min(before[0].clone());
        let end = after[0].clone().max(before[0].clone());
        return Some(("range", Json::String(format!("{start},{end}"))));
    }
    if exact.len() == 1 && after.is_empty() && before.is_empty() {
        return Some(("exact", Json::String(exact.remove(0))));
    }
    None
}

/// `convert`: the rich-filter tree for one saved view's filters.
///
/// `legacy_filters` is the ordered list of `(key, value)` pairs the stored JSON
/// dict held; order matters because the converter builds an `and` list in it.
pub fn convert(legacy_filters: &[(String, Json)]) -> Json {
    let mut flat: Vec<(String, Json)> = Vec::new();
    for (key, value) in legacy_filters {
        if value.is_null() {
            continue;
        }
        if let Some(items) = value.as_array()
            && items.is_empty()
        {
            continue;
        }
        let Some(field) = mapping_for(key) else {
            continue;
        };
        match value {
            Json::Array(items) => {
                let values: Vec<String> = items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect();
                if DATE_FIELDS.contains(&field) {
                    if let Some((lookup, json)) = convert_date_value(field, &values) {
                        flat.push((format!("{field}__{lookup}"), json));
                    }
                    continue;
                }
                let valid: Vec<&String> = values
                    .iter()
                    .filter(|value| valid_value(field, value))
                    .collect();
                if valid.is_empty() {
                    continue;
                }
                let joined = valid
                    .iter()
                    .map(|value| (*value).as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                flat.push((format!("{field}__in"), Json::String(joined)));
            }
            Json::String(text) => {
                if DATE_FIELDS.contains(&field) {
                    if let Some((lookup, json)) =
                        convert_date_value(field, std::slice::from_ref(text))
                    {
                        flat.push((format!("{field}__{lookup}"), json));
                    }
                    continue;
                }
                if valid_value(field, text) {
                    flat.push((format!("{field}__exact"), Json::String(text.clone())));
                }
            }
            _ => {}
        }
    }
    if flat.is_empty() {
        return Json::Object(Map::new());
    }
    if flat.len() == 1 {
        let (key, value) = flat.remove(0);
        let mut object = Map::new();
        object.insert(key, value);
        return Json::Object(object);
    }
    Json::Object(Map::from_iter([(
        "and".to_owned(),
        Json::Array(
            flat.into_iter()
                .map(|(key, value)| {
                    let mut object = Map::new();
                    object.insert(key, value);
                    Json::Object(object)
                })
                .collect(),
        ),
    )]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, Json)]) -> Vec<(String, Json)> {
        items
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn a_single_filter_stays_a_leaf() {
        let converted = convert(&pairs(&[("priority", Json::String("urgent".to_owned()))]));
        assert_eq!(converted, serde_json::json!({"priority__exact": "urgent"}));
    }

    #[test]
    fn lists_become_in_filters() {
        let converted = convert(&pairs(&[(
            "labels",
            serde_json::json!([
                "43b4e751-4243-56ef-a3ee-e2d1316c5a09",
                "895e5cb7-9491-52f5-ac1a-ac28c1a8cba9"
            ]),
        )]));
        assert_eq!(
            converted,
            serde_json::json!({"label_id__in": "43b4e751-4243-56ef-a3ee-e2d1316c5a09,895e5cb7-9491-52f5-ac1a-ac28c1a8cba9"})
        );
    }

    #[test]
    fn dates_become_ranges() {
        let converted = convert(&pairs(&[(
            "start_date",
            serde_json::json!(["2025-01-01;after", "2025-12-31;before"]),
        )]));
        assert_eq!(
            converted,
            serde_json::json!({"start_date__range": "2025-01-01,2025-12-31"})
        );
    }

    #[test]
    fn relative_dates_are_skipped() {
        let converted = convert(&pairs(&[(
            "start_date",
            serde_json::json!(["2_weeks;after;fromnow"]),
        )]));
        assert_eq!(converted, serde_json::json!({}));
    }

    #[test]
    fn invalid_values_are_dropped() {
        let converted = convert(&pairs(&[("state", serde_json::json!(["not-a-uuid"]))]));
        assert_eq!(converted, serde_json::json!({}));
    }

    #[test]
    fn multiple_filters_become_an_and_tree() {
        let converted = convert(&pairs(&[
            ("priority", Json::String("high".to_owned())),
            ("state_group", Json::String("started".to_owned())),
        ]));
        assert_eq!(
            converted,
            serde_json::json!({"and": [{"priority__exact": "high"}, {"state_group__exact": "started"}]})
        );
    }
}
