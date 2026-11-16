// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shallow `dict.update` for a JSON column — the pattern
//! `ProjectMemberPreferenceSerializer.validate_preferences`
//! (`plane/app/serializers/project.py:185-189`) and the ProjectUserProperty
//! endpoints use.
//!
//! Python's `dict.update(other)` keeps the stored key's position when a key is
//! replaced and appends new keys in `other`'s order; the values themselves are
//! replaced wholesale (no deep merge). The result is written with
//! `JsonText::from_json` (Django's `json.dumps`), and the *in-memory* dict is
//! what the response renders, so the order here is observable.

use crate::models::JsonText;
use plane_serde::Error;
use plane_serde::pyjson::{PyDict, Value as Json};

/// `stored.update(patch)` for two JSON objects.
///
/// # Errors
///
/// A value `json.dumps` refuses (a non-UTF-8 string) or a non-object operand.
pub fn merge(stored: &JsonText, patch: &Json) -> Result<JsonText, Error> {
    let stored_value = plane_serde::pyjson::loads(stored.as_str().as_bytes(), "utf-8").ok();
    let mut merged = match &stored_value {
        Some(Json::Dict(object)) => object.clone(),
        _ => PyDict::new(),
    };
    if let Json::Dict(patch) = patch {
        for (key, value) in patch.iter() {
            merged.insert(key.clone(), value.clone());
        }
    } else {
        return Err(Error::Syntax(
            "the preferences patch is not a JSON object".to_owned(),
        ));
    }
    JsonText::from_json(&Json::Dict(merged))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(pairs: &[(&str, Json)]) -> Json {
        let mut object = PyDict::new();
        for (key, value) in pairs {
            object.insert(plane_serde::pyjson::PyStr::from(*key), value.clone());
        }
        Json::Dict(object)
    }

    #[test]
    fn update_keeps_positions_and_appends_new_keys() {
        let stored = JsonText::new(r#"{"a": 1, "b": 2}"#);
        let patch = dict(&[("b", Json::Bool(true)), ("c", Json::Bool(false))]);
        let merged = merge(&stored, &patch).expect("merge");
        assert_eq!(merged.as_str(), r#"{"a": 1, "b": true, "c": false}"#);
    }

    #[test]
    fn a_non_object_patch_is_an_error() {
        let stored = JsonText::new("{}");
        assert!(merge(&stored, &Json::Null).is_err());
    }
}
