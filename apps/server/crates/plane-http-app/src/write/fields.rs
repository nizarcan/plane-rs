// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DRF field input: the accepted formats, the messages and the error codes.
//!
//! Every message here is copied from `rest_framework/fields.py`, because a
//! validation error is compared byte for byte by the differential harness. A
//! [`Detail`] carries DRF's `code` too: it is not rendered in the body, but the
//! serializer's `errors` property has one code-dependent branch
//! (`rest_framework/serializers.py:589-596`, "No data provided").
//!
//! Only the field classes Plane's writable serializers use are implemented:
//! `CharField`, `FloatField`, `DateTimeField`, `JSONField` and the null/required
//! rules from `Field.validate_empty_values` (`fields.py:494-530`). A domain that
//! needs another class adds it here with its messages.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use chrono_tz::Tz;
use plane_db::JsonText;
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use super::pyiso;

/// One DRF error: the rendered text and the `code` behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub message: String,
    pub code: &'static str,
}

impl Detail {
    pub fn new(message: impl Into<String>, code: &'static str) -> Self {
        Self {
            message: message.into(),
            code,
        }
    }

    /// `Field.fail('required')` (`rest_framework/fields.py:563-574`).
    pub fn required() -> Self {
        Self::new("This field is required.", "required")
    }

    /// `Field.fail('null')`.
    pub fn null() -> Self {
        Self::new("This field may not be null.", "null")
    }

    /// `Field.fail('blank')`.
    pub fn blank() -> Self {
        Self::new("This field may not be blank.", "blank")
    }
}

/// The value a field receives, before its own parsing.
///
/// JSON bodies keep their type; a form/multipart body is a string
/// (`html.is_html_input`, `rest_framework/fields.py:394-418`).
#[derive(Debug, Clone, Copy)]
pub enum Raw<'a> {
    Json(&'a Json),
    Text(&'a str),
}

/// `CharField` options. `StickySerializer.name` is
/// `allow_null=True, allow_blank=True` with no `max_length`; `color` and
/// `background_color` add `max_length=255` (`plane/app/serializers/
/// workspace.py:346-350` and the model fields).
#[derive(Debug, Clone, Copy)]
pub struct CharOptions {
    pub allow_null: bool,
    pub allow_blank: bool,
    pub trim_whitespace: bool,
    pub max_length: Option<usize>,
}

impl CharOptions {
    /// `CharField(allow_null=True, allow_blank=True)` with the default trim.
    #[must_use]
    pub const fn nullable() -> Self {
        Self {
            allow_null: true,
            allow_blank: true,
            trim_whitespace: true,
            max_length: None,
        }
    }

    /// A `max_length` in characters, as `max_length=255` on the model field.
    #[must_use]
    pub const fn with_max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }
}

/// `PrimaryKeyRelatedField.to_internal_value` (`rest_framework/relations.py:
/// 250-290`): the pk conversion and the two error messages DRF builds.
///
/// The queryset lookup is asynchronous and lives in
/// [`crate::write::Serializer::primary_key_field`]; this half only decides
/// whether the input is a pk at all. Django's `UUIDField.to_python` accepts an
/// integer as a 128-bit UUID and reports a non-UUID string with its own
/// message, which DRF passes through.
pub fn primary_key(raw: Raw<'_>) -> Result<Uuid, Detail> {
    match raw {
        Raw::Text(text) => parse_pk_uuid(text),
        Raw::Json(Json::Str(value)) => {
            parse_pk_uuid(value.as_str().ok_or_else(|| incorrect_pk_type("str"))?)
        }
        // DRF converts Python `bool` to `incorrect_type` explicitly, before
        // the queryset sees it (`relations.py:264-265`).
        Raw::Json(Json::Bool(_)) => Err(incorrect_pk_type("bool")),
        Raw::Json(Json::Int(value)) if !value.is_negative() => value
            .to_decimal_string()
            .parse::<u128>()
            .map(Uuid::from_u128)
            .map_err(|_| invalid_pk_uuid(&value.to_decimal_string())),
        Raw::Json(Json::Int(value)) => Err(invalid_pk_uuid(&value.to_decimal_string())),
        Raw::Json(Json::Float(value)) => Err(invalid_pk_uuid(&plane_query::pyfloat::repr(*value))),
        // DRF hands a list or dict to `queryset.get(pk=…)`, and Django's
        // `UUIDField.to_python` then fails with the same
        // `“<str(value)>” is not a valid UUID.` a malformed string gets
        // (`django/db/models/fields/__init__.py`, `UUIDField.to_python`).
        Raw::Json(value @ (Json::List(_) | Json::Dict(_))) => {
            Err(invalid_pk_uuid(&python_str(Raw::Json(value))))
        }
        // `field_value` turns JSON null into `Value::Null` before parsing.
        Raw::Json(Json::Null) => Err(invalid_pk_uuid("None")),
    }
}

fn parse_pk_uuid(value: &str) -> Result<Uuid, Detail> {
    Uuid::parse_str(value).map_err(|_| invalid_pk_uuid(value))
}

/// `“%(value)s” is not a valid UUID.` (`django/db/models/fields/__init__.py`,
/// `UUIDField.to_python`), with DRF's `invalid` code.
fn invalid_pk_uuid(value: &str) -> Detail {
    Detail::new(format!("“{value}” is not a valid UUID."), "invalid")
}

/// `Incorrect type. Expected pk value, received {data_type}.`
/// (`rest_framework/relations.py:18-22`).
fn incorrect_pk_type(data_type: &str) -> Detail {
    Detail::new(
        format!("Incorrect type. Expected pk value, received {data_type}."),
        "incorrect_type",
    )
}

/// `CharField.to_internal_value` (`rest_framework/fields.py:755-762`).
///
/// Numbers are coerced with Python's `str()`; a boolean or a composite is
/// `Not a valid string.`. The value is trimmed unless `trim_whitespace=False`.
pub fn char(raw: Raw<'_>, options: &CharOptions) -> Result<String, Detail> {
    let value = match raw {
        Raw::Text(text) => text.to_owned(),
        Raw::Json(value) => match value {
            // `isinstance(data, bool)` is checked first in Python: `True` is an
            // `int`, so this order matters.
            Json::Bool(_) | Json::Null | Json::List(_) | Json::Dict(_) => {
                return Err(Detail::new("Not a valid string.", "invalid"));
            }
            Json::Str(value) => value
                .as_str()
                .ok_or_else(|| Detail::new("Not a valid string.", "invalid"))?
                .to_owned(),
            Json::Int(value) => value.to_string(),
            Json::Float(value) => plane_query::pyfloat::repr(*value),
        },
    };
    let value = if options.trim_whitespace {
        value.trim().to_owned()
    } else {
        value
    };
    if value.is_empty() && !options.allow_blank {
        return Err(Detail::blank());
    }
    if let Some(max_length) = options.max_length {
        // `MaxLengthValidator` counts characters, not bytes.
        if value.chars().count() > max_length {
            return Err(Detail::new(
                format!("Ensure this field has no more than {max_length} characters."),
                "max_length",
            ));
        }
    }
    Ok(value)
}

/// `BooleanField.to_internal_value` (`rest_framework/fields.py:695-720`):
/// `TRUE_VALUES` is `{"t","y","yes","true","on","1",1,True}`, `FALSE_VALUES`
/// is `{"f","n","no","false","off","0",0,0.0,False}` and the input string is
/// lower-cased before the membership test (`_lower_if_str`). CPython's set
/// membership makes `1.0` true (`1.0 == 1`) and `0.0` false; every other
/// number, string, list and dict is `Must be a valid boolean.`.
pub fn boolean(raw: Raw<'_>) -> Result<bool, Detail> {
    let invalid = || Detail::new("Must be a valid boolean.", "invalid");
    match raw {
        Raw::Json(Json::Bool(value)) => Ok(*value),
        Raw::Json(Json::Int(value)) => match value.as_i64() {
            Some(1) => Ok(true),
            Some(0) => Ok(false),
            _ => Err(invalid()),
        },
        Raw::Json(Json::Float(value)) => {
            if *value == 1.0 {
                Ok(true)
            } else if *value == 0.0 {
                Ok(false)
            } else {
                Err(invalid())
            }
        }
        Raw::Json(Json::Str(value)) => {
            let text = value.as_str().ok_or_else(invalid)?;
            boolean_text(text).ok_or_else(invalid)
        }
        Raw::Text(text) => boolean_text(text).ok_or_else(invalid),
        Raw::Json(_) => Err(invalid()),
    }
}

fn boolean_text(text: &str) -> Option<bool> {
    match text.to_lowercase().as_str() {
        "t" | "y" | "yes" | "true" | "on" | "1" => Some(true),
        "f" | "n" | "no" | "false" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// `ChoiceField.to_internal_value` (`rest_framework/fields.py:1940-1960`):
/// the input is compared as Python `str(data)` and the returned value is the
/// choice key. `"{input}" is not a valid choice.` is the failure message.
pub fn integer_choice(raw: Raw<'_>, allowed: &[i16]) -> Result<i16, Detail> {
    let text = match raw {
        Raw::Json(Json::Int(value)) => value.as_i64().map(|value| value.to_string()),
        Raw::Json(Json::Str(value)) => value.as_str().map(str::to_owned),
        Raw::Text(text) => Some(text.to_owned()),
        _ => None,
    };
    let Some(text) = text else {
        return Err(Detail::new(
            format!("\"{}\" is not a valid choice.", python_str(raw)),
            "invalid_choice",
        ));
    };
    // `data == ''` returns `''` in Python; every caller writes that into an
    // integer column and fails, so it is reported as an invalid choice here.
    let parsed: i16 = text.parse().unwrap_or(i16::MIN);
    if text.is_empty() || !allowed.contains(&parsed) {
        return Err(Detail::new(
            format!("\"{text}\" is not a valid choice."),
            "invalid_choice",
        ));
    }
    Ok(parsed)
}

/// Python `str(value)` for a choice error message. Composite values use
/// Python's `repr` for the elements (`str(['x'])` is `['x']`).
pub fn python_str(raw: Raw<'_>) -> String {
    match raw {
        Raw::Text(text) => text.to_owned(),
        Raw::Json(Json::Bool(value)) => {
            if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Raw::Json(Json::Int(value)) => value.to_decimal_string(),
        Raw::Json(Json::Float(value)) => plane_query::pyfloat::repr(*value),
        Raw::Json(Json::Str(value)) => value.as_str().unwrap_or_default().to_owned(),
        Raw::Json(Json::Null) => "None".to_owned(),
        Raw::Json(Json::List(items)) => {
            let rendered: Vec<String> = items
                .iter()
                .map(|item| python_repr(&Raw::Json(item)))
                .collect();
            format!("[{}]", rendered.join(", "))
        }
        Raw::Json(Json::Dict(map)) => {
            let rendered: Vec<String> = map
                .iter()
                .map(|(key, value)| {
                    format!(
                        "'{}': {}",
                        key.as_str().unwrap_or_default(),
                        python_repr(&Raw::Json(value))
                    )
                })
                .collect();
            format!("{{{}}}", rendered.join(", "))
        }
    }
}

fn python_repr(raw: &Raw<'_>) -> String {
    match raw {
        Raw::Json(Json::Str(value)) => format!("'{}'", value.as_str().unwrap_or_default()),
        _ => python_str(*raw),
    }
}

/// Python `str(value)` of a primary-key input, for `Invalid pk "{value}"…`.
pub fn primary_key_text(raw: Raw<'_>) -> String {
    match raw {
        Raw::Json(Json::Str(value)) => value.as_str().unwrap_or_default().to_owned(),
        Raw::Json(Json::Int(value)) => value.to_decimal_string(),
        Raw::Json(Json::Float(value)) => plane_query::pyfloat::repr(*value),
        Raw::Json(Json::Bool(value)) => (if *value { "True" } else { "False" }).to_owned(),
        Raw::Json(Json::Null) => "None".to_owned(),
        Raw::Text(text) => text.to_owned(),
        Raw::Json(Json::List(_)) | Raw::Json(Json::Dict(_)) => String::new(),
    }
}

/// `ChoiceField` over string choices (`rest_framework/fields.py:1930-1970`):
/// the input is compared as `str(data)`; `""` is accepted when
/// `allow_blank=True`.
pub fn choice(raw: Raw<'_>, allow_blank: bool, allowed: &[&str]) -> Result<String, Detail> {
    let input = python_str(raw);
    if input.is_empty() && allow_blank {
        return Ok(input);
    }
    if allowed.contains(&input.as_str()) {
        return Ok(input);
    }
    Err(Detail::new(
        format!("\"{input}\" is not a valid choice."),
        "invalid_choice",
    ))
}

/// What a `DateField` received. `DateTime` is DRF's
/// `Expected a date but got a datetime.` branch; HTTP bodies are strings, so
/// the serializer always passes `Raw`.
#[derive(Debug, Clone, Copy)]
pub enum DateInput<'a> {
    Raw(Raw<'a>),
    DateTime,
}

/// `DateField.to_internal_value` (`rest_framework/fields.py:1200-1240`): ISO
/// 8601 through Django's `parse_date` (`date.fromisoformat`, then the
/// `date_re` fallback) or a declared strptime format;
/// `Date has wrong format. Use one of these formats instead: …`.
pub fn date(input: DateInput<'_>, formats: &[&str]) -> Result<chrono::NaiveDate, Detail> {
    let raw = match input {
        DateInput::DateTime => {
            return Err(Detail::new(
                "Expected a date but got a datetime.",
                "datetime",
            ));
        }
        DateInput::Raw(raw) => raw,
    };
    let text = match raw {
        Raw::Text(text) => text,
        Raw::Json(Json::Str(value)) => value.as_str().ok_or_else(|| invalid_date(formats))?,
        Raw::Json(_) => return Err(invalid_date(formats)),
    };
    for format in formats {
        if format.eq_ignore_ascii_case("iso-8601") {
            if let Some(value) = pyiso::parse_date(text) {
                return Ok(value);
            }
        } else if let Ok(value) = chrono::NaiveDate::parse_from_str(text, format) {
            return Ok(value);
        }
    }
    Err(invalid_date(formats))
}

fn invalid_date(formats: &[&str]) -> Detail {
    Detail::new(
        format!(
            "Date has wrong format. Use one of these formats instead: {}.",
            humanized_date_formats(formats)
        ),
        "invalid",
    )
}

fn humanized_date_formats(formats: &[&str]) -> String {
    formats
        .iter()
        .map(|format| {
            if format.eq_ignore_ascii_case("iso-8601") {
                "YYYY-MM-DD".to_owned()
            } else {
                format
                    .replace("%Y", "YYYY")
                    .replace("%m", "MM")
                    .replace("%d", "DD")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `ListField`'s `not_a_list` (`rest_framework/fields.py:1698-1708`).
pub fn not_a_list(input_type: &str) -> Detail {
    Detail::new(
        format!("Expected a list of items but got type \"{input_type}\"."),
        "not_a_list",
    )
}

/// `ListField`/`ManyRelatedField`'s `allow_empty=False` message.
pub fn empty_list() -> Detail {
    Detail::new("This list may not be empty.", "empty")
}

pub fn list_min_length(min: usize) -> Detail {
    Detail::new(
        format!("Ensure this field has at least {min} elements."),
        "min_length",
    )
}

pub fn list_max_length(max: usize) -> Detail {
    Detail::new(
        format!("Ensure this field has no more than {max} elements."),
        "max_length",
    )
}

/// `IntegerField` options (`min_value`/`max_value`).
#[derive(Debug, Clone, Copy, Default)]
pub struct IntegerOptions {
    pub min_value: Option<i64>,
    pub max_value: Option<i64>,
}

/// `IntegerField.to_internal_value` (`rest_framework/fields.py:1080-1100`):
/// Python `int()` after `re_decimal` strips a trailing `.0…`, then DRF's
/// min/max validators. DRF does not bound the value to 32 bits.
pub fn integer(raw: Raw<'_>, options: &IntegerOptions) -> Result<i64, Detail> {
    let invalid = || Detail::new("A valid integer is required.", "invalid");
    let text = match raw {
        Raw::Text(text) => {
            if text.len() > 1000 {
                return Err(Detail::new("String value too large.", "max_string_length"));
            }
            text.to_owned()
        }
        Raw::Json(Json::Str(value)) => {
            let text = value.as_str().ok_or_else(invalid)?;
            if text.len() > 1000 {
                return Err(Detail::new("String value too large.", "max_string_length"));
            }
            text.to_owned()
        }
        Raw::Json(Json::Int(value)) => value.to_decimal_string(),
        Raw::Json(Json::Float(value)) => plane_query::pyfloat::repr(*value),
        // `int("True")` fails, and null is handled by `field_value`.
        Raw::Json(_) => return Err(invalid()),
    };
    let text = strip_decimal(&text);
    let value: i64 = text.trim().parse().map_err(|_| invalid())?;
    if let Some(max) = options.max_value
        && value > max
    {
        return Err(Detail::new(
            format!("Ensure this value is less than or equal to {max}."),
            "max_value",
        ));
    }
    if let Some(min) = options.min_value
        && value < min
    {
        return Err(Detail::new(
            format!("Ensure this value is greater than or equal to {min}."),
            "min_value",
        ));
    }
    Ok(value)
}

/// `re_decimal.sub('', text)` (`fields.py:1083`): strip `.0…` and trailing
/// whitespace, so `"1.0"` is an int but `"1.2"` is not.
fn strip_decimal(text: &str) -> String {
    let trimmed = text.trim_end();
    if let Some(dot) = trimmed.rfind('.') {
        let fraction = &trimmed[dot + 1..];
        if fraction.chars().all(|character| character == '0') {
            return trimmed[..dot].to_owned();
        }
    }
    trimmed.to_owned()
}

/// `PrimaryKeyRelatedField`'s UUID coercion (`rest_framework/relations.py:
/// 250-290`): exactly [`primary_key`] without the `queryset.get(pk=…)` that
/// follows it. A caller that runs its own lookup (the project serializers'
/// `project_lead`/`default_assignee`) uses this and then rejects a miss with
/// DRF's `Invalid pk "…" - object does not exist.`.
pub fn uuid(raw: Raw<'_>) -> Result<uuid::Uuid, Detail> {
    primary_key(raw)
}

/// DRF's `UUIDField.to_internal_value` (`rest_framework/fields.py:833-844`),
/// the child of `PageSerializer.label_ids`/`project_ids`.
///
/// Different from [`primary_key`]: the input may be an `int` (interpreted as a
/// 128-bit UUID, `uuid.UUID(int=data)`), a `str` in any form Python's hex
/// parser accepts, or anything else — which fails with the field's own
/// `Must be a valid UUID.` message. No queryset is involved.
pub fn uuid_field(raw: Raw<'_>) -> Result<uuid::Uuid, Detail> {
    let invalid = || Detail::new("Must be a valid UUID.", "invalid");
    match raw {
        // `bool` is an `int` in Python (`isinstance(True, int)`), so
        // `uuid.UUID(int=True)` is the all-zero UUID with a trailing 1.
        Raw::Json(Json::Bool(value)) => Ok(uuid::Uuid::from_u128(u128::from(*value))),
        Raw::Json(Json::Int(value)) => value
            .to_decimal_string()
            .parse::<u128>()
            .map(uuid::Uuid::from_u128)
            .map_err(|_| invalid()),
        Raw::Json(Json::Str(value)) => value
            .as_str()
            .ok_or_else(invalid)
            .and_then(|text| uuid::Uuid::parse_str(text).map_err(|_| invalid())),
        Raw::Text(text) => uuid::Uuid::parse_str(text).map_err(|_| invalid()),
        // `self.fail('invalid', value=data)` for every other type.
        Raw::Json(Json::Null | Json::Float(_) | Json::List(_) | Json::Dict(_)) => Err(invalid()),
    }
}

/// `FloatField.to_internal_value` (`rest_framework/fields.py:964-982`).
pub fn float(raw: Raw<'_>) -> Result<f64, Detail> {
    const MAX_STRING_LENGTH: usize = 1000;
    let invalid = || Detail::new("A valid number is required.", "invalid");
    let value = match raw {
        Raw::Json(value) => match value {
            Json::Bool(value) => f64::from(u8::from(*value)),
            Json::Int(value) => {
                let parsed = value
                    .as_i64()
                    .map(|value| value as f64)
                    .or_else(|| value.as_u64().map(|value| value as f64));
                match parsed {
                    Some(value) => value,
                    None => return Err(invalid()),
                }
            }
            Json::Float(value) => *value,
            Json::Str(value) => {
                let text = value.as_str().ok_or_else(invalid)?;
                parse_float(text)?
            }
            Json::Null | Json::List(_) | Json::Dict(_) => return Err(invalid()),
        },
        Raw::Text(text) => {
            if text.len() > MAX_STRING_LENGTH {
                return Err(Detail::new("String value too large.", "max_string_length"));
            }
            parse_float(text)?
        }
    };
    if !value.is_finite() {
        return Err(invalid());
    }
    Ok(value)
}

/// Python's `float(str)`: optional surrounding whitespace, optional sign,
/// `inf`/`infinity`/`nan` in any case, underscores between digits.
fn parse_float(text: &str) -> Result<f64, Detail> {
    let invalid = || Detail::new("A valid number is required.", "invalid");
    let trimmed = text.trim_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        return Err(invalid());
    }
    let (negative, digits) = match trimmed.as_bytes()[0] {
        b'-' => (true, &trimmed[1..]),
        b'+' => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let folded = digits.to_ascii_lowercase();
    if matches!(folded.as_str(), "inf" | "infinity" | "nan") {
        let value = if folded == "nan" {
            f64::NAN
        } else {
            f64::INFINITY
        };
        return Ok(if negative { -value } else { value });
    }
    // Underscores are accepted between digits only (`float("1_0")`).
    if digits.contains('_') {
        let mut cleaned = String::with_capacity(digits.len());
        let mut previous_digit = false;
        for character in digits.chars() {
            if character == '_' {
                if !previous_digit {
                    return Err(invalid());
                }
                previous_digit = false;
                continue;
            }
            previous_digit = character.is_ascii_digit();
            cleaned.push(character);
        }
        if !cleaned.is_empty() && !previous_digit && cleaned.ends_with('_') {
            return Err(invalid());
        }
        return parse_float(&format!("{}{}", if negative { "-" } else { "" }, cleaned));
    }
    trimmed.parse::<f64>().map_err(|_| invalid())
}

/// `DateTimeField.to_internal_value` with DRF's default `DATETIME_INPUT_FORMATS`
/// (`['iso-8601']`, `rest_framework/settings.py:108`) through Django's
/// `parse_datetime` (`django/utils/dateparse.py:104-124`), then
/// `DateTimeField.enforce_timezone` (`fields.py:1168-1189`).
///
/// A naive value is made aware in the *active* timezone; an offset or `Z` is
/// converted to UTC. The accepted shapes are CPython 3.12's
/// `datetime.fromisoformat` (extended and basic dates, ISO week dates, `HHMMSS`
/// times, any single separator character, fractional seconds of any length,
/// `Z` and `±HH[:MM[:SS[.ffffff]]]`) plus Django's `datetime_re` fallback
/// (1–2 digit fields, Unicode digits, `\s*` before the zone, one trailing
/// newline). [`crate::write::pyiso`] is the port; the corpus test below pins
/// every branch.
pub fn datetime(raw: Raw<'_>, timezone: Tz) -> Result<DateTime<Utc>, Detail> {
    let invalid = || {
        Detail::new(
            "Datetime has wrong format. Use one of these formats instead: \
             YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].",
            "invalid",
        )
    };
    let text = match raw {
        Raw::Text(text) => text,
        Raw::Json(Json::Str(value)) => value.as_str().ok_or_else(invalid)?,
        // `isinstance(value, datetime.date)` cannot happen from JSON; a
        // non-string JSON value fails `datetime.fromisoformat` with TypeError,
        // which DRF suppresses, and `strptime` with TypeError again, so the
        // field reports its own format error.
        Raw::Json(_) => return Err(invalid()),
    };
    let parsed = match pyiso::parse_datetime(text) {
        Some(value) => value,
        None => {
            // `DateTimeField.to_internal_value` (`fields.py:1194-1212`) falls
            // through to `datetime.strptime(value, input_format)` even after
            // `parse_datetime` returned `None`. `_strptime` compiles the
            // format string case-insensitively, so the literal `iso-8601`
            // parses to 1900-01-01T00:00:00.
            if !text.eq_ignore_ascii_case("iso-8601") {
                return Err(invalid());
            }
            let Some(value) =
                NaiveDate::from_ymd_opt(1900, 1, 1).and_then(|date| date.and_hms_opt(0, 0, 0))
            else {
                return Err(invalid());
            };
            pyiso::PyDateTime {
                naive: value,
                offset_us: None,
            }
        }
    };
    let utc = match parsed.offset_us {
        // `enforce_timezone`: an aware value is `value.astimezone(tz)`, and an
        // `OverflowError` from that is DRF's `overflow` error. The instant may
        // be inside Python's range while its wall time in the active zone is
        // not (`9999-12-31T23:59:59+05:00` in `Asia/Kolkata`), so the zone's
        // own wall time is checked too.
        Some(offset) => parsed
            .naive
            .checked_sub_signed(chrono::TimeDelta::microseconds(offset))
            .filter(|utc| (1..=9999).contains(&pyiso::local_wall_time(*utc, timezone).year())),
        None => pyiso::localize(parsed.naive, timezone),
    };
    match utc {
        Some(value) if (1..=9999).contains(&value.year()) => Ok(value.and_utc()),
        // `Datetime value out of range.` Also covers the year-1 wall time east
        // of UTC that Django raises an uncaught OverflowError for (a 500).
        _ => Err(Detail::new("Datetime value out of range.", "overflow")),
    }
}

/// `JSONField.to_internal_value` (`rest_framework/fields.py:1848-1858`): any
/// JSON value is stored as-is; a form value is parsed as JSON text.
pub fn json(raw: Raw<'_>) -> Result<JsonText, Detail> {
    match raw {
        Raw::Json(value) => JsonText::from_json(value).map_err(|_| invalid_json()),
        Raw::Text(text) => {
            let value =
                plane_serde::pyjson::loads(text.as_bytes(), "utf-8").map_err(|_| invalid_json())?;
            JsonText::from_json(&value).map_err(|_| invalid_json())
        }
    }
}

/// `JSONField`'s `invalid` detail (`rest_framework/fields.py:1830-1848`).
///
/// A model `JSONField` is `binary=False`, so `to_internal_value` runs
/// `json.dumps(data)` before returning it; a multipart file object — or a
/// `{**request.data}` list holding one — is not serialisable and fails here.
#[must_use]
pub fn invalid_json() -> Detail {
    Detail::new("Value must be valid JSON.", "invalid")
}

/// The `(year, ordinal day)` of a timestamp, used by tests.
#[must_use]
pub fn day_key(value: DateTime<Utc>) -> (i32, u32) {
    (value.year(), value.ordinal())
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson;

    fn parse(text: &str) -> pyjson::Value {
        pyjson::loads(text.as_bytes(), "utf-8").expect("json")
    }

    #[test]
    fn primary_key_reproduces_drf_and_django_messages() {
        let value = parse(r#""3f7c1c0e-1d6a-4d47-9a4f-8f6c6d3b2a10""#);
        assert!(primary_key(Raw::Json(&value)).is_ok());

        let value = parse(r#""not-a-uuid""#);
        assert_eq!(
            primary_key(Raw::Json(&value)).expect_err("uuid"),
            Detail::new("“not-a-uuid” is not a valid UUID.", "invalid")
        );

        let value = parse("true");
        assert_eq!(
            primary_key(Raw::Json(&value)).expect_err("bool"),
            Detail::new(
                "Incorrect type. Expected pk value, received bool.",
                "incorrect_type"
            )
        );

        // Django's `UUIDField` accepts an integer as a 128-bit UUID.
        let value = parse("1");
        assert_eq!(
            primary_key(Raw::Json(&value)).expect("int"),
            Uuid::from_u128(1)
        );

        let value = parse("1.5");
        assert_eq!(
            primary_key(Raw::Json(&value)).expect_err("float"),
            Detail::new("“1.5” is not a valid UUID.", "invalid")
        );
    }

    #[test]
    fn char_coerces_numbers_and_rejects_composites() {
        let options = CharOptions::nullable();
        let value = parse("123");
        assert_eq!(char(Raw::Json(&value), &options).expect("int"), "123");
        let value = parse("true");
        assert_eq!(
            char(Raw::Json(&value), &options).expect_err("bool"),
            Detail::new("Not a valid string.", "invalid")
        );
        let value = parse("[1]");
        assert!(char(Raw::Json(&value), &options).is_err());
        let value = parse("\"  x  \"");
        assert_eq!(char(Raw::Json(&value), &options).expect("trim"), "x");
    }

    #[test]
    fn char_max_length_counts_characters() {
        let options = CharOptions::nullable().with_max_length(2);
        let value = parse("\"ab\"");
        assert!(char(Raw::Json(&value), &options).is_ok());
        let value = parse("\"abc\"");
        assert_eq!(
            char(Raw::Json(&value), &options).expect_err("max"),
            Detail::new(
                "Ensure this field has no more than 2 characters.",
                "max_length"
            )
        );
        let value = parse("\"ééé\"");
        assert!(char(Raw::Json(&value), &options).is_err());
    }

    #[test]
    fn float_matches_python() {
        let value = parse("\"2.5\"");
        assert_eq!(float(Raw::Json(&value)).expect("number"), 2.5);
        let value = parse("\" 1_0 \"");
        assert_eq!(float(Raw::Json(&value)).expect("underscore"), 10.0);
        let value = parse("\"abc\"");
        assert_eq!(
            float(Raw::Json(&value)).expect_err("invalid"),
            Detail::new("A valid number is required.", "invalid")
        );
        let value = parse("true");
        assert_eq!(float(Raw::Json(&value)).expect("bool"), 1.0);
        let value = parse("\"inf\"");
        assert!(float(Raw::Json(&value)).is_err());
    }

    #[test]
    fn booleans_follow_drf_spellings() {
        for (text, expected) in [
            ("true", true),
            ("True", true),
            ("TRUE", true),
            ("t", true),
            ("y", true),
            ("yes", true),
            ("on", true),
            ("1", true),
            ("false", false),
            ("False", false),
            ("f", false),
            ("n", false),
            ("no", false),
            ("off", false),
            ("0", false),
        ] {
            let value = parse(&format!("\"{text}\""));
            assert_eq!(boolean(Raw::Json(&value)).expect(text), expected);
        }
        let value = parse("1");
        assert!(boolean(Raw::Json(&value)).expect("int 1"));
        // `1.0`/`0.0` are set members in CPython (`1.0 == 1`).
        let value = parse("1.0");
        assert!(boolean(Raw::Json(&value)).expect("float 1.0"));
        let value = parse("0.0");
        assert!(!boolean(Raw::Json(&value)).expect("float 0.0"));
        let value = parse("2");
        assert_eq!(
            boolean(Raw::Json(&value)).expect_err("2"),
            Detail::new("Must be a valid boolean.", "invalid")
        );
        let value = parse("2.5");
        assert!(boolean(Raw::Json(&value)).is_err());
        let value = parse("\" maybe\"");
        assert!(boolean(Raw::Json(&value)).is_err());
        assert_eq!(
            boolean(Raw::Text("nope")).expect_err("text"),
            Detail::new("Must be a valid boolean.", "invalid")
        );
    }

    #[test]
    fn choices_keep_drfs_message() {
        let value = parse("2");
        assert_eq!(integer_choice(Raw::Json(&value), &[0, 2]).expect("2"), 2);
        let value = parse("\"0\"");
        assert_eq!(integer_choice(Raw::Json(&value), &[0, 2]).expect("0"), 0);
        let value = parse("\"x\"");
        assert_eq!(
            integer_choice(Raw::Json(&value), &[0, 2]).expect_err("x"),
            Detail::new("\"x\" is not a valid choice.", "invalid_choice")
        );
        let value = parse("3");
        assert_eq!(
            integer_choice(Raw::Json(&value), &[0, 2]).expect_err("3"),
            Detail::new("\"3\" is not a valid choice.", "invalid_choice")
        );
        assert_eq!(
            integer_choice(Raw::Text("y"), &[0, 2]).expect_err("y"),
            Detail::new("\"y\" is not a valid choice.", "invalid_choice")
        );
    }

    #[test]
    fn integer_options_and_max_string_length() {
        let options = IntegerOptions {
            min_value: Some(0),
            max_value: Some(12),
        };
        let value = parse("-1");
        assert_eq!(
            integer(Raw::Json(&value), &options).expect_err("min"),
            Detail::new(
                "Ensure this value is greater than or equal to 0.",
                "min_value"
            )
        );
        let value = parse("13");
        assert_eq!(
            integer(Raw::Json(&value), &options).expect_err("max"),
            Detail::new(
                "Ensure this value is less than or equal to 12.",
                "max_value"
            )
        );
        // `re_decimal` strips a trailing `.0`; `1.2` is not an int.
        let value = parse("1.0");
        assert_eq!(integer(Raw::Json(&value), &options).expect("1.0"), 1);
        let value = parse("1.2");
        assert_eq!(
            integer(Raw::Json(&value), &options).expect_err("1.2"),
            Detail::new("A valid integer is required.", "invalid")
        );
        let too_long = format!("\"{}\"", "9".repeat(1001));
        let value = parse(&too_long);
        assert_eq!(
            integer(Raw::Json(&value), &options).expect_err("long"),
            Detail::new("String value too large.", "max_string_length")
        );
    }

    #[test]
    fn string_choices_use_pythons_str() {
        let value = parse("\"urgent\"");
        assert_eq!(
            choice(Raw::Json(&value), false, &["urgent", "low"]).expect("urgent"),
            "urgent"
        );
        let value = parse("\"nope\"");
        assert_eq!(
            choice(Raw::Json(&value), false, &["urgent"]).expect_err("nope"),
            Detail::new("\"nope\" is not a valid choice.", "invalid_choice")
        );
        let value = parse("1");
        assert_eq!(
            choice(Raw::Json(&value), false, &["urgent"]).expect_err("1"),
            Detail::new("\"1\" is not a valid choice.", "invalid_choice")
        );
        let value = parse("true");
        assert_eq!(
            choice(Raw::Json(&value), false, &["urgent"]).expect_err("bool"),
            Detail::new("\"True\" is not a valid choice.", "invalid_choice")
        );
        let value = parse("[\"x\"]");
        assert_eq!(
            choice(Raw::Json(&value), false, &["urgent"]).expect_err("list"),
            Detail::new("\"[\'x\']\" is not a valid choice.", "invalid_choice")
        );
        let value = parse("{\"a\": 1}");
        assert_eq!(
            choice(Raw::Json(&value), false, &["urgent"]).expect_err("dict"),
            Detail::new("\"{'a': 1}\" is not a valid choice.", "invalid_choice")
        );
        let value = parse("\"\"");
        assert_eq!(
            choice(Raw::Json(&value), true, &["urgent"]).expect("blank"),
            ""
        );
    }

    #[test]
    fn dates_use_drfs_formats_and_messages() {
        let value = parse("\"2026-01-02\"");
        assert_eq!(
            date(DateInput::Raw(Raw::Json(&value)), &["iso-8601"]).expect("date"),
            chrono::NaiveDate::from_ymd_opt(2026, 1, 2).expect("date")
        );
        let value = parse("\"01/02/2026\"");
        assert_eq!(
            date(DateInput::Raw(Raw::Json(&value)), &["iso-8601"]).expect_err("format"),
            Detail::new(
                "Date has wrong format. Use one of these formats instead: YYYY-MM-DD.",
                "invalid"
            )
        );
        assert_eq!(
            date(DateInput::DateTime, &["iso-8601"]).expect_err("datetime"),
            Detail::new("Expected a date but got a datetime.", "datetime")
        );
        let value = parse("\"01/02/2026\"");
        assert_eq!(
            date(DateInput::Raw(Raw::Json(&value)), &["%m/%d/%Y"]).expect("explicit format"),
            chrono::NaiveDate::from_ymd_opt(2026, 1, 2).expect("date")
        );
    }

    #[test]
    fn list_messages_match_drf() {
        assert_eq!(
            not_a_list("str"),
            Detail::new(
                "Expected a list of items but got type \"str\".",
                "not_a_list"
            )
        );
        assert_eq!(
            empty_list(),
            Detail::new("This list may not be empty.", "empty")
        );
        assert_eq!(
            list_min_length(2),
            Detail::new("Ensure this field has at least 2 elements.", "min_length")
        );
        assert_eq!(
            list_max_length(3),
            Detail::new(
                "Ensure this field has no more than 3 elements.",
                "max_length"
            )
        );
    }

    #[test]
    fn integers_and_pks() {
        let value = parse("\" 12 \"");
        assert_eq!(
            integer(Raw::Json(&value), &IntegerOptions::default()).expect("12"),
            12
        );
        let value = parse("\"abc\"");
        assert_eq!(
            integer(Raw::Json(&value), &IntegerOptions::default()).expect_err("abc"),
            Detail::new("A valid integer is required.", "invalid")
        );
        let value = parse("\"not-a-uuid\"");
        assert_eq!(
            uuid(Raw::Json(&value)).expect_err("uuid"),
            Detail::new("“not-a-uuid” is not a valid UUID.", "invalid")
        );
        // DRF hands a non-scalar to `queryset.get(pk=…)`, so Django's
        // `UUIDField` answers with `str(value)` inside its own message.
        let value = parse("[\"x\"]");
        assert_eq!(
            uuid(Raw::Json(&value)).expect_err("list"),
            Detail::new("“['x']” is not a valid UUID.", "invalid")
        );
        let value = parse("{\"a\": 1}");
        assert_eq!(
            uuid(Raw::Json(&value)).expect_err("dict"),
            Detail::new("“{'a': 1}” is not a valid UUID.", "invalid")
        );
        let value = parse("true");
        assert_eq!(
            uuid(Raw::Json(&value)).expect_err("bool"),
            Detail::new(
                "Incorrect type. Expected pk value, received bool.",
                "incorrect_type"
            )
        );
        let value = parse("\"0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f\"");
        assert!(uuid(Raw::Json(&value)).is_ok());
    }

    #[test]
    fn datetime_accepts_djangos_iso_shapes() {
        let utc = chrono_tz::UTC;
        let value = parse("\"2026-01-02T03:04:05Z\"");
        assert_eq!(
            datetime(Raw::Json(&value), utc).expect("zulu").to_string(),
            "2026-01-02 03:04:05 UTC"
        );
        let value = parse("\"2026-01-02\"");
        assert_eq!(
            datetime(Raw::Json(&value), utc).expect("date").to_string(),
            "2026-01-02 00:00:00 UTC"
        );
        let kolkata = chrono_tz::Asia::Kolkata;
        let value = parse("\"2026-01-02T03:04:05\"");
        assert_eq!(
            datetime(Raw::Json(&value), kolkata)
                .expect("naive")
                .to_string(),
            "2026-01-01 21:34:05 UTC"
        );
        let value = parse("\"2026-01-02T03:04:05+0530\"");
        assert_eq!(
            datetime(Raw::Json(&value), utc)
                .expect("offset")
                .to_string(),
            "2026-01-01 21:34:05 UTC"
        );
        let value = parse("\"not-a-date\"");
        assert!(datetime(Raw::Json(&value), utc).is_err());
    }

    #[test]
    fn json_keeps_the_request_value_and_parses_form_text() {
        let value = parse("{\"b\":1,\"a\":2}");
        let text = super::json(Raw::Json(&value)).expect("object");
        assert_eq!(text.as_str(), "{\"b\": 1, \"a\": 2}");
        assert_eq!(
            super::json(Raw::Text("[1, 2]")).expect("form").as_str(),
            "[1, 2]"
        );
        assert!(super::json(Raw::Text("")).is_err());
    }

    /// Recorded from the real chain — `DateTimeField().to_internal_value` and
    /// `DateField().to_internal_value` under an active timezone of `UTC` and
    /// `Asia/Kolkata` (plus `America/New_York` for the four DST cases) — by
    /// `apps/server/tools/inventory/datetime_corpus.py`. Regenerate from the
    /// repository root with:
    ///
    /// ```sh
    /// apps/api/.venv/bin/python apps/server/tools/inventory/datetime_corpus.py
    /// ```
    const DATETIME_CORPUS: &str = include_str!("../../tests/golden/datetime-parsing.json");

    /// The `isoformat()` DRF's `DateTimeField` hands to `JSONRenderer`.
    fn rendered_utc(value: DateTime<Utc>) -> String {
        if value.timestamp_subsec_micros() == 0 {
            value.format("%Y-%m-%dT%H:%M:%SZ").to_string()
        } else {
            value.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
        }
    }

    /// Python's `datetime.isoformat()` of a parsed wall time: the fraction only
    /// when it is non-zero, the offset as `±HH:MM[:SS[.ffffff]]`.
    fn rendered_python(value: crate::write::pyiso::PyDateTime) -> String {
        let mut text = if value.naive.and_utc().timestamp_subsec_micros() == 0 {
            value.naive.format("%Y-%m-%dT%H:%M:%S").to_string()
        } else {
            value.naive.format("%Y-%m-%dT%H:%M:%S%.6f").to_string()
        };
        if let Some(offset) = value.offset_us {
            let sign = if offset < 0 { '-' } else { '+' };
            let magnitude = offset.abs();
            let hours = magnitude / 3_600_000_000;
            let minutes = (magnitude / 60_000_000) % 60;
            let seconds = (magnitude / 1_000_000) % 60;
            let micros = magnitude % 1_000_000;
            text.push_str(&format!("{sign}{hours:02}:{minutes:02}"));
            if seconds != 0 || micros != 0 {
                text.push_str(&format!(":{seconds:02}"));
            }
            if micros != 0 {
                text.push_str(&format!(".{micros:06}"));
            }
        }
        text
    }

    fn check_parse(expected: &serde_json::Value, actual: Option<String>, context: &str) {
        match (expected["ok"].as_bool(), actual) {
            (Some(true), Some(value)) => assert_eq!(
                value,
                expected["value"].as_str().expect("value"),
                "{context}"
            ),
            (Some(true), None) => {
                panic!("{context}: expected {:?}, got nothing", expected["value"])
            }
            (Some(false), None) => {}
            (Some(false), Some(value)) => {
                panic!("{context}: expected no value, got {value}")
            }
            (None, _) => panic!("{context}: malformed expectation"),
        }
    }

    fn check_datetime(
        raw: Raw<'_>,
        zone: chrono_tz::Tz,
        expected: &serde_json::Value,
        context: &str,
    ) {
        let actual = datetime(raw, zone);
        match (expected["ok"].as_bool(), actual) {
            (Some(true), Ok(value)) => assert_eq!(
                rendered_utc(value),
                expected["value"].as_str().expect("value"),
                "{context} ({zone})"
            ),
            (Some(true), Err(detail)) => {
                panic!(
                    "{context} ({zone}): expected {:?}, got {detail:?}",
                    expected["value"]
                )
            }
            (Some(false), Ok(value)) => panic!(
                "{context} ({zone}): expected an error, got {}",
                rendered_utc(value)
            ),
            (Some(false), Err(detail)) => {
                if expected.get("server_error").is_some() {
                    // Django raises an uncaught OverflowError (a 500) for a
                    // year-1 wall time east of UTC; `fields::datetime` cannot
                    // signal that, so any error is accepted here.
                    return;
                }
                assert_eq!(
                    detail.message,
                    expected["message"].as_str().expect("message"),
                    "{context} ({zone})"
                );
                assert_eq!(
                    detail.code,
                    expected["code"].as_str().expect("code"),
                    "{context} ({zone})"
                );
            }
            (None, _) => panic!("{context}: malformed expectation"),
        }
    }

    fn check_date(raw: Raw<'_>, expected: &serde_json::Value, context: &str) {
        let actual = date(DateInput::Raw(raw), &["iso-8601"]);
        match (expected["ok"].as_bool(), actual) {
            (Some(true), Ok(value)) => assert_eq!(
                value.format("%Y-%m-%d").to_string(),
                expected["value"].as_str().expect("value"),
                "{context}"
            ),
            (Some(true), Err(detail)) => {
                panic!(
                    "{context}: expected {:?}, got {detail:?}",
                    expected["value"]
                )
            }
            (Some(false), Ok(value)) => {
                panic!("{context}: expected an error, got {value}")
            }
            (Some(false), Err(detail)) => {
                assert_eq!(
                    detail.message,
                    expected["message"].as_str().expect("message"),
                    "{context}"
                );
                assert_eq!(
                    detail.code,
                    expected["code"].as_str().expect("code"),
                    "{context}"
                );
            }
            (None, _) => panic!("{context}: malformed expectation"),
        }
    }

    #[test]
    fn the_datetime_and_date_parsers_match_drf() {
        let document: serde_json::Value =
            serde_json::from_str(DATETIME_CORPUS).expect("corpus json");
        let cases = document["cases"].as_array().expect("cases");
        assert!(cases.len() >= 1000, "corpus shrank: {}", cases.len());
        let mut checked = 0usize;
        for case in cases {
            let written = serde_json::to_string(&case["input"]).expect("input json");
            let value = pyjson::loads(written.as_bytes(), "utf-8").expect("input");
            let text = case["input"].as_str();
            let zones = case["datetime"].as_object().expect("datetime");
            for (name, expected) in zones {
                let zone: chrono_tz::Tz = name.parse().expect("zone");
                if let Some(text) = text {
                    // A JSON string and a form/multipart value reach the field
                    // as the same Python `str`; both Rust variants must agree.
                    check_datetime(Raw::Text(text), zone, expected, &written);
                    check_datetime(Raw::Json(&value), zone, expected, &written);
                    checked += 2;
                } else {
                    check_datetime(Raw::Json(&value), zone, expected, &written);
                    checked += 1;
                }
            }
            if let Some(text) = text {
                check_date(Raw::Text(text), &case["date"], &written);
                check_date(Raw::Json(&value), &case["date"], &written);
                // Django's `parse_datetime`/`parse_date` themselves: the wall
                // time and offset they return, before DRF applies the active
                // timezone.
                check_parse(
                    &case["parse"]["datetime"],
                    crate::write::pyiso::parse_datetime(text).map(rendered_python),
                    &written,
                );
                check_parse(
                    &case["parse"]["date"],
                    crate::write::pyiso::parse_date(text)
                        .map(|value| value.format("%Y-%m-%d").to_string()),
                    &written,
                );
            } else {
                check_date(Raw::Json(&value), &case["date"], &written);
            }
        }
        assert!(checked >= 2000, "only {checked} datetime checks ran");
    }

    #[test]
    fn the_fromisoformat_quirks_are_ported() {
        let utc = chrono_tz::UTC;
        for (input, expected) in [
            ("20261120", "2026-11-20 00:00:00 UTC"),
            ("2026-W47-5", "2026-11-20 00:00:00 UTC"),
            ("2026W475", "2026-11-20 00:00:00 UTC"),
            // Any single character is a valid date/time separator.
            ("2026-11-20X12:00:00", "2026-11-20 12:00:00 UTC"),
            // A week date with a time hides the day in a different place.
            ("2026-W47-15", "2026-11-16 15:00:00 UTC"),
            // `HHMMSS` with no separators.
            ("20261120T120000", "2026-11-20 12:00:00 UTC"),
            // One ignored character before the zone designator.
            ("2026-11-20T12:00 Z", "2026-11-20 12:00:00 UTC"),
            // `tzinfo_from_isoformat_results` drops sub-second offsets when
            // the whole-second part is zero.
            ("2026-11-20T12:00:00+00:00:00.5", "2026-11-20 12:00:00 UTC"),
            // Django's `datetime_re` fallback: 1-digit fields, Unicode digits.
            ("2026-1-2 1:2", "2026-01-02 01:02:00 UTC"),
            ("٢٠٢٦-١١-٢٠ ١٢:٠٠", "2026-11-20 12:00:00 UTC"),
            // `strptime(value, "iso-8601")` matches its literal format
            // case-insensitively.
            ("iso-8601", "1900-01-01 00:00:00 UTC"),
        ] {
            let value = pyjson::loads(
                serde_json::to_string(input).expect("json").as_bytes(),
                "utf-8",
            )
            .expect("value");
            assert_eq!(
                datetime(Raw::Json(&value), utc).expect(input).to_string(),
                expected,
                "{input:?}"
            );
        }
        // The previous implementation trimmed whitespace; `fromisoformat`
        // does not, and Django's `date_re` has no leading `\s*`.
        let value = pyjson::loads(b"\" 2026-11-2\"", "utf-8").expect("value");
        assert!(datetime(Raw::Json(&value), utc).is_err());
        assert!(date(DateInput::Raw(Raw::Json(&value)), &["iso-8601"]).is_err());
    }

    #[test]
    fn an_ambiguous_or_imaginary_wall_time_uses_fold_zero() {
        let new_york: chrono_tz::Tz = "America/New_York".parse().expect("zone");
        for (input, expected) in [
            // Imaginary: 02:30 does not exist on 2026-03-08; fold=0 keeps the
            // pre-gap offset (-05:00).
            ("2026-03-08 02:30", "2026-03-08 07:30:00 UTC"),
            // Ambiguous: fold=0 is the first 01:30 (-04:00).
            ("2026-11-01 01:30", "2026-11-01 05:30:00 UTC"),
        ] {
            let value = pyjson::loads(
                serde_json::to_string(input).expect("json").as_bytes(),
                "utf-8",
            )
            .expect("value");
            assert_eq!(
                datetime(Raw::Json(&value), new_york)
                    .expect(input)
                    .to_string(),
                expected,
                "{input:?}"
            );
        }
    }
}
