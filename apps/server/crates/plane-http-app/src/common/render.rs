// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `fields=`, `expand=` and the active timezone, in one place.
//!
//! Every DRF serializer renders under three pieces of request state:
//!
//! * `view.fields` — `?fields=a,b` (`plane/app/serializers/base.py:104-140`);
//! * `view.expand` — `?expand=a,b` (`base.py:145-190`);
//! * `django.utils.timezone.get_current_timezone()`, which `TimezoneMixin.initial`
//!   sets from `request.user.user_timezone` and which `DateTimeField` applies.
//!
//! `plane-drf` parses the two query parameters (`ViewCtx::fields`,
//! `ViewCtx::expand`) and activates the timezone (`ViewCtx::timezone`), and
//! `plane-serializers` renders under them. What is left — turning the first two
//! into `FieldEntry`s and defaulting the third to UTC for a route without
//! `TimezoneMixin` — is the same five lines in every handler, so it lives here.
//!
//! ```ignore
//! use plane_http_app::common::{render, RequestRender};
//!
//! let body = render(ctx, &ProjectLiteSerializer { id, name })?;   // fields=/expand=
//! let prepared = RequestRender::from_ctx(ctx);                     // or build it once
//! for project in projects {
//!     body.extend(render(&serializer, &prepared.render_ctx())?);   // ...
//! }
//! ```
//!
//! A route with no `TimezoneMixin` (`routes.json` `timezone_mixin: false`) still
//! has a `None` timezone after the runtime's `deactivate()`, so this resolves to
//! `settings.TIME_ZONE`, which is `UTC` (`ENV.md` §2.9). Never default a
//! datetime to local time by hand.

use chrono_tz::Tz;
use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;
use plane_serializers::{
    DrfSerialize, FieldEntry, RenderCtx, render as render_serializer,
    render_list as render_serializer_list,
};

/// The request state a serializer renders under, owned so several renders can
/// share it.
#[derive(Debug)]
pub struct RequestRender<'a> {
    timezone: Tz,
    fields: Option<Vec<FieldEntry<'a>>>,
    expand: Vec<&'a str>,
}

impl<'a> RequestRender<'a> {
    /// Read `fields=`, `expand=` and the active timezone off the request.
    pub fn from_ctx(ctx: &'a ViewCtx<'a>) -> Self {
        Self {
            // `TimezoneMixin` deactivates for a route without it, leaving
            // `settings.TIME_ZONE`.
            timezone: ctx.timezone.copied().unwrap_or(chrono_tz::UTC),
            fields: ctx
                .fields()
                .map(|names| names.into_iter().map(FieldEntry::name).collect()),
            expand: ctx.expand().unwrap_or_default(),
        }
    }

    /// The context for one `drf_write`.
    ///
    /// `fields` is `None` when the parameter was absent — which is not the same
    /// as an empty list, and DRF renders nothing for the empty list.
    pub fn render_ctx(&self) -> RenderCtx<'_> {
        RenderCtx::new(&self.timezone, self.fields.as_deref(), &self.expand)
    }

    /// The timezone `DateTimeField` writes in.
    pub fn timezone(&self) -> Tz {
        self.timezone
    }
}

/// Render one serializer with the request's `fields=`, `expand=` and timezone.
///
/// This is the one-liner form; reach for [`RequestRender`] when rendering many
/// objects in a loop, so the query string is parsed once.
///
/// # Errors
///
/// A field that cannot be rendered, or an `expand=` name the handler did not
/// resolve. Django raises `AttributeError` for the second, so both are a 500.
pub fn render<'a, T: DrfSerialize + ?Sized>(
    ctx: &'a ViewCtx<'a>,
    value: &T,
) -> Result<Vec<u8>, ViewError> {
    let prepared = RequestRender::from_ctx(ctx);
    render_serializer(value, &prepared.render_ctx()).map_err(ViewError::from)
}

/// Render `many=True`: a JSON array, one entry per item.
///
/// # Errors
///
/// See [`render`].
pub fn render_list<T: DrfSerialize>(
    items: &[T],
    ctx: &RenderCtx<'_>,
) -> Result<Vec<u8>, ViewError> {
    render_serializer_list(items, ctx).map_err(ViewError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serializers::{DrfError, DrfWriter, FilterMode};

    /// A serializer with two fields, honouring the mask the way a generated
    /// serializer does (`RenderCtx::emits`).
    #[derive(Debug)]
    struct Probe;

    impl DrfSerialize for Probe {
        fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
            let mut object = w.begin_object();
            if ctx.emits("a", FilterMode::OutputOnly) {
                object.field("a", &1)?;
            }
            if ctx.emits("b", FilterMode::OutputOnly) {
                object.field("b", &2)?;
            }
            object.end();
            Ok(())
        }
    }

    #[test]
    fn the_mask_is_applied_in_declaration_order() {
        let fields = [FieldEntry::name("b")];
        let prepared = RequestRender {
            timezone: chrono_tz::UTC,
            fields: Some(fields.to_vec()),
            expand: Vec::new(),
        };
        let body = render_serializer(&Probe, &prepared.render_ctx()).expect("render");
        assert_eq!(String::from_utf8(body).expect("utf8"), r#"{"b":2}"#);
    }

    #[test]
    fn no_fields_parameter_renders_everything() {
        let prepared = RequestRender {
            timezone: chrono_tz::UTC,
            fields: None,
            expand: Vec::new(),
        };
        let body = render_serializer(&Probe, &prepared.render_ctx()).expect("render");
        assert_eq!(String::from_utf8(body).expect("utf8"), r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn an_empty_fields_parameter_renders_nothing() {
        // `DynamicBaseSerializer(fields=[])` makes `allowed` the empty set, so
        // every field is skipped and the body is `{}`. Distinguished from the
        // `None` case above, which is why `Option` is kept.
        let prepared = RequestRender {
            timezone: chrono_tz::UTC,
            fields: Some(Vec::new()),
            expand: Vec::new(),
        };
        let body = render_serializer(&Probe, &prepared.render_ctx()).expect("render");
        assert_eq!(String::from_utf8(body).expect("utf8"), "{}");
    }

    #[test]
    fn a_render_failure_is_a_five_hundred() {
        // Django raises `AttributeError` here (`plane/app/serializers/base.py`,
        // the injected field's `get_attribute`) and Plane's family handler
        // answers 500 with its JSON body, not a 400.
        let error = DrfError::MissingExpansion("cycle".to_string());
        assert_eq!(
            ViewError::from(error),
            ViewError::Unknown("expand=\"cycle\" was requested but not resolved".to_string())
        );
    }
}
