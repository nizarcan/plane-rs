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

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use plane_db::JsonText;
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

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
        Raw::Json(Json::List(_)) => Err(incorrect_pk_type("list")),
        Raw::Json(Json::Dict(_)) => Err(incorrect_pk_type("dict")),
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
/// `True`/`"True"`/`"true"`/`"1"`/`1` are true, `False`/`"False"`/`"false"`/
/// `"0"`/`0` false, anything else `Must be a valid boolean.`.
pub fn boolean(raw: Raw<'_>) -> Result<bool, Detail> {
    let invalid = || Detail::new("Must be a valid boolean.", "invalid");
    match raw {
        Raw::Json(Json::Bool(value)) => Ok(*value),
        Raw::Json(Json::Int(value)) => match value.as_i64() {
            Some(1) => Ok(true),
            Some(0) => Ok(false),
            _ => Err(invalid()),
        },
        Raw::Json(Json::Str(value)) => {
            let text = value.as_str().ok_or_else(invalid)?;
            boolean_text(text).ok_or_else(invalid)
        }
        Raw::Text(text) => boolean_text(text).ok_or_else(invalid),
        Raw::Json(_) => Err(invalid()),
    }
}

fn boolean_text(text: &str) -> Option<bool> {
    match text {
        "True" | "true" | "1" => Some(true),
        "False" | "false" | "0" => Some(false),
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

/// Python `str(value)` for the choice error message.
fn python_str(raw: Raw<'_>) -> String {
    match raw {
        Raw::Text(text) => text.to_owned(),
        Raw::Json(Json::Bool(value)) => {
            if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Raw::Json(Json::Int(value)) => value.to_string(),
        Raw::Json(Json::Float(value)) => plane_query::pyfloat::repr(*value),
        Raw::Json(Json::Str(value)) => value.as_str().unwrap_or_default().to_owned(),
        Raw::Json(Json::Null) => "None".to_owned(),
        Raw::Json(_) => String::new(),
    }
}

/// `IntegerField.to_internal_value`: `int(value)`, with
/// `A valid integer is required.` on failure and Python's 32-bit range check
/// left to Postgres (DRF does not bound it).
pub fn integer(raw: Raw<'_>) -> Result<i32, Detail> {
    let invalid = || Detail::new("A valid integer is required.", "invalid");
    let overflow = || Detail::new("A valid integer is required.", "invalid");
    match raw {
        Raw::Json(Json::Int(value)) => value
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(overflow),
        Raw::Json(Json::Str(value)) => {
            let text = value.as_str().ok_or_else(invalid)?;
            integer_text(text)
        }
        Raw::Text(text) => integer_text(text),
        Raw::Json(Json::Float(value)) => {
            // `int(2.0)` succeeds; `int(2.5)` truncates in Python only via
            // `int()`, which DRF calls on the *string* form.
            integer_text(&plane_query::pyfloat::repr(*value))
        }
        _ => Err(invalid()),
    }
}

fn integer_text(text: &str) -> Result<i32, Detail> {
    let invalid = || Detail::new("A valid integer is required.", "invalid");
    text.trim()
        .parse::<i64>()
        .ok()
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(invalid)
}

/// `PrimaryKeyRelatedField`'s UUID coercion
/// (`rest_framework/relations.py:250-290`): a malformed value is
/// `Incorrect type. Expected pk value, received str.` in DRF 3.18
/// (`PKOnlyObject`/`to_internal_value` catches `TypeError`/`ValueError`).
pub fn uuid(raw: Raw<'_>) -> Result<uuid::Uuid, Detail> {
    let invalid = || Detail::new("Incorrect type. Expected pk value.", "incorrect_type");
    let text = match raw {
        Raw::Json(Json::Str(value)) => value.as_str().map(str::to_owned),
        Raw::Json(Json::Int(value)) => value.as_i64().map(|value| value.to_string()),
        Raw::Text(text) => Some(text.to_owned()),
        _ => None,
    };
    let Some(text) = text else {
        return Err(invalid());
    };
    uuid::Uuid::parse_str(&text).map_err(|_| invalid())
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
/// `parse_datetime` (`django/utils/dateparse.py:104-124`).
///
/// A naive value is made aware in the *active* timezone
/// (`DateTimeField.enforce_timezone`, `fields.py:1168-1189`); an offset or `Z`
/// is converted to UTC. The accepted shapes are Django's `datetime_re`:
/// `YYYY-MM-DD` (CPython 3.11+ `fromisoformat` accepts a bare date),
/// `YYYY-MM-DD[T ]H:MM[:SS[.ffffff]][Z|±HH[:MM]|±HHMM]`, with 1–2 digit
/// components.
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
        // which DRF suppresses and then fails `invalid`.
        Raw::Json(_) => return Err(invalid()),
    };
    parse_datetime(text, timezone).ok_or_else(invalid)
}

/// Django `parse_datetime`, restricted to the ISO shapes DRF accepts.
fn parse_datetime(text: &str, timezone: Tz) -> Option<DateTime<Utc>> {
    // Bare date (CPython 3.11+ `datetime.fromisoformat`).
    if text.len() == 10
        && let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d")
    {
        return localize(date.and_hms_opt(0, 0, 0)?, timezone);
    }
    let (date_part, rest) = text.split_once(['T', ' '])?;
    let date = parse_loose_date(date_part)?;
    let (time_part, offset_part) = split_offset(rest)?;
    let time = parse_loose_time(time_part)?;
    let naive = NaiveDateTime::new(date, time);
    match offset_part {
        None => localize(naive, timezone),
        Some(offset) => {
            let offset = parse_offset(offset)?;
            let fixed = chrono::FixedOffset::east_opt(offset)?;
            fixed
                .from_local_datetime(&naive)
                .single()
                .map(|value| value.with_timezone(&Utc))
        }
    }
}

fn parse_loose_date(text: &str) -> Option<NaiveDate> {
    let mut parts = text.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    NaiveDate::from_ymd_opt(year, month, day)
}

fn parse_loose_time(text: &str) -> Option<chrono::NaiveTime> {
    let (clock, fraction) = match text.split_once(['.', ',']) {
        Some((clock, fraction)) => (clock, Some(fraction)),
        None => (text, None),
    };
    let mut parts = clock.split(':');
    let hour: u32 = parts.next()?.parse().ok()?;
    let minute: u32 = parts.next()?.parse().ok()?;
    let second: u32 = match parts.next() {
        Some(second) => second.parse().ok()?,
        None => 0,
    };
    if parts.next().is_some() {
        return None;
    }
    let microsecond = match fraction {
        Some(fraction) => {
            if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            // Django keeps the first six digits.
            let padded: String = fraction
                .chars()
                .chain(std::iter::repeat('0'))
                .take(6)
                .collect();
            padded.parse().ok()?
        }
        None => 0,
    };
    chrono::NaiveTime::from_hms_micro_opt(hour, minute, second, microsecond)
}

/// Split `HH:MM[:SS]` from a trailing `Z`/`±HH[:MM]` offset.
fn split_offset(text: &str) -> Option<(&str, Option<&str>)> {
    if let Some(stripped) = text.strip_suffix(['Z', 'z']) {
        return Some((stripped.trim_end(), Some("Z")));
    }
    // An offset starts at the last `+`/`-` that follows the clock.
    let bytes = text.as_bytes();
    for index in (1..bytes.len()).rev() {
        if bytes[index] == b'+' || bytes[index] == b'-' {
            let (clock, offset) = text.split_at(index);
            return Some((clock.trim_end(), Some(offset)));
        }
        if bytes[index] == b':' {
            continue;
        }
        if !bytes[index].is_ascii_digit() {
            return None;
        }
    }
    Some((text.trim_end(), None))
}

fn parse_offset(text: &str) -> Option<i32> {
    if text == "Z" {
        return Some(0);
    }
    let (sign, digits) = match text.as_bytes()[0] {
        b'-' => (-1, &text[1..]),
        b'+' => (1, &text[1..]),
        _ => return None,
    };
    let (hours, minutes) = match digits.split_once(':') {
        Some((hours, minutes)) => (hours, minutes),
        None => match digits.len() {
            2 => (digits, "0"),
            4 => digits.split_at(2),
            _ => return None,
        },
    };
    let hours: i32 = hours.parse().ok()?;
    let minutes: i32 = minutes.parse().ok()?;
    if minutes >= 60 || hours > 23 {
        return None;
    }
    Some(sign * (hours * 3600 + minutes * 60))
}

/// `timezone.make_aware(value, timezone)`: `None` when the wall-clock time does
/// not exist in `timezone` (Django raises `NonExistentTimeError`, a 500).
fn localize(naive: NaiveDateTime, timezone: Tz) -> Option<DateTime<Utc>> {
    timezone
        .from_local_datetime(&naive)
        .earliest()
        .map(|value| value.with_timezone(&Utc))
}

/// `JSONField.to_internal_value` (`rest_framework/fields.py:1848-1858`): any
/// JSON value is stored as-is; a form value is parsed as JSON text.
pub fn json(raw: Raw<'_>) -> Result<JsonText, Detail> {
    let invalid = || Detail::new("Value must be valid JSON.", "invalid");
    match raw {
        Raw::Json(value) => JsonText::from_json(value).map_err(|_| invalid()),
        Raw::Text(text) => {
            let value =
                plane_serde::pyjson::loads(text.as_bytes(), "utf-8").map_err(|_| invalid())?;
            JsonText::from_json(&value).map_err(|_| invalid())
        }
    }
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
            ("1", true),
            ("false", false),
            ("False", false),
            ("0", false),
        ] {
            let value = parse(&format!("\"{text}\""));
            assert_eq!(boolean(Raw::Json(&value)).expect(text), expected);
        }
        let value = parse("1");
        assert!(boolean(Raw::Json(&value)).expect("int 1"));
        let value = parse("2");
        assert_eq!(
            boolean(Raw::Json(&value)).expect_err("2"),
            Detail::new("Must be a valid boolean.", "invalid")
        );
        let value = parse("\"yes\"");
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
    fn integers_and_pks() {
        let value = parse("\" 12 \"");
        assert_eq!(integer(Raw::Json(&value)).expect("12"), 12);
        let value = parse("\"abc\"");
        assert_eq!(
            integer(Raw::Json(&value)).expect_err("abc"),
            Detail::new("A valid integer is required.", "invalid")
        );
        let value = parse("\"not-a-uuid\"");
        assert_eq!(
            uuid(Raw::Json(&value)).expect_err("uuid"),
            Detail::new("Incorrect type. Expected pk value.", "incorrect_type")
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
}
