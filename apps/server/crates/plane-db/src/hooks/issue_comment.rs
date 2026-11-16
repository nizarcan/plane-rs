// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H14/H15 — `IssueComment.save` and its `Description` row
//! (`plane/db/models/issue.py:481-532`).
//!
//! ```python
//! self.comment_stripped = strip_tags(self.comment_html) if self.comment_html != "" else ""
//! is_creating = self._state.adding
//! with transaction.atomic():
//!     super().save()
//!     if is_creating or not self.description_id:
//!         description = Description.objects.create(workspace_id=…, project_id=…,
//!             created_by_id=…, updated_by_id=…, description_stripped=self.comment_stripped,
//!             description_json=self.comment_json, description_html=self.comment_html)
//!         self.description_id = description.id
//!         super().save(update_fields=["description_id"])
//!     else:
//!         changed = {desc: getattr(self, comment) for comment, desc in
//!                    {"comment_html": "description_html", …}.items()
//!                    if comment in self._changes_on_save}
//!         if changed: Description.objects.filter(pk=self.description_id).update(
//!             **changed, updated_by_id=self.updated_by_id, updated_at=self.updated_at)
//! ```
//!
//! `strip_tags` here is `plane.utils.html_processor.strip_tags`
//! (`HTMLParser(convert_charrefs=True)`), i.e. [`super::html::strip_tags`].
//! The `Description` row's own save hook (H28) uses
//! `django.utils.html.strip_tags`, which differs only for input that ends
//! inside an unterminated construct; the serializer's `nh3` output is
//! well-formed, so both are the same string here.

use crate::models::issue::IssueComment;

/// H14: `comment_stripped` on every save.
pub fn before_save(comment: &mut IssueComment) {
    comment.comment_stripped = if comment.comment_html.is_empty() {
        String::new()
    } else {
        super::html::strip_tags(&comment.comment_html)
    };
}

/// H28: the `description_stripped` `Description.save` derives, `None` for an
/// empty html.
#[must_use]
pub fn description_stripped(description_html: &str) -> Option<String> {
    if description_html.is_empty() {
        None
    } else {
        Some(super::html::strip_tags(description_html))
    }
}

/// Which tracked fields changed between the stored row and the row about to be
/// saved — Python's `_changes_on_save` for `TRACKED_FIELDS`
/// (`plane/db/models/issue.py:479`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DescriptionChanges {
    pub html: bool,
    pub stripped: bool,
    pub json: bool,
}

impl DescriptionChanges {
    /// Whether the `descriptions` row needs the column update.
    #[must_use]
    pub fn any(self) -> bool {
        self.html || self.stripped || self.json
    }
}

/// The three `IssueComment` fields mapped onto the `Description` row.
#[must_use]
pub fn description_changes(old: &IssueComment, new: &IssueComment) -> DescriptionChanges {
    DescriptionChanges {
        html: old.comment_html != new.comment_html,
        stripped: old.comment_stripped != new.comment_stripped,
        // Python compares the decoded dicts: equal content in a different key
        // order is *not* a change.
        json: !json_equal(&old.comment_json, &new.comment_json),
    }
}

/// Python `dict.__eq__` semantics for two `jsonb` texts (key order ignored).
fn json_equal(left: &crate::JsonText, right: &crate::JsonText) -> bool {
    match (
        plane_serde::pyjson::loads(left.as_str().as_bytes(), "utf-8"),
        plane_serde::pyjson::loads(right.as_str().as_bytes(), "utf-8"),
    ) {
        (Ok(left), Ok(right)) => value_equal(&left, &right),
        _ => left.as_str() == right.as_str(),
    }
}

fn value_equal(left: &plane_serde::pyjson::Value, right: &plane_serde::pyjson::Value) -> bool {
    use plane_serde::pyjson::Value;
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::List(a), Value::List(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| value_equal(a, b))
        }
        (Value::Dict(a), Value::Dict(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key.as_str().unwrap_or_default())
                        .is_some_and(|other| value_equal(value, other))
                })
        }
        _ => false,
    }
}
