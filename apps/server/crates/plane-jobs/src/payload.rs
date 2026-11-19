// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The queue payload as the producer wrote it: JSON text, and a Python value
//! whose object keys keep the producer's order.
//!
//! `plane_jobs.jobs.payload` is a `json` column, not `jsonb`: `json` stores
//! the exact text, so `{"b": 1, "a": 2}` keeps "b" before "a". `jsonb` parses
//! the value and stores object keys sorted, which used to reorder a Python
//! dict between two Rust tasks — visible in `send_email_notification` (which
//! actor `data.0` is, the order of comments) and in the webhook payload body,
//! and worked around per task (`webhook_task`'s `reorder_event_data`). The
//! order now survives the queue for every task.
//!
//! The rest of the workspace's `serde_json` stays a sorted `BTreeMap`: this
//! type keeps the ordered [`plane_serde::pyjson::Value`] beside the ordinary
//! [`serde_json::Value`], and handlers that do not care continue to read the
//! latter through [`Deref`]. A handler that *does* care (a dict iterated into
//! an e-mail or a request body) asks for [`Payload::py`] or
//! [`Payload::py_at`], which are the values the producer wrote, in order.
//!
//! [`Job::from_message`](crate::queue::Job::from_message) builds the ordered
//! value from the typed Celery message; [`Job::payload`](crate::queue::Job::payload)
//! for a hand-built payload serialises the `serde_json::Value` (sorted), which
//! is the same order the old `jsonb` column imposed.

use std::ops::Deref;

use plane_serde::pyjson::{self, Value as PyValue};
use serde_json::Value;

/// Why a queued payload could not be read.
#[derive(Debug, thiserror::Error)]
pub enum PayloadError {
    #[error("queue payload is not valid JSON: {0}")]
    Json(#[from] pyjson::LoadError),
    #[error("queue payload could not be converted: {0}")]
    Convert(String),
}

/// One payload, in both representations.
#[derive(Clone, Debug)]
pub struct Payload {
    py: PyValue,
    value: Value,
}

impl Payload {
    /// The empty object a `Job::new` starts with.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            py: PyValue::Dict(pyjson::PyDict::new()),
            value: Value::Object(serde_json::Map::new()),
        }
    }

    /// Read a payload back from the queue (`payload::text`).
    ///
    /// # Errors
    ///
    /// The column does not hold valid JSON.
    pub fn from_text(text: &str) -> Result<Self, PayloadError> {
        let py = pyjson::loads(text.as_bytes(), "utf-8")?;
        Self::from_py(py)
    }

    /// Build from the ordered tree (the producer's `TaskArg` values).
    ///
    /// # Errors
    ///
    /// The value cannot be rendered as `serde_json` (a non-finite float).
    pub(crate) fn from_py(py: PyValue) -> Result<Self, PayloadError> {
        let value =
            serde_json::to_value(&py).map_err(|error| PayloadError::Convert(error.to_string()))?;
        Ok(Self { py, value })
    }

    /// Build from a hand-written `serde_json::Value` (sorted key order).
    #[must_use]
    pub fn from_value(value: Value) -> Self {
        let py = serde_json::to_vec(&value)
            .ok()
            .and_then(|bytes| pyjson::loads(&bytes, "utf-8").ok())
            .unwrap_or(PyValue::Null);
        Self { py, value }
    }

    /// The ordered value the producer wrote.
    #[must_use]
    pub fn py(&self) -> &PyValue {
        &self.py
    }

    /// The ordered value at an object path (`["kwargs", "notification_data"]`).
    #[must_use]
    pub fn py_at(&self, path: &[&str]) -> Option<&PyValue> {
        let mut current = &self.py;
        for key in path {
            let PyValue::Dict(dict) = current else {
                return None;
            };
            current = dict.get(key)?;
        }
        Some(current)
    }

    /// The ordinary `serde_json` view.
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// The payload's text, in producer order.
    ///
    /// `serde_json`'s serializer writes a map's entries in the order the
    /// `Serialize` implementation yields them, and `pyjson`'s yields the
    /// stored order, so this is the producer's text.
    #[must_use]
    pub fn text(&self) -> String {
        serde_json::to_string(&self.py).unwrap_or_else(|_| "{}".to_string())
    }
}

impl Default for Payload {
    fn default() -> Self {
        Self::empty()
    }
}

impl Deref for Payload {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl From<Value> for Payload {
    fn from(value: Value) -> Self {
        Self::from_value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_the_producers_key_order() {
        let payload = Payload::from_text(r#"{"args":[],"kwargs":{"b":1,"a":{"y":2,"x":3}}}"#)
            .expect("payload");
        assert_eq!(
            payload.text(),
            r#"{"args":[],"kwargs":{"b":1,"a":{"y":2,"x":3}}}"#
        );
        // The serde view is sorted, as the rest of the workspace expects.
        assert_eq!(
            payload.value(),
            &serde_json::json!({"args": [], "kwargs": {"a": {"x": 3, "y": 2}, "b": 1}})
        );
        assert_eq!(
            payload
                .py_at(&["kwargs", "a"])
                .and_then(|value| match value {
                    PyValue::Dict(dict) => Some(
                        dict.keys()
                            .map(|key| key.to_string_lossy().into_owned())
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                }),
            Some(vec!["y".to_string(), "x".to_string()])
        );
    }

    #[test]
    fn a_hand_built_value_round_trips() {
        let payload = Payload::from_value(serde_json::json!({"a": 1, "b": [true, null]}));
        assert_eq!(payload.get("a").and_then(Value::as_i64), Some(1));
        assert_eq!(payload.text(), r#"{"a":1,"b":[true,null]}"#);
    }
}
