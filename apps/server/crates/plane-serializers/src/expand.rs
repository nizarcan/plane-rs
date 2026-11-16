// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `expand=`: the expansion mapper, the `many` set, and resolved values.
//!
//! `get_expansion_mapper()` (`plane/app/serializers/base.py:30-84`) maps an
//! `expand` name to the serializer that renders that relation.
//! `MANY_EXPANSION_FIELDS` (`:14-27`) says which of them are to-many. The two
//! are consumed by `DynamicBaseSerializer._filter_fields` (`:104-132`) and
//! `to_representation` (`:142-190`):
//!
//! 1. `fields=` plus `expand=` names form the allowed set; `fields=` alone
//!    never injects a serializer.
//! 2. An `expand` name that is not already a field of the serializer is
//!    **appended** to `self.fields` with `expansion[name](many=name in
//!    MANY_EXPANSION_FIELDS)`.
//! 3. During output, every `expand` name that is now in `self.fields` has its
//!    value replaced by the relation resolved on the instance: a manager →
//!    list, a model → object, `None` → `null`. A name that is in the mapper and
//!    whose attribute does not exist raises `AttributeError` in Django (a 500)
//!    when the injected field is rendered — Rust refuses with
//!    [`crate::DrfError::MissingExpansion`] at the same point.
//! 4. An `expand` name that is a field but *not* in the mapper is replaced by
//!    `getattr(instance, "<name>_id", None)` (`:167-169`), i.e. a bare pk.
//! 5. `issue_attachments` is special-cased after the loop (`:171-188`): it is
//!    not a model field and not in the mapper, it is appended last, and its
//!    value is a `FileAsset` queryset filtered on `entity_type`.
//!
//! The key order that falls out of this (verified by
//! `crates/plane-serializers/tests/golden.rs`):
//!
//! * base fields in declaration order, filtered by `fields=` ∪ `expand=`;
//! * then injected `expand` names, in the order they appear in `expand=`;
//! * then `issue_attachments`, if requested.
//!
//! A name that is both a base field and an `expand` name keeps the base
//! field's position and gets the expanded value. That is why the generated
//! code prefixes every field write with an expansion lookup rather than
//! appending expansions blindly.

use crate::value::DrfValue;
use crate::writer::ObjectWriter;
use crate::{DrfError, FilterMode, RenderCtx};

/// The `MANY_EXPANSION_FIELDS` set, in the order `base.py` lists it.
pub const MANY_EXPANSION_FIELDS: &[&str] = &[
    "members",
    "assignees",
    "labels",
    "issue_cycle",
    "issue_relation",
    "issue_intake",
    "issue_reactions",
    "issue_link",
    "sub_issues",
    "issue_related",
];

/// The expansion mapper as `(expand name, serializer)` pairs, in the order
/// `get_expansion_mapper()` declares them.
pub const EXPANSION_MAPPER: &[(&str, &str)] = &[
    ("user", "plane.app.serializers.user.UserLiteSerializer"),
    (
        "workspace",
        "plane.app.serializers.workspace.WorkspaceLiteSerializer",
    ),
    (
        "project",
        "plane.app.serializers.project.ProjectLiteSerializer",
    ),
    (
        "default_assignee",
        "plane.app.serializers.user.UserLiteSerializer",
    ),
    (
        "project_lead",
        "plane.app.serializers.user.UserLiteSerializer",
    ),
    ("state", "plane.app.serializers.state.StateLiteSerializer"),
    (
        "created_by",
        "plane.app.serializers.user.UserLiteSerializer",
    ),
    (
        "updated_by",
        "plane.app.serializers.user.UserLiteSerializer",
    ),
    ("issue", "plane.app.serializers.issue.IssueSerializer"),
    ("actor", "plane.app.serializers.user.UserLiteSerializer"),
    ("owned_by", "plane.app.serializers.user.UserLiteSerializer"),
    ("members", "plane.app.serializers.user.UserLiteSerializer"),
    ("assignees", "plane.app.serializers.user.UserLiteSerializer"),
    ("labels", "plane.app.serializers.issue.LabelSerializer"),
    (
        "issue_cycle",
        "plane.app.serializers.cycle.CycleIssueSerializer",
    ),
    ("parent", "plane.app.serializers.issue.IssueLiteSerializer"),
    (
        "issue_relation",
        "plane.app.serializers.issue.IssueRelationSerializer",
    ),
    (
        "issue_intake",
        "plane.app.serializers.intake.IntakeIssueLiteSerializer",
    ),
    (
        "issue_related",
        "plane.app.serializers.issue.RelatedIssueSerializer",
    ),
    (
        "issue_reactions",
        "plane.app.serializers.issue.IssueReactionLiteSerializer",
    ),
    (
        "issue_link",
        "plane.app.serializers.issue.IssueLinkLiteSerializer",
    ),
    (
        "sub_issues",
        "plane.app.serializers.issue.IssueLiteSerializer",
    ),
];

/// `issue_attachments` is not in the mapper but is always injectable, and is
/// appended after every other expansion (`base.py:171-188`).
pub const ISSUE_ATTACHMENTS: &str = "issue_attachments";

/// The serializer class `get_expansion_mapper()` returns for `name`.
#[must_use]
pub fn expansion_serializer(name: &str) -> Option<&'static str> {
    EXPANSION_MAPPER
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, serializer)| *serializer)
}

/// Whether `expand=name` injects a serializer field.
#[must_use]
pub fn is_expansion_name(name: &str) -> bool {
    expansion_serializer(name).is_some()
}

/// One resolved expansion value.
#[derive(Clone, Debug)]
pub struct Expansion<'a> {
    /// The `expand` name, as it appears in the request.
    pub name: &'a str,
    /// The value to write. [`DrfValue::Null`] for a null relation (Django
    /// keeps the key and writes `null`), [`DrfValue::Uuid`] for a non-mapper
    /// field's `<name>_id`.
    pub value: DrfValue<'a>,
}

/// The `expand=` names of a request and the values the handler resolved for
/// them.
#[derive(Clone, Copy, Debug, Default)]
pub struct Expansions<'a> {
    requested: &'a [&'a str],
    values: &'a [Expansion<'a>],
}

impl<'a> Expansions<'a> {
    /// No `expand=`.
    pub const NONE: Self = Self {
        requested: &[],
        values: &[],
    };

    #[must_use]
    pub fn new(requested: &'a [&'a str], values: &'a [Expansion<'a>]) -> Self {
        Self { requested, values }
    }

    /// The request's `expand=` names, in request order, including duplicates.
    #[must_use]
    pub fn requested(&self) -> &'a [&'a str] {
        self.requested
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.requested.is_empty()
    }

    /// Whether `name` was requested. Duplicates do not matter.
    #[must_use]
    pub fn is_requested(&self, name: &str) -> bool {
        self.requested.contains(&name)
    }

    /// The resolved value for `name`, if the handler supplied one.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&'a DrfValue<'a>> {
        self.values
            .iter()
            .find(|expansion| expansion.name == name)
            .map(|expansion| &expansion.value)
    }

    /// Write the injected expansion fields that are not base fields of the
    /// serializer, in request order, followed by `issue_attachments`.
    ///
    /// `base_fields` is the serializer's `_readable_fields` order; generated
    /// code passes its `FIELDS` constant. `mode` is the base class's filtering
    /// behaviour: `plane.api.serializers.base.BaseSerializer` removes fields
    /// for `fields=` but never injects, so nothing is written and nothing is
    /// validated in [`FilterMode::RemoveFields`].
    ///
    /// # Errors
    ///
    /// [`DrfError::MissingExpansion`] when a requested name that Django would
    /// render has no value: a mapper name that is not a base field (injected),
    /// or a requested base field that is actually emitted (its value is
    /// replaced, so leaving the default would be wrong). The handler must
    /// resolve *every* requested name, including `null` relations.
    pub fn write_injected(
        &self,
        obj: &mut ObjectWriter<'_>,
        ctx: &RenderCtx<'_>,
        base_fields: &[&str],
        mode: FilterMode,
    ) -> Result<(), DrfError> {
        if mode == FilterMode::None {
            return Ok(());
        }

        for (index, &name) in self.requested.iter().enumerate() {
            // A duplicate name was written at its first position; Python's
            // second assignment would keep that position too.
            if self.requested[..index].contains(&name) {
                continue;
            }
            if base_fields.contains(&name) {
                // The base field's own write already consulted the expansion;
                // refuse a forgotten resolution instead of silently rendering
                // the unexpanded value. Applies to both filtering modes: v1
                // also replaces a base field on `expand=`.
                if ctx.emits(name, mode) && self.get(name).is_none() {
                    return Err(DrfError::MissingExpansion(name.to_owned()));
                }
                continue;
            }
            // The v1 base (`RemoveFields`) never adds a field.
            if mode == FilterMode::RemoveFields {
                continue;
            }
            if name == ISSUE_ATTACHMENTS {
                continue;
            }
            if let Some(value) = self.get(name) {
                obj.key(name)?;
                obj.value(ctx, value)?;
            } else if is_expansion_name(name) {
                return Err(DrfError::MissingExpansion(name.to_owned()));
            }
            // Not a mapper name and not a field: Django emits nothing.
        }

        if self.is_requested(ISSUE_ATTACHMENTS) {
            match self.get(ISSUE_ATTACHMENTS) {
                Some(value) => {
                    obj.key(ISSUE_ATTACHMENTS)?;
                    obj.value(ctx, value)?;
                }
                None => return Err(DrfError::MissingExpansion(ISSUE_ATTACHMENTS.to_owned())),
            }
        }

        Ok(())
    }
}
