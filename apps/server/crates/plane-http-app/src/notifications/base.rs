// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.notification.base.NotificationViewSet.list`
//! (`plane/app/views/notification/base.py:48-154`).
//!
//! ## What the view filters
//!
//! ```text
//! Notification.objects.filter(workspace__slug, receiver_id=user)
//!   .filter(entity_name="issue")
//!   .annotate(is_inbox_issue=Exists(intake issue), is_intake_issue=Exists(same),
//!             is_mentioned_notification=Case(When(sender__icontains="mentioned")))
//!   .select_related("workspace", "project", "triggered_by", "receiver")
//!   .order_by("snoozed_till", "-created_at")
//! snoozed_filters[snoozed]        # "true"/"false" only; anything else is a KeyError → 400
//! archived_filters[archived]      # likewise (handle_exception's KeyError branch)
//! read == "false" | "true"        # anything else adds no filter
//! mentioned ? filter : exclude    # any non-empty string is truthy
//! type.split(",")                 # "subscribed", "assigned", "created"; OR-ed
//! if per_page and cursor: paginate(order_by=sanitize_order_by(?order_by, …))
//! ```
//!
//! Four quirks are behaviour and are ported:
//!
//! * `?snoozed=false` keeps rows whose `snoozed_till` is NULL **or in the
//!   future**, and `?snoozed=true` keeps every row with a non-NULL
//!   `snoozed_till`, so a future snooze appears in both lists;
//! * `assigned=Exists(IssueAssignee.objects.filter(pk=OuterRef("issue_id"), …))`
//!   compares the through row's **id** with an issue id, so the `subscribed`
//!   branch's "not assigned" test almost never matches;
//! * `type=created` for a workspace guest (`role < 15`) yields `.none()`;
//! * the empty `Q()` that remains when `type` names none of the three values is
//!   a no-op filter, so `?type=issue` returns everything (the scenario's
//!   `type=issue` step relies on it).
//!
//! ## Statements
//!
//! One statement for the page: the model columns, the three annotations and
//! the `triggered_by` fields of `UserLiteSerializer`, with the `type` branches
//! inlined as `IN (SELECT …)` subqueries (Django runs each `values_list()`
//! query separately). The avatar `FileAsset` is resolved in one more query for
//! the whole page instead of Django's one per notification.

use chrono::{DateTime, Utc};
use plane_core::error::APPLICATION_JSON;
use plane_db::models::notification::Notification;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::paginator::{MAX_LIMIT, PageQuery};
use plane_query::sql::{Frag, Join, Select, Value};
use plane_serializers::FieldValue;
use plane_serializers::generated::app_notification::NotificationSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use uuid::Uuid;

use super::render::guard_only;
use crate::intake::asset;
use crate::issues::lists::list_common::{client, kwarg_text, map_query_error, user_id};

/// Register `list` and the guard-refused `retrieve`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.notification.base.NotificationViewSet")?
        .action("list", list)?
        // `GET …/notifications/{pk}/`: `retrieve` is served by DRF's mixin under
        // the bare `IsAuthenticated`, so `BaseViewSet`'s guard answers 405.
        .action("retrieve", guard_only)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The statement
// ---------------------------------------------------------------------------

/// `Notification`'s model columns (`plane-db`'s generated select list).
const NOTIFICATION_COLUMNS: &str = "\
 n.\"created_at\", n.\"updated_at\", n.\"created_by_id\", n.\"updated_by_id\", \
 n.\"deleted_at\", n.\"id\", n.\"workspace_id\", n.\"project_id\", n.\"data\", \
 n.\"entity_identifier\", n.\"entity_name\", n.\"title\", n.\"message\", \
 n.\"message_html\", n.\"message_stripped\", n.\"sender\", n.\"triggered_by_id\", \
 n.\"receiver_id\", n.\"read_at\", n.\"snoozed_till\", n.\"archived_at\"";

/// `UserLiteSerializer`'s model columns from the `LEFT OUTER JOIN`.
const TRIGGERED_BY_COLUMNS: &str = "\
 tb.\"id\" AS \"tb_id\", tb.\"first_name\" AS \"tb_first_name\", \
 tb.\"last_name\" AS \"tb_last_name\", tb.\"avatar\" AS \"tb_avatar\", \
 tb.\"avatar_asset_id\" AS \"tb_avatar_asset_id\", tb.\"is_bot\" AS \"tb_is_bot\", \
 tb.\"display_name\" AS \"tb_display_name\", \
 fa.\"id\" AS \"tb_avatar_asset_row_id\", \
 fa.\"entity_type\" AS \"tb_avatar_entity_type\", \
 fa.\"project_id\" AS \"tb_avatar_project_id\", \
 fa.\"issue_id\" AS \"tb_avatar_issue_id\", \
 w2.\"slug\" AS \"tb_avatar_workspace_slug\"";

/// `snoozed_filters[snoozed]` (`base.py:81-86`).
///
/// `true`: `snoozed_till < now OR snoozed_till IS NOT NULL`;
/// `false`: `snoozed_till >= now OR snoozed_till IS NULL`.
fn snoozed_filter(value: &str, now: DateTime<Utc>) -> Result<Frag, ViewError> {
    match value {
        "true" => Ok(
            Frag::compare("n.\"snoozed_till\"", "<", Value::Timestamp(now))
                .or(Frag::is_not_null("n.\"snoozed_till\"")),
        ),
        "false" => Ok(
            Frag::compare("n.\"snoozed_till\"", ">=", Value::Timestamp(now))
                .or(Frag::is_null("n.\"snoozed_till\"")),
        ),
        _other => Err(ViewError::Key),
    }
}

/// `archived_filters[archived]` (`base.py:88-93`).
fn archived_filter(value: &str) -> Result<Frag, ViewError> {
    match value {
        "true" => Ok(Frag::is_not_null("n.\"archived_at\"")),
        "false" => Ok(Frag::is_null("n.\"archived_at\"")),
        // `handle_exception`'s `KeyError` branch: 400 `{"error": "The required
        // key does not exist."}` (`plane/app/views/base.py:210-215`).
        _other => Err(ViewError::Key),
    }
}

/// `sender__icontains="mentioned"` (`base.py:70-75,101-104`).
fn mentioned_predicate() -> Frag {
    Frag::raw("UPPER(n.\"sender\"::text) LIKE UPPER('%mentioned%')")
}

/// One `type=` branch, as the ORM's `entity_identifier IN (SELECT …)`
/// (`base.py:106-136`).
fn type_branch(kind: &str, slug: &str, user: Uuid) -> Option<Frag> {
    let mut frag = Frag::raw("n.\"entity_identifier\" IN (");
    match kind {
        "subscribed" => {
            frag.push_raw(
                "SELECT V0.\"issue_id\" FROM \"issue_subscribers\" V0 \
                 INNER JOIN \"workspaces\" V2 ON (V0.\"workspace_id\" = V2.\"id\") \
                 WHERE V0.\"deleted_at\" IS NULL AND V0.\"subscriber_id\" = ",
            );
            frag.push(Frag::bind(Value::Uuid(user)));
            frag.push_raw(" AND V2.\"slug\" = ");
            frag.push(Frag::bind(Value::Text(slug.to_owned())));
            // `assigned=Exists(IssueAssignee.objects.filter(pk=OuterRef("issue_id"),
            // assignee=user))` — the `pk` is the through row's id, the bug the
            // module docs name.
            frag.push_raw(
                " AND NOT EXISTS(SELECT 1 FROM \"issue_assignees\" U0 \
                 WHERE U0.\"deleted_at\" IS NULL AND U0.\"assignee_id\" = ",
            );
            frag.push(Frag::bind(Value::Uuid(user)));
            frag.push_raw(" AND U0.\"id\" = V0.\"issue_id\" LIMIT 1)");
            frag.push_raw(
                " AND NOT EXISTS(SELECT 1 FROM \"issues\" U0 \
                 WHERE U0.\"deleted_at\" IS NULL AND U0.\"created_by_id\" = ",
            );
            frag.push(Frag::bind(Value::Uuid(user)));
            frag.push_raw(" AND U0.\"id\" = V0.\"issue_id\" LIMIT 1)");
        }
        "assigned" => {
            frag.push_raw(
                "SELECT U0.\"issue_id\" FROM \"issue_assignees\" U0 \
                 INNER JOIN \"workspaces\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
                 WHERE U0.\"deleted_at\" IS NULL AND U0.\"assignee_id\" = ",
            );
            frag.push(Frag::bind(Value::Uuid(user)));
            frag.push_raw(" AND U2.\"slug\" = ");
            frag.push(Frag::bind(Value::Text(slug.to_owned())));
        }
        "created" => {
            // The guest branch is handled before the statement is built; this
            // is the member/admin form (`base.py:132-135`).
            frag.push_raw(
                "SELECT U0.\"id\" FROM \"issues\" U0 \
                 INNER JOIN \"workspaces\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
                 WHERE U0.\"deleted_at\" IS NULL AND U0.\"created_by_id\" = ",
            );
            frag.push(Frag::bind(Value::Uuid(user)));
            frag.push_raw(" AND U2.\"slug\" = ");
            frag.push(Frag::bind(Value::Text(slug.to_owned())));
        }
        _ => return None,
    }
    frag.push_raw(")");
    Some(frag)
}

/// `sanitize_order_by(value, NOTIFICATION_ORDER_BY_ALLOWLIST, "-created_at")`
/// (`plane/utils/order_queryset.py:135-156`) followed by `OffsetPaginator`'s
/// key: the leading `-` selects `F(field).desc(nulls_last=True)`, otherwise
/// `asc(nulls_last=True)`, and `"-created_at"` is appended either way
/// (`plane/utils/paginator.py:113-135`).
fn paginator_order(value: &str) -> (&'static str, bool) {
    let (bare, descending) = match value.strip_prefix('-') {
        Some(rest) if !rest.is_empty() && !rest.starts_with('-') => (rest, true),
        Some(_) => return ("created_at", true),
        None => (value, false),
    };
    match bare {
        "created_at" => ("created_at", descending),
        "updated_at" => ("updated_at", descending),
        _ => ("created_at", true),
    }
}

/// The joins, predicate, projection and ordering of one list request.
struct Statement {
    joins: Vec<Join>,
    predicate: Frag,
    projection: Vec<Frag>,
    order: Frag,
}

fn build_statement(
    slug: &str,
    user: Uuid,
    query: &plane_drf::QueryDict,
    snoozed: Frag,
    archived: Frag,
    paginator: Option<(&'static str, bool)>,
) -> Statement {
    let read = query.get("read");
    // `request.GET.get("mentioned", False)`: only absent/empty is false.
    let mentioned = query
        .get("mentioned")
        .is_some_and(|value| !value.is_empty());
    let types: Vec<&str> = query.get_or("type", "all").split(',').collect();

    let mut predicate = Frag::raw("n.\"deleted_at\" IS NULL")
        .and(Frag::compare("n.\"receiver_id\"", "=", Value::Uuid(user)))
        .and(Frag::raw("w.\"slug\" = ").append(Frag::bind(Value::Text(slug.to_owned()))))
        .and(Frag::compare(
            "n.\"entity_name\"",
            "=",
            Value::Text("issue".to_owned()),
        ))
        .and(snoozed)
        .and(archived);
    match read {
        Some("false") => predicate = predicate.and(Frag::is_null("n.\"read_at\"")),
        Some("true") => predicate = predicate.and(Frag::is_not_null("n.\"read_at\"")),
        _ => {}
    }
    predicate = predicate.and(if mentioned {
        mentioned_predicate()
    } else {
        mentioned_predicate().negate()
    });

    // `notifications.filter(q_filters)`; each recognised type ORs an `IN`
    // subquery, and a type that names none of the three leaves the empty `Q()`
    // as a no-op.
    let mut branches = Frag::empty();
    for kind in &types {
        if let Some(branch) = type_branch(kind, slug, user) {
            branches = branches.or(branch);
        }
    }
    predicate = predicate.and(branches);

    let mut annotations = Frag::raw(ANNOTATIONS_PREFIX);
    annotations.push(Frag::bind(Value::Text(slug.to_owned())));
    annotations.push_raw(ANNOTATIONS_MIDDLE);
    annotations.push(Frag::bind(Value::Text(slug.to_owned())));
    annotations.push_raw(ANNOTATIONS_SUFFIX);

    let order = match paginator {
        Some((column, true)) => Frag::raw(format!(
            "n.\"{column}\" DESC NULLS LAST, n.\"created_at\" DESC"
        )),
        Some((column, false)) => Frag::raw(format!(
            "n.\"{column}\" ASC NULLS LAST, n.\"created_at\" DESC"
        )),
        None => Frag::raw("n.\"snoozed_till\" ASC, n.\"created_at\" DESC"),
    };

    Statement {
        joins: vec![
            Join::inner(
                "workspaces",
                "w",
                Frag::raw("(n.\"workspace_id\" = w.\"id\")"),
            ),
            Join::left(
                "users",
                "tb",
                Frag::raw("(n.\"triggered_by_id\" = tb.\"id\")"),
            ),
            // `User.avatar_url` reads `avatar_asset.asset_url` through the FK
            // base manager (no `deleted_at` filter), and the issue-asset
            // branches need the asset's workspace slug. Joined here instead of
            // Django's one query per notification.
            Join::left(
                "file_assets",
                "fa",
                Frag::raw("(tb.\"avatar_asset_id\" = fa.\"id\")"),
            ),
            Join::left(
                "workspaces",
                "w2",
                Frag::raw("(fa.\"workspace_id\" = w2.\"id\")"),
            ),
        ],
        predicate,
        projection: vec![
            Frag::raw(NOTIFICATION_COLUMNS),
            annotations,
            Frag::raw(TRIGGERED_BY_COLUMNS),
        ],
        order,
    }
}

/// The `EXISTS` annotation, split around its two slug binds
/// (`plane/app/views/notification/base.py:58-68`).
const ANNOTATIONS_PREFIX: &str = "\
 EXISTS(SELECT 1 FROM \"issues\" U0 \
   INNER JOIN \"intake_issues\" U1 ON (U0.\"id\" = U1.\"issue_id\") \
   INNER JOIN \"workspaces\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
   WHERE U0.\"deleted_at\" IS NULL AND U1.\"status\" IN (0, 2, -2) \
     AND U0.\"id\" = n.\"entity_identifier\" AND U2.\"slug\" = ";

const ANNOTATIONS_MIDDLE: &str = ") AS \"is_inbox_issue\", \
 EXISTS(SELECT 1 FROM \"issues\" U0 \
   INNER JOIN \"intake_issues\" U1 ON (U0.\"id\" = U1.\"issue_id\") \
   INNER JOIN \"workspaces\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
   WHERE U0.\"deleted_at\" IS NULL AND U1.\"status\" IN (0, 2, -2) \
     AND U0.\"id\" = n.\"entity_identifier\" AND U2.\"slug\" = ";

const ANNOTATIONS_SUFFIX: &str = ") AS \"is_intake_issue\", \
 CASE WHEN UPPER(n.\"sender\"::text) LIKE UPPER('%mentioned%') THEN true ELSE false END \
   AS \"is_mentioned_notification\"";

// ---------------------------------------------------------------------------
// The handler
// ---------------------------------------------------------------------------

/// `NotificationViewSet.list`.
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;
        let query = ctx.query();

        // The two dictionary lookups run before the paginator in Python
        // (`base.py:86-93` vs `:141-151`), so an invalid `snoozed`/`archived`
        // wins over an invalid cursor.
        let now = Utc::now();
        let snoozed = snoozed_filter(query.get_or("snoozed", "false"), now)?;
        let archived = archived_filter(query.get_or("archived", "false"))?;

        // `if request.GET.get("per_page", False) and request.GET.get("cursor", False)`
        // — two non-empty strings, not two present keys.
        let paginated = query.get("per_page").is_some_and(|value| !value.is_empty())
            && query.get("cursor").is_some_and(|value| !value.is_empty());
        let page = if paginated {
            match PageQuery::parse_with(query, MAX_LIMIT, MAX_LIMIT, None, None) {
                Ok(page) => Some(page),
                Err(error) => return map_query_error(error),
            }
        } else {
            None
        };

        // `type=created` for a workspace guest is `.none()` (`base.py:126-130`).
        let types: Vec<&str> = query.get_or("type", "all").split(',').collect();
        let mut empty = false;
        if types.contains(&"created") {
            let statement = client
                .prepare_cached(super::queries::WORKSPACE_GUEST_EXISTS.sql)
                .await
                .map_err(ViewError::from)?;
            empty = client
                .query_opt(&statement, &[&user, &slug])
                .await
                .map_err(ViewError::from)?
                .is_some();
        }

        if empty {
            return Ok(match page {
                Some(page) => empty_page(page),
                None => ViewResponse::raw(200, APPLICATION_JSON, b"[]".to_vec()),
            });
        }

        let order = page
            .as_ref()
            .map(|_| paginator_order(query.get_or("order_by", "-created_at")));
        let statement = build_statement(slug, user, query, snoozed, archived, order);

        let limit = page
            .as_ref()
            .map(|page| i64::from(page.per_page.min(MAX_LIMIT)));
        let offset = page
            .as_ref()
            .map(|page| page.cursor.offset * i64::from(page.per_page.min(MAX_LIMIT)));

        let mut select = Select::new("notifications n");
        select.joins = statement.joins;
        select.predicate = Some(statement.predicate);
        select.projection = statement.projection;
        select.order_by = vec![statement.order];
        if let (Some(limit), Some(offset)) = (limit, offset) {
            // `queryset[offset:offset+limit+1]` then `[:limit]`.
            select.limit = Some(limit + 1);
            select.offset = Some(offset);
        }

        let total = if page.is_some() {
            {
                let mut count = Select::new("notifications n");
                count.joins = select.joins.clone();
                count.predicate = select.predicate.clone();
                count.distinct = true;
                count.projection = vec![Frag::raw("n.\"id\"")];
                let sql = count.count_sql(Some("n.\"id\""), None);
                let prepared = client.prepare(&sql.text).await.map_err(ViewError::from)?;
                let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
                    .params
                    .iter()
                    .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
                    .collect();
                let row = client
                    .query_one(&prepared, &params)
                    .await
                    .map_err(ViewError::from)?;
                Some(row.try_get::<_, i64>(0).map_err(ViewError::from)?)
            }
        } else {
            None
        };

        let sql = select.into_sql();
        let prepared = client.prepare(&sql.text).await.map_err(ViewError::from)?;
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
            .params
            .iter()
            .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
            .collect();
        let rows = client
            .query(&prepared, &params)
            .await
            .map_err(ViewError::from)?;

        let (rows, has_results) = match limit {
            Some(limit) => {
                let has_results = rows.len() as i64 > limit;
                (
                    rows.into_iter().take(limit as usize).collect::<Vec<_>>(),
                    has_results,
                )
            }
            None => (rows, false),
        };

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let rendered = render_rows(&rows, &timezone)?;

        match (page, total) {
            (Some(page), Some(total)) => {
                let limit = i64::from(page.per_page.min(MAX_LIMIT));
                Ok(paginated_page(page, limit, total, has_results, rendered))
            }
            _ => {
                let mut body = Vec::with_capacity(2 + rendered.iter().map(Vec::len).sum::<usize>());
                body.push(b'[');
                for (index, item) in rendered.iter().enumerate() {
                    if index > 0 {
                        body.push(b',');
                    }
                    body.extend_from_slice(item);
                }
                body.push(b']');
                Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
            }
        }
    })
}

/// `render_rows`: decode the notification and `triggered_by` columns, resolve
/// the avatar assets for the whole page, then render `NotificationSerializer`.
fn render_rows(
    rows: &[tokio_postgres::Row],
    timezone: &chrono_tz::Tz,
) -> Result<Vec<Vec<u8>>, ViewError> {
    let context = plane_serializers::RenderCtx::new(timezone, None, &[]);

    let mut rendered = Vec::with_capacity(rows.len());
    for row in rows {
        let notification = Notification::from_row(row, 0)?;
        let triggered_by = decode_triggered_by(row)?;
        let triggered_by_serializer = triggered_by.as_ref().map(|user| user.lite());
        let serializer = NotificationSerializer {
            id: notification.id,
            triggered_by_details: triggered_by_serializer.as_ref(),
            // All three annotations exist: the two `EXISTS`s and the `CASE`.
            is_inbox_issue: FieldValue::value(row.get(21)),
            is_intake_issue: FieldValue::value(row.get(22)),
            is_mentioned_notification: FieldValue::value(row.get(23)),
            created_at: notification.created_at,
            updated_at: notification.updated_at,
            deleted_at: notification.deleted_at,
            data: notification
                .data
                .as_ref()
                .map(plane_serializers::PgJson::from),
            entity_identifier: notification.entity_identifier,
            entity_name: &notification.entity_name,
            title: &notification.title,
            message: notification
                .message
                .as_ref()
                .map(plane_serializers::PgJson::from),
            message_html: &notification.message_html,
            message_stripped: notification.message_stripped.as_deref(),
            sender: &notification.sender,
            read_at: notification.read_at,
            snoozed_till: notification.snoozed_till,
            archived_at: notification.archived_at,
            created_by: notification.created_by_id,
            updated_by: notification.updated_by_id,
            workspace: notification.workspace_id,
            project: notification.project_id,
            triggered_by: notification.triggered_by_id,
            receiver: notification.receiver_id,
        };
        rendered.push(plane_serializers::render(&serializer, &context)?);
    }
    Ok(rendered)
}

/// The owned form of `UserLiteSerializer`'s input.
struct TriggeredBy {
    id: Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    avatar_url: Option<String>,
    is_bot: bool,
    display_name: String,
}

impl TriggeredBy {
    fn lite(&self) -> UserLiteSerializer<'_> {
        UserLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
        }
    }
}

fn decode_triggered_by(row: &tokio_postgres::Row) -> Result<Option<TriggeredBy>, ViewError> {
    let Some(id) = row.get::<_, Option<Uuid>>(24) else {
        // `triggered_by_details` is a nested serializer whose source is null:
        // DRF writes `null` (`rest_framework/serializers.py:499-521`).
        return Ok(None);
    };
    let avatar: String = row.get(27);
    let avatar_asset_id: Option<Uuid> = row.get(28);
    let avatar_url = match avatar_asset_id {
        Some(asset_id) => {
            let row_id: Option<Uuid> = row.get(31);
            if row_id.is_none() {
                // The FK points at a row the base manager cannot see: Django
                // raises `RelatedObjectDoesNotExist` (a 500).
                return Err(ViewError::Unknown(format!(
                    "the file asset {asset_id} referenced by this row does not exist"
                )));
            }
            asset::asset_url(
                asset_id,
                row.get(32),
                row.get(33),
                row.get(34),
                row.get::<_, Option<String>>(35).as_deref(),
            )
        }
        None if !avatar.is_empty() => Some(avatar.clone()),
        None => None,
    };
    Ok(Some(TriggeredBy {
        id,
        first_name: row.get(25),
        last_name: row.get(26),
        avatar,
        avatar_url,
        is_bot: row.get(29),
        display_name: row.get(30),
    }))
}

/// The paginated envelope for an empty (`none()`) queryset.
fn empty_page(page: PageQuery) -> ViewResponse {
    let limit = i64::from(page.per_page.min(MAX_LIMIT));
    body_with_results(limit, page.cursor.offset, 0, false, Vec::new())
}

/// [`body_with_results`] for a real page.
fn paginated_page(
    page: PageQuery,
    limit: i64,
    total: i64,
    has_results: bool,
    results: Vec<Vec<u8>>,
) -> ViewResponse {
    body_with_results(limit, page.cursor.offset, total, has_results, results)
}

/// `BasePaginator`'s envelope (`plane/utils/paginator.py:740-755`), key order
/// included.
fn body_with_results(
    limit: i64,
    page_index: i64,
    total: i64,
    has_results: bool,
    results: Vec<Vec<u8>>,
) -> ViewResponse {
    let total_pages = if total == 0 {
        0
    } else {
        (total + limit - 1) / limit
    };
    let mut body = Vec::with_capacity(128 + results.iter().map(Vec::len).sum::<usize>());
    body.extend_from_slice(b"{\"grouped_by\":null,\"sub_grouped_by\":null,");
    body.extend_from_slice(format!("\"total_count\":{total},").as_bytes());
    body.extend_from_slice(format!("\"next_cursor\":\"{limit}:{}:0\",", page_index + 1).as_bytes());
    body.extend_from_slice(format!("\"prev_cursor\":\"{limit}:{}:1\",", page_index - 1).as_bytes());
    body.extend_from_slice(
        format!(
            "\"next_page_results\":{has_results},\"prev_page_results\":{},",
            page_index > 0
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "\"count\":{},\"total_pages\":{total_pages},\"total_results\":{total},\
             \"extra_stats\":null,\"results\":[",
            results.len()
        )
        .as_bytes(),
    );
    for (index, item) in results.iter().enumerate() {
        if index > 0 {
            body.push(b',');
        }
        body.extend_from_slice(item);
    }
    body.extend_from_slice(b"]}");
    ViewResponse::raw(200, APPLICATION_JSON, body)
}
