// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IntakeIssueViewSet.partial_update` — `PATCH
//! /api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/` (order 56)
//! and its `inbox-issues/` alias (order 60). `{pk}` is the **issue** id: the
//! link is looked up by `issue_id`.
//!
//! Python: `plane/app/views/intake/base.py:335-505`, with the work spread over
//! `plane/app/serializers/intake.py` (`IntakeIssueSerializer.validate`/`update`,
//! `:44-91`), `plane/app/serializers/issue.py` (`IssueCreateSerializer`,
//! `:82-340`, bound with `allow_triage_state=True`), `plane/db/models/intake.py`
//! (H16, `:100-109`), `plane/db/models/issue.py` (H10a/H10b/H12/H01 on the
//! accept move) and `plane/utils/timezone_converter.py` (`snooze_day_start`,
//! `:103-112`).
//!
//! ## The order, which is the behaviour (`plans/INTAKE.md` §4)
//!
//! ```text
//!  allow_permission([ADMIN], creator=True, model=Issue)   ← the runtime's
//!  skip_activity = request.data.pop("skip_activity", False)
//!  is_description_update = request.data.get("description_html") is not None
//!  Intake.objects.filter(workspace__slug, project_id).first()      → NULL → 404
//!  IntakeIssue.objects.get(issue_id=pk, …, intake_id=intake_id)     → 404
//!  ProjectMember role │ WorkspaceMember admin                       (403)
//!  project guest and not the link's creator                         (400)
//!  issue_data = request.data.pop("issue", False)
//!  if truthy: Issue.objects.annotate(label_ids, assignee_ids).get(…) → 404
//!             (a project guest → the three-key projection)
//!             IssueDetailSerializer(issue).data   (current_instance)
//!             IssueCreateSerializer(issue, data, partial=True, allow_triage)
//!             writable_data(serializer, issue_data)  (500 if not a mapping)
//!             is_valid()                                          → 400
//!  if project role > MEMBER or workspace admin:
//!             IntakeIssueSerializer(link).data    (current_instance)
//!             IntakeIssueSerializer(link, data=request.data, partial=True)
//!             is_valid()                                          → 400
//!  issue_serializer.save() + issue.activity.updated + description version
//!     (both skipped by `skip_activity and is_description_update`)
//!  intake_serializer.save() (H16; on status == 1 the triage→default move)
//!     + intake.activity.created (notification=False)
//!  re-fetch (the retrieve queryset) → 200
//! ```
//!
//! There is **no** transaction (`base.py` has no `atomic`), so a failure after
//! the issue save leaves the issue write and its activity committed.
//!
//! ## Upstream quirks ported as they are
//!
//! * the nested issue serializer is validated first, so **its** errors win over
//!   the intake serializer's, and a falsy `issue` skips it entirely (`:385`);
//! * `skip_activity` only suppresses the three *issue* publishes; the
//!   `intake.activity.created` message is always sent;
//! * `is_description_update` reads the **top-level** `description_html`, which
//!   is a field of neither serializer, so `{"skip_activity": true,
//!   "description_html": "x"}` suppresses the issue activity while editing
//!   nothing;
//! * a project guest (role ≤ 5) that reaches the view — i.e. the issue's
//!   creator — has `issue_data` reduced to `name`/`description_html`/
//!   `description_json`, and the reduction also applies to a workspace admin
//!   whose project role is guest (`:405-410`, no `not is_workspace_admin`);
//! * `IntakeIssueSerializer.created_by` is writable, so a PATCH can rewrite the
//!   creator the guest check reads;
//! * a truthy non-mapping `issue` is a **500** (`issue_data.get` for a guest,
//!   `writable_data`'s `.items()` otherwise), not the nested serializer's 400;
//! * the accept move publishes no activity and no webhook: it is a bare
//!   `issue.save()`, so only the issue row changes (H10a/H10b/H12/H01).
//!
//! ## `snoozed_till` three times
//!
//! DRF parses the body value in the **user's** timezone
//! (`DateTimeField.enforce_timezone`; a bare `YYYY-MM-DD` is midnight, because
//! CPython 3.11's `fromisoformat` accepts it), then
//! `IntakeIssueSerializer.validate` replaces a bare date with midnight in the
//! **project's** timezone (`snooze_day_start`), and H16 floors the resulting
//! instant to midnight of its project-local day inside `save()`. `null` skips
//! all three.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::hooks;
use plane_db::hooks::intake_issue;
use plane_db::models::intake::IntakeIssue;
use plane_db::models::issue::Issue;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityCall};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::FieldValue;
use plane_serializers::generated::app_issue::IssueIntakeSerializer;
use uuid::Uuid;

use super::issue::{
    IntakeIssueDetailSerializer, IntakeIssueFields, IntakeIssueSerializer as IntakeIssuePreImage,
    decode_detail_issue, decode_issue_intake,
};
use super::queries;
use super::render::plain_context;
use super::write_intake_issue_patch_queries as patch_queries;
use super::write_queries;
use crate::common::{client, jobs};
use crate::issues::lists::list_common::{error_body, kwarg_text, kwarg_uuid, user_id};
use crate::issues::write_support;
use crate::issues::{write_issue, write_issue::Extra};
use crate::write::generated::intake_issue_serializer;
use crate::write::{self, Serializer, Validated};

/// `ROLE.GUEST.value` (`plane/app/permissions/base.py:16`).
const ROLE_GUEST: i16 = 5;
/// `ROLE.MEMBER.value` (`plane/app/permissions/base.py:15`).
const ROLE_MEMBER: i16 = 15;
/// `StateGroup.TRIAGE.value` (`plane/db/models/state.py:21`).
const STATE_GROUP_TRIAGE: &str = "triage";

/// Register `partial_update` on the view that already serves `list`/`retrieve`
/// (both paths — `intake-issues/` and `inbox-issues/` — select the same view).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.intake.base.IntakeIssueViewSet")?
        .action("partial_update", patch)?;
    Ok(())
}

/// `IntakeIssueViewSet.partial_update` (`plane/app/views/intake/base.py:335-505`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `skip_activity = request.data.pop("skip_activity", False)` and
        // `request.data.get("description_html") is not None` (`:337-338`). A
        // body that is not a mapping fails the `pop` (a 500) before any query.
        let body = PoppedBody::new(ctx)?;

        // `Intake.objects.filter(workspace__slug=…, project_id=…).first()`
        // (`:340`). `INTAKE_ONLY` is ordered by `Intake.Meta.ordering`; a miss
        // is `None`, not an error.
        let intake_statement = client
            .prepare_cached(queries::INTAKE_ONLY.sql)
            .await
            .map_err(ViewError::from)?;
        let intake_id: Option<Uuid> = client
            .query_opt(&intake_statement, &[&slug, &project_id])
            .await
            .map_err(ViewError::from)?
            .map(|row| row.get(0));

        // `IntakeIssue.objects.get(...)` (`:341-346`): the default
        // soft-deletion manager, so a soft-deleted link is a 404. A project
        // without an intake passes `intake_id = NULL`, which matches no row
        // (`intake_id` is NOT NULL) and is the same 404 — the plan's "missing
        // intake is 404, not 500".
        let link_statement = client
            .prepare_cached(write_queries::INTAKE_ISSUE_FOR_ISSUE.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(link_row) = client
            .query_opt(&link_statement, &[&intake_id, &pk, &project_id, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let link = IntakeIssue::from_row(&link_row, 0)?;

        // `ProjectMember.objects.filter(...).first()` (`:348-353`) and
        // `WorkspaceMember.objects.filter(role=ADMIN, is_active=True).exists()`
        // (`:355-360`). The project role comes from the statement the
        // project-member routes use; `ProjectMember.Meta.ordering` is
        // `-created_at` and the live-row uniqueness constraint leaves one row.
        let member_statement = client
            .prepare_cached(crate::projects::queries::PROJECT_MEMBER_SELF_ROLE.sql)
            .await
            .map_err(ViewError::from)?;
        let project_role: Option<i16> = client
            .query_opt(&member_statement, &[&project_id, &user, &slug])
            .await
            .map_err(ViewError::from)?
            .map(|row| row.get(0));
        let admin_statement = client
            .prepare_cached(crate::projects::queries::WORKSPACE_ADMIN_EXISTS.sql)
            .await
            .map_err(ViewError::from)?;
        let is_workspace_admin = client
            .query_opt(&admin_statement, &[&user, &slug])
            .await
            .map_err(ViewError::from)?
            .is_some();

        // `if not project_member and not is_workspace_admin` (`:362-366`). A
        // caller allowed by the decorator's creator branch is an active project
        // member, so this is unreachable in practice; ported anyway.
        if project_role.is_none() && !is_workspace_admin {
            return Ok(error_body(
                403,
                "Only admin or creator can update the intake work items",
            ));
        }

        // `((project_member and role <= GUEST) and not is_workspace_admin) and
        // str(intake_issue.created_by_id) != str(request.user.id)`
        // (`:369-375`).
        let is_project_guest = project_role.is_some_and(|role| role <= ROLE_GUEST);
        if is_project_guest && !is_workspace_admin && link.created_by_id != Some(user) {
            return Ok(error_body(400, "You cannot edit intake issues"));
        }

        // The nested issue (`:378-421`), only when `bool(issue_data)`.
        let mut nested = None;
        if body.issue_is_truthy() {
            match prepare_nested(
                ctx,
                client,
                slug,
                project_id,
                &link,
                &body,
                is_project_guest,
            )
            .await?
            {
                Prepared::Ready(edit) => nested = Some(edit),
                Prepared::Invalid(response) => return Ok(response),
            }
        }

        // The intake serializer only for a project ADMIN (role > MEMBER) or a
        // workspace admin (`:427-432`).
        let intake_permitted =
            project_role.is_some_and(|role| role > ROLE_MEMBER) || is_workspace_admin;
        let mut intake_edit = None;
        if intake_permitted {
            match prepare_intake(ctx, client, &link).await? {
                Prepared::Ready(edit) => intake_edit = Some(edit),
                Prepared::Invalid(response) => return Ok(response),
            }
        }

        // `if issue_serializer: issue_serializer.save()` and its two publishes
        // (`:435-460`).
        if let Some(nested) = nested {
            save_nested(client, &nested, user).await?;
            // `is_migration_description_update = skip_activity and
            // is_description_update` (`:439`).
            if !(body.skip_activity && body.is_description_update) {
                let origin = write_support::base_host(ctx)?;
                publish_issue_activity(
                    ctx,
                    IssueActivityCall::view_with_intake(
                        write_issue::activity_base(
                            "issue.activity.updated",
                            task_text(&nested.requested_data),
                            user,
                            &nested.issue.id.to_string(),
                            &project_id.to_string(),
                            task_text(&nested.current_instance),
                        ),
                        origin.clone(),
                        true,
                        link.id.to_string(),
                    ),
                )
                .await?;
                publish_description_version(
                    ctx,
                    task_text(&nested.current_instance),
                    &pk.to_string(),
                    user,
                )
                .await?;
            }
        }

        // `if intake_serializer: intake_serializer.save()` (`:462-463`) and the
        // `notification=False` activity (`:465-476`).
        if let Some(edit) = &intake_edit {
            save_intake(client, edit, &link, user).await?;
            let origin = write_support::base_host(ctx)?;
            publish_issue_activity(
                ctx,
                IssueActivityCall::view_with_intake(
                    write_issue::activity_base(
                        "intake.activity.created",
                        task_text(&body.requested_data(ctx)?),
                        user,
                        &pk.to_string(),
                        &project_id.to_string(),
                        task_text(&edit.current_instance),
                    ),
                    origin,
                    false,
                    link.id.to_string(),
                ),
            )
            .await?;
        }

        // The re-fetch (`:479-503`): byte-identical to `retrieve`'s queryset,
        // so it is the read route's statement.
        let Some(intake_id) = intake_id else {
            // Unreachable: the link lookup above matched a row for this
            // `intake_id`, and `intake_issues.intake_id` is NOT NULL.
            return Err(ViewError::Unknown(
                "'NoneType' object has no attribute 'id'".to_owned(),
            ));
        };
        let statement = client
            .prepare_cached(queries::INTAKE_ISSUE_DETAIL.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&intake_id, &pk, &project_id])
            .await
            .map_err(ViewError::from)?;
        let row = match rows.as_slice() {
            [] => return Err(ViewError::ObjectDoesNotExist),
            [row] => row,
            _ => {
                return Err(ViewError::Unknown(
                    "IntakeIssue matching query does not exist.".to_owned(),
                ));
            }
        };
        render_detail(ctx, client, row).await
    })
}

/// A validated edit, or the `400` response that replaces it.
enum Prepared<T> {
    Ready(T),
    Invalid(ViewResponse),
}

// ---------------------------------------------------------------------------
// The popped body
// ---------------------------------------------------------------------------

/// `request.data` after `partial_update`'s two `pop`s (`:337`, `:378`).
///
/// The Python mutates the mapping in place; every later reader — the intake
/// serializer (`:429`) and the `intake.activity.created` payload (`:467`) —
/// sees the body without `skip_activity` and `issue`.
struct PoppedBody {
    /// `skip_activity`'s Python truthiness. A form value is always the
    /// one-element list `MultiValueDict` stores, so `skip_activity=` is true.
    skip_activity: bool,
    /// `request.data.get("description_html") is not None` after the pop.
    is_description_update: bool,
    /// `request.data.pop("issue", False)`.
    issue: Json,
}

impl PoppedBody {
    /// # Errors
    ///
    /// [`ViewError::Unknown`] for a JSON body that is not an object: Python's
    /// `list.pop`/`str.pop` is an `AttributeError` (a 500) before any query.
    fn new(ctx: &ViewCtx<'_>) -> Result<Self, ViewError> {
        match ctx.data()? {
            Data::Json(Json::Dict(object)) => Ok(Self {
                skip_activity: object
                    .get("skip_activity")
                    .is_some_and(write_support::truthy),
                is_description_update: object
                    .get("description_html")
                    .is_some_and(|value| !value.is_null()),
                issue: object.get("issue").cloned().unwrap_or(Json::Bool(false)),
            }),
            Data::Json(other) => Err(ViewError::Unknown(format!(
                "AttributeError: '{}' object has no attribute 'pop'",
                python_type_name(other)
            ))),
            Data::Form(query) => {
                // `request.data` for a form/multipart body: `_load_data_and_files`
                // assigns the parser's `QueryDict` directly when there are no
                // files, and `FormParser`/`MultipartParser` build it immutable
                // (`QueryDict(stream.read(), encoding=…)`,
                // `rest_framework/request.py:283-293`). `pop` on an immutable
                // `QueryDict` is `AttributeError: This QueryDict instance is
                // immutable` — a 500 before any query. A multipart body with at
                // least one file part takes the `.copy()` branch, which *is*
                // mutable, so the pops work there.
                if !query.has_files() {
                    return Err(ViewError::Unknown(
                        "AttributeError: This QueryDict instance is immutable".to_owned(),
                    ));
                }
                let entries = form_entries(query);
                Ok(Self {
                    // `dict.pop` on a `MultiValueDict` returns the stored list;
                    // `bool(["true"])`, `bool([""])` and a file part are all
                    // true.
                    skip_activity: entries.iter().any(|entry| entry.key == "skip_activity"),
                    is_description_update: entries
                        .iter()
                        .any(|entry| entry.key == "description_html"),
                    issue: match entries.iter().find(|entry| entry.key == "issue") {
                        Some(entry) if entry.file => Json::List(vec![Json::Null]),
                        Some(_) => {
                            Json::List(query.getlist("issue").into_iter().map(Json::from).collect())
                        }
                        None => Json::Bool(false),
                    },
                })
            }
        }
    }

    /// `bool(issue_data)`.
    fn issue_is_truthy(&self) -> bool {
        write_support::truthy(&self.issue)
    }

    /// `json.dumps(request.data, cls=DjangoJSONEncoder)` (`:467`) with the two
    /// popped keys gone.
    ///
    /// A form body is a `QueryDict`, and `json.dumps` walks it through
    /// `.items()`: one entry per key, first-seen order, last value wins — the
    /// rules of [`crate::write::request_data_value`]. A multipart file part
    /// that survives the pops is the encoder's `TypeError`, i.e. a 500 at the
    /// publish, after the writes.
    ///
    /// # Errors
    ///
    /// A parser failure, or the encoder's `TypeError` for a file part.
    fn requested_data(&self, ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
        match ctx.data()? {
            Data::Json(Json::Dict(object)) => {
                let mut kept = PyDict::with_capacity(object.len());
                for (key, value) in object.iter() {
                    if matches!(key.as_str(), Some("skip_activity" | "issue")) {
                        continue;
                    }
                    kept.insert(key.clone(), value.clone());
                }
                write_support::dumps(&Json::Dict(kept))
            }
            Data::Json(_) => Err(ViewError::Unknown(
                "AttributeError: request.data is not a mapping".to_owned(),
            )),
            Data::Form(query) => {
                let mut kept = PyDict::with_capacity(query.len());
                for entry in form_entries(query) {
                    if matches!(entry.key, "skip_activity" | "issue") {
                        continue;
                    }
                    if entry.file {
                        // `request.data` holds the file object, and
                        // `json.dumps` refuses it.
                        return Err(ViewError::Unknown(
                            "TypeError: Object of type InMemoryUploadedFile is not JSON \
                             serializable"
                                .to_owned(),
                        ));
                    }
                    if let Some(value) = query.get(entry.key) {
                        kept.insert(PyStr::new(entry.key), Json::Str(PyStr::new(value)));
                    }
                }
                write_support::dumps(&Json::Dict(kept))
            }
        }
    }
}

/// One key of `request.data` for a multipart body.
struct FormEntry<'a> {
    key: &'a str,
    /// `request.data[key]` is `[<InMemoryUploadedFile>]`, not `[str]`.
    file: bool,
}

/// The keys `request.data` exposes for a multipart body, in order.
///
/// DRF builds `_full_data = self._data.copy()` and then
/// `_full_data.update(self._files)` (`rest_framework/request.py:283-293`), so
/// the text keys come first in first-seen order and a file part with a new name
/// is appended at the end. A file part that shadows a text key keeps the text
/// key's position and replaces its value with the file object.
fn form_entries(query: &plane_drf::QueryDict) -> Vec<FormEntry<'_>> {
    let mut entries: Vec<FormEntry<'_>> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for key in query.keys() {
        if !seen.insert(key) {
            continue;
        }
        entries.push(FormEntry {
            key,
            file: query.is_file(key),
        });
    }
    for key in query.file_keys() {
        let key = key.as_str();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) {
            entry.file = true;
            continue;
        }
        if seen.insert(key) {
            entries.push(FormEntry { key, file: true });
        }
    }
    entries
}

/// Python's `type(value).__name__`.
fn python_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::List(_) => "list",
        Json::Dict(_) => "dict",
    }
}

// ---------------------------------------------------------------------------
// The nested issue
// ---------------------------------------------------------------------------

/// A validated `request.data["issue"]`, ready for `save()`.
struct Nested {
    issue: Issue,
    extra: Extra,
    /// `json.dumps(IssueDetailSerializer(issue).data, …)` — the pre-image.
    current_instance: String,
    /// `json.dumps(writable_data(issue_serializer, issue_data), …)`.
    requested_data: String,
}

/// Validate `request.data["issue"]` (`plane/app/views/intake/base.py:385-421`).
async fn prepare_nested(
    ctx: &ViewCtx<'_>,
    client: &Client,
    slug: &str,
    project_id: Uuid,
    link: &IntakeIssue,
    body: &PoppedBody,
    is_project_guest: bool,
) -> Result<Prepared<Nested>, ViewError> {
    // `Issue.objects.annotate(label_ids=…, assignee_ids=…).get(pk=…,
    // workspace__slug=…, project_id=…)` (`:386-403`): a soft-deleted or
    // foreign issue is `DoesNotExist` → 404.
    let statement = client
        .prepare_cached(patch_queries::INTAKE_ISSUE_PATCH_ISSUE.sql)
        .await
        .map_err(ViewError::from)?;
    let Some(row) = client
        .query_opt(&statement, &[&link.issue_id, &project_id, &slug])
        .await
        .map_err(ViewError::from)?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let mut issue = Issue::from_row(&row, 0)?;

    // `if project_member and project_member.role <= GUEST: issue_data = {...}`
    // (`:405-410`). Note the missing `not is_workspace_admin`: a workspace
    // admin with a guest project role gets the reduced body too.
    let issue_data = if is_project_guest {
        let Json::Dict(object) = &body.issue else {
            // `issue_data.get(...)` on a `str`/`list` is an `AttributeError`.
            return Err(ViewError::Unknown(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                python_type_name(&body.issue)
            )));
        };
        let mut reduced = PyDict::with_capacity(3);
        reduced.insert(
            PyStr::new("name"),
            object
                .get("name")
                .cloned()
                .unwrap_or_else(|| Json::Str(PyStr::new(&issue.name))),
        );
        reduced.insert(
            PyStr::new("description_html"),
            object
                .get("description_html")
                .cloned()
                .unwrap_or_else(|| Json::Str(PyStr::new(&issue.description_html))),
        );
        reduced.insert(
            PyStr::new("description_json"),
            object
                .get("description_json")
                .cloned()
                .unwrap_or_else(|| json_column(&issue.description_json)),
        );
        Json::Dict(reduced)
    } else {
        body.issue.clone()
    };

    // `issue_current_instance = json.dumps(IssueDetailSerializer(issue).data,
    // cls=DjangoJSONEncoder)` (`:412`).
    let detail = crate::issues::lists::list_retrieve::intake_pre_image(&row)?;
    let current_instance = detail.dje_json(write_support::timezone(ctx))?;

    // `IssueCreateSerializer(issue, data=issue_data, partial=True,
    // context={"project_id": …, "allow_triage_state": True})` (`:414-416`).
    // Every non-mapping value has already raised in `.get` (guest) or in
    // `writable_data` below, so `from_value`'s non-mapping 400 is unreachable
    // from this route; the shape check is kept because it is the constructor's.
    let mut serializer = Serializer::from_value(ctx, &issue_data, true);
    write_support::mark_serializer_patch_fields(&mut serializer);
    validate_nested_issue(&mut serializer, client).await?;

    // `issue_requested_data = json.dumps(writable_data(issue_serializer,
    // issue_data), …)` (`:418`) runs **before** `is_valid()`: a non-mapping
    // `issue_data` is this `.items()` `AttributeError` (a 500), never the
    // serializer's 400.
    let requested_data =
        write_support::dumps(&write_support::accepted_data_from(&issue_data, true)?)?;

    let validated = match serializer.finish() {
        Ok(validated) => validated,
        Err(mut errors) => {
            errors.sort_by_field_order(&write_issue::field_order());
            return Ok(Prepared::Invalid(errors.into_response()));
        }
    };

    let extra = match write_issue::validate_issue(client, project_id, &validated, true).await {
        Ok(extra) => extra,
        Err(errors) => return Ok(Prepared::Invalid(errors.into_response())),
    };

    write_issue::apply_patch(&mut issue, &validated, &extra);
    Ok(Prepared::Ready(Nested {
        issue,
        extra,
        current_instance,
        requested_data,
    }))
}

/// `IssueCreateSerializer.save()`'s update branch on the nested issue
/// (`plane/app/serializers/issue.py:286-340`) plus `Issue.save()`
/// (`plane/db/models/issue.py:180-255`).
///
/// The through rows are replaced **before** the issue UPDATE, with the
/// pre-update issue's audit ids (that is what `IssueCreateSerializer.update`
/// passes to `IssueAssignee(...)`/`IssueLabel(...)`), exactly as
/// `issues/write_issue.rs::patch` does.
async fn save_nested(client: &Client, nested: &Nested, user: Uuid) -> Result<(), ViewError> {
    let mut issue = nested.issue.clone();
    // `Issue.save()`'s H10a runs before H10b's `has_changed("state_id")`, so a
    // NULL state becomes the project default first.
    let old_state = issue.state_id;
    if issue.state_id.is_none() {
        issue.state_id = hooks::issue::ensure_default_state(client, issue.project_id).await?;
    }
    let state_changed = issue.state_id != old_state;

    let m2m_now = write::now();
    if let Some(assignees) = &nested.extra.assignees {
        hooks::issue_m2m::replace_assignees(client, &issue, assignees, m2m_now).await?;
    }
    if let Some(labels) = &nested.extra.labels {
        hooks::issue_m2m::replace_labels(client, &issue, labels, m2m_now).await?;
    }

    // H12 recomputes `description_stripped` from the (possibly unchanged)
    // `description_html`.
    issue.description_stripped = write_issue::strip_html(&issue.description_html);
    hooks::issue::update_issue_row(client, &mut issue, state_changed, Some(user), write::now())
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The intake link
// ---------------------------------------------------------------------------

/// The link's issue, as `intake_issue.issue` loads it (`intake.py:59,81`):
/// the forward FK through the **base** manager, plus its state's group.
struct LinkIssue {
    issue: Issue,
    /// `issue.state.group`, `None` when `state_id` is NULL.
    state_group: Option<String>,
}

async fn load_link_issue(client: &Client, link: &IntakeIssue) -> Result<LinkIssue, ViewError> {
    let statement = client
        .prepare_cached(patch_queries::INTAKE_ISSUE_PATCH_LINK_ISSUE.sql)
        .await
        .map_err(ViewError::from)?;
    let Some(row) = client
        .query_opt(&statement, &[&link.issue_id])
        .await
        .map_err(ViewError::from)?
    else {
        // A live FK always resolves; a missing row would be a broken link.
        return Err(ViewError::Unknown(format!(
            "the intake link's issue {} does not exist",
            link.issue_id
        )));
    };
    Ok(LinkIssue {
        issue: Issue::from_row(&row, 0)?,
        state_group: row.try_get(28).map_err(ViewError::from)?,
    })
}

/// A validated intake edit, ready for `save()`.
struct IntakeEdit {
    validated: Validated,
    /// The value H16's input is derived from: the serializer's
    /// `attrs["snoozed_till"]` (already project-localised for a bare date).
    snoozed_till: Option<DateTime<Utc>>,
    /// `json.dumps(IntakeIssueSerializer(link).data, …)` — the pre-image.
    current_instance: String,
    link_issue: LinkIssue,
}

/// `IntakeIssueSerializer(instance=link, data=request.data, partial=True)`
/// (`plane/app/serializers/intake.py:28-97`).
async fn prepare_intake(
    ctx: &ViewCtx<'_>,
    client: &Client,
    link: &IntakeIssue,
) -> Result<Prepared<IntakeEdit>, ViewError> {
    let link_issue = load_link_issue(client, link).await?;
    // The pre-image dump happens before the serializer is bound (`:428`).
    let current_instance = intake_current_instance(ctx, link, &link_issue.issue)?;

    // `self.fields` order: `status`, `duplicate_to`, `snoozed_till`, `source`,
    // `created_by` (`id` and `issue` are read-only).
    let mut serializer = Serializer::new(ctx, true)?;
    intake_issue_serializer::validate(&mut serializer, client).await?;
    let validated = match serializer.finish() {
        Ok(validated) => validated,
        Err(errors) => return Ok(Prepared::Invalid(errors.into_response())),
    };

    // `if attrs.get("snoozed_till"): attrs["snoozed_till"] = snooze_day_start(
    // self.initial_data.get("snoozed_till"), self.instance.project_id) or
    // attrs["snoozed_till"]` (`intake.py:50-54`).
    let mut snoozed_till = validated.datetime("snoozed_till");
    if snoozed_till.is_some() {
        let initial = initial_value(ctx, "snoozed_till")?;
        if let Some(local) = snooze_day_start(client, initial.as_ref(), link.project_id).await? {
            snoozed_till = Some(local);
        }
    }

    // `if attrs.get("status") == 1:` — the accept guard (`intake.py:56-71`).
    if validated.integer("status") == Some(1) {
        let in_triage = link_issue.issue.state_id.is_some()
            && link_issue.state_group.as_deref() == Some(STATE_GROUP_TRIAGE);
        if in_triage
            && default_state(client, link.workspace_id, link.project_id)
                .await?
                .is_none()
        {
            return Ok(Prepared::Invalid(write::field_string_error(
                "status",
                "Cannot accept intake issue: No default state found for the project",
            )));
        }
    }

    Ok(Prepared::Ready(IntakeEdit {
        validated,
        snoozed_till,
        current_instance,
        link_issue,
    }))
}

/// `IntakeIssue.save()` (`plane/db/models/intake.py:100-109`) and the accept
/// transition (`plane/app/serializers/intake.py:75-91`).
async fn save_intake(
    client: &Client,
    edit: &IntakeEdit,
    link: &IntakeIssue,
    user: Uuid,
) -> Result<(), ViewError> {
    let validated = &edit.validated;

    // H16: `if self.snoozed_till and (self._state.adding or
    // self.has_changed("snoozed_till"))`. `adding` is false, so the floor runs
    // only when the body carried a *different* `snoozed_till`.
    let requested_snooze = if validated.contains("snoozed_till") {
        edit.snoozed_till
    } else {
        link.snoozed_till
    };
    let snoozed_till = match requested_snooze {
        Some(value) if Some(value) != link.snoozed_till => {
            // `Project.objects.values_list("timezone", flat=True).get(id=…)`:
            // the default manager, so a soft-deleted project is a 404.
            let timezone = project_timezone(client, link.project_id).await?;
            intake_issue::before_save(Some(value), false, true, timezone)
        }
        other => other,
    };

    // H01's update branch (`plane/db/models/base.py:40-42`): `updated_by` only.
    // `created_by` is written when the body carried it.
    let created_by = if validated.contains("created_by") {
        validated.uuid("created_by")
    } else {
        link.created_by_id
    };
    let status = validated
        .integer("status")
        .map_or(link.status, |value| value as i32);
    let duplicate_to = if validated.contains("duplicate_to") {
        validated.uuid("duplicate_to")
    } else {
        link.duplicate_to_id
    };
    let source = if validated.contains("source") {
        validated.text("source").map(str::to_owned)
    } else {
        link.source.clone()
    };

    let statement = client
        .prepare_cached(patch_queries::INTAKE_ISSUE_PATCH_UPDATE.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .execute(
            &statement,
            &[
                &link.id,
                &write::now(),
                &created_by,
                &Some(user),
                &status,
                &snoozed_till,
                &duplicate_to,
                &source,
            ],
        )
        .await
        .map_err(ViewError::from)?;

    // `if validated_data.get("status") == 1:` (`intake.py:80-89`) — the *body*
    // value, not the stored one, and only a triage issue moves.
    if validated.integer("status") == Some(1)
        && edit.link_issue.issue.state_id.is_some()
        && edit.link_issue.state_group.as_deref() == Some(STATE_GROUP_TRIAGE)
        && let Some(state_id) = default_state(client, link.workspace_id, link.project_id).await?
    {
        // `issue.state = default_state; issue.save()`: a full save, so H10a,
        // H10b (`state_changed`, hence `completed_at`), H12 and H01 all run.
        // No activity and no webhook are published for this move.
        let mut issue = edit.link_issue.issue.clone();
        issue.state_id = Some(state_id);
        issue.description_stripped = write_issue::strip_html(&issue.description_html);
        hooks::issue::update_issue_row(client, &mut issue, true, Some(user), write::now()).await?;
    }
    Ok(())
}

/// `State.objects.filter(workspace=…, project=…, default=True).first()`
/// (`plane/app/serializers/intake.py:64-66`, `:84-86`).
async fn default_state(
    client: &Client,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<Option<Uuid>, ViewError> {
    let statement = client
        .prepare_cached(patch_queries::INTAKE_ISSUE_PATCH_DEFAULT_STATE.sql)
        .await
        .map_err(ViewError::from)?;
    Ok(client
        .query_opt(&statement, &[&workspace_id, &project_id])
        .await
        .map_err(ViewError::from)?
        .map(|row| row.get(0)))
}

/// The request body's raw value for one key, as `self.initial_data.get(key)`
/// sees it.
fn initial_value(ctx: &ViewCtx<'_>, key: &str) -> Result<Option<Json>, ViewError> {
    Ok(match ctx.data()? {
        Data::Json(Json::Dict(object)) => object.get(key).cloned(),
        Data::Json(_) => None,
        Data::Form(query) => query.get(key).map(|value| Json::Str(PyStr::new(value))),
    })
}

/// `json.dumps(IntakeIssueSerializer(intake_issue).data, cls=DjangoJSONEncoder)`
/// (`plane/app/views/intake/base.py:428`).
fn intake_current_instance(
    ctx: &ViewCtx<'_>,
    link: &IntakeIssue,
    issue: &Issue,
) -> Result<String, ViewError> {
    let nested = IssueIntakeSerializer {
        id: issue.id,
        name: &issue.name,
        priority: &issue.priority,
        sequence_id: i64::from(issue.sequence_id),
        project_id: issue.project_id,
        created_at: issue.created_at,
        // A plain `Issue` has no `label_ids` attribute, so DRF skips the key.
        label_ids: FieldValue::absent(),
        created_by: issue.created_by_id,
    };
    let serializer = IntakeIssuePreImage::new(
        IntakeIssueFields {
            id: link.id,
            status: i64::from(link.status),
            duplicate_to: link.duplicate_to_id,
            snoozed_till: link.snoozed_till,
            source: link.source.as_deref(),
        },
        nested,
        link.created_by_id,
    );
    django_dumps_drf(&serializer, ctx)
}

/// `json.dumps(serializer.data, cls=DjangoJSONEncoder)` for a serializer the
/// codebase renders through [`plane_serializers::DrfSerialize`].
///
/// `serializer.data` is the in-memory tree DRF built: every value is already a
/// field's `to_representation` output (ISO strings for datetimes in the request
/// timezone, canonical uuid strings, `null`s). Rendering with the DRF dialect
/// and transcoding to the Django dialect therefore reproduces
/// `json.dumps(..., cls=DjangoJSONEncoder)` exactly: same key order and values,
/// Django's `", "`/`": "` separators and `ensure_ascii=True`.
fn django_dumps_drf<T: plane_serializers::DrfSerialize + ?Sized>(
    serializer: &T,
    ctx: &ViewCtx<'_>,
) -> Result<String, ViewError> {
    django_dumps_drf_at(serializer, write_support::timezone(ctx))
}

/// [`django_dumps_drf`] with the timezone supplied directly, so the shape is
/// testable without a request.
fn django_dumps_drf_at<T: plane_serializers::DrfSerialize + ?Sized>(
    serializer: &T,
    timezone: chrono_tz::Tz,
) -> Result<String, ViewError> {
    let context = plane_serializers::RenderCtx::new(&timezone, None, &[]);
    let drf = plane_serializers::render(serializer, &context)?;
    let text = std::str::from_utf8(&drf).map_err(|error| ViewError::Unknown(error.to_string()))?;
    let mut out = Vec::with_capacity(drf.len());
    plane_serde::transcode_json(plane_serde::Dialect::Django, text, &mut out)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(out).map_err(|error| ViewError::Unknown(error.to_string()))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `json.loads` of a `JSONField` column, for the guest projection's
/// `issue_data.get("description_json", issue.description_json)`.
fn json_column(value: &plane_db::JsonText) -> Json {
    plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8").unwrap_or(Json::Null)
}

/// `Project.objects.values_list("timezone", flat=True).get(id=…)` — the same
/// statement the cycle routes use, including the 404 for a soft-deleted
/// project and the 500 for an unknown zone (`pytz.UnknownTimeZoneError`).
async fn project_timezone(client: &Client, project_id: Uuid) -> Result<chrono_tz::Tz, ViewError> {
    crate::cycles::base::project_timezone(client, project_id).await
}

/// `snooze_day_start(value, project_id)`
/// (`plane/utils/timezone_converter.py:103-112`).
///
/// A bare `YYYY-MM-DD` becomes midnight in the **project's** timezone. Every
/// other value — a datetime string, a number, `null` — is `None`, and the
/// caller keeps `attrs["snoozed_till"]` (`intake.py:50-54`).
async fn snooze_day_start(
    client: &Client,
    value: Option<&Json>,
    project_id: Uuid,
) -> Result<Option<DateTime<Utc>>, ViewError> {
    let Some(Json::Str(text)) = value else {
        return Ok(None);
    };
    let Some(text) = text.as_str() else {
        return Ok(None);
    };
    let Some(day) = parse_date(text) else {
        return Ok(None);
    };
    let timezone = project_timezone(client, project_id).await?;
    Ok(Some(intake_issue::project_day_start(day, timezone)))
}

/// Django's `parse_date` (`django/utils/dateparse.py:56-69`, CPython 3.12's
/// `date.fromisoformat`) restricted to the one shape DRF's `DateTimeField` also
/// accepts as a bare date: zero-padded `YYYY-MM-DD`.
///
/// Django's fallback regex would accept a loose `2026-1-2`, but the field
/// rejects that string first, so the stricter check cannot change a reachable
/// response — it only keeps a malformed value from being *converted* when the
/// Rust side's `DateTimeField` was more lenient than Django's.
fn parse_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    if bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return None;
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").ok()
}

/// The PATCH's response: `IntakeIssueDetailSerializer(intake_issue).data` over
/// the re-fetched row (`plane/app/views/intake/base.py:504-505`).
async fn render_detail(
    ctx: &ViewCtx<'_>,
    client: &Client,
    row: &tokio_postgres::Row,
) -> Result<ViewResponse, ViewError> {
    // `duplicate_to` dereferences through the base manager, so a soft-deleted
    // target still renders (`intake.py:102`); `ISSUE_INTAKE_BY_ID` is the read
    // route's statement for it.
    let duplicate_to: Option<Uuid> = row.get(12);
    let duplicate_row = match duplicate_to {
        Some(id) => {
            let statement = client
                .prepare_cached(queries::ISSUE_INTAKE_BY_ID.sql)
                .await
                .map_err(ViewError::from)?;
            let Some(issue) = client
                .query_opt(&statement, &[&id])
                .await
                .map_err(ViewError::from)?
            else {
                return Err(ViewError::Unknown(format!(
                    "the duplicate target issue {id} does not exist"
                )));
            };
            Some(decode_issue_intake(&issue))
        }
        None => None,
    };
    let duplicate_serializer = duplicate_row
        .as_ref()
        .map(super::issue::IssueIntakeRow::serializer);

    let label_ids: Vec<Uuid> = row.get(18);
    let assignee_ids: Vec<Uuid> = row.get(19);
    let issue = decode_detail_issue(row, &label_ids, &assignee_ids);
    let source: Option<String> = row.get(13);
    let serializer = IntakeIssueDetailSerializer::new(
        IntakeIssueFields {
            id: row.get(5),
            status: i64::from(row.get::<_, i32>(10)),
            duplicate_to,
            snoozed_till: row.get(11),
            source: source.as_deref(),
        },
        duplicate_serializer.as_ref(),
        issue,
    );
    let context = plain_context(ctx);
    Ok(ViewResponse::raw(
        200,
        APPLICATION_JSON,
        plane_serializers::render(&serializer, &context)?,
    ))
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

async fn publish_issue_activity(
    ctx: &ViewCtx<'_>,
    call: IssueActivityCall,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    let message = tasks::issue_activity(call);
    jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::from_message(&message),
    )
    .await?;
    Ok(())
}

/// `issue_description_version_task.delay(updated_issue=…, issue_id=str(pk),
/// user_id=…)` (`plane/app/views/intake/base.py:456-460`): the update site, so
/// `is_creating` is absent.
async fn publish_description_version(
    ctx: &ViewCtx<'_>,
    updated_issue: TaskArg,
    issue_id: &str,
    user_id: Uuid,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::ISSUE_DESCRIPTION_VERSION_TASK;
    let message =
        tasks::issue_description_version_task(updated_issue, issue_id.to_owned(), user_id, false);
    let job = plane_jobs::queue::Job::from_message(&message);
    jobs::dispatch_task(ctx, task, &message, job).await?;
    Ok(())
}

/// The nested issue serializer's field table, as
/// `tools/inventory/serializers.py` emits it for `IssueCreateSerializer`.
///
/// It is the same table the issue routes use (`write/generated/
/// issue_create_serializer.rs`); this wrapper only keeps the call site's name
/// explicit about which serializer is bound.
async fn validate_nested_issue(
    serializer: &mut Serializer<'_>,
    client: &Client,
) -> Result<(), ViewError> {
    crate::write::generated::issue_create_serializer::validate(serializer, client).await
}

/// `json.dumps` of a string task argument; the value is already JSON text, so
/// this is the serializer-string shape `requested_data=`/`current_instance=`
/// take.
fn task_text(text: &str) -> TaskArg {
    TaskArg::json_text(text.to_owned())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use plane_serde::pyjson::PyStr;

    use super::*;

    fn timestamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
    }

    /// `parse_date` only takes the shape DRF's `DateTimeField` also accepts as
    /// a bare date: a zero-padded `YYYY-MM-DD`. A loose date (`2026-1-2`) or a
    /// leading space is rejected by the field first in Django, so it never
    /// reaches `snooze_day_start` there.
    #[test]
    fn parse_date_is_the_iso_date_only() {
        assert_eq!(
            parse_date("2026-11-20"),
            Some(NaiveDate::from_ymd_opt(2026, 11, 20).expect("date"))
        );
        for value in [
            "2026-1-2",
            "20261120",
            "2026-11-20T00:00:00",
            "2026-11-20 ",
            " 2026-11-2",
            "",
        ] {
            assert_eq!(parse_date(value), None, "{value:?}");
        }
    }

    /// `json.dumps(IntakeIssueSerializer(link).data, cls=DjangoJSONEncoder)`:
    /// seven keys, the nested issue's seven (no `label_ids`, because a plain
    /// `Issue` has no such attribute), Django's separators and `ensure_ascii`.
    #[test]
    fn intake_current_instance_is_the_django_json_dump() {
        let issue = Issue {
            created_at: timestamp(),
            updated_at: timestamp(),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::from_u128(2),
            project_id: Uuid::from_u128(3),
            workspace_id: Uuid::nil(),
            parent_id: None,
            state_id: None,
            point: None,
            estimate_point_id: None,
            name: "Nested".to_owned(),
            description_json: hooks::issue::empty_json(),
            description_html: "<p></p>".to_owned(),
            description_stripped: None,
            description_binary: None,
            priority: "none".to_owned(),
            start_date: None,
            target_date: None,
            sequence_id: 7,
            sort_order: 65_535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            type_id: None,
        };
        let link = IntakeIssue {
            created_at: timestamp(),
            updated_at: timestamp(),
            created_by_id: Some(Uuid::from_u128(4)),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(3),
            workspace_id: Uuid::nil(),
            intake_id: Uuid::from_u128(5),
            issue_id: Uuid::from_u128(2),
            status: -2,
            snoozed_till: None,
            duplicate_to_id: None,
            source: Some("IN_APP".to_owned()),
            source_email: None,
            external_source: None,
            external_id: None,
            extra: plane_db::JsonText::new("{}"),
        };

        let nested = IssueIntakeSerializer {
            id: issue.id,
            name: &issue.name,
            priority: &issue.priority,
            sequence_id: i64::from(issue.sequence_id),
            project_id: issue.project_id,
            created_at: issue.created_at,
            label_ids: FieldValue::absent(),
            created_by: None,
        };
        let serializer = IntakeIssuePreImage::new(
            IntakeIssueFields {
                id: link.id,
                status: i64::from(link.status),
                duplicate_to: link.duplicate_to_id,
                snoozed_till: link.snoozed_till,
                source: link.source.as_deref(),
            },
            nested,
            link.created_by_id,
        );
        let text = django_dumps_drf_at(&serializer, chrono_tz::UTC).expect("render");
        assert_eq!(
            text,
            "{\"id\": \"00000000-0000-0000-0000-000000000001\", \"status\": -2, \
             \"duplicate_to\": null, \"snoozed_till\": null, \"source\": \"IN_APP\", \
             \"issue\": {\"id\": \"00000000-0000-0000-0000-000000000002\", \
             \"name\": \"Nested\", \"priority\": \"none\", \"sequence_id\": 7, \
             \"project_id\": \"00000000-0000-0000-0000-000000000003\", \
             \"created_at\": \"2026-01-01T00:00:00Z\", \"created_by\": null}, \
             \"created_by\": \"00000000-0000-0000-0000-000000000004\"}"
        );
    }

    /// A multipart body's key order: the text keys first, then a file part's
    /// name, and a file part that shadows a text key stays in the text key's
    /// position (`_full_data = _data.copy()` then `.update(_files)`).
    #[test]
    fn form_entries_append_files_after_the_text_keys() {
        let mut query = plane_drf::QueryDict::parse("status=1&notes=text", None).expect("form");
        query.mark_file(Some("notes"));
        query.mark_file(Some("upload"));
        let keys: Vec<(&str, bool)> = form_entries(&query)
            .into_iter()
            .map(|entry| (entry.key, entry.file))
            .collect();
        assert_eq!(keys, [("status", false), ("notes", true), ("upload", true)]);
    }

    /// A popping body's `bool(issue_data)` follows Python truthiness: `null`,
    /// `false`, `0`, `""` and `{}` skip the nested serializer.
    #[test]
    fn falsy_issue_values_skip_the_nested_serializer() {
        for value in [
            Json::Bool(false),
            Json::Null,
            Json::Int(plane_serde::pyjson::PyInt::Small(0)),
            Json::Str(PyStr::new("")),
            Json::Dict(PyDict::new()),
            Json::List(Vec::new()),
        ] {
            assert!(!write_support::truthy(&value), "{value:?}");
        }
        assert!(write_support::truthy(&Json::List(vec![Json::Str(
            PyStr::new("")
        )])));
    }
}
