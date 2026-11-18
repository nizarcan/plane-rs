// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The public intake writes: routes 234/235 (`POST …/intake-issues/` and its
//! `inbox-issues/` alias, one handler) and 236 (`PATCH`/`DELETE
//! …/intake-issues/{pk}/`), `plane/space/views/intake.py`.
//!
//! ## create (`:108-187`)
//!
//! Unlike the app route, the public create does **not** use
//! `IssueCreateSerializer`: it validates three things itself (name, priority,
//! the sanitised description) and then calls `Issue.objects.create(...)` with
//! only `name`, `description_json`, `description_html`, `priority`,
//! `project_id` and `state_id`. Every other column keeps its model default and
//! the model hooks (H10/H11/H12) run inside `Issue.save()`.
//!
//! 1. board (`.get(anchor, entity_name="project")` → the family 404);
//! 2. `board.intake is None` → 400 `Intake is not enabled for this Project
//!    Board`;
//! 3. `str(intake_id) != str(board.intake_id)` → 400 `Intake does not belong to
//!    this Project Board`;
//! 4. `request.data.get("issue", {}).get("name", False)` → 400 `Name is
//!    required`; a non-mapping `issue` is an `AttributeError` 500;
//! 5. `priority` (`"none"` default) outside the five values → 400 `Invalid
//!    priority`;
//! 6. `State.triage_objects.filter(project_id, workspace_id).first()`, else
//!    `State.objects.create(name="Triage", group="triage", sequence=65000,
//!    color="#4E5355", default=False)` — H19 rewrites `sequence` to
//!    `MAX(non-triage)+15000` and leaves `is_triage=False`;
//! 7. `validate_html_content(raw description_html)`: only the **sanitized**
//!    value is used, and `None` (falsy input, over 10 MB) becomes `<p></p>`;
//! 8. `Issue.objects.create(...)` → H10a picks no default state (one is
//!    passed), H10b stamps `completed_at` from the triage group, H10's advisory
//!    lock assigns `sequence_id`/`sort_order`, H11 writes the sequence row and
//!    H12 derives `description_stripped`;
//! 9. `issue_activity.delay(type="issue.activity.created",
//!    requested_data=json.dumps(request.data), …)` — **before** the
//!    `intake_issues` row, and only the seven base kwargs;
//! 10. `IntakeIssue.objects.create(intake_id, project_id, issue, source=IN_APP)`
//!     — `ProjectBaseModel.save` derives the workspace, status is `-2`;
//! 11. `IssueStateIntakeSerializer(issue)` → **200**, not 201. The instance has
//!     no `sub_issues_count` annotation, so DRF skips the field (absent, not
//!     `null`), and `issue_intake` reads the row created in step 10.
//!
//! ## partial_update (`:189-248`)
//!
//! Board → intake enabled → `IntakeIssue.objects.get(pk, workspace_id,
//! project_id, intake_id)` (family 404) → `created_by` must be the caller (400
//! `You cannot edit intake issues`) → `issue_data = request.data.pop("issue",
//! False)` (a `QueryDict` raises on `pop`, so a urlencoded body is a 500) →
//! `Issue.objects.get(pk, workspace_id, project_id)` (family 404) → the body is
//! reduced to `name`/`description_html`/`description_json` with the issue's
//! current values as defaults (`issue_data.get` on `False` is a 500) →
//! `IssueCreateSerializer(issue, data=…, partial=True,
//! allow_triage_state=True)` → the activity (**before** `save()`) with
//! `current_instance=json.dumps(IssueSerializer(issue).data)` and
//! `requested_data=json.dumps(issue_data)` → `save()` → 200
//! `IssueCreateSerializer.data` (36 keys).
//!
//! ## destroy (`:272-294`)
//!
//! Board → intake enabled → the link lookup → the **same** `created_by` check
//! but 400 `You cannot delete intake issue` → `intake_issue.delete()` (H03 plus
//! the `soft_delete_related_objects` task, `db`/`intakeissue`) → 204.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::ApiError;
use plane_db::hooks;
use plane_db::models::issue::Issue;
use plane_db::{JsonText, hooks::audit::Audit};
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_query::json::Json as Doc;
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::FieldValue;
use plane_serializers::PgJson;
use plane_serializers::generated::app_intake::{
    IntakeIssueLiteSerializer, IssueStateIntakeSerializer,
};
use plane_serializers::generated::app_issue::LabelLiteSerializer;
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_state::StateLiteSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use uuid::Uuid;

use crate::common::client;
use crate::intake::write_intake_issue_queries as intake_queries;
use crate::issues::sub_resources::sub_common::{ProjectLiteRow, UserLiteRow};
use crate::issues::{write_issue, write_support};
use crate::public::board;
use crate::public::queries;
use crate::public::{ok, plain_context};
use crate::states::queries as states;
use crate::write::generated::issue_create_serializer;
use crate::write::{self, Serializer};

use plane_db::models::state::State as DbState;

const PRIORITIES: [&str; 5] = ["low", "medium", "high", "urgent", "none"];
const SOURCE_IN_APP: &str = "IN_APP";
const GROUP_TRIAGE: &str = "triage";
const TRIAGE_NAME: &str = "Triage";
const TRIAGE_COLOR: &str = "#4E5355";
const TRIAGE_SEQUENCE: f64 = 65_000.0;

/// Register the four verbs on the view that already serves `list`/`retrieve`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.space.views.intake.IntakeIssuePublicViewSet")?
        .action("create", create)?
        .action("partial_update", patch)?
        .action("destroy", destroy)?;
    Ok(())
}

fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}

fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    ctx.kwarg(name)
        .ok_or(ViewError::Key)?
        .parse()
        .map_err(|_| ViewError::Validation)
}

fn epoch() -> i64 {
    Utc::now().timestamp()
}

fn intake_not_enabled() -> ViewResponse {
    write::error_body(400, "Intake is not enabled for this Project Board")
}

/// `IntakeIssuePublicViewSet.create` (`plane/space/views/intake.py:108-187`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let intake_id = uuid_kwarg(ctx, "intake_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        let Some(board_intake_id) = board.intake_id else {
            return Ok(intake_not_enabled());
        };
        if intake_id != board_intake_id {
            return Ok(write::error_body(
                400,
                "Intake does not belong to this Project Board",
            ));
        }
        let Some(project_id) = board.project_id else {
            return Ok(intake_not_enabled());
        };

        // `request.data.get("issue", {})`.
        let data = ctx.data()?;
        let issue_body = issue_body(data)?;
        let Json::Dict(issue_object) = &issue_body else {
            return Err(attribute_error("get", &issue_body));
        };
        if !issue_object.get("name").is_some_and(truthy) {
            return Ok(write::error_body(400, "Name is required"));
        }
        if let Some(value) = issue_object.get("priority")
            && !is_priority(value)
        {
            return Ok(write::error_body(400, "Invalid priority"));
        }

        // `State.triage_objects.filter(project_id, workspace_id).first()`
        // (`:138-140`), then the plain create (`:142-151`).
        let statement = queries::PUBLIC_TRIAGE_STATE.prepare(client).await?;
        let triage_id = match client
            .query_opt(&statement, &[&project_id, &board.workspace_id])
            .await?
        {
            Some(row) => row.get::<_, Uuid>(0),
            None => create_triage_state(client, project_id, board.workspace_id, user).await?,
        };

        // `raw_description_html = …get("description_html", "<p></p>")` and
        // `validate_html_content` (`:153-156`).
        let raw_html = issue_object
            .get("description_html")
            .cloned()
            .unwrap_or_else(|| Json::Str(PyStr::new("<p></p>")));
        let safe_html = safe_description_html(&raw_html)?;

        let name = python_str(issue_object.get("name").cloned().unwrap_or(Json::Null));
        let description_json = issue_object
            .get("description_json")
            .cloned()
            .unwrap_or_else(empty_object);
        let priority = issue_object
            .get("priority")
            .and_then(Json::as_str)
            .unwrap_or("low")
            .to_owned();

        let now = write::now();
        let mut issue = Issue {
            created_at: now,
            updated_at: now,
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id,
            workspace_id: Uuid::nil(),
            parent_id: None,
            state_id: Some(triage_id),
            point: None,
            estimate_point_id: None,
            name,
            description_json: JsonText::from_json(&description_json)?,
            description_stripped: write_issue::strip_html(&safe_html),
            description_html: safe_html,
            description_binary: None,
            priority,
            start_date: None,
            target_date: None,
            sequence_id: 1,
            sort_order: 65_535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            type_id: None,
        };
        hooks::issue::create_issue_row(client, &mut issue, Some(user), now)
            .await
            .map_err(hook_error)?;

        // `issue_activity.delay(...)` before the link row (`:169-177`).
        let activity = tasks::issue_activity(IssueActivityCall::bare(IssueActivityBase::new(
            "issue.activity.created",
            write::request_data_json_text(ctx)?,
            user.to_string(),
            TaskArg::str(issue.id.to_string()),
            TaskArg::str(project_id.to_string()),
            TaskArg::Null,
            epoch(),
        )));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        // `IntakeIssue.objects.create(intake_id=…, project_id=…, issue=issue,
        // source=SourceType.IN_APP)` (`:178-184`).
        let link_id = Uuid::new_v4();
        let link_now = write::now();
        let statement = intake_queries::INTAKE_ISSUE_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &link_now,
                    &link_now,
                    &Some(user),
                    &Option::<Uuid>::None,
                    &Option::<DateTime<Utc>>::None,
                    &link_id,
                    &project_id,
                    &board.workspace_id,
                    &intake_id,
                    &issue.id,
                    &-2_i32,
                    &Option::<DateTime<Utc>>::None,
                    &Option::<Uuid>::None,
                    &Some(SOURCE_IN_APP),
                    &Option::<String>::None,
                    &Option::<String>::None,
                    &Option::<String>::None,
                    &JsonText::new("{}"),
                ],
            )
            .await
            .map_err(integrity)?;

        // `IssueStateIntakeSerializer(issue)` → 200 (`:186-187`). The
        // serializer reads the **in-memory** instance, so `description_json`
        // keeps the request's key order (jsonb would reorder it).
        let mut row = load_issue_row(client, issue.id, board.workspace_id, project_id).await?;
        row.issue.description_json = issue.description_json.clone();
        let body = render_entry(ctx, &row).await?;
        Ok(ok(body))
    })
}

/// `IntakeIssuePublicViewSet.partial_update` (`plane/space/views/intake.py:189-248`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let intake_id = uuid_kwarg(ctx, "intake_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if board.intake_id.is_none() {
            return Ok(intake_not_enabled());
        }
        let Some(project_id) = board.project_id else {
            return Ok(intake_not_enabled());
        };
        let statement = queries::PUBLIC_INTAKE_LINK_BY_PK.prepare(client).await?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&pk, &board.workspace_id, &project_id, &intake_id],
            )
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let issue_id: Uuid = row.get(0);
        let created_by: Option<Uuid> = row.get(1);
        if created_by != Some(user) {
            return Ok(write::error_body(400, "You cannot edit intake issues"));
        }

        // `issue_data = request.data.pop("issue", False)` (`:211`).
        let issue_data = pop_issue(ctx)?;

        let statement = queries::PUBLIC_ISSUE_ROW_BY_PK.prepare(client).await?;
        let Some(issue_row) = client
            .query_opt(&statement, &[&issue_id, &board.workspace_id, &project_id])
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let mut issue = Issue::from_row(&issue_row, 0)?;

        // The three-key reduction (`:219-223`). A falsy `issue_data` (the
        // missing key's `False`) is an `AttributeError` 500.
        let reduced = reduce_issue_data(&issue_data, &issue)?;

        let value = Json::Dict(reduced.clone());
        let mut serializer = Serializer::from_value(ctx, &value, true);
        write_support::mark_serializer_patch_fields(&mut serializer);
        issue_create_serializer::validate(&mut serializer, client).await?;
        let validated = match serializer.finish() {
            Ok(validated) => validated,
            Err(mut errors) => {
                errors.sort_by_field_order(&write_issue::field_order());
                return Ok(errors.into_response());
            }
        };
        let extra = match write_issue::validate_issue(client, project_id, &validated, true).await {
            Ok(extra) => extra,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `current_instance = issue`, then the activity **before** `save()`
        // (`:232-246`).
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let requested_data = django_json(&Json::Dict(reduced))?;
        let current_instance = write_support::issue_serializer_current_instance(&issue, timezone)?;
        let activity = tasks::issue_activity(IssueActivityCall::bare(IssueActivityBase::new(
            "issue.activity.updated",
            TaskArg::json_text(requested_data),
            user.to_string(),
            TaskArg::str(issue.id.to_string()),
            TaskArg::str(project_id.to_string()),
            TaskArg::json_text(current_instance),
            epoch(),
        )));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::new(activity.task()),
        )
        .await?;

        // `IssueCreateSerializer.update`: the validated fields onto the
        // instance, `updated_at = timezone.now()`, then a full `Issue.save()`.
        write_issue::apply_patch(&mut issue, &validated, &extra);
        issue.updated_at = write::now();
        issue.description_stripped = write_issue::strip_html(&issue.description_html);
        hooks::issue::update_issue_row(client, &mut issue, false, Some(user), write::now())
            .await
            .map_err(ViewError::from)?;

        // `IssueCreateSerializer.data`: 36 keys, the 34 readable fields then
        // `assignee_ids`/`label_ids` from `initial_data` (absent → `[]`).
        let assignees = ids_for(client, write_draft_queries::ISSUE_ASSIGNEE_IDS, issue.id).await?;
        let labels = ids_for(client, write_draft_queries::ISSUE_LABEL_IDS, issue.id).await?;
        let description_json = parse_json(&issue.description_json)?;
        let body = issue_create_representation(
            &issue,
            timezone,
            &assignees,
            &labels,
            description_json,
            Doc::Array(Vec::new()),
            Doc::Array(Vec::new()),
        );
        Ok(ok(body.to_drf()))
    })
}

/// `IntakeIssuePublicViewSet.destroy` (`plane/space/views/intake.py:272-294`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let anchor = ctx.kwarg("anchor").ok_or(ViewError::Key)?;
        let intake_id = uuid_kwarg(ctx, "intake_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        let board = board::require_published_board(client, anchor).await?;
        if board.intake_id.is_none() {
            return Ok(intake_not_enabled());
        }
        let Some(project_id) = board.project_id else {
            return Ok(intake_not_enabled());
        };
        let statement = queries::PUBLIC_INTAKE_LINK_BY_PK.prepare(client).await?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&pk, &board.workspace_id, &project_id, &intake_id],
            )
            .await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let created_by: Option<Uuid> = row.get(1);
        if created_by != Some(user) {
            return Ok(write::error_body(400, "You cannot delete intake issue"));
        }
        write::soft_delete(
            client,
            "intake_issues",
            "id",
            pk,
            Audit::on_update(Some(user)),
            write::now(),
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "intakeissue", pk).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Request body helpers
// ---------------------------------------------------------------------------

fn empty_object() -> Json {
    Json::Dict(PyDict::new())
}

/// `request.data.get("issue", {})` for the body shapes that can reach it.
fn issue_body(data: &Data) -> Result<Json, ViewError> {
    match data {
        Data::Json(Json::Dict(object)) => {
            Ok(object.get("issue").cloned().unwrap_or_else(empty_object))
        }
        Data::Json(_) => Err(attribute_error("get", &Json::Null)),
        Data::Form(query) => Ok(query
            .get("issue")
            .map_or_else(empty_object, |text| Json::Str(PyStr::new(text)))),
    }
}

/// `request.data.pop("issue", False)` (`intake.py:211`).
fn pop_issue(ctx: &ViewCtx<'_>) -> Result<Json, ViewError> {
    match ctx.data()? {
        Data::Json(Json::Dict(object)) => {
            Ok(object.get("issue").cloned().unwrap_or(Json::Bool(false)))
        }
        // A `QueryDict` is immutable; `pop` raises `AttributeError`.
        Data::Form(_) => Err(ViewError::Unknown(
            "AttributeError: This QueryDict instance is immutable".to_owned(),
        )),
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: 'request.data' has no attribute 'pop'".to_owned(),
        )),
    }
}

/// The three-key projection (`:219-223`).
fn reduce_issue_data(issue_data: &Json, issue: &Issue) -> Result<PyDict, ViewError> {
    let Json::Dict(body) = issue_data else {
        return Err(attribute_error("get", issue_data));
    };
    let mut reduced = PyDict::new();
    reduced.insert(
        PyStr::new("name"),
        body.get("name")
            .cloned()
            .unwrap_or_else(|| Json::Str(PyStr::new(&issue.name))),
    );
    reduced.insert(
        PyStr::new("description_html"),
        body.get("description_html")
            .cloned()
            .unwrap_or_else(|| Json::Str(PyStr::new(&issue.description_html))),
    );
    reduced.insert(
        PyStr::new("description_json"),
        body.get("description_json")
            .cloned()
            .unwrap_or_else(|| json_column(&issue.description_json)),
    );
    Ok(reduced)
}

fn attribute_error(method: &str, value: &Json) -> ViewError {
    ViewError::Unknown(format!(
        "AttributeError: '{}' object has no attribute '{}'",
        python_type_name(value),
        method
    ))
}

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

/// Python truthiness for the values the name/priority checks see.
fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(flag) => *flag,
        Json::Int(number) => number.as_i64().is_none_or(|value| value != 0),
        Json::Float(number) => *number != 0.0,
        Json::Str(text) => text.as_str().is_some_and(|value| !value.is_empty()),
        Json::List(items) => !items.is_empty(),
        Json::Dict(map) => !map.is_empty(),
    }
}

fn is_priority(value: &Json) -> bool {
    matches!(value.as_str(), Some(text) if PRIORITIES.contains(&text))
}

/// `str(value)` for the values `Issue.name`'s `get_prep_value` can see.
fn python_str(value: Json) -> String {
    match &value {
        Json::Str(text) => text.as_str().unwrap_or_default().to_owned(),
        Json::Null => "None".to_owned(),
        Json::Bool(flag) => if *flag { "True" } else { "False" }.to_owned(),
        Json::Int(number) => number.to_string(),
        Json::Float(number) => plane_serde::float_repr(*number).unwrap_or_else(|| "nan".to_owned()),
        other => crate::write::repr::python_repr(other),
    }
}

/// `validate_html_content(raw)`'s sanitized half: `None` becomes `<p></p>`
/// (`intake.py:154-156`).
fn safe_description_html(raw: &Json) -> Result<String, ViewError> {
    if !truthy(raw) {
        return Ok("<p></p>".to_owned());
    }
    let Some(text) = raw.as_str() else {
        return Err(ViewError::Unknown(
            "AttributeError: 'dict' object has no attribute 'encode'".to_owned(),
        ));
    };
    match crate::write::description::validate_html_content(text) {
        crate::write::description::HtmlValidation::Unchanged => Ok("<p></p>".to_owned()),
        crate::write::description::HtmlValidation::Sanitized(clean) => Ok(clean),
        crate::write::description::HtmlValidation::Invalid => Ok("<p></p>".to_owned()),
    }
}

// ---------------------------------------------------------------------------
// Triage state
// ---------------------------------------------------------------------------

/// `State.objects.create(name="Triage", group="triage", sequence=65000,
/// color="#4E5355", default=False)` — H19 rewrites `sequence` when the project
/// has live non-triage states (`plane/db/models/state.py:117-126`).
async fn create_triage_state(
    client: &Client,
    project_id: Uuid,
    workspace_id: Uuid,
    user: Uuid,
) -> Result<Uuid, ViewError> {
    let statement = states::STATE_SEQUENCE_MAX.prepare(client).await?;
    let row = client.query_one(&statement, &[&project_id]).await?;
    let max_sequence: Option<f64> = row.get(0);
    let saved = hooks::state::before_save(TRIAGE_NAME, true, TRIAGE_SEQUENCE, max_sequence);

    let now = write::now();
    let state = DbState {
        created_at: now,
        updated_at: now,
        created_by_id: Some(user),
        updated_by_id: None,
        deleted_at: None,
        id: Uuid::new_v4(),
        project_id,
        workspace_id,
        name: TRIAGE_NAME.to_owned(),
        description: String::new(),
        color: TRIAGE_COLOR.to_owned(),
        slug: saved.slug,
        sequence: saved.sequence,
        group: GROUP_TRIAGE.to_owned(),
        is_triage: false,
        default: false,
        external_source: None,
        external_id: None,
    };
    let statement = states::STATE_INSERT.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &state.created_at,
                &state.updated_at,
                &state.created_by_id,
                &state.updated_by_id,
                &state.deleted_at,
                &state.id,
                &state.project_id,
                &state.workspace_id,
                &state.name,
                &state.description,
                &state.color,
                &state.slug,
                &state.sequence,
                &state.group,
                &state.is_triage,
                &state.default,
                &state.external_source,
                &state.external_id,
            ],
        )
        .await
        .map_err(integrity)?;
    Ok(state.id)
}

// ---------------------------------------------------------------------------
// Rows and rendering
// ---------------------------------------------------------------------------

struct IssueRow {
    issue: Issue,
    state: Option<StateRow>,
    project: Option<ProjectLiteRow>,
    labels: Vec<LabelRow>,
    assignees: Vec<UserLiteRow>,
    links: Vec<LinkRow>,
}

struct StateRow {
    id: Uuid,
    name: String,
    color: String,
    group: String,
}

struct LabelRow {
    id: Uuid,
    name: String,
    color: String,
}

struct LinkRow {
    id: Uuid,
    status: i64,
    duplicate_to: Option<Uuid>,
    snoozed_till: Option<DateTime<Utc>>,
    source: Option<String>,
}

/// `Issue.objects.get(pk=…, workspace_id=…, project_id=…)` plus the nested
/// blocks `IssueStateIntakeSerializer` reads.
async fn load_issue_row(
    client: &Client,
    issue_id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
) -> Result<IssueRow, ViewError> {
    let statement = queries::PUBLIC_INTAKE_ISSUE_VALUES.prepare(client).await?;
    let Some(row) = client
        .query_opt(&statement, &[&issue_id, &workspace_id, &project_id])
        .await?
    else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    let point: Option<i32> = row.try_get(4).map_err(ViewError::from)?;
    let issue = Issue {
        id: row.try_get(0).map_err(ViewError::from)?,
        created_at: row.try_get(1).map_err(ViewError::from)?,
        updated_at: row.try_get(2).map_err(ViewError::from)?,
        deleted_at: row.try_get(3).map_err(ViewError::from)?,
        point,
        name: row.try_get(5).map_err(ViewError::from)?,
        description_json: row.try_get(6).map_err(ViewError::from)?,
        description_html: row.try_get(7).map_err(ViewError::from)?,
        description_stripped: row.try_get(8).map_err(ViewError::from)?,
        description_binary: row.try_get(9).map_err(ViewError::from)?,
        priority: row.try_get(10).map_err(ViewError::from)?,
        start_date: row.try_get(11).map_err(ViewError::from)?,
        target_date: row.try_get(12).map_err(ViewError::from)?,
        sequence_id: row.try_get(13).map_err(ViewError::from)?,
        sort_order: row.try_get(14).map_err(ViewError::from)?,
        completed_at: row.try_get(15).map_err(ViewError::from)?,
        archived_at: row.try_get(16).map_err(ViewError::from)?,
        is_draft: row.try_get(17).map_err(ViewError::from)?,
        external_source: row.try_get(18).map_err(ViewError::from)?,
        external_id: row.try_get(19).map_err(ViewError::from)?,
        created_by_id: row.try_get(20).map_err(ViewError::from)?,
        updated_by_id: row.try_get(21).map_err(ViewError::from)?,
        project_id: row.try_get(22).map_err(ViewError::from)?,
        workspace_id: row.try_get(23).map_err(ViewError::from)?,
        parent_id: row.try_get(24).map_err(ViewError::from)?,
        state_id: row.try_get(25).map_err(ViewError::from)?,
        estimate_point_id: row.try_get(26).map_err(ViewError::from)?,
        type_id: row.try_get(27).map_err(ViewError::from)?,
    };
    let state = match issue.state_id {
        Some(id) => Some(StateRow {
            id,
            name: row.try_get(31).map_err(ViewError::from)?,
            color: row.try_get(32).map_err(ViewError::from)?,
            group: row.try_get(33).map_err(ViewError::from)?,
        }),
        None => None,
    };
    let project = ProjectLiteRow::from_row(&row, 34).map_err(ViewError::from)?;

    let ids = [issue_id];
    let labels = fetch_labels(client, &ids).await?;
    let assignees = fetch_assignees(client, &ids).await?;
    let links = fetch_links(client, &ids).await?;
    Ok(IssueRow {
        issue,
        state,
        project,
        labels,
        assignees,
        links,
    })
}

async fn fetch_labels(client: &Client, ids: &[Uuid]) -> Result<Vec<LabelRow>, ViewError> {
    let statement = queries::PUBLIC_INTAKE_LABELS.prepare(client).await?;
    let rows = client.query(&statement, &[&ids]).await?;
    Ok(rows
        .iter()
        .map(|row| LabelRow {
            id: row.get(1),
            name: row.get(2),
            color: row.get(3),
        })
        .collect())
}

async fn fetch_assignees(client: &Client, ids: &[Uuid]) -> Result<Vec<UserLiteRow>, ViewError> {
    let statement = queries::PUBLIC_INTAKE_ASSIGNEES.prepare(client).await?;
    let rows = client.query(&statement, &[&ids]).await?;
    rows.iter()
        .map(|row| {
            UserLiteRow::from_row(row, 1)
                .map_err(ViewError::from)?
                .ok_or_else(|| ViewError::Unknown("the assignee row has no user".to_owned()))
        })
        .collect()
}

async fn fetch_links(client: &Client, ids: &[Uuid]) -> Result<Vec<LinkRow>, ViewError> {
    let statement = queries::PUBLIC_INTAKE_LINKS.prepare(client).await?;
    let rows = client.query(&statement, &[&ids]).await?;
    Ok(rows
        .iter()
        .map(|row| LinkRow {
            id: row.get(1),
            status: i64::from(row.get::<_, i32>(2)),
            duplicate_to: row.get(3),
            snoozed_till: row.get(4),
            source: row.get(5),
        })
        .collect())
}

/// Render one `IssueStateIntakeSerializer` with the create/retrieve shape: no
/// `sub_issues_count` annotation, so the field is absent.
async fn render_entry(ctx: &ViewCtx<'_>, row: &IssueRow) -> Result<Vec<u8>, ViewError> {
    let issue = &row.issue;
    let state = row.state.as_ref().map(|state| StateLiteSerializer {
        id: state.id,
        name: &state.name,
        color: &state.color,
        group: &state.group,
    });
    let project: Option<ProjectLiteSerializer<'_>> =
        row.project.as_ref().map(ProjectLiteRow::serializer);
    let labels: Vec<LabelLiteSerializer<'_>> = row
        .labels
        .iter()
        .map(|label| LabelLiteSerializer {
            id: label.id,
            name: &label.name,
            color: &label.color,
        })
        .collect();
    let assignees: Vec<UserLiteSerializer<'_>> =
        row.assignees.iter().map(UserLiteRow::serializer).collect();
    let links: Vec<IntakeIssueLiteSerializer<'_>> = row
        .links
        .iter()
        .map(|link| IntakeIssueLiteSerializer {
            id: link.id,
            status: link.status,
            duplicate_to: link.duplicate_to,
            snoozed_till: link.snoozed_till,
            source: link.source.as_deref(),
        })
        .collect();
    let assignee_ids: Vec<Uuid> = row.assignees.iter().map(|user| user.id).collect();
    let label_ids: Vec<Uuid> = row.labels.iter().map(|label| label.id).collect();
    let serializer = IssueStateIntakeSerializer {
        id: issue.id,
        state_detail: state.as_ref(),
        project_detail: project.as_ref(),
        label_details: &labels,
        assignee_details: &assignees,
        sub_issues_count: FieldValue::Absent,
        issue_intake: &links,
        created_at: issue.created_at,
        updated_at: issue.updated_at,
        deleted_at: issue.deleted_at,
        point: issue.point.map(i64::from),
        name: &issue.name,
        description_json: Some(PgJson::from_json_text(&issue.description_json)),
        description_html: &issue.description_html,
        description_stripped: issue.description_stripped.as_deref(),
        description_binary: issue.description_binary.as_deref(),
        priority: &issue.priority,
        start_date: issue.start_date,
        target_date: issue.target_date,
        sequence_id: i64::from(issue.sequence_id),
        sort_order: issue.sort_order,
        completed_at: issue.completed_at,
        archived_at: issue.archived_at,
        is_draft: issue.is_draft,
        external_source: issue.external_source.as_deref(),
        external_id: issue.external_id.as_deref(),
        created_by: issue.created_by_id,
        updated_by: issue.updated_by_id,
        project: issue.project_id,
        workspace: issue.workspace_id,
        parent: issue.parent_id,
        state: issue.state_id,
        estimate_point: issue.estimate_point_id,
        r#type: issue.type_id,
        assignees: &assignee_ids,
        labels: &label_ids,
    };
    let prepared = plain_context(ctx);
    plane_serializers::render(&serializer, &prepared).map_err(ViewError::from)
}

// ---------------------------------------------------------------------------
// `IssueCreateSerializer.data` (36 keys)
// ---------------------------------------------------------------------------

/// `IssueCreateSerializer(issue).data` (`plane/app/serializers/issue.py:126-132`).
fn issue_create_representation(
    issue: &Issue,
    timezone: chrono_tz::Tz,
    assignees: &[Uuid],
    labels: &[Uuid],
    description_json: Doc,
    assignee_ids: Doc,
    label_ids: Doc,
) -> Doc {
    let zoned =
        |value: DateTime<Utc>| Doc::DateTimeZoned(value.with_timezone(&timezone).fixed_offset());
    let zoned_opt = |value: Option<DateTime<Utc>>| value.map_or(Doc::Null, zoned);
    Doc::Object(
        vec![
            ("id".to_owned(), Doc::uuid(issue.id)),
            ("state_id".to_owned(), Doc::opt_uuid(issue.state_id)),
            ("parent_id".to_owned(), Doc::opt_uuid(issue.parent_id)),
            ("project_id".to_owned(), Doc::uuid(issue.project_id)),
            ("workspace_id".to_owned(), Doc::uuid(issue.workspace_id)),
            ("created_at".to_owned(), zoned(issue.created_at)),
            ("updated_at".to_owned(), zoned(issue.updated_at)),
            ("deleted_at".to_owned(), zoned_opt(issue.deleted_at)),
            (
                "point".to_owned(),
                issue
                    .point
                    .map_or(Doc::Null, |value| Doc::Int(i64::from(value))),
            ),
            ("name".to_owned(), Doc::string(issue.name.as_str())),
            ("description_json".to_owned(), description_json),
            (
                "description_html".to_owned(),
                Doc::string(issue.description_html.as_str()),
            ),
            (
                "description_stripped".to_owned(),
                Doc::opt_string(issue.description_stripped.clone()),
            ),
            // `ModelField` of a `BinaryField`: NULL here because the serializer
            // never writes the column.
            ("description_binary".to_owned(), Doc::Null),
            ("priority".to_owned(), Doc::string(issue.priority.as_str())),
            ("start_date".to_owned(), Doc::opt_date(issue.start_date)),
            ("target_date".to_owned(), Doc::opt_date(issue.target_date)),
            (
                "sequence_id".to_owned(),
                Doc::Int(i64::from(issue.sequence_id)),
            ),
            ("sort_order".to_owned(), Doc::Float(issue.sort_order)),
            ("completed_at".to_owned(), zoned_opt(issue.completed_at)),
            ("archived_at".to_owned(), Doc::opt_date(issue.archived_at)),
            ("is_draft".to_owned(), Doc::Bool(issue.is_draft)),
            (
                "external_source".to_owned(),
                Doc::opt_string(issue.external_source.clone()),
            ),
            (
                "external_id".to_owned(),
                Doc::opt_string(issue.external_id.clone()),
            ),
            ("created_by".to_owned(), Doc::opt_uuid(issue.created_by_id)),
            ("updated_by".to_owned(), Doc::opt_uuid(issue.updated_by_id)),
            ("project".to_owned(), Doc::uuid(issue.project_id)),
            ("workspace".to_owned(), Doc::uuid(issue.workspace_id)),
            ("parent".to_owned(), Doc::opt_uuid(issue.parent_id)),
            ("state".to_owned(), Doc::opt_uuid(issue.state_id)),
            (
                "estimate_point".to_owned(),
                Doc::opt_uuid(issue.estimate_point_id),
            ),
            ("type".to_owned(), Doc::opt_uuid(issue.type_id)),
            (
                "assignees".to_owned(),
                Doc::Array(assignees.iter().copied().map(Doc::uuid).collect()),
            ),
            (
                "labels".to_owned(),
                Doc::Array(labels.iter().copied().map(Doc::uuid).collect()),
            ),
            ("assignee_ids".to_owned(), assignee_ids),
            ("label_ids".to_owned(), label_ids),
        ]
        .into_iter()
        .collect(),
    )
}

async fn ids_for(client: &Client, sql: &str, issue_id: Uuid) -> Result<Vec<Uuid>, ViewError> {
    let rows = client.query(sql, &[&issue_id]).await?;
    Ok(rows.iter().map(|row| row.get(0)).collect())
}

fn parse_json(value: &JsonText) -> Result<Doc, ViewError> {
    let parsed = plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8")
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(pyjson_to_doc(&parsed))
}

fn pyjson_to_doc(value: &Json) -> Doc {
    match value {
        Json::Null => Doc::Null,
        Json::Bool(flag) => Doc::Bool(*flag),
        Json::Int(number) => number.as_i64().map_or_else(
            || Doc::Float(number.to_string().parse().unwrap_or_default()),
            Doc::Int,
        ),
        Json::Float(number) => Doc::Float(*number),
        Json::Str(text) => Doc::string(text.as_str().unwrap_or_default()),
        Json::List(items) => Doc::Array(items.iter().map(pyjson_to_doc).collect()),
        Json::Dict(object) => Doc::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str().unwrap_or_default().to_owned(),
                        pyjson_to_doc(value),
                    )
                })
                .collect(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn json_column(value: &JsonText) -> Json {
    plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8").unwrap_or(Json::Null)
}

/// `json.dumps(value, cls=DjangoJSONEncoder)`.
fn django_json(value: &Json) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// Django's `IntegrityError` (SQLSTATE class 23) → the family 400.
fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// `Issue.save()`'s hook errors: a missing project is a 404, an integrity
/// failure the family 400.
fn hook_error(error: hooks::issue::IssueHookError) -> ViewError {
    match error {
        hooks::issue::IssueHookError::ProjectNotFound => ViewError::ObjectDoesNotExist,
        hooks::issue::IssueHookError::Postgres(error) => integrity(error),
        hooks::issue::IssueHookError::Db(error) => ViewError::from(error),
    }
}

use crate::issues::write_draft_queries;
