// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `django.core.serializers.serialize("json", objects)` — the format Django's
//! `python.Serializer` emits, used as a string argument by the cycle-issue
//! activity (`plane/app/views/cycle/issue.py:298-306`) and, later, by module
//! issues.
//!
//! One array of `{"model", "pk", "fields"}` objects:
//!
//! * `model` is the app label and lowercased model name (`db.cycleissue`);
//! * `pk` is the pk rendered by `DjangoJSONEncoder` (a string);
//! * `fields` holds `_meta.local_fields` in order, `serialize=True` only, each
//!   value from `field.value_from_object` — a FK contributes its id (a UUID
//!   string, or `null`), a datetime the encoder's millisecond form
//!   (`DjangoJSONEncoder`: `isoformat()` sliced to three subsecond digits,
//!   `+00:00` → `Z`);
//! * the document is `json.dumps(..., cls=DjangoJSONEncoder)` with its default
//!   separators and `ensure_ascii=True` — [`Dialect::Django`].
//!
//! The golden test below pins the bytes against a row recorded from
//! `serializers.serialize("json", [row])` (including a microsecond datetime,
//! which the encoder truncates to `.123Z`).

use chrono::{DateTime, SecondsFormat, Utc};
use plane_drf::error::ViewError;
use plane_query::json::Json;
use uuid::Uuid;

/// `json.dumps(value, cls=DjangoJSONEncoder)` for an order-preserving document
/// — the activity call sites' `requested_data` / `current_instance` strings.
///
/// # Errors
///
/// A value the encoder refuses.
pub fn django_json(value: &Json) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes)
        .map_err(|error| ViewError::Unknown(format!("json.dumps produced non-UTF-8: {error}")))
}

/// One serialized field value.
#[derive(Clone, Copy, Debug)]
pub enum DjangoValue<'a> {
    Null,
    Uuid(Uuid),
    OptUuid(Option<Uuid>),
    DateTime(DateTime<Utc>),
    OptDateTime(Option<DateTime<Utc>>),
    Str(&'a str),
    OptStr(Option<&'a str>),
}

/// One field of a serialized model, in `_meta.local_fields` order.
#[derive(Clone, Copy, Debug)]
pub struct DjangoField<'a> {
    pub name: &'static str,
    pub value: DjangoValue<'a>,
}

/// One serialized object.
#[derive(Clone, Debug)]
pub struct DjangoObject<'a> {
    /// `"<app_label>.<model_name>"`, e.g. `"db.cycleissue"`.
    pub model: &'static str,
    pub pk: Uuid,
    pub fields: Vec<DjangoField<'a>>,
}

impl DjangoValue<'_> {
    fn to_json(self) -> Json {
        match self {
            Self::Null => Json::Null,
            Self::Uuid(value) => Json::uuid(value),
            Self::OptUuid(value) => Json::opt_uuid(value),
            Self::DateTime(value) => Json::string(django_datetime(value)),
            Self::OptDateTime(value) => {
                value.map_or(Json::Null, |value| Json::string(django_datetime(value)))
            }
            Self::Str(value) => Json::string(value),
            Self::OptStr(value) => Json::opt_string(value),
        }
    }
}

/// `DjangoJSONEncoder.default` for a datetime: `isoformat()` with the last
/// three subsecond digits sliced off and `+00:00` replaced by `Z`
/// (`django/core/serializers/json.py:93-99`).
fn django_datetime(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// `serializers.serialize("json", objects)`.
///
/// # Errors
///
/// A value the encoder refuses (none of the supported variants can).
pub fn django_serialize(objects: &[DjangoObject<'_>]) -> Result<String, ViewError> {
    let document = Json::Array(
        objects
            .iter()
            .map(|object| {
                Json::object(vec![
                    ("model", Json::string(object.model)),
                    ("pk", Json::uuid(object.pk)),
                    (
                        "fields",
                        Json::Object(
                            object
                                .fields
                                .iter()
                                .map(|field| (field.name.to_owned(), field.value.to_json()))
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect(),
    );
    django_json(&document)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from `serializers.serialize("json", [row])` (Django 5.2, a
    /// fixture `db.cycleissue` row) with `created_at` forced to a microsecond
    /// value and `updated_at` to one microsecond, which the encoder truncates.
    #[test]
    fn django_serialize_matches_django() {
        let created_at = "2026-11-16T20:41:12.123456Z".parse().expect("datetime");
        let updated_at = "2026-11-16T20:41:12.000001Z".parse().expect("datetime");
        let row = DjangoObject {
            model: "db.cycleissue",
            pk: Uuid::parse_str("04f1dfd4-6bbc-422e-9330-53b2a8d6fdb4").expect("uuid"),
            fields: vec![
                DjangoField {
                    name: "created_at",
                    value: DjangoValue::DateTime(created_at),
                },
                DjangoField {
                    name: "updated_at",
                    value: DjangoValue::DateTime(updated_at),
                },
                DjangoField {
                    name: "created_by",
                    value: DjangoValue::OptUuid(Some(
                        Uuid::parse_str("d44ae570-3de4-4413-af08-f686bf5b7235").expect("uuid"),
                    )),
                },
                DjangoField {
                    name: "updated_by",
                    value: DjangoValue::OptUuid(None),
                },
                DjangoField {
                    name: "deleted_at",
                    value: DjangoValue::OptDateTime(None),
                },
                DjangoField {
                    name: "project",
                    value: DjangoValue::Uuid(
                        Uuid::parse_str("b0063033-54f9-49d8-b5f3-6b7a1551c6a6").expect("uuid"),
                    ),
                },
                DjangoField {
                    name: "workspace",
                    value: DjangoValue::Uuid(
                        Uuid::parse_str("a91c3d80-5ed0-40f3-92a8-a28c015f4388").expect("uuid"),
                    ),
                },
                DjangoField {
                    name: "issue",
                    value: DjangoValue::Uuid(
                        Uuid::parse_str("5fff8fc7-ac69-4031-9856-d63fb237d371").expect("uuid"),
                    ),
                },
                DjangoField {
                    name: "cycle",
                    value: DjangoValue::Uuid(
                        Uuid::parse_str("1df0c827-94ce-419f-b6ed-4e69b69769ae").expect("uuid"),
                    ),
                },
            ],
        };
        let expected = concat!(
            r#"[{"model": "db.cycleissue", "pk": "04f1dfd4-6bbc-422e-9330-53b2a8d6fdb4", "fields": {"#,
            r#""created_at": "2026-11-16T20:41:12.123Z", "updated_at": "2026-11-16T20:41:12.000Z", "#,
            r#""created_by": "d44ae570-3de4-4413-af08-f686bf5b7235", "updated_by": null, "#,
            r#""deleted_at": null, "project": "b0063033-54f9-49d8-b5f3-6b7a1551c6a6", "#,
            r#""workspace": "a91c3d80-5ed0-40f3-92a8-a28c015f4388", "#,
            r#""issue": "5fff8fc7-ac69-4031-9856-d63fb237d371", "#,
            r#""cycle": "1df0c827-94ce-419f-b6ed-4e69b69769ae"}}]"#,
        );
        assert_eq!(django_serialize(&[row]).expect("render"), expected);
    }

    #[test]
    fn an_empty_list_is_empty_json() {
        assert_eq!(django_serialize(&[]).expect("render"), "[]");
    }
}
