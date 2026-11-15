// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Insertion-ordered JSON objects.
//!
//! Python dicts keep insertion order and both JSON encoders preserve it
//! (HTTP_CONTRACT §10.1, §10.3). The server workspace deliberately does not
//! enable `serde_json`'s `preserve_order` feature — it pulls in `indexmap` and
//! the harness keeps that feature to itself — so `serde_json::Value` objects are
//! backed by a `BTreeMap` and would serialise **alphabetically**. Anything whose
//! key order is visible on the wire must use [`Object`] instead of `json!({..})`
//! with more than one key.
//!
//! Single-key objects are unaffected by ordering; `json!` is fine there.

use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::Value;

/// A JSON object whose keys are written in the order they were added.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Object(Vec<(String, Value)>);

impl Object {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Add a field, preserving insertion order.
    pub fn field(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.0.push((key.into(), value.into()));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }
}

impl Serialize for Object {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl From<Object> for Value {
    fn from(object: Object) -> Self {
        // Best effort for contexts that need a `Value`: order is lost here, so
        // prefer serialising the `Object` directly.
        let mut map = serde_json::Map::new();
        for (key, value) in object.0 {
            map.insert(key, value);
        }
        Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn insertion_order_is_preserved() {
        let object = Object::new()
            .field("error", "REQUEST_BODY_TOO_LARGE")
            .field(
                "detail",
                "The size of the request body exceeds the maximum allowed size.",
            );
        let drf = plane_serde::to_vec(plane_serde::Dialect::Drf, &object).unwrap();
        assert_eq!(
            String::from_utf8(drf).unwrap(),
            r#"{"error":"REQUEST_BODY_TOO_LARGE","detail":"The size of the request body exceeds the maximum allowed size."}"#
        );
        let django = plane_serde::to_vec(plane_serde::Dialect::Django, &object).unwrap();
        assert_eq!(
            String::from_utf8(django).unwrap(),
            r#"{"error": "REQUEST_BODY_TOO_LARGE", "detail": "The size of the request body exceeds the maximum allowed size."}"#
        );
    }

    #[test]
    fn values_keep_their_own_order_rules() {
        // A raw `json!` object is alphabetical because the workspace has no
        // `preserve_order`; this is why multi-key bodies use `Object`.
        let value = json!({"status": "OK"});
        assert_eq!(
            plane_serde::to_vec(plane_serde::Dialect::Django, &value).unwrap(),
            b"{\"status\": \"OK\"}"
        );
    }
}
