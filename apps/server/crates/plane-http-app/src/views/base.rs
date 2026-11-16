// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.view.base` (`plane/app/views/view/base.py`), the saved
//! workspace and project view reads.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/workspaces/{slug}/views/` | `WorkspaceViewViewSet.list` | `base.py:84-91` |
//! | `GET /api/workspaces/{slug}/views/{pk}/` | `WorkspaceViewViewSet.retrieve` | `base.py:115-147` |
//! | `GET /api/workspaces/{slug}/projects/{project_id}/views/` | `IssueViewViewSet.list` | `base.py:331-348` |
//! | `GET /api/workspaces/{slug}/projects/{project_id}/views/{pk}/` | `IssueViewViewSet.retrieve` | `base.py:350-383` |
//! | `GET /api/workspaces/{slug}/projects/{project_id}/user-favorite-views/` | `IssueViewFavoriteViewSet.list` | `base.py:443-453` |
//!
//! The last one is a 405: `list` is DRF's `ListModelMixin` action, the viewset
//! has no `serializer_class`, and its only permission class is
//! `IsAuthenticated`, so the runtime's `BaseViewSet` guard refuses it
//! (`plane/app/views/base.py:103-198`). It is registered with
//! [`crate::cycles::base::guard_only`] so the guard is reachable; the handler
//! never runs.
//!
//! ## `is_favorite`
//!
//! `IssueViewSerializer` declares `is_favorite = BooleanField(read_only=True)`
//! (`plane/app/serializers/view.py:58`). `IssueViewViewSet.get_queryset`
//! annotates it with an `Exists(...)` subquery, so the project-view routes emit
//! `"is_favorite": false/true`; `WorkspaceViewViewSet.get_queryset` does **not**
//! annotate it, and a read-only field whose attribute is missing raises
//! `SkipField` (`rest_framework/fields.py:421-446`), so the workspace-view
//! routes emit no key at all. That is [`FieldValue::absent`] versus
//! [`FieldValue::value`], not `Option`.
//!
//! ## `fields=`
//!
//! Both `list` actions build the serializer with
//! `fields=fields if fields else None` from `?fields=` (`base.py:87-90`,
//! `:346-347`); both `retrieve` actions build it with no arguments, so they
//! ignore `?fields=` and `?expand=`. No route here passes `expand=`, so the
//! `DynamicBaseSerializer` expansion mapper is never consulted.
//!
//! ## Ordering
//!
//! `WorkspaceViewViewSet` sorts by
//! `sanitize_order_by(request.GET.get("order_by", "-created_at"),
//! VIEW_ORDER_BY_ALLOWLIST, "-created_at")`
//! (`plane/utils/order_queryset.py:135-156`) — `created_at`, `updated_at` or
//! `name`, optionally descending — while `IssueViewViewSet` always sorts by
//! `-is_favorite, name`.
//!
//! ## Queries per request
//!
//! | Route | Statements |
//! | --- | --- |
//! | workspace list | guest `exists` + page |
//! | workspace retrieve | page + guest `exists` (in that order) |
//! | project list | `Project.objects.get` + guest `exists` + page |
//! | project retrieve | page + `Project.objects.get` + guest `exists` + enqueue |

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_query::sql::{Sql, Value};
use plane_serializers::generated::app_view::IssueViewSerializer;
use plane_serializers::{FieldValue, PgJson, RenderCtx};
use uuid::Uuid;

use crate::common::render::RequestRender;
use crate::common::{client, jobs};
use crate::views::queries;

pub fn register(
    registry: &mut plane_drf::registry::Registry,
) -> Result<(), plane_drf::registry::RegistryError> {
    registry
        .viewset("plane.app.views.view.base.WorkspaceViewViewSet")?
        .action("list", workspace_list)?
        .action("retrieve", workspace_retrieve)?;
    registry
        .viewset("plane.app.views.view.base.IssueViewViewSet")?
        .action("list", issue_list)?
        .action("retrieve", issue_retrieve)?;
    registry
        .viewset("plane.app.views.view.base.IssueViewFavoriteViewSet")?
        // `GET …/user-favorite-views/`: the guard answers 405 (`COVERAGE.md`).
        .action("list", crate::cycles::base::guard_only)?;
    Ok(())
}

/// One `issue_views` row with the columns `IssueViewSerializer` reads.
#[derive(Debug)]
struct Row {
    id: Uuid,
    /// `Some` for the project-view routes (the `Exists` annotation), `None`
    /// for the workspace-view routes (the attribute is missing).
    is_favorite: Option<bool>,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    name: String,
    description: String,
    query: JsonText,
    filters: JsonText,
    display_filters: JsonText,
    display_properties: JsonText,
    rich_filters: JsonText,
    access: i16,
    sort_order: f64,
    logo_props: JsonText,
    is_locked: bool,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    workspace: Uuid,
    project: Option<Uuid>,
    owned_by: Uuid,
}

impl Row {
    /// `has_favorite` says whether the statement selected the `is_favorite`
    /// column at index 1 (all following columns shift by one).
    fn decode(row: &tokio_postgres::Row, has_favorite: bool) -> Result<Self, ViewError> {
        let base = usize::from(has_favorite);
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            is_favorite: if has_favorite {
                Some(row.try_get(1).map_err(ViewError::from)?)
            } else {
                None
            },
            created_at: row.try_get(1 + base).map_err(ViewError::from)?,
            updated_at: row.try_get(2 + base).map_err(ViewError::from)?,
            deleted_at: row.try_get(3 + base).map_err(ViewError::from)?,
            name: row.try_get(4 + base).map_err(ViewError::from)?,
            description: row.try_get(5 + base).map_err(ViewError::from)?,
            query: row.try_get(6 + base).map_err(ViewError::from)?,
            filters: row.try_get(7 + base).map_err(ViewError::from)?,
            display_filters: row.try_get(8 + base).map_err(ViewError::from)?,
            display_properties: row.try_get(9 + base).map_err(ViewError::from)?,
            rich_filters: row.try_get(10 + base).map_err(ViewError::from)?,
            access: row.try_get(11 + base).map_err(ViewError::from)?,
            sort_order: row.try_get(12 + base).map_err(ViewError::from)?,
            logo_props: row.try_get(13 + base).map_err(ViewError::from)?,
            is_locked: row.try_get(14 + base).map_err(ViewError::from)?,
            archived_at: row.try_get(15 + base).map_err(ViewError::from)?,
            created_by: row.try_get(16 + base).map_err(ViewError::from)?,
            updated_by: row.try_get(17 + base).map_err(ViewError::from)?,
            workspace: row.try_get(18 + base).map_err(ViewError::from)?,
            project: row.try_get(19 + base).map_err(ViewError::from)?,
            owned_by: row.try_get(20 + base).map_err(ViewError::from)?,
        })
    }

    fn value(&self) -> IssueViewSerializer<'_> {
        IssueViewSerializer {
            id: self.id,
            is_favorite: match self.is_favorite {
                // `SkipField`: the attribute does not exist on the instance.
                None => FieldValue::absent(),
                Some(value) => FieldValue::value(value),
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            name: &self.name,
            description: &self.description,
            query: Some(PgJson::from(&self.query)),
            filters: Some(PgJson::from(&self.filters)),
            display_filters: Some(PgJson::from(&self.display_filters)),
            display_properties: Some(PgJson::from(&self.display_properties)),
            rich_filters: Some(PgJson::from(&self.rich_filters)),
            access: i64::from(self.access),
            sort_order: self.sort_order,
            logo_props: Some(PgJson::from(&self.logo_props)),
            is_locked: self.is_locked,
            archived_at: self.archived_at,
            created_by: self.created_by,
            updated_by: self.updated_by,
            workspace: self.workspace,
            project: self.project,
            owned_by: self.owned_by,
        }
    }
}

/// The `order_by` values `VIEW_ORDER_BY_ALLOWLIST` permits, after
/// `sanitize_order_by` (`plane/utils/order_queryset.py:135-156`).
#[derive(Debug, Clone, Copy)]
enum Order {
    CreatedAtAsc,
    CreatedAtDesc,
    UpdatedAtAsc,
    UpdatedAtDesc,
    NameAsc,
    NameDesc,
}

impl Order {
    fn resolve(raw: Option<&str>) -> Self {
        let Some(raw) = raw.filter(|value| !value.is_empty()) else {
            return Self::CreatedAtDesc;
        };
        let (descending, bare) = match raw.strip_prefix('-') {
            Some(bare) => (true, bare),
            None => (false, raw),
        };
        // `--created_at` is rejected, not treated as descending `-created_at`.
        if bare.starts_with('-') {
            return Self::CreatedAtDesc;
        }
        match (bare, descending) {
            ("created_at", false) => Self::CreatedAtAsc,
            ("created_at", true) => Self::CreatedAtDesc,
            ("updated_at", false) => Self::UpdatedAtAsc,
            ("updated_at", true) => Self::UpdatedAtDesc,
            ("name", false) => Self::NameAsc,
            ("name", true) => Self::NameDesc,
            _ => Self::CreatedAtDesc,
        }
    }

    /// The `ORDER BY` term. Django renders `-field` as `DESC` and a bare field
    /// as `ASC` (Postgres' defaults), with no `NULLS` clause.
    fn fragment(self) -> &'static str {
        match self {
            Self::CreatedAtAsc => "iv.created_at ASC",
            Self::CreatedAtDesc => "iv.created_at DESC",
            Self::UpdatedAtAsc => "iv.updated_at ASC",
            Self::UpdatedAtDesc => "iv.updated_at DESC",
            Self::NameAsc => "iv.name ASC",
            Self::NameDesc => "iv.name DESC",
        }
    }
}

/// `WorkspaceViewViewSet.get_queryset` (`base.py:67-82`) as SQL.
///
/// The `order_by` string is the sanitized allowlist value, never request text,
/// so it is safe to splice; everything else is a bound parameter.
fn workspace_views_sql(order: Order, restricted: bool, by_pk: bool) -> String {
    let mut sql = String::from(
        "SELECT DISTINCT iv.id, iv.created_at, iv.updated_at, iv.deleted_at, \
         iv.name, iv.description, iv.query, iv.filters, iv.display_filters, \
         iv.display_properties, iv.rich_filters, iv.access, iv.sort_order, \
         iv.logo_props, iv.is_locked, iv.archived_at, iv.created_by_id, \
         iv.updated_by_id, iv.workspace_id, iv.project_id, iv.owned_by_id \
         FROM issue_views iv \
         INNER JOIN workspaces w ON iv.workspace_id = w.id \
         WHERE iv.deleted_at IS NULL AND w.slug = $1 \
           AND iv.project_id IS NULL \
           AND (iv.owned_by_id = $2 OR iv.access = 1)",
    );
    if restricted {
        sql.push_str(" AND iv.owned_by_id = $2");
    }
    if by_pk {
        sql.push_str(" AND iv.id = $3");
    }
    sql.push_str(" ORDER BY ");
    sql.push_str(order.fragment());
    if by_pk {
        sql.push_str(" LIMIT 1");
    }
    sql
}

/// `WorkspaceMember…role=5…exists()` (`base.py:88`, `:131`).
async fn workspace_guest_exists(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::WORKSPACE_GUEST_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&slug, &user])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `ProjectMember…role=5…exists()` (`base.py:335-342`, `:359-366`).
async fn project_guest_exists(
    client: &deadpool_postgres::Client,
    slug: &str,
    project_id: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::PROJECT_GUEST_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&slug, &project_id, &user])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

/// `Project.objects.get(id=project_id)` (`base.py:334`, `:353`): only
/// `guest_view_all_features` is read. A miss — a soft-deleted project included
/// — is `DoesNotExist`, i.e. the family 404 body.
async fn project_guest_view_all_features(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::PROJECT_GUEST_FLAG.sql)
        .await
        .map_err(ViewError::from)?;
    match client
        .query_opt(&statement, &[&project_id])
        .await
        .map_err(ViewError::from)?
    {
        Some(row) => Ok(row.try_get(0).map_err(ViewError::from)?),
        None => Err(ViewError::ObjectDoesNotExist),
    }
}

/// `WorkspaceViewViewSet.list` (`base.py:84-91`).
pub fn workspace_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let restricted = workspace_guest_exists(client, slug, user).await?;
        let sql = workspace_views_sql(
            Order::resolve(ctx.query().get("order_by")),
            restricted,
            false,
        );
        let sql = Sql {
            text: sql,
            params: vec![Value::Text(slug.to_owned()), Value::Uuid(user)],
        };
        let rows = sql.query(client).await.map_err(ViewError::from)?;

        let mut views = Vec::with_capacity(rows.len());
        for row in &rows {
            views.push(Row::decode(row, false)?);
        }
        let values: Vec<IssueViewSerializer<'_>> = views.iter().map(Row::value).collect();
        let prepared = RequestRender::from_ctx(ctx);
        let body = crate::common::render_list(&values, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceViewViewSet.retrieve` (`base.py:115-147`).
pub fn workspace_retrieve<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = crate::common::lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(ViewError::from)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `self.get_queryset().filter(pk=pk).first()` (`:118`).
        let sql = workspace_views_sql(Order::resolve(ctx.query().get("order_by")), false, true);
        let sql = Sql {
            text: sql,
            params: vec![
                Value::Text(slug.to_owned()),
                Value::Uuid(user),
                Value::Uuid(pk),
            ],
        };
        let Some(row) = sql
            .query(client)
            .await
            .map_err(ViewError::from)?
            .into_iter()
            .next()
        else {
            // `:122-126`.
            return Ok(error_body(404, "The required object does not exist."));
        };
        let view = Row::decode(&row, false)?;

        // `:130-137`: a workspace GUEST sees only views they own; 404, not 403.
        if workspace_guest_exists(client, slug, user).await? && view.owned_by != user {
            return Ok(error_body(404, "The required object does not exist."));
        }

        // `recent_visited_task.delay(slug, project_id=None, entity_name="view",
        // entity_identifier=pk, user_id)` (`:140-146`), published before
        // `serializer.data` is evaluated (`:147`).
        jobs::recent_visited_project(ctx, "view", TaskArg::uuid(pk), None, TaskArg::uuid(user))
            .await?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&view.value(), &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IssueViewViewSet.list` (`base.py:331-348`).
pub fn issue_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = crate::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )
        .map_err(ViewError::from)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `project = Project.objects.get(id=project_id)` (`:334`), then the
        // guest test (`:335-345`).
        let guest_view_all_features = project_guest_view_all_features(client, project_id).await?;
        let restricted = if guest_view_all_features {
            false
        } else {
            project_guest_exists(client, slug, project_id, user).await?
        };

        let statement = client
            .prepare_cached(if restricted {
                queries::ISSUE_VIEW_LIST_RESTRICTED.sql
            } else {
                queries::ISSUE_VIEW_LIST.sql
            })
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&slug, &project_id, &user])
            .await
            .map_err(ViewError::from)?;

        let mut views = Vec::with_capacity(rows.len());
        for row in &rows {
            views.push(Row::decode(row, true)?);
        }
        let values: Vec<IssueViewSerializer<'_>> = views.iter().map(Row::value).collect();
        let prepared = RequestRender::from_ctx(ctx);
        let body = crate::common::render_list(&values, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IssueViewViewSet.retrieve` (`base.py:350-383`).
pub fn issue_retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = crate::common::lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )
        .map_err(ViewError::from)?;
        let pk = crate::common::lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(ViewError::from)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `issue_view = self.get_queryset().filter(pk=pk, project_id=…).first()`
        // (`:352`).
        let statement = client
            .prepare_cached(queries::ISSUE_VIEW_BY_PK.sql)
            .await
            .map_err(ViewError::from)?;
        let issue_view = client
            .query_opt(&statement, &[&slug, &project_id, &user, &pk])
            .await
            .map_err(ViewError::from)?
            .map(|row| Row::decode(&row, true))
            .transpose()?;

        // `project = Project.objects.get(id=project_id)` (`:353`).
        let guest_view_all_features = project_guest_view_all_features(client, project_id).await?;

        // `:359-373`: a restricted project guest only sees their own view; 403.
        if !guest_view_all_features && project_guest_exists(client, slug, project_id, user).await? {
            match &issue_view {
                Some(view) if view.owned_by == user => {}
                Some(_) => {
                    return Ok(error_body(403, "You are not allowed to view this issue"));
                }
                // `issue_view.owned_by` on `None` is an `AttributeError` (500)
                // before the branch can return a response.
                None => {
                    return Err(ViewError::Unknown(
                        "AttributeError: 'NoneType' object has no attribute 'owned_by'".to_owned(),
                    ));
                }
            }
        }

        let Some(issue_view) = issue_view else {
            // `IssueViewSerializer(None)`: DRF's `BaseSerializer.data` sees
            // `self.instance is None` with no validated data and returns
            // `get_initial()` — a 200 whose body is the *writable* fields with
            // their `get_initial()` (`rest_framework/serializers.py:500-520`).
            // `CharField.initial = ''` is a class attribute, so `name` and
            // `description` are `""`; everything else is `None`. The visit is
            // still recorded: the task is published on the line before
            // `serializer.data` is evaluated (`:376-383`).
            jobs::recent_visited_project(
                ctx,
                "view",
                TaskArg::uuid(pk),
                // `project_id` is the URL kwarg, a `uuid.UUID` object
                // (TASKS.md §8.3 site 10).
                Some(TaskArg::uuid(project_id)),
                TaskArg::uuid(user),
            )
            .await?;
            return Ok(missing_issue_view_body());
        };

        // `recent_visited_task.delay(..., project_id=project_id, ...)`
        // (`:376-382`), published before `serializer.data` (`:383`).
        jobs::recent_visited_project(
            ctx,
            "view",
            TaskArg::uuid(pk),
            Some(TaskArg::uuid(project_id)),
            TaskArg::uuid(user),
        )
        .await?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&issue_view.value(), &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `IssueViewSerializer(None).data`, i.e. `BaseSerializer.get_initial()` for
/// `IssueViewSerializer` (`plane/app/serializers/view.py:58-83`): every field
/// that is not `read_only`, in declaration order, with `Field.get_initial()`.
///
/// `read_only_fields` are `workspace`, `project`, `query`, `owned_by`,
/// `access`, `is_locked`, `created_by`, `updated_by`; `id`, `created_at`,
/// `updated_at` and `is_favorite` are read-only by construction. `CharField`
/// carries a class-level `initial = ''`, which is why `name` and `description`
/// are empty strings rather than `null`.
fn missing_issue_view_body() -> ViewResponse {
    use plane_query::json::Json;
    let body = Json::object(vec![
        ("deleted_at", Json::Null),
        ("name", Json::string("")),
        ("description", Json::string("")),
        ("filters", Json::Null),
        ("display_filters", Json::Null),
        ("display_properties", Json::Null),
        ("rich_filters", Json::Null),
        ("sort_order", Json::Null),
        ("logo_props", Json::Null),
        ("archived_at", Json::Null),
    ]);
    ViewResponse::raw(200, APPLICATION_JSON, body.to_drf())
}

/// A one-key `{"error": …}` body, the family `handle_exception` shape.
fn error_body(status: u16, message: &str) -> ViewResponse {
    ViewResponse::raw(
        status,
        APPLICATION_JSON,
        plane_query::json::Json::object(vec![("error", plane_query::json::Json::string(message))])
            .to_drf(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_order_by_follows_the_allowlist() {
        assert!(matches!(Order::resolve(None), Order::CreatedAtDesc));
        assert!(matches!(Order::resolve(Some("")), Order::CreatedAtDesc));
        assert!(matches!(Order::resolve(Some("name")), Order::NameAsc));
        assert!(matches!(Order::resolve(Some("-name")), Order::NameDesc));
        assert!(matches!(
            Order::resolve(Some("sort_order")),
            Order::CreatedAtDesc
        ));
        assert!(matches!(
            Order::resolve(Some("--created_at")),
            Order::CreatedAtDesc
        ));
        assert!(matches!(
            Order::resolve(Some("created_at; DROP TABLE issue_views")),
            Order::CreatedAtDesc
        ));
    }

    #[test]
    fn the_workspace_sql_has_the_python_predicates() {
        let sql = workspace_views_sql(Order::NameDesc, true, true);
        assert!(sql.contains("iv.project_id IS NULL"));
        assert!(sql.contains("(iv.owned_by_id = $2 OR iv.access = 1)"));
        assert!(sql.contains("AND iv.owned_by_id = $2"));
        assert!(sql.contains("AND iv.id = $3"));
        assert!(sql.ends_with("ORDER BY iv.name DESC LIMIT 1"));
    }
}
