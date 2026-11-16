// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IssueViewSet`'s write actions and the issue-core bulk endpoints
//! (`plans/WORK_ITEMS.md` set A).
//!
//! | Route | Python |
//! | --- | --- |
//! | `POST   …/issues/` | `IssueViewSet.create` (`plane/app/views/issue/base.py:406-497`) |
//! | `PATCH  …/issues/{pk}/` | `IssueViewSet.partial_update` (`:628-716`) |
//! | `DELETE …/issues/{pk}/` | `IssueViewSet.destroy` (`:718-741`) |
//! | `DELETE …/bulk-delete-issues/` | `BulkDeleteIssuesEndpoint.delete` (`:769-788`) |
//! | `POST   …/bulk-archive-issues/` | `BulkArchiveIssuesEndpoint.post` (`archive.py:305-339`) |
//! | `POST   …/issues/{pk}/archive/` | `IssueArchiveViewSet.archive` (`archive.py:256-281`) |
//! | `DELETE …/issues/{pk}/archive/` | `IssueArchiveViewSet.unarchive` (`archive.py:284-302`) |
//! | `POST   …/issue-dates/` | `IssueBulkUpdateDateEndpoint.post` (`base.py:1123-1181`) |
//!
//! `PUT …/issues/{pk}/` is `rest_framework.mixins.UpdateModelMixin.update`,
//! which `BaseViewSet`'s guard refuses with 405 (`routes.json`'s
//! `viewset_guard.refused_methods`). The action is registered as
//! [`guard_only`] because the runtime only dispatches a method that has a
//! handler; the guard answers before the handler is reached.
//!
//! ## Cross-cutting behaviour
//!
//! * An issue INSERT/UPDATE goes through `plane_db::hooks::issue`
//!   (`create_issue_row`/`update_issue_row`), which own the advisory lock,
//!   sequence, sort order, default state, `completed_at` and
//!   `description_stripped` timestamp rules. The handler only fills the model
//!   columns the serializer validated.
//! * Assignee/label rows go through `plane_db::hooks::issue_m2m`; the create
//!   path's default-assignee fallback is the same decision as
//!   `IssueAssignee.objects.create` inside `try/except IntegrityError`.
//! * The four task publishers are `plane_jobs::celery::tasks`; PATCH publishes
//!   `issue_activity`, `model_activity`, then `issue_description_version_task`,
//!   in that order (`base.py:687-712`).
//! * Request order and key order matter for `accepted_data` and the raw
//!   `request.data` payloads; [`super::write_support`] keeps both.
//!
//! ## Upstream quirks ported as they are
//!
//! * `create` looks the project up by id only (`Project.objects.get(pk=…)`), so
//!   a project of another workspace is accepted and the URL slug is ignored;
//! * a PATCH body that is not a mapping (list/scalar/null) raises on
//!   `request.data.pop(...)` before the serializer, i.e. a 500, not the
//!   serializer's 400;
//! * the bulk-archive loop publishes activity rows for earlier issues and then
//!   returns 400 for a later non-completed issue, leaving those messages and no
//!   `archived_at` update (the `bulk_update` never runs);
//! * `issue-dates` publishes both date activities before `bulk_update`; an
//!   invalid date string therefore returns 400 after the messages; a
//!   non-`YYYY-MM-DD` string is a 500 from `datetime.strptime` in
//!   `validate_dates`, while an invalid value reaching `bulk_update` is the
//!   family's 400;
//! * `issue-dates` mutates the single in-memory issue per request, so a
//!   repeated id sees the first update's dates when the second activity's
//!   `current_instance` is rendered;
//! * `UserRecentVisit` rows are hard-deleted, unlike every other delete here.

use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks::issue::IssueHookError;
use plane_db::hooks::{self, issue_m2m};
use plane_db::models::issue::Issue;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall, ModelActivityCall};
use plane_query::json::Json as Doc;
use plane_serde::pyjson::{PyStr, Value as PValue};
use uuid::Uuid;

use crate::common::{client, jobs, lookup};
use crate::issues::write_issue_queries as queries;
use crate::issues::write_support::{self, IssueSummary};
use crate::write::description::{self, HtmlValidation};
use crate::write::fields::Detail;
use crate::write::generated::issue_create_serializer;
use crate::write::{self, Serializer, Validated, ValidationErrors, Value};

/// `PUT` (`rest_framework.mixins.UpdateModelMixin.update`) is refused by
/// `BaseViewSet`'s guard (`routes.json` `viewset_guard.refused_methods`): the
/// runtime answers 405 before dispatch, so this handler is unreachable. It is
/// registered because `Runtime::is_registered` only dispatches a method that
/// has a handler.
pub fn guard_only<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Err(ViewError::Unknown(format!(
            "{} reached a handler its BaseViewSet guard always refuses",
            ctx.view
        )))
    })
}

// ---------------------------------------------------------------------------
// create — POST …/issues/ (order 64)
// ---------------------------------------------------------------------------

/// `IssueViewSet.create` (`plane/app/views/issue/base.py:406-497`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg(ctx, "slug")?;
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Project.objects.get(pk=project_id)` (`:407`): no workspace slug and
        // no `archived_at` filter, deliberately.
        let project = project_for_create(client, project_id).await?;

        let prepared = prepare_create(ctx, client, project_id).await?;
        let Created { mut issue, extra } = match prepared {
            PreparedCreate::Valid(created) => *created,
            PreparedCreate::Invalid(response) => return Ok(response),
        };
        write_created_issue(
            client,
            &mut issue,
            &extra,
            project.default_assignee_id,
            user,
        )
        .await?;

        // `issue_activity.delay` (`:419-430`) — `requested_data` is
        // `json.dumps(self.request.data, cls=DjangoJSONEncoder)`.
        let body = write_support::request_object(ctx)?;
        let origin = write_support::base_host(ctx)?;
        publish_issue_activity(
            ctx,
            IssueActivityCall::view(
                activity_base(
                    "issue.activity.created",
                    TaskArg::json_text_from(&body)
                        .map_err(|error| ViewError::Unknown(error.to_string()))?,
                    user,
                    &issue.id.to_string(),
                    &project_id.to_string(),
                    TaskArg::Null,
                ),
                origin.clone(),
            ),
        )
        .await?;

        // `queryset.filter(pk=serializer.data["id"]).values(…).first()`: the
        // 201 body is the flat row, not a serializer (`:466-489`).
        let row = crate::issues::row::render_issue_row(ctx, client, issue.id).await?;

        publish_model_activity(
            ctx,
            "issue",
            &issue.id.to_string(),
            TaskArg::Json(body.clone()),
            None,
            user,
            &slug,
            &origin,
        )
        .await?;
        publish_description_version(
            ctx,
            TaskArg::json_text_from(&body)
                .map_err(|error| ViewError::Unknown(error.to_string()))?,
            &issue.id.to_string(),
            user,
            true,
        )
        .await?;

        Ok(ViewResponse::raw(201, APPLICATION_JSON, row.to_drf()))
    })
}

/// The result of the serializer half of an issue create.
pub(crate) enum PreparedCreate {
    /// `serializer.is_valid()` returned True; the model row is built.
    Valid(Box<Created>),
    /// `serializer.errors`, already rendered (400).
    Invalid(ViewResponse),
}

/// `IssueCreateSerializer` validated against one project, plus the model row
/// [`build_issue`] produced from it.
///
/// Shared by `IssueViewSet.create` and `WorkspaceDraftIssueViewSet.
/// create_draft_to_issue` (`plane/app/views/workspace/draft.py:275-285`): both
/// build the serializer with a `project_id`/`workspace_id`/`default_assignee_id`
/// context and both call `serializer.save()`, which is
/// [`write_created_issue`].
pub(crate) struct Created {
    pub issue: Issue,
    pub extra: Extra,
}

/// The serializer sequence of `IssueCreateSerializer(data=…, context=…)` +
/// `is_valid()` + `IssueCreateSerializer.create`'s `Issue(...)` construction.
///
/// # Errors
///
/// A database failure from the generated validators or `validate()`'s
/// project-scoped checks.
pub(crate) async fn prepare_create(
    ctx: &ViewCtx<'_>,
    client: &Client,
    project_id: Uuid,
) -> Result<PreparedCreate, ViewError> {
    let mut serializer = Serializer::new(ctx, false)?;
    write::generated::issue_create_serializer::validate(&mut serializer, client).await?;
    recheck_manager_fields(client, &mut serializer).await?;
    let validated = match serializer.finish() {
        Ok(validated) => validated,
        Err(mut errors) => {
            errors.sort_by_field_order(&field_order());
            return Ok(PreparedCreate::Invalid(errors.into_response()));
        }
    };

    let extra = match validate_issue(client, project_id, &validated).await {
        Ok(extra) => extra,
        Err(errors) => return Ok(PreparedCreate::Invalid(errors.into_response())),
    };

    Ok(PreparedCreate::Valid(Box::new(Created {
        issue: build_issue(&validated, project_id, &extra),
        extra,
    })))
}

/// `serializer.save()` on a create: `Issue.objects.create(...)` (H10/H11) and
/// then `IssueCreateSerializer.create`'s assignee/label through rows
/// (`plane/app/serializers/issue.py:217-284`).
///
/// # Errors
///
/// A database failure. `issue_m2m` reproduces the Python's
/// `try/except IntegrityError: pass` per `bulk_create` call.
pub(crate) async fn write_created_issue(
    client: &Client,
    issue: &mut Issue,
    extra: &Extra,
    default_assignee_id: Option<Uuid>,
    user: Uuid,
) -> Result<(), ViewError> {
    hooks::issue::create_issue_row(client, issue, Some(user), write::now())
        .await
        .map_err(issue_hook_error)?;

    // `IssueCreateSerializer.create` (`:209-286`): the m2m rows use the
    // issue's audit ids just stamped by the INSERT.
    let m2m_now = write::now();
    let has_assignees = extra
        .assignees
        .as_ref()
        .is_some_and(|assignees| !assignees.is_empty());
    if has_assignees {
        issue_m2m::insert_assignees(
            client,
            issue,
            extra.assignees.as_deref().unwrap_or_default(),
            m2m_now,
        )
        .await?;
    } else if let Some(default) =
        issue_m2m::default_assignee(client, issue.project_id, default_assignee_id).await?
    {
        issue_m2m::insert_assignees(client, issue, &[default], m2m_now).await?;
    }
    if extra
        .labels
        .as_ref()
        .is_some_and(|labels| !labels.is_empty())
    {
        issue_m2m::insert_labels(
            client,
            issue,
            extra.labels.as_deref().unwrap_or_default(),
            m2m_now,
        )
        .await?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// PATCH — PATCH …/issues/{pk}/ (order 67)
// ---------------------------------------------------------------------------

/// `IssueViewSet.partial_update` (`plane/app/views/issue/base.py:628-716`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg(ctx, "slug")?;
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `queryset.annotate(…).filter(pk=pk).first()` (`:640-678`): the
        // pre-image is also the activity `current_instance`.
        let Some(detail) =
            crate::issues::lists::list_retrieve::patch_detail(client, &slug, project_id, pk)
                .await?
        else {
            return Ok(write::error_body(404, "Issue not found"));
        };
        let current_instance = detail.current_instance(ctx)?;

        // `request.data.pop("skip_activity", False)` and
        // `request.data.get("description_html") is not None` (`:630-631`).
        // A non-mapping body fails the `pop` before the serializer (500).
        let (skip_activity, is_description_update) = patch_flags(ctx)?;

        let Some(mut issue) = load_full_issue(client, project_id, &slug, pk).await? else {
            return Ok(write::error_body(404, "Issue not found"));
        };
        let old_state = issue.state_id;

        let mut serializer = Serializer::new(ctx, true)?;
        write_support::mark_serializer_patch_fields(&mut serializer);
        issue_create_serializer::validate(&mut serializer, client).await?;
        recheck_manager_fields(client, &mut serializer).await?;
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(mut errors) => {
                errors.sort_by_field_order(&field_order());
                return Ok(errors.into_response());
            }
        };

        let extra = match validate_issue(client, project_id, &validated).await {
            Ok(extra) => extra,
            Err(errors) => return Ok(errors.into_response()),
        };

        apply_patch(&mut issue, &validated, &extra);
        // `Issue.save()`'s H10a runs before H10b's `has_changed("state_id")`,
        // so a NULL state becomes the project default before the comparison.
        if issue.state_id.is_none() {
            issue.state_id = hooks::issue::ensure_default_state(client, project_id).await?;
        }
        let state_changed = issue.state_id != old_state;

        // `IssueCreateSerializer.update` (`:288-333`) replaces the through rows
        // before `super().update()`, so their audit ids are the pre-update
        // issue's.
        let m2m_now = write::now();
        if let Some(assignees) = &extra.assignees {
            issue_m2m::replace_assignees(client, &issue, assignees, m2m_now).await?;
        }
        if let Some(labels) = &extra.labels {
            issue_m2m::replace_labels(client, &issue, labels, m2m_now).await?;
        }

        // `Issue.save()`'s update branch recomputes `description_stripped` from
        // the (possibly unchanged) `description_html`
        // (`plane/db/models/issue.py:214-222`).
        issue.description_stripped = strip_html(&issue.description_html);
        hooks::issue::update_issue_row(client, &mut issue, state_changed, Some(user), write::now())
            .await?;

        // `writable_data(serializer, request.data)` (`:683`): read-only keys
        // are ignored on save, so they are not reported as changes either.
        let accepted = write_support::accepted_data(ctx, true)?;
        let is_migration_description_update = skip_activity && is_description_update;
        if !is_migration_description_update {
            let origin = write_support::base_host(ctx)?;
            publish_issue_activity(
                ctx,
                IssueActivityCall::view(
                    activity_base(
                        "issue.activity.updated",
                        TaskArg::json_text_from(&accepted)
                            .map_err(|error| ViewError::Unknown(error.to_string()))?,
                        user,
                        &pk.to_string(),
                        &project_id.to_string(),
                        TaskArg::json_text(current_instance.clone()),
                    ),
                    origin.clone(),
                ),
            )
            .await?;
            publish_model_activity(
                ctx,
                "issue",
                &issue.id.to_string(),
                TaskArg::Json(accepted),
                Some(current_instance.clone()),
                user,
                &slug,
                &origin,
            )
            .await?;
            publish_description_version(
                ctx,
                TaskArg::json_text(current_instance),
                &issue.id.to_string(),
                user,
                false,
            )
            .await?;
        }

        Ok(ViewResponse::empty(204))
    })
}

/// Read the two PATCH body flags, reproducing the `pop`/`get` order.
///
/// # Errors
///
/// [`ViewError::Unknown`] for a non-mapping JSON body (Python's
/// `AttributeError`/`TypeError` before the serializer).
fn patch_flags(ctx: &ViewCtx<'_>) -> Result<(bool, bool), ViewError> {
    match ctx.data()? {
        Data::Json(PValue::Dict(object)) => Ok((
            object
                .get("skip_activity")
                .is_some_and(write_support::truthy),
            object
                .get("description_html")
                .is_some_and(|value| !value.is_null()),
        )),
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: request.data has no pop".to_owned(),
        )),
        Data::Form(query) => Ok((
            query
                .get("skip_activity")
                .is_some_and(|value| !value.is_empty()),
            query.get("description_html").is_some(),
        )),
    }
}

// ---------------------------------------------------------------------------
// DELETE — DELETE …/issues/{pk}/ (order 67)
// ---------------------------------------------------------------------------

/// `IssueViewSet.destroy` (`plane/app/views/issue/base.py:718-741`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg(ctx, "slug")?;
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Issue.objects.get(workspace__slug=…, project_id=…, pk=…)`
        // (`:719`): the soft-deletion manager, not `issue_objects`.
        let statement = client
            .prepare_cached(queries::ISSUE_FOR_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&slug, &project_id, &pk])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut issue = Issue::from_row(&row, 0)?;

        // `issue.delete()`: `deleted_at = now`, a full save (H10a, H10b, H12,
        // H01), then `soft_delete_related_objects.delay("db", "issue", pk,
        // using=None)` (`plane/db/mixins.py:65-77`).
        let state_changed = ensure_state_for_save(client, &mut issue).await?;
        let now = write::now();
        issue.deleted_at = Some(now);
        issue.description_stripped = strip_html(&issue.description_html);
        hooks::issue::update_issue_row(client, &mut issue, state_changed, Some(user), now).await?;
        write::publish_soft_delete_cascade(ctx, "db", "issue", pk).await?;

        // `UserRecentVisit.objects.filter(...).delete(soft=False)` (`:723-730`):
        // a hard DELETE, unlike every other delete in this file.
        let statement = client
            .prepare_cached(queries::DELETE_RECENT_VISITS.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(&statement, &[&project_id, &slug, &pk])
            .await
            .map_err(ViewError::from)?;

        let origin = write_support::base_host(ctx)?;
        let requested = PValue::Dict(
            [(
                PyStr::new("issue_id"),
                PValue::Str(PyStr::from_string(pk.to_string())),
            )]
            .into_iter()
            .collect(),
        );
        publish_issue_activity(
            ctx,
            IssueActivityCall::view_with_subscriber(
                activity_base(
                    "issue.activity.deleted",
                    TaskArg::json_text(write_support::dumps(&requested)?),
                    user,
                    &pk.to_string(),
                    &project_id.to_string(),
                    TaskArg::Map(Vec::new()),
                ),
                origin,
            ),
        )
        .await?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// archive / unarchive — POST/DELETE …/issues/{pk}/archive/ (order 92)
// ---------------------------------------------------------------------------

/// `IssueArchiveViewSet.archive` (`plane/app/views/issue/archive.py:256-281`).
pub fn archive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg(ctx, "slug")?;
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Issue.issue_objects.get(...)` (`:257`): the triage/archived/draft
        // and project-archived exclusions are part of the lookup.
        let statement = client
            .prepare_cached(queries::ISSUE_FOR_ARCHIVE.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&slug, &project_id, &pk])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut issue = Issue::from_row(&row, 0)?;
        let state_group: Option<String> = row.try_get(28).map_err(ViewError::from)?;

        let Some(group) = state_group.filter(|_| issue.state_id.is_some()) else {
            // `issue.state.group` on a NULL state is an `AttributeError`.
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'group'".to_owned(),
            ));
        };
        if group != "completed" && group != "cancelled" {
            return Ok(write::error_body(
                400,
                "Can only archive completed or cancelled state group issue",
            ));
        }

        let date = write_support::today();
        let origin = write_support::base_host(ctx)?;
        let requested = Doc::object(vec![
            ("archived_at", Doc::string(date.to_string())),
            ("automation", Doc::Bool(false)),
        ]);
        let current_instance =
            write_support::issue_serializer_current_instance(&issue, write_support::timezone(ctx))?;
        publish_issue_activity(
            ctx,
            IssueActivityCall::view(
                activity_base(
                    "issue.activity.updated",
                    TaskArg::json_text(
                        String::from_utf8(write_support::dje_bytes(&requested)?)
                            .map_err(|error| ViewError::Unknown(error.to_string()))?,
                    ),
                    user,
                    &issue.id.to_string(),
                    &project_id.to_string(),
                    TaskArg::json_text(current_instance),
                ),
                origin,
            ),
        )
        .await?;

        let state_changed = ensure_state_for_save(client, &mut issue).await?;
        issue.archived_at = Some(date);
        issue.description_stripped = strip_html(&issue.description_html);
        hooks::issue::update_issue_row(client, &mut issue, state_changed, Some(user), write::now())
            .await?;

        Ok(ok_json(Doc::object(vec![(
            "archived_at",
            Doc::string(date.to_string()),
        )])))
    })
}

/// `IssueArchiveViewSet.unarchive` (`plane/app/views/issue/archive.py:284-302`).
pub fn unarchive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg(ctx, "slug")?;
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::ISSUE_FOR_UNARCHIVE.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&slug, &project_id, &pk])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut issue = Issue::from_row(&row, 0)?;

        let requested = Doc::object(vec![("archived_at", Doc::Null)]);
        let current_instance =
            write_support::issue_serializer_current_instance(&issue, write_support::timezone(ctx))?;
        let origin = write_support::base_host(ctx)?;
        publish_issue_activity(
            ctx,
            IssueActivityCall::view(
                activity_base(
                    "issue.activity.updated",
                    TaskArg::json_text(
                        String::from_utf8(write_support::dje_bytes(&requested)?)
                            .map_err(|error| ViewError::Unknown(error.to_string()))?,
                    ),
                    user,
                    &issue.id.to_string(),
                    &project_id.to_string(),
                    TaskArg::json_text(current_instance),
                ),
                origin,
            ),
        )
        .await?;

        let state_changed = ensure_state_for_save(client, &mut issue).await?;
        issue.archived_at = None;
        issue.description_stripped = strip_html(&issue.description_html);
        hooks::issue::update_issue_row(client, &mut issue, state_changed, Some(user), write::now())
            .await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Bulk delete — DELETE …/bulk-delete-issues/ (order 71)
// ---------------------------------------------------------------------------

/// `BulkDeleteIssuesEndpoint.delete` (`plane/app/views/issue/base.py:769-788`).
pub fn bulk_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg(ctx, "slug")?;
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let client = client(ctx)?;

        let issue_ids = issue_ids_body(ctx)?;
        if issue_ids.is_empty() {
            return Ok(write::error_body(400, "Issue IDs are required"));
        }

        // `Issue.issue_objects.filter(...)`: evaluated by `len(issues)`, then
        // the three queryset deletes. The select is one statement in Django
        // too (`len` fetches every row).
        let statement = client
            .prepare_cached(queries::ISSUES_FOR_BULK_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&slug, &project_id, &issue_ids])
            .await
            .map_err(ViewError::from)?;
        let selected: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();
        let total = selected.len();

        if !selected.is_empty() {
            // Each `.delete()` is its own `timezone.now()` call.
            for query in [
                &queries::BULK_DELETE_CYCLE_ISSUES,
                &queries::BULK_DELETE_MODULE_ISSUES,
                &queries::BULK_DELETE_ISSUES,
            ] {
                let statement = client
                    .prepare_cached(query.sql)
                    .await
                    .map_err(ViewError::from)?;
                client
                    .execute(&statement, &[&write::now(), &selected])
                    .await
                    .map_err(ViewError::from)?;
            }
        }

        Ok(ok_json(Doc::object(vec![(
            "message",
            Doc::string(format!("{total} issues were deleted")),
        )])))
    })
}

// ---------------------------------------------------------------------------
// Bulk archive — POST …/bulk-archive-issues/ (order 72)
// ---------------------------------------------------------------------------

/// `BulkArchiveIssuesEndpoint.post` (`plane/app/views/issue/archive.py:305-339`).
pub fn bulk_archive<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg(ctx, "slug")?;
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let issue_ids = issue_ids_body(ctx)?;
        if issue_ids.is_empty() {
            return Ok(write::error_body(400, "Issue IDs are required"));
        }

        // `Issue.objects.filter(...).select_related("state")`
        // (`archive.py:308-310`): the soft-deletion manager only, so archived,
        // triage and draft issues are candidates too.
        let statement = client
            .prepare_cached(queries::ISSUES_FOR_BULK_ARCHIVE.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&slug, &project_id, &issue_ids])
            .await
            .map_err(ViewError::from)?;
        let summaries: Vec<IssueSummary> = rows
            .iter()
            .map(IssueSummary::from_row)
            .collect::<Result<_, _>>()?;

        let date = write_support::today();
        let origin = write_support::base_host(ctx)?;
        let timezone = write_support::timezone(ctx);
        let mut archived: Vec<Uuid> = Vec::with_capacity(summaries.len());

        for summary in &summaries {
            // `issue.state.group` on a NULL state is an `AttributeError` (500),
            // which happens before the 400 branch in Python.
            if summary.data.state_id().is_none() {
                return Err(ViewError::Unknown(
                    "AttributeError: 'NoneType' object has no attribute 'group'".to_owned(),
                ));
            }
            let group = summary.state_group.as_deref();
            if group != Some("completed") && group != Some("cancelled") {
                // Earlier iterations already published their activity rows;
                // the update below never runs (`archive.py:316-325`).
                return Ok(ViewResponse::raw(
                    400,
                    APPLICATION_JSON,
                    Doc::object(vec![
                        ("error_code", Doc::Int(4091)),
                        ("error_message", Doc::string("INVALID_ARCHIVE_STATE_GROUP")),
                    ])
                    .to_drf(),
                ));
            }
            let current_instance =
                String::from_utf8(write_support::dje_bytes(&summary.data.to_json(timezone))?)
                    .map_err(|error| ViewError::Unknown(error.to_string()))?;
            let requested = Doc::object(vec![
                ("archived_at", Doc::string(date.to_string())),
                ("automation", Doc::Bool(false)),
            ]);
            publish_issue_activity(
                ctx,
                IssueActivityCall::view(
                    activity_base(
                        "issue.activity.updated",
                        TaskArg::json_text(
                            String::from_utf8(write_support::dje_bytes(&requested)?)
                                .map_err(|error| ViewError::Unknown(error.to_string()))?,
                        ),
                        user,
                        &summary.data.id().to_string(),
                        &project_id.to_string(),
                        TaskArg::json_text(current_instance),
                    ),
                    origin.clone(),
                ),
            )
            .await?;
            archived.push(summary.data.id());
        }

        if !archived.is_empty() {
            // `Issue.objects.bulk_update(issues, ["archived_at"])`: one
            // UPDATE, no audit columns and no `updated_at`.
            let statement = client
                .prepare_cached(queries::BULK_ARCHIVE_UPDATE.sql)
                .await
                .map_err(ViewError::from)?;
            client
                .execute(&statement, &[&date, &archived])
                .await
                .map_err(ViewError::from)?;
        }

        Ok(ok_json(Doc::object(vec![(
            "archived_at",
            Doc::string(write_support::today().to_string()),
        )])))
    })
}

// ---------------------------------------------------------------------------
// Issue dates — POST …/issue-dates/ (order 96)
// ---------------------------------------------------------------------------

/// `IssueBulkUpdateDateEndpoint.post` (`plane/app/views/issue/base.py:1123-1181`).
pub fn issue_dates<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg(ctx, "slug")?;
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let updates = updates_body(ctx)?;
        if updates.is_empty() {
            return Ok(ok_json(Doc::object(vec![(
                "message",
                Doc::string("Issues updated successfully"),
            )])));
        }

        let requested_ids = update_ids(&updates)?;
        let mut issues: HashMap<String, Issue> = HashMap::new();
        if !requested_ids.is_empty() {
            let statement = client
                .prepare_cached(queries::ISSUES_FOR_ISSUE_DATES.sql)
                .await
                .map_err(ViewError::from)?;
            let rows = client
                .query(&statement, &[&slug, &project_id, &requested_ids])
                .await
                .map_err(ViewError::from)?;
            for row in &rows {
                let issue = Issue::from_row(row, 0)?;
                issues.insert(issue.id.to_string(), issue);
            }
        }

        let epoch = Utc::now().timestamp();
        let mut touched: Vec<Uuid> = Vec::new();
        let mut seen: HashSet<Uuid> = HashSet::new();

        for update in &updates {
            let PValue::Dict(object) = update else {
                return Err(ViewError::Unknown(
                    "TypeError: update is not a mapping".to_owned(),
                ));
            };
            let Some(id_value) = object.get("id") else {
                return Err(ViewError::Key);
            };
            let key = write_support::python_str(id_value);
            let Some(issue) = issues.get_mut(&key) else {
                continue;
            };

            let start_value = object.get("start_date");
            let target_value = object.get("target_date");
            if !validate_dates(
                issue.start_date,
                issue.target_date,
                start_value,
                target_value,
            )? {
                return Ok(ViewResponse::raw(
                    400,
                    APPLICATION_JSON,
                    Doc::object(vec![(
                        "message",
                        Doc::string("Start date cannot exceed target date"),
                    )])
                    .to_drf(),
                ));
            }

            if start_value.is_some_and(write_support::truthy) {
                let requested = PValue::Dict(
                    [(
                        PyStr::new("start_date"),
                        start_value.cloned().unwrap_or(PValue::Null),
                    )]
                    .into_iter()
                    .collect(),
                );
                publish_issue_activity(
                    ctx,
                    IssueActivityCall::bulk_date(activity_base_at(
                        "issue.activity.updated",
                        TaskArg::json_text(write_support::dumps(&requested)?),
                        user,
                        &write_support::python_str(id_value),
                        &project_id.to_string(),
                        TaskArg::json_text(dumped_date(issue.start_date, "start_date")?),
                        epoch,
                    )),
                )
                .await?;
                issue.start_date = parse_activity_date(start_value, "start_date")?;
                remember(&mut touched, &mut seen, issue.id);
            }
            if target_value.is_some_and(write_support::truthy) {
                let requested = PValue::Dict(
                    [(
                        PyStr::new("target_date"),
                        target_value.cloned().unwrap_or(PValue::Null),
                    )]
                    .into_iter()
                    .collect(),
                );
                publish_issue_activity(
                    ctx,
                    IssueActivityCall::bulk_date(activity_base_at(
                        "issue.activity.updated",
                        TaskArg::json_text(write_support::dumps(&requested)?),
                        user,
                        &write_support::python_str(id_value),
                        &project_id.to_string(),
                        TaskArg::json_text(dumped_date(issue.target_date, "target_date")?),
                        epoch,
                    )),
                )
                .await?;
                issue.target_date = parse_activity_date(target_value, "target_date")?;
                remember(&mut touched, &mut seen, issue.id);
            }
        }

        if !touched.is_empty() {
            // `Issue.objects.bulk_update(issues_to_update, ["start_date",
            // "target_date"])`: one UPDATE, no `updated_at`.
            let mut sql = String::from(
                "UPDATE issues SET start_date = v.start_date, \
                 target_date = v.target_date FROM (VALUES ",
            );
            for (index, _) in touched.iter().enumerate() {
                if index != 0 {
                    sql.push_str(", ");
                }
                let base = index * 3;
                sql.push_str(&format!(
                    "(${}::uuid, ${}::date, ${}::date)",
                    base + 1,
                    base + 2,
                    base + 3
                ));
            }
            sql.push_str(") AS v(id, start_date, target_date) WHERE issues.id = v.id");
            let statement = client.prepare(&sql).await.map_err(ViewError::from)?;
            let values: Vec<(Uuid, Option<NaiveDate>, Option<NaiveDate>)> = touched
                .iter()
                .map(|id| {
                    let issue = issues
                        .get(&id.to_string())
                        .expect("touched issues stay in the map");
                    (*id, issue.start_date, issue.target_date)
                })
                .collect();
            let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
                Vec::with_capacity(values.len() * 3);
            for (id, start, target) in &values {
                params.push(id);
                params.push(start);
                params.push(target);
            }
            client
                .execute(&statement, &params)
                .await
                .map_err(ViewError::from)?;
        }

        Ok(ok_json(Doc::object(vec![(
            "message",
            Doc::string("Issues updated successfully"),
        )])))
    })
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// The extra `validate()` results: sanitised HTML and the project-filtered
/// through-row ids (`plane/app/serializers/issue.py:133-205`).
#[derive(Debug, Default)]
pub(crate) struct Extra {
    pub description_html: Option<String>,
    pub assignees: Option<Vec<Uuid>>,
    pub labels: Option<Vec<Uuid>>,
}

/// `IssueCreateSerializer.validate` (`plane/app/serializers/issue.py:133-205`),
/// after the generated field validators.
///
/// The Python applies these branches in this order and raises on the first:
/// date range, HTML content, binary content (unreachable: `description_binary`
/// is read-only), assignee filtering, label filtering, then the state, parent
/// and estimate-point project checks.
async fn validate_issue(
    client: &Client,
    project_id: Uuid,
    validated: &Validated,
) -> Result<Extra, ValidationErrors> {
    // `start_date > target_date`.
    if let (Some(start), Some(target)) =
        (validated.date("start_date"), validated.date("target_date"))
        && start > target
    {
        return Err(ValidationErrors::new(
            "non_field_errors",
            Detail::new("Start date cannot exceed target date", "invalid"),
        ));
    }

    // `validate_html_content` replaces `description_html`; the serializer
    // reports the generic field error `{"error": ["html content is not valid"]}`.
    let mut extra = Extra::default();
    if let Some(html) = validated.text("description_html")
        && !html.is_empty()
    {
        match description::validate_html_content(html) {
            HtmlValidation::Sanitized(sanitized) => extra.description_html = Some(sanitized),
            HtmlValidation::Unchanged => {}
            HtmlValidation::Invalid => {
                return Err(ValidationErrors::new(
                    "error",
                    Detail::new("html content is not valid", "invalid"),
                ));
            }
        }
    }

    // Assignees: active project members with role >= 15.
    if let Some(ids) = validated.uuid_list("assignee_ids") {
        extra.assignees = Some(filter_assignees(client, project_id, ids).await.map_err(
            |error| {
                ValidationErrors::new(
                    "non_field_errors",
                    Detail::new(format!("{error:?}"), "invalid"),
                )
            },
        )?);
    }

    // Labels: labels of the project.
    if let Some(ids) = validated.uuid_list("label_ids") {
        extra.labels = Some(
            filter_labels(client, project_id, ids)
                .await
                .map_err(|error| {
                    ValidationErrors::new(
                        "non_field_errors",
                        Detail::new(format!("{error:?}"), "invalid"),
                    )
                })?,
        );
    }

    // State: `State.objects` (non-triage, non-deleted) and this project.
    if let Some(Some(state_id)) = fk_value(validated, "state", "state_id")
        && !exists(
            client,
            &queries::VALID_STATE_IN_PROJECT,
            state_id,
            project_id,
        )
        .await?
    {
        return Err(ValidationErrors::new(
            "non_field_errors",
            Detail::new("State is not valid please pass a valid state_id", "invalid"),
        ));
    }

    // Parent: `Issue.objects` (soft delete only) and this project.
    if let Some(Some(parent_id)) = fk_value(validated, "parent", "parent_id")
        && !exists(
            client,
            &queries::VALID_PARENT_IN_PROJECT,
            parent_id,
            project_id,
        )
        .await?
    {
        return Err(ValidationErrors::new(
            "non_field_errors",
            Detail::new(
                "Parent is not valid issue_id please pass a valid issue_id",
                "invalid",
            ),
        ));
    }

    // Estimate point: this project.
    if let Some(Some(estimate_id)) = fk_value(validated, "estimate_point", "estimate_point")
        && !exists(
            client,
            &queries::VALID_ESTIMATE_POINT_IN_PROJECT,
            estimate_id,
            project_id,
        )
        .await?
    {
        return Err(ValidationErrors::new(
            "non_field_errors",
            Detail::new(
                "Estimate point is not valid please pass a valid estimate_point_id",
                "invalid",
            ),
        ));
    }

    Ok(extra)
}

async fn exists(
    client: &Client,
    query: &plane_db::Query,
    id: Uuid,
    project_id: Uuid,
) -> Result<bool, ValidationErrors> {
    let statement = client.prepare_cached(query.sql).await.map_err(|error| {
        ValidationErrors::new(
            "non_field_errors",
            Detail::new(format!("{error:?}"), "invalid"),
        )
    })?;
    Ok(client
        .query_opt(&statement, &[&id, &project_id])
        .await
        .map_err(|error| {
            ValidationErrors::new(
                "non_field_errors",
                Detail::new(format!("{error:?}"), "invalid"),
            )
        })?
        .is_some())
}

async fn filter_assignees(
    client: &Client,
    project_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, ViewError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let statement = client
        .prepare_cached(queries::FILTER_ASSIGNEES.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&project_id, &ids])
        .await
        .map_err(ViewError::from)?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

async fn filter_labels(
    client: &Client,
    project_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, ViewError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let statement = client
        .prepare_cached(queries::FILTER_LABELS.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&project_id, &ids])
        .await
        .map_err(ViewError::from)?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

// ---------------------------------------------------------------------------
// Field re-checks
// ---------------------------------------------------------------------------

/// The generated validator checks a pk with `SELECT 1 FROM <table> WHERE id`,
/// but the serializer's fields use model managers, and every one of these
/// tables is soft-deletable:
///
/// * `parent`/`parent_id`: `Issue.objects` (`deleted_at IS NULL`);
/// * `label_ids` children: `Label.objects`;
/// * `estimate_point`: `EstimatePoint.objects`;
/// * `type`: `IssueType.objects`.
///
/// `state`/`state_id` are deliberately absent: the serializer uses
/// `State.all_state_objects`, which is the plain manager and includes
/// soft-deleted and triage rows (the project check in `validate()` is what
/// rejects triage).
async fn recheck_manager_fields(
    client: &Client,
    serializer: &mut Serializer<'_>,
) -> Result<(), ViewError> {
    for (field, query) in [
        ("parent_id", &queries::SOFT_DELETED_ISSUES),
        ("parent", &queries::SOFT_DELETED_ISSUES),
        ("estimate_point", &queries::SOFT_DELETED_ESTIMATE_POINTS),
        ("type", &queries::SOFT_DELETED_ISSUE_TYPES),
    ] {
        let Some(Value::Uuid(id)) = serializer.value(field).cloned() else {
            continue;
        };
        let statement = client
            .prepare_cached(query.sql)
            .await
            .map_err(ViewError::from)?;
        if client
            .query_opt(&statement, &[&vec![id]])
            .await
            .map_err(ViewError::from)?
            .is_some()
        {
            let raw = serializer.raw_text(field).unwrap_or_else(|| id.to_string());
            serializer.reject(
                field,
                Detail::new(
                    format!("Invalid pk \"{raw}\" - object does not exist."),
                    "does_not_exist",
                ),
            );
        }
    }

    let Some(Value::UuidList(ids)) = serializer.value("label_ids").cloned() else {
        return Ok(());
    };
    let statement = client
        .prepare_cached(queries::SOFT_DELETED_LABELS.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&ids])
        .await
        .map_err(ViewError::from)?;
    let deleted: HashSet<Uuid> = rows.iter().map(|row| row.get(0)).collect();
    for (index, id) in ids.iter().enumerate() {
        if deleted.contains(id) {
            serializer.reject_indexed(
                "label_ids",
                index,
                Detail::new(
                    format!("Invalid pk \"{id}\" - object does not exist."),
                    "does_not_exist",
                ),
            );
        }
    }
    Ok(())
}

/// The generated serializer's `self.fields` order (the key order
/// `serializer.errors` renders).
fn field_order() -> Vec<&'static str> {
    issue_create_serializer::FIELDS
        .iter()
        .map(|field| field.name)
        .collect()
}

// ---------------------------------------------------------------------------
// Serializer -> model
// ---------------------------------------------------------------------------

/// The field a `PrimaryKeyRelatedField` wrote: the later declared field wins
/// when a body sends both spellings (`state` over `state_id`, `parent` over
/// `parent_id`), because DRF visits `self.fields` in that order and the
/// `source` key is assigned twice.
fn fk_value(validated: &Validated, late: &str, early: &str) -> Option<Option<Uuid>> {
    if validated.contains(late) {
        return Some(validated.uuid(late));
    }
    if validated.contains(early) {
        return Some(validated.uuid(early));
    }
    None
}

fn build_issue(validated: &Validated, project_id: Uuid, extra: &Extra) -> Issue {
    let description_html = extra
        .description_html
        .clone()
        .or_else(|| validated.text("description_html").map(str::to_owned))
        .unwrap_or_else(|| hooks::issue::DEFAULT_DESCRIPTION_HTML.to_owned());
    let (parent_id, state_id, estimate_point_id, type_id) = (
        fk_value(validated, "parent", "parent_id").flatten(),
        fk_value(validated, "state", "state_id").flatten(),
        fk_value(validated, "estimate_point", "estimate_point").flatten(),
        fk_value(validated, "type", "type").flatten(),
    );
    let now = write::now();
    Issue {
        created_at: now,
        updated_at: now,
        created_by_id: None,
        updated_by_id: None,
        deleted_at: None,
        id: Uuid::new_v4(),
        project_id,
        workspace_id: Uuid::nil(),
        parent_id,
        state_id,
        point: validated.integer("point").map(|value| value as i32),
        estimate_point_id,
        name: validated
            .text("name")
            .expect("the generated validator requires name")
            .to_owned(),
        description_json: validated
            .json("description_json")
            .cloned()
            .unwrap_or_else(hooks::issue::empty_json),
        description_stripped: strip_html(&description_html),
        description_html,
        description_binary: None,
        priority: validated.text("priority").unwrap_or("none").to_owned(),
        start_date: validated.date("start_date"),
        target_date: validated.date("target_date"),
        sequence_id: 1,
        sort_order: validated.float("sort_order").unwrap_or(65_535.0),
        completed_at: None,
        archived_at: None,
        is_draft: validated.bool("is_draft").unwrap_or(false),
        external_source: validated.text("external_source").map(str::to_owned),
        external_id: validated.text("external_id").map(str::to_owned),
        type_id,
    }
}

fn apply_patch(issue: &mut Issue, validated: &Validated, extra: &Extra) {
    if validated.contains("state") || validated.contains("state_id") {
        issue.state_id = fk_value(validated, "state", "state_id").flatten();
    }
    if validated.contains("parent") || validated.contains("parent_id") {
        issue.parent_id = fk_value(validated, "parent", "parent_id").flatten();
    }
    if validated.contains("estimate_point") {
        issue.estimate_point_id = validated.uuid("estimate_point");
    }
    if validated.contains("type") {
        issue.type_id = validated.uuid("type");
    }
    if validated.contains("point") {
        issue.point = validated.integer("point").map(|value| value as i32);
    }
    if let Some(name) = validated.text("name") {
        issue.name = name.to_owned();
    }
    if let Some(json) = validated.json("description_json") {
        issue.description_json = json.clone();
    }
    if validated.contains("description_html") {
        issue.description_html = extra
            .description_html
            .clone()
            .or_else(|| validated.text("description_html").map(str::to_owned))
            .unwrap_or_default();
    }
    if let Some(priority) = validated.text("priority") {
        issue.priority = priority.to_owned();
    }
    if validated.contains("start_date") {
        issue.start_date = validated.date("start_date");
    }
    if validated.contains("target_date") {
        issue.target_date = validated.date("target_date");
    }
    if let Some(sort_order) = validated.float("sort_order") {
        issue.sort_order = sort_order;
    }
    if validated.contains("external_source") {
        issue.external_source = validated.text("external_source").map(str::to_owned);
    }
    if validated.contains("external_id") {
        issue.external_id = validated.text("external_id").map(str::to_owned);
    }
}

/// `Issue.save()`'s H10a on the update-shaped paths, returning whether the
/// state changed (the H10b input).
async fn ensure_state_for_save(client: &Client, issue: &mut Issue) -> Result<bool, ViewError> {
    let old = issue.state_id;
    if issue.state_id.is_none() {
        issue.state_id = hooks::issue::ensure_default_state(client, issue.project_id).await?;
    }
    Ok(issue.state_id != old)
}

/// `strip_tags(description_html)` with the `"" -> None` rule from H12.
fn strip_html(html: &str) -> Option<String> {
    if html.is_empty() {
        None
    } else {
        Some(hooks::html::strip_tags(html))
    }
}

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

struct ProjectForCreate {
    default_assignee_id: Option<Uuid>,
}

async fn project_for_create(
    client: &Client,
    project_id: Uuid,
) -> Result<ProjectForCreate, ViewError> {
    let statement = client
        .prepare_cached(queries::PROJECT_FOR_ISSUE_CREATE.sql)
        .await
        .map_err(ViewError::from)?;
    let Some(row) = client
        .query_opt(&statement, &[&project_id])
        .await
        .map_err(ViewError::from)?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok(ProjectForCreate {
        default_assignee_id: row.try_get(1).map_err(ViewError::from)?,
    })
}

async fn load_full_issue(
    client: &Client,
    project_id: Uuid,
    slug: &str,
    pk: Uuid,
) -> Result<Option<Issue>, ViewError> {
    let statement = client
        .prepare_cached(queries::ISSUE_FOR_PATCH.sql)
        .await
        .map_err(ViewError::from)?;
    match client
        .query_opt(&statement, &[&project_id, &slug, &pk])
        .await
        .map_err(ViewError::from)?
    {
        Some(row) => Ok(Some(Issue::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Task publishing
// ---------------------------------------------------------------------------

pub(crate) async fn publish_issue_activity(
    ctx: &ViewCtx<'_>,
    call: IssueActivityCall,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    let message = tasks::issue_activity(call);
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
    Ok(())
}

pub(crate) fn activity_base(
    activity_type: &str,
    requested_data: TaskArg,
    actor_id: Uuid,
    issue_id: &str,
    project_id: &str,
    current_instance: TaskArg,
) -> IssueActivityBase {
    activity_base_at(
        activity_type,
        requested_data,
        actor_id,
        issue_id,
        project_id,
        current_instance,
        Utc::now().timestamp(),
    )
}

fn activity_base_at(
    activity_type: &str,
    requested_data: TaskArg,
    actor_id: Uuid,
    issue_id: &str,
    project_id: &str,
    current_instance: TaskArg,
    epoch: i64,
) -> IssueActivityBase {
    IssueActivityBase::new(
        activity_type,
        requested_data,
        actor_id.to_string(),
        TaskArg::str(issue_id),
        TaskArg::str(project_id),
        current_instance,
        epoch,
    )
}

#[allow(clippy::too_many_arguments)]
async fn publish_model_activity(
    ctx: &ViewCtx<'_>,
    model_name: &str,
    model_id: &str,
    requested_data: TaskArg,
    current_instance: Option<String>,
    actor_id: Uuid,
    slug: &str,
    origin: &str,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::MODEL_ACTIVITY;
    let message = tasks::model_activity(ModelActivityCall {
        model_name: model_name.to_owned(),
        model_id: model_id.to_owned(),
        requested_data,
        current_instance,
        actor_id,
        slug: slug.to_owned(),
        origin: origin.to_owned(),
    });
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
    Ok(())
}

async fn publish_description_version(
    ctx: &ViewCtx<'_>,
    updated_issue: TaskArg,
    issue_id: &str,
    user_id: Uuid,
    is_creating: bool,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::ISSUE_DESCRIPTION_VERSION_TASK;
    let message = tasks::issue_description_version_task(
        updated_issue,
        issue_id.to_owned(),
        user_id,
        is_creating,
    );
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Request bodies and small helpers
// ---------------------------------------------------------------------------

fn kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<String, ViewError> {
    ctx.kwarg(name).map(str::to_owned).ok_or(ViewError::Key)
}

fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}

fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    let value = ctx.kwarg(name).ok_or(ViewError::Key)?;
    lookup::uuid_kwarg(name, value).map_err(ViewError::from)
}

fn ok_json(document: Doc) -> ViewResponse {
    ViewResponse::raw(200, APPLICATION_JSON, document.to_drf())
}

fn issue_hook_error(error: IssueHookError) -> ViewError {
    match error {
        IssueHookError::ProjectNotFound => ViewError::ObjectDoesNotExist,
        other => ViewError::Unknown(other.to_string()),
    }
}

/// `request.data.get("issue_ids", [])` for the two bulk endpoints.
///
/// The Python then calls `len(issue_ids)`; `None` and scalars raise there
/// (500), while a string is iterated by `pk__in` and fails UUID conversion
/// (400). We reproduce those outcomes without inventing a different one.
fn issue_ids_body(ctx: &ViewCtx<'_>) -> Result<Vec<Uuid>, ViewError> {
    let present = body_has(ctx, "issue_ids")?;
    let Some(value) = write_support::body_get(ctx, "issue_ids")? else {
        if present {
            return Err(ViewError::Unknown(
                "TypeError: object of type 'NoneType' has no len()".to_owned(),
            ));
        }
        return Ok(Vec::new());
    };
    match &value {
        PValue::List(items) => parse_uuid_items(items),
        PValue::Str(text) => {
            // `len(str)` is non-zero; `pk__in="…"` then converts each character.
            if text.as_str().is_none_or(str::is_empty) {
                Ok(Vec::new())
            } else {
                Err(ViewError::Validation)
            }
        }
        other => Err(ViewError::Unknown(format!(
            "TypeError: object of type '{}' has no len()",
            python_type(other)
        ))),
    }
}

fn parse_uuid_items(items: &[PValue]) -> Result<Vec<Uuid>, ViewError> {
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        match item {
            PValue::Str(text) => {
                let text = text.as_str().unwrap_or_default();
                ids.push(lookup::uuid_kwarg("id", text).map_err(ViewError::from)?);
            }
            PValue::Null => {}
            other => {
                return Err(ViewError::Unknown(format!(
                    "AttributeError: '{}' object has no attribute 'replace'",
                    python_type(other)
                )));
            }
        }
    }
    Ok(ids)
}

/// `request.data.get("updates", [])`, then `[update["id"] for update in updates]`.
fn updates_body(ctx: &ViewCtx<'_>) -> Result<Vec<PValue>, ViewError> {
    let present = body_has(ctx, "updates")?;
    let Some(value) = write_support::body_get(ctx, "updates")? else {
        if present {
            return Err(ViewError::Unknown(
                "TypeError: 'NoneType' object is not iterable".to_owned(),
            ));
        }
        return Ok(Vec::new());
    };
    match &value {
        PValue::List(items) => Ok(items.clone()),
        other => Err(ViewError::Unknown(format!(
            "TypeError: '{}' object is not iterable",
            python_type(other)
        ))),
    }
}

fn update_ids(updates: &[PValue]) -> Result<Vec<Uuid>, ViewError> {
    let mut ids = Vec::with_capacity(updates.len());
    let mut seen = HashSet::new();
    for update in updates {
        let PValue::Dict(object) = update else {
            return Err(ViewError::Unknown(
                "TypeError: update is not a mapping".to_owned(),
            ));
        };
        let Some(value) = object.get("id") else {
            return Err(ViewError::Key);
        };
        match value {
            PValue::Str(text) => {
                let text = text.as_str().unwrap_or_default();
                let id = lookup::uuid_kwarg("id", text).map_err(ViewError::from)?;
                if seen.insert(id) {
                    ids.push(id);
                }
            }
            PValue::Null => {}
            other => {
                return Err(ViewError::Unknown(format!(
                    "AttributeError: '{}' object has no attribute 'replace'",
                    python_type(other)
                )));
            }
        }
    }
    Ok(ids)
}

fn body_has(ctx: &ViewCtx<'_>, key: &str) -> Result<bool, ViewError> {
    match ctx.data()? {
        Data::Json(PValue::Dict(object)) => Ok(object.contains_key(key)),
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: request.data is not a mapping".to_owned(),
        )),
        Data::Form(query) => Ok(query.contains_key(key)),
    }
}

fn python_type(value: &PValue) -> &'static str {
    match value {
        PValue::Null => "NoneType",
        PValue::Bool(_) => "bool",
        PValue::Int(_) => "int",
        PValue::Float(_) => "float",
        PValue::Str(_) => "str",
        PValue::List(_) => "list",
        PValue::Dict(_) => "dict",
    }
}

/// `IssueBulkUpdateDateEndpoint.validate_dates` (`base.py:1099-1117`).
///
/// `new_start or current_start` uses Python truthiness; a non-empty string is
/// parsed with `datetime.strptime("%Y-%m-%d").date()` (a `ValueError` is an
/// uncaught 500), then the two dates are compared.
fn validate_dates(
    current_start: Option<NaiveDate>,
    current_target: Option<NaiveDate>,
    new_start: Option<&PValue>,
    new_target: Option<&PValue>,
) -> Result<bool, ViewError> {
    let start = match new_start.filter(|value| write_support::truthy(value)) {
        Some(value) => parse_activity_date(Some(value), "start_date")?,
        None => current_start,
    };
    let target = match new_target.filter(|value| write_support::truthy(value)) {
        Some(value) => parse_activity_date(Some(value), "target_date")?,
        None => current_target,
    };
    Ok(!matches!((start, target), (Some(start), Some(target)) if start > target))
}

/// `datetime.strptime(value, "%Y-%m-%d").date()` for the raw date value.
fn parse_activity_date(
    value: Option<&PValue>,
    field: &str,
) -> Result<Option<NaiveDate>, ViewError> {
    match value {
        None | Some(PValue::Null) => Ok(None),
        Some(PValue::Str(text)) if text.as_str().is_some_and(str::is_empty) => Ok(None),
        Some(PValue::Str(text)) => {
            let text = text.as_str().unwrap_or_default();
            NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .map(Some)
                .map_err(|_| {
                    ViewError::Unknown(format!(
                        "ValueError: time data {text:?} does not match format '%Y-%m-%d' for {field}"
                    ))
                })
        }
        Some(other) => Err(ViewError::Unknown(format!(
            "TypeError: strptime() argument 1 must be str, not {}",
            python_type(other)
        ))),
    }
}

/// `json.dumps({"<field>": str(issue.<field>)})`.
fn dumped_date(value: Option<NaiveDate>, field: &str) -> Result<String, ViewError> {
    write_support::dumps(&PValue::Dict(
        [(
            PyStr::new(field),
            PValue::Str(PyStr::from_string(
                value.map_or_else(|| "None".to_owned(), |value| value.to_string()),
            )),
        )]
        .into_iter()
        .collect(),
    ))
}

fn remember(touched: &mut Vec<Uuid>, seen: &mut HashSet<Uuid>, id: Uuid) {
    if seen.insert(id) {
        touched.push(id);
    }
}
