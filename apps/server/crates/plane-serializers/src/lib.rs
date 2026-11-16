// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-serializers`: DRF serializer output, byte for byte.
//!
//! Every GET handler in Phase 3 renders objects that Django REST Framework
//! serialises today. Getting the bytes right means reproducing, together:
//!
//! * the field order DRF's `Serializer.to_representation` walks
//!   (`rest_framework/serializers.py:530-551`): `_readable_fields` order, which
//!   is the declaration order of `Serializer.fields`, with `write_only` fields
//!   skipped;
//! * each field's `to_representation`, including the surrounding null rule
//!   (`to_representation` writes `null` without calling the field when the
//!   attribute is `None`) and the fact that an unknown attribute is a 500, not
//!   an omitted key;
//! * the active timezone, which `TimezoneMixin.initial`
//!   (`plane/app/views/base.py:34-45`) sets from `request.user.user_timezone`
//!   and `DateTimeField.to_representation` applies through
//!   `enforce_timezone` (`rest_framework/fields.py:1180-1190,1233-1244`);
//! * `DynamicBaseSerializer`'s `fields=`/`expand=` rewriting
//!   (`plane/app/serializers/base.py:91-190`);
//! * `JSONRenderer`'s dialect: compact separators, `ensure_ascii=False`,
//!   U+2028/U+2029 escaped (see [`plane_serde::Dialect::Drf`]).
//!
//! ## Rendering model
//!
//! [`DrfWriter`] writes straight into a `Vec<u8>`. Scalars go through
//! [`plane_serde::to_writer`] so Python's float and string rules are applied by
//! the crate that already owns them; `json`/`jsonb` text goes through
//! [`plane_serde::transcode_json`], which keeps Postgres' key order and Python's
//! number formatting. That last point is why the renderer cannot go through
//! `serde::Serialize` for whole objects: a pre-encoded JSON fragment has to be
//! written into the output, and `serde`'s data model has no raw-value variant
//! (plane-serde's `PySerializer` implements no such extension either). A
//! serializer struct therefore implements [`DrfSerialize`], not
//! `serde::Serialize`: the trait has the same shape, but it also carries the
//! active timezone and the field mask.
//!
//! `json`/`jsonb` columns arrive as [`plane_db::JsonText`]; wrap one with
//! [`fields::PgJson::from`] (or `PgJson::from_json_text`) and the generated field
//! renderer writes it through `transcode_json`. Decimal columns arrive as
//! [`plane_db::Numeric`]; [`fields::drf_decimal_numeric`] quantises one with the
//! serializer's `max_digits`/`decimal_places`.
//!
//! ## Usage
//!
//! ```ignore
//! use plane_drf::view::ViewCtx;
//! use plane_serializers::generated::app_user::UserLiteSerializer;
//! use plane_serializers::{FieldEntry, RenderCtx, render};
//!
//! // `ctx.timezone` is `ViewCtx::timezone`; `None` means DRF's `deactivate()`,
//! // i.e. settings.TIME_ZONE = "UTC".
//! let tz = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
//! // `?fields=` / `?expand=` from the query string, in request order.
//! let fields: Option<Vec<FieldEntry>> = ctx.fields().map(|names| {
//!     names.into_iter().map(FieldEntry::name).collect()
//! });
//! let expand = ctx.expand().unwrap_or_default();
//! let render_ctx = RenderCtx::new(&tz, fields.as_deref(), &expand);
//!
//! let model = UserLiteSerializer { id, first_name, .. };
//! let body = render(&model, &render_ctx)?;
//! ```
//!
//! ## What is not here
//!
//! Writing (`to_internal_value`, `create`, `update`, validation) is Phase 5.
//! The inventory in `apps/server/serializers.json` records every validator and
//! hook so none is lost; `apps/server/SERIALIZERS.md` lists the classes whose
//! `to_representation` is hand-written (`manual`) rather than generated.

pub mod expand;
pub mod fields;
pub mod generated;
pub mod mask;
pub mod ser;
pub mod value;
pub mod writer;

pub use expand::{
    Expansion, Expansions, MANY_EXPANSION_FIELDS, expansion_serializer, is_expansion_name,
};
pub use fields::{
    DrfBoolList, DrfDateTime, DrfDecimal, DrfFloatList, DrfPk, DrfScalar, DrfStrList, DrfUuid,
    DrfUuidList, EncoderDecimal, FieldValue, PgJson, PyBase64, PyDate, PyTime, drf_datetime,
    drf_datetime_opt,
};
pub use mask::{FieldEntry, FieldMask};
pub use plane_serde::PyDecimal;
pub use ser::{DrfSerialize, FilterMode, RenderCtx, render, render_into, render_list};
pub use value::DrfValue;
pub use writer::{DrfWriter, ObjectWriter};

/// The error type every render path returns.
#[derive(Debug, thiserror::Error)]
pub enum DrfError {
    /// The JSON formatter rejected a value (a non-finite float under
    /// `allow_nan=False`, a decimal that cannot be quantised, …).
    #[error(transparent)]
    Json(#[from] plane_serde::Error),
    /// `expand=` asked for a relation the handler did not resolve. Django
    /// raises `AttributeError` here (a 500) because the injected field's
    /// `get_attribute` requires the attribute; Rust refuses at the same point
    /// instead of silently emitting the unexpanded value.
    #[error("expand={0:?} was requested but not resolved")]
    MissingExpansion(String),
}
