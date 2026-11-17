// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `MarkAllReadNotificationViewSet.create`, route 121
//! (`plane/app/views/notification/base.py:237-293`).
//!
//! The view is a bulk write and nothing else: no serializer, no single-row
//! lookup, and the answer is the literal `{"message": "Successful"}` (200).
//!
//! ## Order
//!
//! 1. `request.data.get("snoozed", False)`, `.get("archived", False)` and
//!    `.get("type", "all")` (`base.py:240-242`) — the body is read **first**, so
//!    `{not json` here is the parser's 400 (`base.py:98-99`;
//!    `plans/NOTIFICATIONS.md` §4), unlike 117/122.
//! 2. The queryset: `workspace__slug`, `receiver_id=request.user`,
//!    `read_at__isnull=True`, ordered `snoozed_till ASC, created_at DESC`
//!    (`base.py:244-248`). `Notification.objects` adds `deleted_at IS NULL`
//!    (`plane/db/mixins.py:56-58`).
//! 3. `snoozed`/`archived` are tested for Python **truthiness**
//!    (`base.py:251,257`): the JSON `false` takes the `else`, the form string
//!    `"false"` (truthy) takes the `if`. An absent key is `False`.
//! 4. `type` is compared with `==` against `"watching"`, `"assigned"` and
//!    `"created"` (`base.py:263,270,277`) — no comma split, no `"subscribed"`,
//!    and no `entity_name="issue"` or `mentioned` filter (unlike `list`). For a
//!    workspace guest (`role < 15`) `type=created` replaces the queryset with
//!    `.none()` (`base.py:278-281`).
//! 5. The loop calls `timezone.now()` once per row **in scan order**
//!    (`base.py:289-290`), then `bulk_update(..., ["read_at"],
//!    batch_size=100)` (`:292`): `read_at` only, no `updated_at`/`updated_by`,
//!    inside `transaction.atomic(savepoint=False)` opened after the scan
//!    (`django/db/models/query.py:922`).
//!
//! ## Statements
//!
//! | Step | Django | Here |
//! | --- | --- | --- |
//! | guest probe (`type=created` only) | 1 `exists()` | 1, reusing `queries::WORKSPACE_GUEST_EXISTS` |
//! | scan | 1 (with the branch as a subquery) | 1 (`write_bulk_queries::scan`) |
//! | update | 1 per 100 rows | 1 per 100 rows (`NOTIFICATION_MARK_ALL_READ`) |

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json;
use plane_serde::pyjson::Value as PyJson;
use uuid::Uuid;

use super::queries;
use super::write_bulk_queries;
use crate::issues::lists::list_common::{client, kwarg_text, user_id};
use crate::views::write_support::{Body, body_get};
use crate::write;

/// Register the route.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.notification.base.MarkAllReadNotificationViewSet")?
        .action("create", create)?;
    Ok(())
}

/// Python truthiness of a `request.data.get(...)` value.
///
/// `None`, `False`, `0`, `0.0`, `""`, `[]` and `{}` are falsy; every other
/// value is truthy. A `QueryDict` value is always a `str`, so the empty form
/// value is falsy and `"false"` is truthy (`base.py:251,257`).
fn truthy(body: &Body<'_>) -> bool {
    match body {
        Body::Missing => false,
        Body::Form(text) => !text.is_empty(),
        Body::Json(PyJson::Null) => false,
        Body::Json(PyJson::Bool(value)) => *value,
        Body::Json(PyJson::Int(value)) => value.as_i64() != Some(0),
        Body::Json(PyJson::Float(value)) => *value != 0.0,
        Body::Json(PyJson::Str(value)) => value.as_str().is_some_and(|text| !text.is_empty()),
        Body::Json(PyJson::List(items)) => !items.is_empty(),
        Body::Json(PyJson::Dict(members)) => !members.is_empty(),
    }
}

/// The raw `type` value when it is the Python string `kind`.
///
/// `type == "watching"` is a Python `==`: only a `str` equal to the literal
/// matches; a number, list or dict never does.
fn type_is(body: &Body<'_>, kind: &str) -> bool {
    match body {
        Body::Missing => false,
        Body::Form(text) => *text == kind,
        Body::Json(PyJson::Str(value)) => value.as_str() == Some(kind),
        Body::Json(_) => false,
    }
}

/// `WorkspaceMember.objects.filter(workspace__slug=slug, member=request.user,
/// role__lt=15, is_active=True).exists()` (`base.py:278-280`).
async fn is_guest(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::WORKSPACE_GUEST_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&user, &slug])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `MarkAllReadNotificationViewSet.create` (`base.py:237-293`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `request.data.get(...)` (`base.py:240-242`) — before any query, so a
        // malformed body is the parser's 400 and a non-mapping body the
        // `AttributeError` 500.
        let data = ctx.data()?;
        let snoozed = truthy(&body_get(data, "snoozed")?);
        let archived = truthy(&body_get(data, "archived")?);
        let type_ = body_get(data, "type")?;

        let now = write::now();

        // `type=created` for a workspace guest is `Notification.objects.none()`
        // (`base.py:278-281`): the earlier filters are discarded with it.
        let empty = type_is(&type_, "created") && is_guest(client, slug, user).await?;
        if !empty {
            let branch = ["watching", "assigned", "created"]
                .into_iter()
                .find(|kind| type_is(&type_, kind))
                .and_then(|kind| write_bulk_queries::type_filter(kind, slug, user));

            let sql = write_bulk_queries::scan(slug, user, now, snoozed, archived, branch);
            let rows = sql.query(client).await.map_err(ViewError::from)?;

            // `for notification in notifications: notification.read_at =
            // timezone.now()` (`base.py:289-290`) — one value per id, in scan
            // order.
            let ids: Vec<Uuid> = rows
                .iter()
                .map(|row| row.try_get::<_, Uuid>(0).map_err(ViewError::from))
                .collect::<Result<_, _>>()?;
            let stamps: Vec<chrono::DateTime<chrono::Utc>> =
                ids.iter().map(|_| write::now()).collect();

            if !ids.is_empty() {
                let statement = client
                    .prepare_cached(write_bulk_queries::NOTIFICATION_MARK_ALL_READ.sql)
                    .await
                    .map_err(ViewError::from)?;
                // `transaction.atomic(savepoint=False)` opens *after* the scan
                // (`django/db/models/query.py:922`); `client.transaction()`
                // needs `&mut Client`, so BEGIN/COMMIT on the request's
                // connection is the same boundary (the precedent is
                // `views/write_view.rs:405`).
                client
                    .batch_execute("BEGIN")
                    .await
                    .map_err(ViewError::from)?;
                let mut result = Ok(());
                for (batch_ids, batch_stamps) in ids
                    .chunks(write_bulk_queries::BATCH_SIZE)
                    .zip(stamps.chunks(write_bulk_queries::BATCH_SIZE))
                {
                    let batch_ids = batch_ids.to_vec();
                    let batch_stamps = batch_stamps.to_vec();
                    if let Err(error) = client
                        .execute(&statement, &[&batch_ids, &batch_stamps])
                        .await
                        .map_err(ViewError::from)
                    {
                        result = Err(error);
                        break;
                    }
                }
                match result {
                    Ok(()) => {
                        client
                            .batch_execute("COMMIT")
                            .await
                            .map_err(ViewError::from)?;
                    }
                    Err(error) => {
                        let _ = client.batch_execute("ROLLBACK").await;
                        return Err(error);
                    }
                }
            }
        }

        let body = Json::object(vec![("message", Json::string("Successful"))]);
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body.to_drf()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson;

    fn json(text: &str) -> plane_drf::parsers::Data {
        plane_drf::parsers::Data::Json(pyjson::loads(text.as_bytes(), "utf-8").expect("json"))
    }

    #[test]
    fn json_truthiness_matches_python() {
        for (text, expected) in [
            ("false", false),
            ("true", true),
            ("null", false),
            ("0", false),
            ("0.0", false),
            ("-0.0", false),
            ("1", true),
            ("-1", true),
            ("2.5", true),
            ("\"\"", false),
            ("\"false\"", true),
            ("\"0\"", true),
            ("[]", false),
            ("[0]", true),
            ("{}", false),
            ("{\"a\": 0}", true),
        ] {
            let data = json(&format!("{{\"snoozed\": {text}}}"));
            assert_eq!(
                truthy(&body_get(&data, "snoozed").expect("body")),
                expected,
                "{text}"
            );
        }
        // An absent key is the `False` default.
        let data = json("{}");
        assert!(!truthy(&body_get(&data, "snoozed").expect("body")));
    }

    #[test]
    fn empty_form_values_are_falsy_and_strings_are_the_python_value() {
        let form =
            plane_drf::request::QueryDict::parse("snoozed=&archived=false", None).expect("form");
        let data = plane_drf::parsers::Data::Form(form);
        assert!(!truthy(&body_get(&data, "snoozed").expect("empty")));
        assert!(truthy(&body_get(&data, "archived").expect("false")));
    }

    #[test]
    fn type_is_a_string_equality() {
        let data = json("{\"type\": \"watching\"}");
        assert!(type_is(&body_get(&data, "type").expect("type"), "watching"));
        let data = json("{\"type\": \"all\"}");
        assert!(!type_is(
            &body_get(&data, "type").expect("type"),
            "watching"
        ));
        let data = json("{\"type\": 3}");
        assert!(!type_is(&body_get(&data, "type").expect("type"), "created"));
        let data = json("{\"type\": [\"created\"]}");
        assert!(!type_is(&body_get(&data, "type").expect("type"), "created"));
    }
}
