// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `POST /api/workspaces/{slug}/export-analytics/` — route 4,
//! `ExportAnalyticsEndpoint.post` (`plane/app/views/analytic/base.py:235-261`).
//!
//! The shortest write in the group: it writes nothing, looks nothing up and
//! answers 200 after publishing `analytic_export_task`. The worker rebuilds the
//! graph, renders the rows as a CSV and e-mails it
//! (`plane/bgtasks/analytic_plot_export.py:350-412`), swallowing every
//! exception, so there is no row to compare afterwards — the Celery message is
//! the whole side effect.
//!
//! ## Order (`base.py:237-261`)
//!
//! 1. `allow_permission([ADMIN, MEMBER], level="WORKSPACE")` — the runtime's
//!    policy layer.
//! 2. `request.data`: a JSON **list** body is `AttributeError` (`list.get`) →
//!    the family 500. There is **no** workspace lookup: a member of a
//!    soft-deleted workspace reaches the publish and gets a 200 (plan §4).
//! 3. The two axis checks, both `400 {"error": …}`.
//! 4. `analytic_export_task.delay(email=request.user.email, data=request.data,
//!    slug=slug, user_id=str(request.user.id))` (`:256`): `data` is the whole
//!    parsed body, so a form body reaches the worker as the `QueryDict`'s
//!    last-value-per-key dict (`write::request_data_arg`).
//! 5. `200 {"message": "Once the export is ready it will be emailed to you at
//!    <email>"}`.

use plane_core::error::ApiError;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::tasks;
use plane_jobs::queue::Job;
use plane_serde::pyjson::PyStr;
use plane_serde::pyjson::Value as Json;

use crate::analytics::base::message_body;
use crate::analytics::plot::{is_valid_field, is_valid_y_axis};
use crate::common::jobs;
use crate::users_me::support::user_email;
use crate::views::write_support::{Body, body_get};
use crate::write;

/// `ExportAnalyticsEndpoint.post` (`plane/app/views/analytic/base.py:235-261`).
pub fn export_analytics<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let email = user_email(ctx)?;

        // `request.data.get("x_axis", False)` etc. (`:238-240`).
        let data = ctx.data()?;
        let x_axis = body_value(data, "x_axis")?;
        let y_axis = body_value(data, "y_axis")?;
        let segment = body_value(data, "segment")?;

        // `if not x_axis or not y_axis or x_axis not in VALID_ANALYTICS_FIELDS
        // or y_axis not in VALID_YAXIS` (`:243`). `not x_axis` is subsumed:
        // an empty string is not a valid field either, and a non-string never
        // equals one.
        let (Some(x_axis), Some(_)) = (valid_field(&x_axis), valid_y_axis(&y_axis)) else {
            return Ok(write::error_body(
                400,
                "x-axis and y-axis dimensions are required and the values should be valid",
            ));
        };

        // `if segment and (segment not in VALID_ANALYTICS_FIELDS or x_axis ==
        // segment)` (`:250`).
        if plane_db::hooks::issue_view::truthy(&segment) {
            match valid_field(&segment) {
                Some(segment) if segment != x_axis => {}
                _ => {
                    return Ok(write::error_body(
                        400,
                        "Both segment and x axis cannot be same and segment should be valid",
                    ));
                }
            }
        }

        // `analytic_export_task.delay(...)` (`:256`), keyword order preserved.
        let message = tasks::analytic_export_task(
            email.clone(),
            write::request_data_arg(ctx)?,
            slug.to_owned(),
            user.to_string(),
        );
        jobs::dispatch_task(ctx, message.task(), &message, Job::new(message.task())).await?;

        Ok(message_body(
            200,
            &format!("Once the export is ready it will be emailed to you at {email}"),
        ))
    })
}

/// `request.data.get(key, False)`, as the Python value the view sees.
fn body_value(data: &plane_drf::parsers::Data, key: &str) -> Result<Json, ViewError> {
    Ok(match body_get(data, key)? {
        Body::Missing => Json::Bool(false),
        Body::Json(value) => value.clone(),
        Body::Form(text) => Json::Str(PyStr::new(text)),
    })
}

/// The string a Python value would compare equal to a `VALID_ANALYTICS_FIELDS`
/// member, i.e. a `str`/`QueryDict` value in that list.
fn valid_field(value: &Json) -> Option<&str> {
    match value {
        Json::Str(text) => text.as_str().filter(|text| is_valid_field(text)),
        _ => None,
    }
}

/// [`valid_field`] against `VALID_YAXIS`.
fn valid_y_axis(value: &Json) -> Option<&str> {
    match value {
        Json::Str(text) => text.as_str().filter(|text| is_valid_y_axis(text)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Json {
        plane_serde::pyjson::loads(raw.as_bytes(), "utf-8").expect("json")
    }

    #[test]
    fn only_a_string_in_the_list_is_a_valid_axis() {
        for raw in [r#""state_id""#, r#""priority""#, r#""created_at""#] {
            assert!(valid_field(&parse(raw)).is_some(), "{raw}");
        }
        for raw in [
            "false",
            "null",
            "5",
            "[]",
            r#"["priority"]"#,
            r#""""#,
            r#""state""#,
        ] {
            assert!(valid_field(&parse(raw)).is_none(), "{raw}");
        }
        assert!(valid_y_axis(&parse(r#""issue_count""#)).is_some());
        assert!(valid_y_axis(&parse(r#""estimate""#)).is_some());
        assert!(valid_y_axis(&parse(r#""priority""#)).is_none());
    }
}
