// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Task arguments with the Python type each call site actually passes.
//!
//! Celery serialises arguments with kombu's JSON encoder, which is not plain
//! JSON (`apps/server/TASKS.md` §2.5): a Python `uuid.UUID` becomes
//! `{"__type__": "uuid", "__value__": {"hex": "…"}}`, while its `str()`
//! becomes a plain JSON string. Plane's call sites use both spellings for the
//! same value, so the publisher cannot infer a type from a single argument
//! model — the call site has to state which Python expression it replaces.
//!
//! That is what [`TaskArg`] is for. Each variant names a Python runtime type:
//!
//! | variant | Python value | wire form |
//! | --- | --- | --- |
//! | [`TaskArg::Str`] | `str` | JSON string |
//! | [`TaskArg::Uuid`] | `uuid.UUID` | kombu `uuid` envelope |
//! | [`TaskArg::JsonText`] | `str` from `json.dumps(…, cls=DjangoJSONEncoder)` | JSON string containing JSON |
//! | [`TaskArg::Map`] / [`TaskArg::List`] | `dict` / `list` | JSON object / array |
//! | [`TaskArg::Json`] | a `dict`/`list` parsed from the request body | JSON object / array |
//! | [`TaskArg::Null`] / [`TaskArg::Bool`] / [`TaskArg::Int`] / [`TaskArg::Float`] | `None`, `bool`, `int`, `float` | JSON null/true/false/number |
//!
//! The brief's `ArgStr`/`ArgUuid`/`ArgJsonText` are the [`TaskArg::Str`],
//! [`TaskArg::Uuid`] and [`TaskArg::JsonText`] variants.

use plane_serde::values::KombuUuid;
use plane_serde::{Dialect, to_vec};
use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};
use uuid::Uuid;

use crate::celery::repr;

/// A Python value in a Celery message body.
#[derive(Clone, Debug, PartialEq)]
pub enum TaskArg {
    /// Python `None`.
    Null,
    /// Python `bool`.
    Bool(bool),
    /// Python `int`.
    Int(i64),
    /// Python `float`.
    Float(f64),
    /// Python `str`.
    Str(String),
    /// Python `uuid.UUID` (kombu's type envelope, not a bare string).
    Uuid(Uuid),
    /// A Python `str` that already contains JSON, normally from
    /// `json.dumps(value, cls=DjangoJSONEncoder)`.
    JsonText(String),
    /// Python `list`.
    List(Vec<TaskArg>),
    /// Python `dict`, in insertion order.
    Map(Vec<(String, TaskArg)>),
    /// A JSON value parsed from a request body (`request.data`): a `dict`,
    /// `list` or scalar made of plain JSON types.
    Json(plane_serde::pyjson::Value),
}

impl TaskArg {
    /// A Python `str`.
    #[must_use]
    pub fn str(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    /// A Python `uuid.UUID`; the body carries kombu's `uuid` envelope.
    #[must_use]
    pub fn uuid(value: Uuid) -> Self {
        Self::Uuid(value)
    }

    /// A Python `str` produced by `json.dumps(…, cls=DjangoJSONEncoder)`.
    #[must_use]
    pub fn json_text(value: impl Into<String>) -> Self {
        Self::JsonText(value.into())
    }

    /// `json.dumps(value, cls=DjangoJSONEncoder)` for a parsed request body.
    ///
    /// Django's encoder writes `str` for decimals, ISO-8601 for dates and
    /// datetimes, and `", "`/`": "` separators with `ensure_ascii=True`; that
    /// is [`Dialect::Django`].
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::Json`] if the value cannot be rendered by the
    /// serializer (only possible for a malformed internal value; parsed JSON
    /// always renders).
    pub fn json_text_from(value: &plane_serde::pyjson::Value) -> Result<Self, EncodeError> {
        let bytes = to_vec(Dialect::Django, value).map_err(EncodeError::Json)?;
        let text = String::from_utf8(bytes).map_err(|error| {
            EncodeError::Json(plane_serde::Error::Custom(format!(
                "json.dumps produced invalid UTF-8: {error}"
            )))
        })?;
        Ok(Self::JsonText(text))
    }

    /// A JSON value from a request body, serialised inline (not as a string).
    #[must_use]
    pub fn json(value: plane_serde::pyjson::Value) -> Self {
        Self::Json(value)
    }

    /// A Python `dict`, in insertion order.
    #[must_use]
    pub fn map<K, I>(entries: I) -> Self
    where
        K: Into<String>,
        I: IntoIterator<Item = (K, TaskArg)>,
    {
        Self::Map(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// A Python `list`.
    #[must_use]
    pub fn list(items: impl IntoIterator<Item = TaskArg>) -> Self {
        Self::List(items.into_iter().collect())
    }
}

impl Serialize for TaskArg {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            TaskArg::Null => serializer.serialize_unit(),
            TaskArg::Bool(value) => serializer.serialize_bool(*value),
            TaskArg::Int(value) => serializer.serialize_i64(*value),
            TaskArg::Float(value) => serializer.serialize_f64(*value),
            TaskArg::Str(value) | TaskArg::JsonText(value) => serializer.serialize_str(value),
            TaskArg::Uuid(value) => KombuUuid(*value).serialize(serializer),
            TaskArg::List(items) => {
                let mut sequence = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    sequence.serialize_element(item)?;
                }
                sequence.end()
            }
            TaskArg::Map(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
            TaskArg::Json(value) => value.serialize(serializer),
        }
    }
}

/// A failure to render a message.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// kombu's JSON encoder refused a value.
    #[error("serialising the task body: {0}")]
    Json(#[from] plane_serde::Error),
}

/// Celery's `saferepr` of the positional argument tuple.
///
/// `celery/app/amqp.py:320-321` computes `argsrepr = saferepr(args, 1024)`.
#[must_use]
pub fn argsrepr(args: &[TaskArg]) -> String {
    repr::saferepr_args(args)
}

/// Celery's `saferepr` of the keyword argument dict.
///
/// `celery/app/amqp.py:322-323` computes `kwargsrepr = saferepr(kwargs, 1024)`.
#[must_use]
pub fn kwargsrepr(kwargs: &[(&'static str, TaskArg)]) -> String {
    repr::saferepr_kwargs(kwargs)
}
