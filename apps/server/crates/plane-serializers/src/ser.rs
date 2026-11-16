// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The serializer trait, the request context it renders under, and the entry
//! points a handler calls.

use chrono_tz::Tz;

use crate::DrfError;
use crate::expand::Expansions;
use crate::mask::{FieldEntry, FieldMask};
use crate::writer::DrfWriter;

/// Which `fields=` implementation a serializer inherits.
///
/// Plane has two, and they do not agree:
///
/// * [`FilterMode::OutputOnly`] — `plane.app.serializers.base.DynamicBaseSerializer`
///   (the `/api/` surface). `fields=` restricts output only: validation and
///   saving still see every field, and `_readable_fields` skips the rest
///   (`base.py:129-140`). `expand=` may add fields.
/// * [`FilterMode::RemoveFields`] — `plane.api.serializers.base.BaseSerializer`
///   (the `/api/v1/` surface). `fields=` *removes* fields from `self.fields`
///   (`base.py:36-37`), so nested serializers that test `"x" in self.fields`
///   also see the reduced set. `expand=` never adds a field; it only replaces
///   one that survived the removal. An empty `fields` list is falsy in Python,
///   so it does not filter at all (`base.py:36`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterMode {
    /// A plain `ModelSerializer`/`Serializer`: it does not accept `fields=` or
    /// `expand=` at all (DRF raises `TypeError` for the unknown kwargs). The
    /// generated renderer has no filtering and no expansion pass.
    None,
    /// `DynamicBaseSerializer`: output-only filtering, `expand=` injects.
    OutputOnly,
    /// `plane.api.serializers.base.BaseSerializer`: removal, no injection.
    RemoveFields,
}

/// A renderable serializer.
///
/// Implemented by `crate::generated` (from `tools/inventory/serializers.py
/// --rust`) and by hand for the serializers whose `to_representation` is not
/// field-by-field. See `apps/server/SERIALIZERS.md` for the list.
///
/// `Debug` is a supertrait because a nested serializer may be held as
/// `&dyn DrfSerialize` in [`crate::DrfValue`], and the derived `Debug` of the
/// containing structs must work.
pub trait DrfSerialize: std::fmt::Debug {
    /// Write this serializer's object.
    ///
    /// # Errors
    ///
    /// A [`DrfError`] from a field renderer or an unresolved expansion.
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError>;
}

/// Everything a render needs from the request.
///
/// The timezone is mandatory: `DateTimeField` converts with the *active*
/// timezone (`django.utils.timezone.get_current_timezone()`), which
/// `TimezoneMixin.initial` sets from `request.user.user_timezone`, and
/// `TimezoneMixin` deactivates for anonymous requests — leaving
/// `settings.TIME_ZONE`, "UTC". A handler with no `TimezoneMixin` route also
/// has to say so explicitly with [`RenderCtx::utc`].
#[derive(Clone, Copy, Debug)]
pub struct RenderCtx<'a> {
    tz: &'a Tz,
    mask: FieldMask<'a>,
    expansions: Expansions<'a>,
}

impl<'a> RenderCtx<'a> {
    /// A render context for `?fields=` / `?expand=`.
    ///
    /// `fields` is `None` when the parameter was absent. The entries are the
    /// request's order; `expand` likewise.
    #[must_use]
    pub fn new(tz: &'a Tz, fields: Option<&'a [FieldEntry<'a>]>, expand: &'a [&'a str]) -> Self {
        Self {
            tz,
            mask: FieldMask::from_option(fields),
            expansions: Expansions::new(expand, &[]),
        }
    }

    /// A render context with no filtering and no expansions, in UTC.
    ///
    /// Only for routes that do not run `TimezoneMixin`; a route that does must
    /// pass the user's timezone to [`RenderCtx::new`].
    #[must_use]
    pub fn utc() -> Self {
        Self {
            tz: &chrono_tz::UTC,
            mask: FieldMask::All,
            expansions: Expansions::NONE,
        }
    }

    /// The same request context, with the handler's resolved expansions.
    #[must_use]
    pub fn with_expansions(mut self, expansions: Expansions<'a>) -> Self {
        self.expansions = expansions;
        self
    }

    /// A nested serializer's context: same timezone, no `fields=`/`expand=`.
    ///
    /// A nested serializer is instantiated by its parent field with no
    /// arguments (`expansion[name](related)` / `UserLiteSerializer(...)`), so
    /// it always renders every field.
    #[must_use]
    pub fn full(tz: &'a Tz) -> Self {
        Self {
            tz,
            mask: FieldMask::All,
            expansions: Expansions::NONE,
        }
    }

    /// The active timezone.
    #[must_use]
    pub fn tz(&self) -> &'a Tz {
        self.tz
    }

    /// The `fields=` mask (without `expand=`).
    #[must_use]
    pub fn mask(&self) -> &FieldMask<'a> {
        &self.mask
    }

    /// The requested and resolved expansions.
    #[must_use]
    pub fn expansions(&self) -> &Expansions<'a> {
        &self.expansions
    }

    /// Whether a base field is emitted.
    ///
    /// * [`FilterMode::OutputOnly`]: `fields is None`, or the name is in
    ///   `fields=`, or it is an `expand=` name. `_filter_fields` adds every
    ///   `expand` name to the allowed set (`base.py:113-118`).
    /// * [`FilterMode::RemoveFields`]: `fields` is absent or truthy and
    ///   contains the name. An empty `fields=` list does not filter.
    #[must_use]
    pub fn emits(&self, name: &str, mode: FilterMode) -> bool {
        if mode == FilterMode::None {
            return true;
        }
        match self.mask {
            FieldMask::All => true,
            FieldMask::Only(entries) => {
                if mode == FilterMode::RemoveFields && entries.is_empty() {
                    return true;
                }
                entries.iter().any(|entry| entry.allows(name))
                    || (mode == FilterMode::OutputOnly && self.expansions.is_requested(name))
            }
        }
    }
}

/// A list of serializers, rendered as `ListSerializer` renders `.data`.
///
/// `ListSerializer.to_representation` is
/// `[self.child.to_representation(item) for item in iterable]`
/// (`rest_framework/serializers.py:727-737`), so the child's own
/// `fields=`/`expand=` (none: the view passes those to the list serializer's
/// `child`, and `many_init` copies them — see `SERIALIZERS.md` for the two
/// views that do) and the same timezone apply.
#[derive(Debug)]
pub struct SerializerList<'a, T: DrfSerialize>(pub &'a [T]);
impl<T: DrfSerialize> DrfSerialize for SerializerList<'_, T> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        // `many=True` passes the custom `fields=`/`expand=` kwargs down to the
        // child serializer (`many_init` calls `cls(*args, **kwargs)`), so every
        // item renders with the request's context, not a full one.
        w.write_byte(b'[');
        for (index, item) in self.0.iter().enumerate() {
            if index != 0 {
                w.write_byte(b',');
            }
            item.drf_write(w, ctx)?;
        }
        w.write_byte(b']');
        Ok(())
    }
}

/// Render one serializer into `out`.
///
/// # Errors
///
/// See [`DrfSerialize::drf_write`].
pub fn render_into<T: DrfSerialize + ?Sized>(
    value: &T,
    ctx: &RenderCtx<'_>,
    out: &mut DrfWriter,
) -> Result<(), DrfError> {
    value.drf_write(out, ctx)
}

/// Render one serializer and return the body bytes.
///
/// The bytes are exactly `JSONRenderer().render(serializer.data)`; a handler
/// wraps them in a `ViewResponse::Raw` with `plane_core::error::APPLICATION_JSON`.
///
/// # Errors
///
/// See [`DrfSerialize::drf_write`].
pub fn render<T: DrfSerialize + ?Sized>(
    value: &T,
    ctx: &RenderCtx<'_>,
) -> Result<Vec<u8>, DrfError> {
    let mut writer = DrfWriter::new();
    value.drf_write(&mut writer, ctx)?;
    Ok(writer.into_vec())
}

/// Render `many=True` and return the body bytes.
///
/// # Errors
///
/// See [`DrfSerialize::drf_write`].
pub fn render_list<T: DrfSerialize>(items: &[T], ctx: &RenderCtx<'_>) -> Result<Vec<u8>, DrfError> {
    render(&SerializerList(items), ctx)
}
