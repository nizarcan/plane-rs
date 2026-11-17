// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.api.serializers.base.BaseSerializer`: the `/api/v1/` expansion map.
//!
//! v1's base is not the app's. Both honour `fields=`/`expand=`, but they differ
//! in four ways a handler has to know about
//! (`plane/api/serializers/base.py:26-141`):
//!
//! 1. `fields=` **removes** fields from `self.fields` (`:36-37`), before
//!    `expand=` runs. [`crate::FilterMode::RemoveFields`] carries that.
//! 2. `expand=` never injects. `if expand in self.fields` (`:85`) is the whole
//!    gate: a name that `fields=` removed, or that the serializer never had, is
//!    ignored, including a name in this map.
//! 3. The mapper has 14 names (`:98-113`), not the app's 22: no `assignees`,
//!    `labels`, `issue_cycle`, `issue_relation`, `issue_intake`,
//!    `issue_reactions`, `issue_link`, `sub_issues`, `issue_related`. It has
//!    `estimate_point`, which the app's does not.
//! 4. A name that is a field but **not** in the mapper is replaced by
//!    `getattr(instance, f"{expand}_id", None)` (`:139`).
//!
//! A mapper name resolves against the instance, not the serialized value
//! (`:121-136`):
//!
//! * a `models.Manager` renders `expansion[name](related, many=True).data`;
//! * a `models.Model` renders `expansion[name](related).data`;
//! * `None` stays `None` (the key keeps its position);
//! * anything else — an annotation, a `SerializerMethodField`, a missing
//!   reverse relation (which raises `RelatedObjectDoesNotExist`, an
//!   `AttributeError`) — leaves the value the serializer already produced.
//!
//! ## The `_id` fallback
//!
//! `getattr(instance, "<name>_id", None)` is the bare primary key when `<name>`
//! names a concrete relation on the instance, and `None` otherwise:
//!
//! * `?expand=member` on `ProjectMemberSerializer` → the member's UUID
//!   (`ProjectMember.member_id`);
//! * `?expand=name` on `WorkspaceLiteSerializer` → `null` (`Workspace` has no
//!   `name_id`);
//! * `?expand=project_id` on `IssueLiteSerializer` → `null`
//!   (`Issue.project_id_id` does not exist), even though `project_id` *is* a
//!   field.
//!
//! [`id_fallback`] is that value, so a handler resolves a non-mapper name with
//! `id_fallback(Some(pk))` or `id_fallback(None)` instead of guessing. Every
//! requested name must be resolved: [`crate::Expansions::write_injected`]
//! refuses an emitted base field without one rather than silently writing the
//! unexpanded value.
//!
//! Pinned by `tests/golden/serializers.jsonl` (`v1_issue_lite/expand_project_id`,
//! `v1_workspace_lite/expand_name`, `v1_workspace_lite/fields_and_expand`),
//! recorded from a real Django process by `tools/golden/serializers_record.py`.

use uuid::Uuid;

use crate::value::DrfValue;

/// v1's `expansion` dict, in declaration order
/// (`plane/api/serializers/base.py:98-113`).
pub const V1_EXPANSION_MAPPER: &[(&str, &str)] = &[
    ("user", "plane.api.serializers.user.UserLiteSerializer"),
    (
        "workspace",
        "plane.api.serializers.workspace.WorkspaceLiteSerializer",
    ),
    (
        "project",
        "plane.api.serializers.project.ProjectLiteSerializer",
    ),
    (
        "default_assignee",
        "plane.api.serializers.user.UserLiteSerializer",
    ),
    (
        "project_lead",
        "plane.api.serializers.user.UserLiteSerializer",
    ),
    ("state", "plane.api.serializers.state.StateLiteSerializer"),
    (
        "created_by",
        "plane.api.serializers.user.UserLiteSerializer",
    ),
    (
        "updated_by",
        "plane.api.serializers.user.UserLiteSerializer",
    ),
    ("issue", "plane.api.serializers.issue.IssueSerializer"),
    ("actor", "plane.api.serializers.user.UserLiteSerializer"),
    ("owned_by", "plane.api.serializers.user.UserLiteSerializer"),
    ("members", "plane.api.serializers.user.UserLiteSerializer"),
    ("parent", "plane.api.serializers.issue.IssueLiteSerializer"),
    (
        "estimate_point",
        "plane.api.serializers.estimate.EstimatePointSerializer",
    ),
];

/// The serializer class `BaseSerializer.to_representation`'s `expansion` dict
/// returns for `name`, or `None` when `name` takes the `_id` fallback.
#[must_use]
pub fn expansion_serializer(name: &str) -> Option<&'static str> {
    V1_EXPANSION_MAPPER
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, serializer)| *serializer)
}

/// Whether `expand=name` resolves through the v1 mapper (and therefore through
/// the instance) rather than through [`id_fallback`].
#[must_use]
pub fn is_expansion_name(name: &str) -> bool {
    expansion_serializer(name).is_some()
}

/// `getattr(instance, f"{name}_id", None)` for a name that is not in the map.
///
/// `id` is `Some` only when `<name>` is a concrete relation on the instance
/// (a `ForeignKey`/`OneToOneField`, whose `<name>_id` attribute holds the
/// primary key). A field of any other kind — a `CharField`, the model's own
/// primary key, an annotation — has no `<name>_id`, so Django writes `null`.
#[must_use]
pub fn id_fallback(id: Option<Uuid>) -> DrfValue<'static> {
    match id {
        Some(id) => DrfValue::Uuid(id),
        None => DrfValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 14 names and their order, transcribed from
    /// `plane/api/serializers/base.py:98-113`.
    #[test]
    fn the_map_is_the_v1_dict() {
        let names: Vec<&str> = V1_EXPANSION_MAPPER.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "user",
                "workspace",
                "project",
                "default_assignee",
                "project_lead",
                "state",
                "created_by",
                "updated_by",
                "issue",
                "actor",
                "owned_by",
                "members",
                "parent",
                "estimate_point",
            ]
        );
        assert_eq!(
            expansion_serializer("estimate_point"),
            Some("plane.api.serializers.estimate.EstimatePointSerializer")
        );
        assert_eq!(
            expansion_serializer("parent"),
            Some("plane.api.serializers.issue.IssueLiteSerializer")
        );
        assert_eq!(
            expansion_serializer("workspace"),
            Some("plane.api.serializers.workspace.WorkspaceLiteSerializer")
        );
    }

    /// The app mapper's to-many names are absent from v1's, and v1 has
    /// `estimate_point` which the app's does not
    /// (`plane/api/serializers/base.py:98-113` vs
    /// `plane/app/serializers/base.py:30-84`). A handler that reuses the app's
    /// map would render `?expand=assignees` as an array where Django writes the
    /// bare `assignees_id` attribute (which does not exist: `null`).
    #[test]
    fn the_app_only_names_are_not_in_the_v1_map() {
        for name in [
            "assignees",
            "labels",
            "issue_cycle",
            "issue_relation",
            "issue_intake",
            "issue_reactions",
            "issue_link",
            "sub_issues",
            "issue_related",
        ] {
            assert!(
                !is_expansion_name(name),
                "{name} is an app-only expansion name"
            );
            assert!(crate::expand::is_expansion_name(name));
        }
        assert!(is_expansion_name("estimate_point"));
        assert!(!crate::expand::is_expansion_name("estimate_point"));
    }

    /// `getattr(instance, "<name>_id", None)`.
    #[test]
    fn the_id_fallback_is_a_pk_or_null() {
        let id = Uuid::parse_str("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb").expect("uuid");
        match id_fallback(Some(id)) {
            DrfValue::Uuid(value) => assert_eq!(value, id),
            other => panic!("expected a uuid, got {other:?}"),
        }
        assert!(matches!(id_fallback(None), DrfValue::Null));
    }
}
