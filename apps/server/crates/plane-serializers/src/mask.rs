// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `fields=` as DRF sees it.
//!
//! Two shapes exist in the Python code:
//!
//! * the query parameter, split on commas
//!   (`[f for f in request.GET.get("fields", "").split(",") if f]`); every
//!   entry is a plain name;
//! * the dict-item form accepted by `DynamicBaseSerializer._filter_fields`
//!   (`plane/app/serializers/base.py:104-132`): `{"name": [...]}` adds `"name"`
//!   to the allowed set and **ignores** the nested list (the docstring says
//!   "nested filtering is not supported").
//!
//! [`FieldEntry::Nested`] models the second form. Its `fields` are kept in the
//! inventory for documentation, but [`FieldMask`] never descends into them —
//! exactly like `allowed.update(item.keys())`.

/// One entry of a `fields=` list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldEntry<'a> {
    /// A bare field name.
    Name(&'a str),
    /// The dict-item form. DRF adds **every** key of the dict to the allowed
    /// set (`allowed.update(item.keys())`) and ignores the value entirely.
    Nested {
        names: &'a [&'a str],
        fields: &'a [FieldEntry<'a>],
    },
}

impl<'a> FieldEntry<'a> {
    /// A bare name.
    #[must_use]
    pub fn name(name: &'a str) -> Self {
        Self::Name(name)
    }

    /// Whether this entry allows `name`.
    #[must_use]
    pub fn allows(&self, name: &str) -> bool {
        match self {
            Self::Name(field) => *field == name,
            Self::Nested { names, .. } => names.contains(&name),
        }
    }
}

/// The set of names a `fields=` argument allows.
///
/// `All` is the no-argument case: DRF's `_output_fields` stays `None` and
/// `_readable_fields` yields everything.
#[derive(Clone, Copy, Debug)]
pub enum FieldMask<'a> {
    /// No `fields=` argument was given.
    All,
    /// `fields=` was given. An empty slice means "nothing", which is what
    /// `DynamicBaseSerializer(fields=[])` produces (`allowed` is the empty set).
    Only(&'a [FieldEntry<'a>]),
}

impl<'a> FieldMask<'a> {
    #[must_use]
    pub fn from_option(fields: Option<&'a [FieldEntry<'a>]>) -> Self {
        match fields {
            Some(entries) => Self::Only(entries),
            None => Self::All,
        }
    }

    #[must_use]
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// Whether `name` is in the mask. Does not consider `expand=`.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(entries) => entries.iter().any(|entry| entry.allows(name)),
        }
    }
}
