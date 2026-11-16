// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the work-item sub-resource write handlers
//! (`plans/WORK_ITEMS.md` §3, set C): links, relations, subscribers and the
//! sub-issue re-parent.
//!
//! Every statement is declared with [`plane_query!`](plane_db::plane_query) so
//! `tests/schema_check.rs` checks its columns against a migrated database. The
//! column lists and filters are derived from the ORM SQL each Python view
//! emits; every statement names the line it reproduces.
//!
//! The managers decide most of the predicates and they differ per call site:
//!
//! * `Issue.objects` is `SoftDeletionManager` — `deleted_at IS NULL` only
//!   (`plane/db/mixins.py:53-56`), used by the link create, the subscriber
//!   create and `subscribe` (`Issue.objects.get(...)`);
//! * `Issue.issue_objects` is `IssueManager` — additionally excludes triage
//!   states, archived/draft issues and archived projects
//!   (`plane/db/models/issue.py:92-101`), used by `issue_in_project` on the
//!   relation writes and by the sub-issues handler.

use plane_db::plane_query;

// ---------------------------------------------------------------------------
// Issue lookups
// ---------------------------------------------------------------------------

plane_query! {
    /// `Issue.objects.get(pk=issue_id, workspace__slug=slug, project_id=…)`
    /// (`plane/app/views/issue/link.py:57`, `subscriber.py:36,104`,
    /// `subscribe`): the soft-delete manager, no `issue_objects` exclusions —
    /// an archived, draft or triage issue is reachable.
    ISSUE_ANY_IN_PROJECT = "issues_write_issue_any_in_project",
    "SELECT EXISTS (SELECT 1 FROM issues i \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND i.id = $1 AND i.project_id = $2 AND w.slug = $3) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `issue_in_project(issue_id, slug, project_id)` without
    /// `include_archived=True` (`plane/app/views/issue/relation.py:34-56,265`):
    /// `Issue.issue_objects.filter(pk, workspace__slug, project_id).exists()`.
    ISSUE_LIVE_IN_PROJECT = "issues_write_issue_live_in_project",
    "SELECT EXISTS (SELECT 1 FROM issues i \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     INNER JOIN projects p ON p.id = i.project_id \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND (s.group IS NULL OR s.group <> 'triage') \
       AND i.archived_at IS NULL AND p.archived_at IS NULL AND NOT i.is_draft \
       AND i.id = $1 AND i.project_id = $2 AND w.slug = $3) AS exists",
    [("exists", Bool)]
}

// ---------------------------------------------------------------------------
// Issue links (routes 74/75)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueLink.objects.get(workspace__slug=…, project_id=…, issue_id=…,
    /// pk=…)` (`plane/app/views/issue/link.py:83,113`) with the nested
    /// `created_by_detail` the serializer's `current_instance` needs
    /// (`IssueLinkSerializer`, `plane/app/serializers/issue.py:560-575`).
    ///
    /// Unlike the read queryset this has no membership or archived-project
    /// filter, because the view fetches the row from the default manager.
    LINK_BY_PK = "issues_write_link_by_pk",
    "SELECT l.created_at, l.updated_at, l.created_by_id, l.updated_by_id, l.deleted_at, l.id, \
     l.project_id, l.workspace_id, l.title, l.url, l.issue_id, l.metadata, \
     u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, \
     ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug \
     FROM issue_links l \
     INNER JOIN workspaces w ON w.id = l.workspace_id \
     LEFT JOIN users u ON u.id = l.created_by_id \
     LEFT JOIN file_assets ua ON ua.id = u.avatar_asset_id \
     LEFT JOIN workspaces uaw ON uaw.id = ua.workspace_id \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND l.issue_id = $3 AND l.id = $4",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("title", Varchar),
        ("url", Text),
        ("issue_id", Uuid),
        ("metadata", Jsonb),
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("avatar", Text),
        ("is_bot", Bool),
        ("display_name", Varchar),
        ("id", Uuid),
        ("entity_type", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("slug", Varchar),
    ],
}

plane_query! {
    /// `IssueLinkViewSet.create`'s duplicate check
    /// (`plane/app/serializers/issue.py:596-599`):
    /// `IssueLink.objects.filter(url=…, issue_id=…).exists()`.
    LINK_URL_TAKEN = "issues_write_link_url_taken",
    "SELECT EXISTS (SELECT 1 FROM issue_links l \
     WHERE l.deleted_at IS NULL AND l.url = $1 AND l.issue_id = $2) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `IssueLinkSerializer.update`'s duplicate check
    /// (`plane/app/serializers/issue.py:601-609`): the same filter
    /// `.exclude(pk=instance.id)`.
    LINK_URL_TAKEN_ELSEWHERE = "issues_write_link_url_taken_elsewhere",
    "SELECT EXISTS (SELECT 1 FROM issue_links l \
     WHERE l.deleted_at IS NULL AND l.url = $1 AND l.issue_id = $2 AND l.id <> $3) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `IssueLink.objects.create(**validated_data)` plus
    /// `ProjectBaseModel.save`'s `workspace = project.workspace`
    /// (`plane/db/models/project.py:237-239`) and `BaseModel.save`'s audit
    /// stamps (`plane/db/models/base.py:23-45`).
    LINK_INSERT = "issues_write_link_insert",
    "INSERT INTO issue_links \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, project_id, \
      workspace_id, title, url, issue_id, metadata) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    []
}

plane_query! {
    /// `IssueLinkSerializer.update` → `instance.save()`: the four writable
    /// columns plus `BaseModel.save`'s `updated_by` and the `auto_now`
    /// `updated_at` (`plane/db/models/base.py:23-45`,
    /// `plane/db/mixins.py:12-21`).
    LINK_UPDATE = "issues_write_link_update",
    "UPDATE issue_links SET title = $1, url = $2, metadata = $3, deleted_at = $4, \
     updated_at = $5, updated_by_id = $6 WHERE id = $7",
    []
}

// ---------------------------------------------------------------------------
// Issue relations (routes 93/94)
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.get(pk=project_id)` (`relation.py:259`): the project's
    /// `workspace_id` for the inserted rows' `WorkspaceBaseModel.save`.
    PROJECT_WORKSPACE = "issues_write_project_workspace",
    "SELECT p.workspace_id FROM projects p WHERE p.deleted_at IS NULL AND p.id = $1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// `Project.objects.get(pk=project_id).workspace` through the FK's base
    /// manager (`WorkspaceBaseModel.save`, `plane/db/models/project.py:237-239`),
    /// which has **no** soft-delete filter: a subscriber row can be written
    /// against a soft-deleted project.
    PROJECT_WORKSPACE_ANY = "issues_write_project_workspace_any",
    "SELECT p.workspace_id FROM projects p WHERE p.id = $1",
    [("workspace_id", Uuid)]
}

plane_query! {
    /// The body `issues` list (`relation.py:270-277`):
    /// `Issue.issue_objects.filter(workspace__slug=slug, pk__in=issues)
    /// .values_list("id", flat=True)`, ordered by `Issue.Meta.ordering`
    /// (`-created_at`, `plane/db/models/issue.py:178`).
    ///
    /// The five sibling columns feed the response serializers, whose fields
    /// come from the same issue row (`RelatedIssueSerializer` and
    /// `IssueRelationSerializer`, `plane/app/serializers/issue.py:412-486`).
    RELATION_ISSUES = "issues_write_relation_issues",
    "SELECT i.id, i.project_id, i.sequence_id, i.name, i.state_id, i.priority \
     FROM issues i \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     INNER JOIN projects p ON p.id = i.project_id \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND (s.group IS NULL OR s.group <> 'triage') \
       AND i.archived_at IS NULL AND p.archived_at IS NULL AND NOT i.is_draft \
       AND w.slug = $1 AND i.id = ANY($2) \
     ORDER BY i.created_at DESC",
    [
        ("id", Uuid),
        ("project_id", Uuid),
        ("sequence_id", Int4),
        ("name", Varchar),
        ("state_id", Uuid),
        ("priority", Varchar),
    ],
}

plane_query! {
    /// The URL issue's relation-target columns for
    /// `IssueRelationSerializer` (`relation.py:246-289`, non-blocking reply)
    /// and for `remove_relation`'s `current_instance` (`:336`).
    ///
    /// `IssueRelationSerializer.id`/`project_id`/… read
    /// `related_issue.<column>`; Django resolves the FK through the base
    /// manager, so no `issue_objects` predicate applies here — the caller has
    /// already run `issue_in_project` where it matters.
    ISSUE_RELATION_REF = "issues_write_issue_relation_ref",
    "SELECT i.id, i.project_id, i.sequence_id, i.name, i.state_id, i.priority \
     FROM issues i WHERE i.id = $1",
    [
        ("id", Uuid),
        ("project_id", Uuid),
        ("sequence_id", Int4),
        ("name", Varchar),
        ("state_id", Uuid),
        ("priority", Varchar),
    ],
}

plane_query! {
    /// `IssueRelation.objects.bulk_create([…], batch_size=10,
    /// ignore_conflicts=True)` (`relation.py:279-302`).
    ///
    /// `ignore_conflicts` is `ON CONFLICT DO NOTHING` (any unique violation,
    /// here the partial `(issue, related_issue)` constraint); `bulk_create`
    /// still returns every object it was given, including skipped rows.
    RELATION_INSERT = "issues_write_relation_insert",
    "INSERT INTO issue_relations \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, project_id, \
      workspace_id, issue_id, related_issue_id, relation_type) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10) \
     ON CONFLICT DO NOTHING",
    []
}

plane_query! {
    /// `remove_relation`'s two-direction lookup (`relation.py:322-332`):
    /// `IssueRelation.objects.filter(workspace__slug=slug).filter(
    /// Q(issue_id=related, related_issue_id=issue) |
    /// Q(issue_id=issue, related_issue_id=related)).first()` — `.first()` with
    /// `IssueRelation.Meta.ordering = ("-created_at",)`
    /// (`plane/db/models/issue.py:317`).
    RELATION_LOOKUP = "issues_write_relation_lookup",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, r.deleted_at, \
     r.id, r.project_id, r.workspace_id, r.issue_id, r.related_issue_id, r.relation_type, \
     i.id, i.project_id, i.sequence_id, i.name, i.state_id, i.priority \
     FROM issue_relations r \
     INNER JOIN workspaces w ON w.id = r.workspace_id \
     INNER JOIN issues i ON i.id = r.related_issue_id \
     WHERE r.deleted_at IS NULL AND w.slug = $1 \
       AND ((r.issue_id = $2 AND r.related_issue_id = $3) \
         OR (r.issue_id = $3 AND r.related_issue_id = $2)) \
     ORDER BY r.created_at DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("issue_id", Uuid),
        ("related_issue_id", Uuid),
        ("relation_type", Varchar),
        ("id", Uuid),
        ("project_id", Uuid),
        ("sequence_id", Int4),
        ("name", Varchar),
        ("state_id", Uuid),
        ("priority", Varchar),
    ],
}

// ---------------------------------------------------------------------------
// Issue subscribers (routes 83/84/85)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueSubscriberViewSet.perform_create`'s membership check
    /// (`subscriber.py:41-48`): an active `ProjectMember` on the URL project.
    PROJECT_MEMBER_ACTIVE = "issues_write_project_member_active",
    "SELECT EXISTS (SELECT 1 FROM project_members pm \
     INNER JOIN workspaces w ON w.id = pm.workspace_id \
     WHERE pm.deleted_at IS NULL AND w.slug = $1 AND pm.project_id = $2 \
       AND pm.member_id = $3 AND pm.is_active) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `IssueSubscriber.objects.create(issue_id, subscriber_id, project_id)`
    /// plus `ProjectBaseModel.save`'s workspace and `BaseModel.save`'s audit
    /// stamps (`subscriber.py:112`).
    SUBSCRIBER_INSERT = "issues_write_subscriber_insert",
    "INSERT INTO issue_subscribers \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, project_id, \
      workspace_id, issue_id, subscriber_id) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9)",
    []
}

plane_query! {
    /// `IssueSubscriber.objects.get(project=…, subscriber=…,
    /// workspace__slug=…, issue=…)`:
    ///
    /// * route 84 (`subscriber.py:87-95`) — `subscriber` is the URL
    ///   `subscriber_id`;
    /// * `unsubscribe` (`subscriber.py:119-125`) — `subscriber` is the caller.
    ///
    /// One column is enough: the row is soft-deleted by primary key.
    SUBSCRIBER_BY_SUBSCRIBER = "issues_write_subscriber_by_subscriber",
    "SELECT s.id FROM issue_subscribers s \
     INNER JOIN workspaces w ON w.id = s.workspace_id \
     WHERE s.deleted_at IS NULL AND s.project_id = $1 AND s.subscriber_id = $2 \
       AND w.slug = $3 AND s.issue_id = $4",
    [("id", Uuid)]
}

plane_query! {
    /// `subscribe`'s duplicate guard (`subscriber.py:105-113`):
    /// `IssueSubscriber.objects.filter(issue_id, subscriber=request.user,
    /// workspace__slug, project_id).exists()`.
    SUBSCRIBER_EXISTS_FOR_USER = "issues_write_subscriber_exists_for_user",
    "SELECT EXISTS (SELECT 1 FROM issue_subscribers s \
     INNER JOIN workspaces w ON w.id = s.workspace_id \
     WHERE s.deleted_at IS NULL AND s.issue_id = $1 AND s.subscriber_id = $2 \
       AND w.slug = $3 AND s.project_id = $4) AS exists",
    [("exists", Bool)]
}

// ---------------------------------------------------------------------------
// Sub-issues (route 73)
// ---------------------------------------------------------------------------

plane_query! {
    /// The project-scoped sub-issues the POST re-parents (`sub_issue.py:229-232`):
    /// `Issue.issue_objects.filter(id__in=sub_issue_ids, workspace__slug=slug,
    /// project_id=…)`, ordered by `Issue.Meta.ordering` — the order the
    /// activities are published in.
    SUB_ISSUE_IDS = "issues_write_sub_issue_ids",
    "SELECT i.id FROM issues i \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     INNER JOIN projects p ON p.id = i.project_id \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND (s.group IS NULL OR s.group <> 'triage') \
       AND i.archived_at IS NULL AND p.archived_at IS NULL AND NOT i.is_draft \
       AND i.project_id = $1 AND w.slug = $2 AND i.id = ANY($3) \
     ORDER BY i.created_at DESC",
    [("id", Uuid)]
}

plane_query! {
    /// `Issue.objects.bulk_update(sub_issues, ["parent"], batch_size=10)`
    /// (`sub_issue.py:236`): one column, no `updated_at` (Django's
    /// `bulk_update` runs no `pre_save` for it) and no audit stamp.
    ///
    /// The manager is `Issue.objects`, so the statement keeps
    /// `deleted_at IS NULL`; every id was just selected through
    /// `issue_objects`, so the batches Django would split cannot differ here.
    SUB_ISSUE_SET_PARENT = "issues_write_sub_issue_set_parent",
    "UPDATE issues SET parent_id = $1 WHERE deleted_at IS NULL AND id = ANY($2)",
    []
}

plane_query! {
    /// The response's `updated_sub_issues` (`sub_issue.py:238-240`):
    /// `Issue.issue_objects.filter(id__in=scoped_sub_issue_ids,
    /// workspace__slug=slug, project_id=project_id)
    /// .annotate(state_group=F("state__group"))`.
    ///
    /// `IssueSerializer` (not the detail one) is the serializer, so this
    /// projects the model columns and the `state_group` annotation; the
    /// declared computed fields (`cycle_id`, the m2m arrays and the three
    /// counts) are absent on the queryset and DRF skips them.
    SUB_ISSUES = "issues_write_sub_issues",
    "SELECT i.created_at, i.updated_at, i.created_by_id, i.updated_by_id, i.deleted_at, \
     i.id, i.project_id, i.workspace_id, i.parent_id, i.state_id, i.point, \
     i.estimate_point_id, i.name, i.description_json, i.description_html, \
     i.description_stripped, i.description_binary, i.priority, i.start_date, \
     i.target_date, i.sequence_id, i.sort_order, i.completed_at, i.archived_at, \
     i.is_draft, i.external_source, i.external_id, i.type_id, s.group AS state_group \
     FROM issues i \
     LEFT OUTER JOIN states s ON s.id = i.state_id \
     INNER JOIN projects p ON p.id = i.project_id \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND (s.group IS NULL OR s.group <> 'triage') \
       AND i.archived_at IS NULL AND p.archived_at IS NULL AND NOT i.is_draft \
       AND i.project_id = $1 AND w.slug = $2 AND i.id = ANY($3) \
     ORDER BY i.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("parent_id", Uuid),
        ("state_id", Uuid),
        ("point", Int4),
        ("estimate_point_id", Uuid),
        ("name", Varchar),
        ("description_json", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("sequence_id", Int4),
        ("sort_order", Float8),
        ("completed_at", Timestamptz),
        ("archived_at", Date),
        ("is_draft", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("type_id", Uuid),
        ("state_group", Varchar),
    ],
}

// ---------------------------------------------------------------------------
// Shared plumbing for the set C write handlers
// ---------------------------------------------------------------------------

use chrono_tz::Tz;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;
use plane_jobs::celery::args::TaskArg;
use plane_query::json::Json;

/// `json.dumps(value, cls=DjangoJSONEncoder)` — the JSON dialect the activity
/// call sites use for `requested_data` / `current_instance` strings
/// (`", "`/`": "` separators, `ensure_ascii=True`).
///
/// # Errors
///
/// A value the encoder refuses.
pub(crate) fn django_json(value: &Json) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes)
        .map_err(|error| ViewError::Unknown(format!("json.dumps produced non-UTF-8: {error}")))
}

/// `json.dumps(request.data, cls=DjangoJSONEncoder)` at the two call sites that
/// publish the raw body (`link.py:85`, `relation.py:337`) as a `str` argument.
///
/// A form body is a Django `QueryDict`, which `json.dumps` refuses — the
/// family handler answers 500 (`TypeError`), as [`write::request_data_arg`]
/// documents for the same reason.
///
/// [`write::request_data_arg`]: crate::write::request_data_arg
///
/// # Errors
///
/// [`ViewError::Unknown`] for a form body or a value the encoder refuses.
pub(crate) fn request_data_arg(ctx: &ViewCtx<'_>) -> Result<TaskArg, ViewError> {
    match ctx.data()? {
        plane_drf::parsers::Data::Json(value) => {
            let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &value)
                .map_err(|error| ViewError::Unknown(error.to_string()))?;
            let text = String::from_utf8(bytes).map_err(|error| {
                ViewError::Unknown(format!("json.dumps produced non-UTF-8: {error}"))
            })?;
            Ok(TaskArg::json_text(text))
        }
        plane_drf::parsers::Data::Form(_) => Err(ViewError::Unknown(
            "TypeError: Object of type QueryDict is not JSON serializable".to_owned(),
        )),
    }
}

/// `base_host(request=request, is_app=True)` (`plane/utils/host.py:17-57`).
///
/// # Errors
///
/// A missing setting, as the Python's `KeyError`/`AttributeError` would be.
pub(crate) fn base_host(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// A serialized `DateTimeField` value: DRF's `isoformat()` in the active
/// timezone, with `+00:00` written as `Z` (`rest_framework/fields.py:1270-1340`).
pub(crate) fn drf_datetime(value: chrono::DateTime<chrono::Utc>, timezone: Tz) -> Json {
    Json::DateTimeZoned(value.with_timezone(&timezone).fixed_offset())
}

/// A `jsonb` column as the parsed document Django renders: `jsonb` re-orders
/// keys (length-then-bytes), and both the response and `current_instance` read
/// the row back from the database, so the order is the column's, not the
/// request body's.
pub(crate) fn json_column(value: &JsonText) -> Result<Json, ViewError> {
    let parsed = plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(pyjson_to_json(&parsed))
}

/// `json` value tree → the order-preserving document. Numbers outside `i64`
/// become floats (a `jsonb` column cannot hold them either way).
fn pyjson_to_json(value: &plane_serde::pyjson::Value) -> Json {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Null => Json::Null,
        Py::Bool(flag) => Json::Bool(*flag),
        Py::Int(number) => number.as_i64().map_or_else(
            || Json::Float(number.to_string().parse().unwrap_or_default()),
            Json::Int,
        ),
        Py::Float(number) => Json::Float(*number),
        Py::Str(text) => Json::string(text.as_str().unwrap_or_default()),
        Py::List(items) => Json::Array(items.iter().map(pyjson_to_json).collect()),
        Py::Dict(object) => Json::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str().unwrap_or_default().to_owned(),
                        pyjson_to_json(value),
                    )
                })
                .collect(),
        ),
    }
}

/// The catalog entry `tests/schema_check.rs` verifies against a migrated
/// database.
#[allow(dead_code)]
pub static ALL_QUERIES: &[&plane_db::Query] = &[
    &ISSUE_ANY_IN_PROJECT,
    &ISSUE_LIVE_IN_PROJECT,
    &LINK_BY_PK,
    &LINK_URL_TAKEN,
    &LINK_URL_TAKEN_ELSEWHERE,
    &LINK_INSERT,
    &LINK_UPDATE,
    &PROJECT_WORKSPACE,
    &PROJECT_WORKSPACE_ANY,
    &RELATION_ISSUES,
    &ISSUE_RELATION_REF,
    &RELATION_INSERT,
    &RELATION_LOOKUP,
    &PROJECT_MEMBER_ACTIVE,
    &SUBSCRIBER_INSERT,
    &SUBSCRIBER_BY_SUBSCRIBER,
    &SUBSCRIBER_EXISTS_FOR_USER,
    &SUB_ISSUE_IDS,
    &SUB_ISSUE_SET_PARENT,
    &SUB_ISSUES,
];

/// `request.data.get(name)` with the JSON shape kept: `Data::get` only returns
/// strings, and these handlers must tell an absent key from `null` from a
/// non-string value (the serializer's and the ORM's failure modes differ).
pub(crate) enum RawField<'a> {
    /// The key is not in the body (or the body is not an object).
    Absent,
    /// An explicit JSON `null` (impossible in a form body).
    Null,
    /// A string value.
    Text(&'a str),
    /// Any other JSON value; serializers reject most of these.
    Other(&'a plane_serde::pyjson::Value),
}

/// See [`RawField`].
pub(crate) fn raw_field<'a>(data: &'a plane_drf::parsers::Data, name: &str) -> RawField<'a> {
    use plane_drf::parsers::Data;
    use plane_serde::pyjson::Value as Py;
    match data {
        Data::Form(query) => query.get(name).map_or(RawField::Absent, RawField::Text),
        Data::Json(object) => match object.get(name) {
            None => RawField::Absent,
            Some(Py::Null) => RawField::Null,
            Some(Py::Str(text)) => text.as_str().map_or_else(
                || RawField::Other(object.get(name).expect("the key was just read")),
                RawField::Text,
            ),
            Some(other) => RawField::Other(other),
        },
    }
}
