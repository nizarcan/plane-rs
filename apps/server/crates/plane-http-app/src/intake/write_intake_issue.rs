// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.intake.base.IntakeIssueViewSet.create` — the intake-issue
//! create (`plans/INTAKE.md` set B, orders 55 and 59).
//!
//! | Route | Method |
//! | --- | --- |
//! | `/api/workspaces/{slug}/projects/{project_id}/intake-issues/` | `POST` |
//! | `/api/workspaces/{slug}/projects/{project_id}/inbox-issues/` | `POST` |
//!
//! The view does not look at the path: `intake-issues` and `inbox-issues` are
//! two patterns for one view (`plane/app/urls/intake.py:26-35`, `:46-55`), so a
//! single handler serves both. `@allow_permission([ADMIN, MEMBER, GUEST])`
//! (`base.py:229`) is the dispatcher's, from `routes.json`.
//!
//! ## The view, statement for statement (`plane/app/views/intake/base.py:229-331`)
//!
//! 1. `request.data.get("issue", {}).get("name", False)` → 400
//!    `{"error": "Name is required"}`. A body whose `issue` is missing reads as
//!    `{}`; an `issue` that is not a mapping (a string from a form body, a
//!    list, `null`, a number, a file part) raises `AttributeError` → the family
//!    500. Getting here means `request.data` was parsed, so a malformed body is
//!    the parser's 400/415 first.
//! 2. `… .get("priority", "none")` not in the five priorities → 400
//!    `{"error": "Invalid priority"}`. Absent means `"none"`; `null`, `""` and
//!    any non-string are invalid.
//! 3. `Project.objects.get(pk=project_id)` → `DoesNotExist` is 404
//!    `{"error": "The required object does not exist."}`. No workspace or
//!    archived filter, as in issue create.
//! 4. `State.triage_objects.filter(project_id, workspace__slug).first()`, and
//!    when there is none, `State.objects.create(name="Triage", group="triage",
//!    sequence=65000, color="#4E5355", default=False)`. That runs **H19**: the
//!    slug becomes `slugify("Triage")` and, because the project has live
//!    non-triage states, `sequence` becomes `MAX(non-triage)+15000`; `is_triage`
//!    stays `False` (it is not passed) — an upstream oddity kept as it is.
//!    `created_by` is the caller (H01) and `workspace_id` the project's
//!    (`ProjectBaseModel.save`).
//! 5. `request.data["issue"]["state_id"] = triage_state.id` **mutates the
//!    parsed body**; the activity and description-version tasks dump the
//!    mutated body (`:283`, `:295`).
//! 6. `IssueCreateSerializer(data=request.data["issue"],
//!    context={… "allow_triage_state": True})` + `is_valid()` + `save()` —
//!    [`crate::issues::write_issue::prepare_create_from_value`] and
//!    [`crate::issues::write_issue::write_created_issue`]. An invalid body is
//!    the serializer's 400 **after** step 4's state row was committed, and
//!    after step 5 ran.
//! 7. `Intake.objects.filter(workspace__slug, project_id).first()`, then
//!    `IntakeIssue.objects.create(...)`. A project with no intake dereferences
//!    `None.id` → the family 500 **after** the issue, its `issue_sequences` row
//!    and the assignee/label rows are committed, and **before** either task.
//! 8. `issue_activity.delay(... intake=str(intake_issue.id))` (notification
//!    true), then `issue_description_version_task.delay(is_creating=True)`.
//!    Intake publishes **no** `model_activity`.
//! 9. The re-fetch with the view's own assignee filter and
//!    `IntakeIssueDetailSerializer` → **200**, not 201 (`:331`).
//!
//! ## No transactions
//!
//! `plane/app/views/intake/base.py` has no `transaction.atomic` and no
//! `select_for_update`. The handler adds none: `create_issue_row` opens its own
//! `BEGIN`/`COMMIT` and `issue_m2m` must stay outermost.
//!
//! ## Upstream quirks kept
//!
//! * a project with an intake but no triage state gets one created by a plain
//!   `POST`, and a body that then fails validation leaves it behind (a 400 that
//!   wrote a row);
//! * a project with no intake 500s after writing the work item;
//! * the fallback triage state has `group="triage"` but `is_triage=False`, so
//!   `State.objects` (non-triage) still lists it;
//! * the create re-fetch's `assignee_ids` filters
//!   `issue__assignees__member_project__is_active`, which joins
//!   `project_members` by `member_id` only — an assignee with an active
//!   membership of **any** project is listed, and a soft-deleted through row is
//!   not filtered out;
//! * the `issue` body's `state_id` is always overwritten by the triage state,
//!   while `state` (the model field, visited later by DRF) is **not** — a body
//!   may still send `state` and be validated against the triage manager.

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks;
use plane_db::models::state::State;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::IssueActivityCall;
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use uuid::Uuid;

use super::issue::{
    IntakeIssueDetailSerializer, IntakeIssueFields, IssueIntakeRow, decode_detail_issue,
    decode_issue_intake,
};
use super::queries;
use super::render::plain_context;
use super::write_intake_issue_queries as write_queries;
use crate::issues::lists::list_common::{client, kwarg_text, kwarg_uuid, user_id};
use crate::issues::{write_issue, write_support};
use crate::states::queries as state_queries;
use crate::write;

/// `IssueCreateSerializer`'s `PRIORITY_CHOICES`, in the order the view's list
/// spells them (`plane/app/views/intake/base.py:236-242`).
const PRIORITIES: [&str; 5] = ["low", "medium", "high", "urgent", "none"];

/// `SourceType.IN_APP` (`plane/db/models/intake.py:38-39`).
const SOURCE_IN_APP: &str = "IN_APP";

/// `StateGroup.TRIAGE` (`plane/db/models/state.py:20`).
const GROUP_TRIAGE: &str = "triage";

/// The fallback state's name and colour (`base.py:251-257`).
const TRIAGE_NAME: &str = "Triage";
const TRIAGE_COLOR: &str = "#4E5355";
const TRIAGE_SEQUENCE: f64 = 65_000.0;

/// Register `IntakeIssueViewSet.create`. The action is shared by orders 55 and
/// 59 (`routes.json`).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.intake.base.IntakeIssueViewSet")?
        .action("create", create)?;
    Ok(())
}

/// `IntakeIssueViewSet.create` (`plane/app/views/intake/base.py:229-331`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `request.data.get("issue", {})`, for every body shape DRF accepts.
        // A form body's value is a string; a multipart file part is merged into
        // `request.data` as a list of files. Both raise on `.get` below,
        // exactly like Python.
        let data = ctx.data()?;
        let issue = issue_body(data)?;
        let Json::Dict(issue_object) = &issue else {
            return Err(attribute_error("get", &issue));
        };

        // `if not … .get("name", False)` (`:231`).
        if !issue_object.get("name").is_some_and(write_support::truthy) {
            return Ok(write::error_body(400, "Name is required"));
        }

        // `… .get("priority", "none") not in [...]` (`:234-242`). A missing key
        // is `"none"`; every non-string value is "not in".
        if let Some(value) = issue_object.get("priority")
            && !is_priority(value)
        {
            return Ok(write::error_body(400, "Invalid priority"));
        }

        // `Project.objects.get(pk=project_id)` (`:244`).
        let statement = client
            .prepare_cached(write_queries::PROJECT_FOR_INTAKE_CREATE.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(project) = client
            .query_opt(&statement, &[&project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = project.get(0);
        let default_assignee_id: Option<Uuid> = project.get(1);

        // `State.triage_objects.filter(...).first()` (`:247-249`), then the
        // `State.objects.create(...)` fallback (`:250-257`).
        let statement = client
            .prepare_cached(write_queries::TRIAGE_STATE_FOR_CREATE.sql)
            .await
            .map_err(ViewError::from)?;
        let triage_id = match client
            .query_opt(&statement, &[&project_id, &slug])
            .await
            .map_err(ViewError::from)?
        {
            Some(row) => row.get::<_, Uuid>(0),
            None => create_triage_state(client, project_id, workspace_id, user).await?,
        };

        // `request.data["issue"]["state_id"] = triage_state.id` (`:258`).
        // Python's dict assignment keeps an existing `state_id`'s position and
        // appends it otherwise; `PyDict::insert` does the same.
        let mut issue_object = issue_object.clone();
        issue_object.insert(
            PyStr::new("state_id"),
            Json::Str(PyStr::new(&triage_id.to_string())),
        );
        let issue_value = Json::Dict(issue_object);

        // The body the two tasks dump: `json.dumps(request.data,
        // cls=DjangoJSONEncoder)`, with the mutated nested object. Only a JSON
        // object body can reach this point (a form body's `issue` is a string).
        let mut body = match data {
            Data::Json(Json::Dict(outer)) => outer.clone(),
            _ => {
                return Err(ViewError::Unknown(
                    "request.data stopped being a JSON object after the name check".to_owned(),
                ));
            }
        };
        body.insert(PyStr::new("issue"), issue_value.clone());

        // `IssueCreateSerializer(data=request.data["issue"], …allow_triage…)`
        // and `serializer.save()` (`:261-271`).
        let prepared =
            write_issue::prepare_create_from_value(ctx, client, project_id, true, &issue_value)
                .await?;
        let write_issue::Created { mut issue, extra } = match prepared {
            write_issue::PreparedCreate::Valid(created) => *created,
            write_issue::PreparedCreate::Invalid(response) => return Ok(response),
        };
        write_issue::write_created_issue(client, &mut issue, &extra, default_assignee_id, user)
            .await?;

        // `Intake.objects.filter(...).first()`; a project without one is
        // `'NoneType' object has no attribute 'id'` after the writes (`:272-279`).
        let statement = client
            .prepare_cached(queries::INTAKE_ONLY.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(intake_row) = client
            .query_opt(&statement, &[&slug, &project_id])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::Unknown(
                "'NoneType' object has no attribute 'id'".to_owned(),
            ));
        };
        let intake_id: Uuid = intake_row.get(0);

        // `IntakeIssue.objects.create(intake_id=…, project_id=…,
        // issue_id=serializer.data["id"], source=SourceType.IN_APP)`
        // (`:272-279`), after H01 and `ProjectBaseModel.save`'s workspace.
        let intake_issue_id = Uuid::new_v4();
        let now = write::now();
        let statement = client
            .prepare_cached(write_queries::INTAKE_ISSUE_INSERT.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &Some(user),
                    &Option::<Uuid>::None,
                    &Option::<chrono::DateTime<chrono::Utc>>::None,
                    &intake_issue_id,
                    &project_id,
                    &workspace_id,
                    &intake_id,
                    &issue.id,
                    &-2_i32,
                    &Option::<chrono::DateTime<chrono::Utc>>::None,
                    &Option::<Uuid>::None,
                    &Some(SOURCE_IN_APP),
                    &Option::<String>::None,
                    &Option::<String>::None,
                    &Option::<String>::None,
                    &JsonText::new("{}"),
                ],
            )
            .await
            .map_err(ViewError::from)?;

        // `issue_activity.delay(... intake=str(intake_issue.id))` (`:281-291`)
        // and `issue_description_version_task.delay(… is_creating=True)`
        // (`:293-298`), in that order. No `model_activity`.
        let requested = TaskArg::json_text_from(&Json::Dict(body))
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        let origin = write_support::base_host(ctx)?;
        write_issue::publish_issue_activity(
            ctx,
            IssueActivityCall::view_with_intake(
                write_issue::activity_base(
                    "issue.activity.created",
                    requested.clone(),
                    user,
                    &issue.id.to_string(),
                    &project_id.to_string(),
                    TaskArg::Null,
                ),
                origin,
                true,
                intake_issue_id.to_string(),
            ),
        )
        .await?;
        write_issue::publish_description_version(ctx, requested, &issue.id.to_string(), user, true)
            .await?;

        // The create re-fetch (`:300-329`) and `IntakeIssueDetailSerializer`
        // (`:330-331`), answering 200.
        let statement = client
            .prepare_cached(write_queries::INTAKE_ISSUE_FOR_CREATE.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&intake_id, &issue.id, &project_id])
            .await
            .map_err(ViewError::from)?;
        let row = match rows.as_slice() {
            [] => return Err(ViewError::ObjectDoesNotExist),
            // `QuerySet.get()` raises `MultipleObjectsReturned`, which is not an
            // `ObjectDoesNotExist`.
            [_, _, ..] => {
                return Err(ViewError::Unknown(
                    "IntakeIssue query returned multiple objects".to_owned(),
                ));
            }
            [row] => row,
        };

        // `duplicate_issue_detail` is the `duplicate_to` expansion; the insert
        // always leaves it NULL, but the re-fetch reads the row.
        let duplicate_to: Option<Uuid> = row.get(12);
        let duplicate_row = match duplicate_to {
            Some(id) => {
                let statement = client
                    .prepare_cached(queries::ISSUE_INTAKE_BY_ID.sql)
                    .await
                    .map_err(ViewError::from)?;
                match client
                    .query_opt(&statement, &[&id])
                    .await
                    .map_err(ViewError::from)?
                {
                    Some(issue) => Some(decode_issue_intake(&issue)),
                    None => {
                        return Err(ViewError::Unknown(format!(
                            "the duplicate target issue {id} does not exist"
                        )));
                    }
                }
            }
            None => None,
        };
        let duplicate_serializer = duplicate_row.as_ref().map(IssueIntakeRow::serializer);

        let label_ids: Vec<Uuid> = row.get(18);
        let assignee_ids: Vec<Uuid> = row.get(19);
        let issue_detail = decode_detail_issue(row, &label_ids, &assignee_ids);
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
            issue_detail,
        );
        let context = plain_context(ctx);
        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            plane_serializers::render(&serializer, &context)?,
        ))
    })
}

// ---------------------------------------------------------------------------
// Request body
// ---------------------------------------------------------------------------

/// `request.data.get("issue", {})` for the body shapes that can reach it.
///
/// A missing key is `{}` (`get`'s default), a JSON value is itself, and a
/// `QueryDict`'s value is its last string; a multipart file part named `issue`
/// becomes a file object in `request.data`, which is not a mapping either.
fn issue_body(data: &Data) -> Result<Json, ViewError> {
    match data {
        Data::Json(Json::Dict(object)) => {
            Ok(object.get("issue").cloned().unwrap_or_else(empty_object))
        }
        Data::Json(other) => Err(attribute_error("get", other)),
        Data::Form(query) => {
            if query.is_file("issue") {
                // DRF merges `request.FILES` into `request.data` as a list of
                // files under the field name, so `.get` is `AttributeError` on
                // a `list` (`Request._load_data_and_files`).
                return Err(ViewError::Unknown(
                    "AttributeError: 'list' object has no attribute 'get'".to_owned(),
                ));
            }
            match query.get("issue") {
                Some(value) => Ok(Json::Str(PyStr::new(value))),
                None => Ok(empty_object()),
            }
        }
    }
}

fn empty_object() -> Json {
    Json::Dict(PyDict::new())
}

/// `issue_object.get("priority", "none") in ["low", "medium", "high",
/// "urgent", "none"]`: `str` equality, so every other JSON type is "not in".
fn is_priority(value: &Json) -> bool {
    match value {
        Json::Str(text) => text.as_str().is_some_and(|text| PRIORITIES.contains(&text)),
        _ => false,
    }
}

/// The `AttributeError` Python raises for `.get` on a non-mapping, with the
/// `type(value).__name__` the message would show.
fn attribute_error(method: &str, value: &Json) -> ViewError {
    ViewError::Unknown(format!(
        "AttributeError: '{}' object has no attribute '{method}'",
        python_type_name(value)
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

// ---------------------------------------------------------------------------
// The triage fallback
// ---------------------------------------------------------------------------

/// `State.objects.create(name="Triage", group=StateGroup.TRIAGE.value,
/// project_id=project_id, workspace_id=project.workspace_id,
/// color="#4E5355", sequence=65000, default=False)`
/// (`plane/app/views/intake/base.py:250-257`) — model defaults plus H19.
///
/// The view passes `sequence=65000`, but H19 overwrites it with
/// `MAX(non-triage `states`.sequence) + 15000` whenever the project has a live
/// non-triage state (`plane/db/models/state.py:117-126`); `is_triage` is not
/// passed and stays `False`. `created_by` is the caller (H01) and the timestamps
/// are the ORM's.
async fn create_triage_state(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
    workspace_id: Uuid,
    user: Uuid,
) -> Result<Uuid, ViewError> {
    let statement = client
        .prepare_cached(state_queries::STATE_SEQUENCE_MAX.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_one(&statement, &[&project_id])
        .await
        .map_err(ViewError::from)?;
    let max_sequence: Option<f64> = row.get(0);
    let saved = hooks::state::before_save(TRIAGE_NAME, true, TRIAGE_SEQUENCE, max_sequence);

    let now = write::now();
    let state = State {
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

    let statement = client
        .prepare_cached(state_queries::STATE_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
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
        .map_err(ViewError::from)?;
    Ok(state.id)
}
