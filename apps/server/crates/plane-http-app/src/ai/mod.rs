// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `ai` (`deploy/route-groups.yaml`): the LLM assistant, workspace
//! and project scoped.
//!
//! * `POST /api/workspaces/{slug}/projects/{project_id}/ai-assistant/` —
//!   `GPTIntegrationEndpoint.post` (`plane/app/views/external/base.py:148-181`),
//!   route 51;
//! * `POST /api/workspaces/{slug}/ai-assistant/` —
//!   `WorkspaceGPTIntegrationEndpoint.post` (`external/base.py:184-212`),
//!   route 52.
//!
//! Both `BaseAPIView`s carry `@allow_permission` (`:149`, `:185`), which the
//! runtime applies from `routes.json`: ADMIN or MEMBER at PROJECT level for 51
//! and at WORKSPACE level for 52, after `IsAuthenticated` — so anonymous is the
//! authenticator's 401, a workspace member without the role is the 403 body,
//! and an unknown slug is the same 403 (the membership join finds nothing). The
//! handlers below therefore start where the Python body starts.
//!
//! ## The statement order is the contract
//!
//! `get_llm_config()` runs **first** (`:151`/`:187`), and `request.data` is not
//! touched before it (`:159`/`:195`). With no key in the instance
//! configuration — the state `configure_instance` seeds, because it writes
//! `LLM_API_KEY = encrypt_data("") = ""`
//! (`plane/utils/instance_config_variables/core.py:216-242`) — **every** body is
//! the same `400 {"error": "LLM provider API key and model are required"}`,
//! including an empty one, a JSON list, and malformed JSON. The parse error only
//! exists once a key is configured.
//!
//! Then `task = request.data.get("task", False)` and a falsy value is
//! `400 {"error": "Task is required"}` (`:159-161`). Python truthiness is
//! reproduced exactly (`[]`, `{}`, `0`, `0.0`, `""`, `null`, `false` are all
//! falsy), because `{"task": []}` is the 400, not an error.
//!
//! After that the LLM call runs, and only **then** are the workspace and the
//! project looked up (`:170-181`). Two upstream quirks are kept:
//!
//! * the project lookup is `Project.objects.get(pk=project_id)` — **not**
//!   scoped to the workspace, so a foreign project's id is serialized when the
//!   permission check happens to pass for it;
//! * `text.replace("\n", "<br/>")` runs on the SDK's `content`, which is
//!   `None` when the model answers with a null content. That is an
//!   `AttributeError` — the family 500 body
//!   (`plane/app/views/base.py:235-239`), *not* the view's own "internal
//!   error" body. `get_llm_response`'s `if not text and error` only catches the
//!   exception case, where `error` is set.
//!
//! The success path cannot be exercised by the differential harness (it needs a
//! real LLM), so it is pinned by `tests/openai_wire.rs` (the request bytes) and
//! the unit tests below (the branch table); the two 400s, the 500s that happen
//! before any HTTP call, and the permission matrix are differential
//! (`scenarios/write/ai-assistant*.yaml`).

use ::http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;
use plane_serde::pyjson::Value;
use plane_serializers::RenderCtx;
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use uuid::Uuid;

use crate::ai::config::{LlmConfig, llm_config};
use crate::ai::http as llm_http;
use crate::ai::http::LlmReply;
use crate::common::asset_url::{self, AssetRef};
use crate::common::client;
use crate::common::lookup;

pub mod config;
pub mod http;
pub mod queries;

/// The view dotted paths `routes.json` uses for the two endpoints.
const PROJECT_VIEW: &str = "plane.app.views.external.base.GPTIntegrationEndpoint";
const WORKSPACE_VIEW: &str = "plane.app.views.external.base.WorkspaceGPTIntegrationEndpoint";

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view(PROJECT_VIEW)?
        .method(Method::POST, post_project)?;
    registry
        .api_view(WORKSPACE_VIEW)?
        .method(Method::POST, post_workspace)?;
    Ok(())
}

/// `get_llm_config()` answered `(None, None, None)` (`external/base.py:151-157`,
/// `:187-193`).
fn missing_config() -> ViewResponse {
    ViewResponse::json(
        400,
        serde_json::json!({"error": "LLM provider API key and model are required"}),
    )
}

/// The view's own 500 for a failed LLM call (`:164-168`, `:200-204`).
fn internal_error() -> ViewResponse {
    ViewResponse::json(
        500,
        serde_json::json!({"error": "An internal error has occurred."}),
    )
}

/// `400 {"error": "Task is required"}` (`:159-161`, `:195-197`).
fn task_required() -> ViewResponse {
    ViewResponse::json(400, serde_json::json!({"error": "Task is required"}))
}

/// `task = request.data.get("task", False)` and `prompt =
/// request.data.get("prompt", False)`.
///
/// `request.data` is a `dict` (JSON object), a `QueryDict` (form body), or any
/// other `json.loads` value (list, string, number, `null`). Only the first two
/// have `.get`; the rest raise `AttributeError` — uncaught, so the family
/// handler answers 500 (`plane/app/views/base.py:235-239`).
fn member<'a>(data: &'a Data, key: &str) -> Result<Member<'a>, ViewError> {
    match data {
        Data::Json(value) => match value {
            Value::Dict(_) => Ok(Member::Json(value.get(key))),
            other => Err(ViewError::Unknown(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                python_type_name(other)
            ))),
        },
        Data::Form(query) => Ok(Member::Form(query.get(key))),
    }
}

/// One `request.data.get(...)` result.
enum Member<'a> {
    /// A JSON body: the value, or `None` when the key is absent (Python's
    /// `False` default, which is falsy and not a `str`).
    Json(Option<&'a Value>),
    /// A form body: `QueryDict.get` always answers a `str` or the default.
    Form(Option<&'a str>),
}

impl Member<'_> {
    /// Python truthiness of the value.
    fn truthy(&self) -> bool {
        match self {
            Self::Json(None) | Self::Form(None) => false,
            Self::Json(Some(value)) => match value {
                Value::Null => false,
                Value::Bool(value) => *value,
                Value::Int(value) => value.to_decimal_string() != "0",
                // `NaN != 0.0` is True in Python, so a NaN is truthy.
                Value::Float(value) => *value != 0.0,
                Value::Str(value) => !value.is_empty(),
                Value::List(items) => !items.is_empty(),
                Value::Dict(dict) => !dict.is_empty(),
            },
            Self::Form(Some(value)) => !value.is_empty(),
        }
    }

    /// The value as a Python `str` for the `task + "\n" + prompt` join.
    fn as_text(&self) -> Operand<'_> {
        match self {
            Self::Form(Some(value)) => Operand::Text(value),
            Self::Form(None) => Operand::NotText("bool"),
            Self::Json(Some(value)) => match value {
                Value::Str(value) => {
                    if value.has_surrogates() {
                        // `json.dumps(ensure_ascii=False)` keeps the surrogate
                        // and httpx's `.encode("utf-8")` raises
                        // `UnicodeEncodeError` inside the SDK — inside
                        // `get_llm_response`'s `try`.
                        Operand::LoneSurrogate
                    } else {
                        Operand::Text(value.as_str().unwrap_or_default())
                    }
                }
                other => Operand::NotText(python_type_name(other)),
            },
            Self::Json(None) => Operand::NotText("bool"),
        }
    }
}

/// The result of reading one operand as a `str`.
enum Operand<'a> {
    Text(&'a str),
    /// A `str` with a lone surrogate: concatenation works, encoding fails.
    LoneSurrogate,
    /// Not a `str` (`task + "\n"` raises `TypeError`, which is outside the
    /// `try` at `external/base.py:125`); holds the Python type name for the
    /// message.
    NotText(&'static str),
}

/// `type(value).__name__`, for the log message of the `AttributeError` above.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) => "str",
        Value::List(_) => "list",
        Value::Dict(_) => "dict",
    }
}

/// The `TypeError` from `task + "\n"` / `… + prompt` — uncaught, so the family
/// handler answers 500 (`plane/app/views/base.py:235-239`).
fn concatenation_error(what: &str) -> ViewError {
    ViewError::Unknown(format!(
        "TypeError: can only concatenate str (not \"{what}\") to str"
    ))
}

/// `GPTIntegrationEndpoint.post` (`external/base.py:148-181`).
fn post_project<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let prepared = match prepare_call(ctx).await? {
            Ok(prepared) => prepared,
            Err(response) => return Ok(response),
        };
        let reply = match call_llm(&prepared).await? {
            Ok(reply) => reply,
            Err(response) => return Ok(response),
        };

        // `workspace = Workspace.objects.get(slug=slug)`
        // (`external/base.py:170`): a miss is the family's
        // `404 {"error": "The required object does not exist."}`.
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)
                .map_err(ViewError::from)?;
        let db = client(ctx)?;
        let workspace = workspace_lite(db, slug).await?;
        // `project = Project.objects.get(pk=project_id)` — unscoped on purpose
        // (`external/base.py:171`).
        let project = project_lite(db, project_id).await?;

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let render_ctx = RenderCtx::full(&timezone);
        let workspace_logo_url =
            asset_url::workspace_logo_url(workspace.logo.as_deref(), &workspace.asset)?;
        let project_cover_image_url =
            asset_url::project_cover_image_url(project.cover_image.as_deref(), &project.asset)?;
        let project_detail = ProjectLiteSerializer {
            id: project.id,
            identifier: &project.identifier,
            name: &project.name,
            cover_image: project.cover_image.as_deref(),
            cover_image_url: project_cover_image_url.as_deref(),
            logo_props: Some(plane_serializers::PgJson::from_json_text(
                &project.logo_props,
            )),
            description: &project.description,
        };
        let workspace_detail = WorkspaceLiteSerializer {
            name: &workspace.name,
            slug: &workspace.slug,
            id: workspace.id,
            logo_url: workspace_logo_url.as_deref(),
        };

        let mut body = Vec::with_capacity(reply.len() * 2 + 512);
        body.extend_from_slice(b"{\"response\":");
        body.extend_from_slice(&plane_serde::to_vec(Dialect::Drf, &reply)?);
        body.extend_from_slice(b",\"response_html\":");
        body.extend_from_slice(&plane_serde::to_vec(
            Dialect::Drf,
            &reply.replace('\n', "<br/>"),
        )?);
        body.extend_from_slice(b",\"project_detail\":");
        body.extend_from_slice(&plane_serializers::render(&project_detail, &render_ctx)?);
        body.extend_from_slice(b",\"workspace_detail\":");
        body.extend_from_slice(&plane_serializers::render(&workspace_detail, &render_ctx)?);
        body.push(b'}');
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceGPTIntegrationEndpoint.post` (`external/base.py:184-212`).
fn post_workspace<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let prepared = match prepare_call(ctx).await? {
            Ok(prepared) => prepared,
            Err(response) => return Ok(response),
        };
        let reply = match call_llm(&prepared).await? {
            Ok(reply) => reply,
            Err(response) => return Ok(response),
        };

        let mut body = Vec::with_capacity(reply.len() * 2 + 64);
        body.extend_from_slice(b"{\"response\":");
        body.extend_from_slice(&plane_serde::to_vec(Dialect::Drf, &reply)?);
        body.extend_from_slice(b",\"response_html\":");
        body.extend_from_slice(&plane_serde::to_vec(
            Dialect::Drf,
            &reply.replace('\n', "<br/>"),
        )?);
        body.push(b'}');
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// The shared head of both handlers: config, then `task`, then `prompt`.
///
/// `Ok(Ok(prepared))` reaches the LLM call; `Ok(Err(response))` is one of the
/// view's own 400/500 answers; `Err(_)` is an exception the family handler
/// turns into its 500.
async fn prepare_call(ctx: &ViewCtx<'_>) -> Result<Result<Prepared, ViewResponse>, ViewError> {
    // `api_key, model, provider = get_llm_config()` before anything reads the
    // body (`external/base.py:151`, `:187`).
    let Some(config) = llm_config(ctx).await? else {
        return Ok(Err(missing_config()));
    };

    let data = ctx.data()?;
    let task = member(data, "task")?;
    if !task.truthy() {
        return Ok(Err(task_required()));
    }
    let prompt = member(data, "prompt")?;
    // The two `UnicodeEncodeError` branches are the SDK's, inside its `try`;
    // the two `TypeError` branches are the concatenation outside it.
    let task_text = match task.as_text() {
        Operand::Text(text) => text,
        Operand::LoneSurrogate => return Ok(Err(internal_error())),
        Operand::NotText(name) => return Err(concatenation_error(name)),
    };
    let prompt_text = match prompt.as_text() {
        Operand::Text(text) => text,
        Operand::LoneSurrogate => return Ok(Err(internal_error())),
        Operand::NotText(name) => return Err(concatenation_error(name)),
    };
    Ok(Ok(Prepared {
        content: format!("{task_text}\n{prompt_text}"),
        config,
    }))
}

/// Everything the LLM call needs, from one `get_llm_config()`.
struct Prepared {
    config: LlmConfig,
    /// `task + "\n" + prompt` (`external/base.py:125`).
    content: String,
}

/// `text, error = get_llm_response(...)`, then the view's `if not text and
/// error` branch (`external/base.py:163-168`, `:199-204`).
fn finish(reply: LlmReply) -> Result<Result<String, ViewResponse>, ViewError> {
    match reply {
        LlmReply::Failed(_) => Ok(Err(internal_error())),
        // `error` is `None`, so the view proceeds to `text.replace`, which is
        // an `AttributeError` on `None`: the family 500 body, not this view's.
        LlmReply::Text(None) => Err(ViewError::Unknown(
            "AttributeError: 'NoneType' object has no attribute 'replace'".to_string(),
        )),
        LlmReply::Text(Some(text)) => Ok(Ok(text)),
    }
}

/// One LLM call, with the reply mapped the way the view maps it.
async fn call_llm(prepared: &Prepared) -> Result<Result<String, ViewResponse>, ViewError> {
    finish(llm_http::get_llm_response(&prepared.content, &prepared.config).await)
}

/// `Workspace.objects.get(slug=slug)` + `Workspace.logo_url`
/// (`external/base.py:170`, `plane/db/models/workspace.py:145-154`).
struct WorkspaceLite {
    id: Uuid,
    name: String,
    slug: String,
    logo: Option<String>,
    asset: AssetRef,
}

async fn workspace_lite(
    db: &deadpool_postgres::Client,
    slug: &str,
) -> Result<WorkspaceLite, ViewError> {
    let statement = db
        .prepare_cached(queries::WORKSPACE_LITE_BY_SLUG.sql)
        .await?;
    let row = db.query_opt(&statement, &[&slug]).await?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok(WorkspaceLite {
        id: row.try_get(0)?,
        name: row.try_get(1)?,
        slug: row.try_get(2)?,
        logo: row.try_get(3)?,
        asset: AssetRef::from_row(&row, 4)?,
    })
}

/// `Project.objects.get(pk=project_id)` + `Project.cover_image_url`
/// (`external/base.py:171`, `plane/db/models/project.py:129-139`).
struct ProjectLite {
    id: Uuid,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    logo_props: JsonText,
    description: String,
    asset: AssetRef,
}

async fn project_lite(
    db: &deadpool_postgres::Client,
    project_id: Uuid,
) -> Result<ProjectLite, ViewError> {
    let statement = db.prepare_cached(queries::PROJECT_LITE_BY_PK.sql).await?;
    let row = db.query_opt(&statement, &[&project_id]).await?;
    let Some(row) = row else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok(ProjectLite {
        id: row.try_get(0)?,
        identifier: row.try_get(1)?,
        name: row.try_get(2)?,
        cover_image: row.try_get(3)?,
        logo_props: row.try_get(4)?,
        description: row.try_get(5)?,
        asset: AssetRef::from_row(&row, 6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use plane_serde::pyjson;

    fn data(body: &str) -> Data {
        Data::Json(pyjson::loads(body.as_bytes(), "utf-8").expect("json"))
    }

    /// `not task` — every Python-falsy JSON shape is the "Task is required"
    /// 400, and every other shape reaches the call.
    #[test]
    fn task_truthiness_is_python_truthiness() {
        for body in [
            r#"{}"#,
            r#"{"task": null}"#,
            r#"{"task": false}"#,
            r#"{"task": 0}"#,
            r#"{"task": 0.0}"#,
            r#"{"task": -0.0}"#,
            r#"{"task": ""}"#,
            r#"{"task": []}"#,
            r#"{"task": {}}"#,
        ] {
            let parsed = data(body);
            let task = member(&parsed, "task").expect("dict body");
            assert!(!task.truthy(), "{body}");
        }
        for body in [
            r#"{"task": "t"}"#,
            r#"{"task": 1}"#,
            r#"{"task": -1}"#,
            r#"{"task": 0.5}"#,
            r#"{"task": 1e400}"#,
            r#"{"task": [0]}"#,
            r#"{"task": {"a": false}}"#,
            r#"{"task": true}"#,
        ] {
            let parsed = data(body);
            let task = member(&parsed, "task").expect("dict body");
            assert!(task.truthy(), "{body}");
        }
    }

    /// A non-object JSON body has no `.get`: the family 500, not a 400.
    #[test]
    fn a_non_object_body_is_the_attribute_error() {
        for body in [r#"[]"#, r#""x""#, r#"5"#, r#"null"#, r#"true"#] {
            let parsed = data(body);
            assert!(member(&parsed, "task").is_err(), "{body}");
        }
    }

    /// `task + "\n"` works for a `str` and raises `TypeError` for anything else.
    /// An absent `prompt` is `False`, not `""`.
    #[test]
    fn only_strings_concatenate() {
        let parsed = data(r#"{"task": "t", "prompt": ""}"#);
        let task = member(&parsed, "task").expect("dict");
        let prompt = member(&parsed, "prompt").expect("dict");
        assert!(matches!(task.as_text(), Operand::Text("t")));
        assert!(matches!(prompt.as_text(), Operand::Text("")));

        let parsed = data(r#"{"task": "t"}"#);
        let prompt = member(&parsed, "prompt").expect("dict");
        assert!(matches!(prompt.as_text(), Operand::NotText(_)));

        let parsed = data(r#"{"task": 5, "prompt": "p"}"#);
        let task = member(&parsed, "task").expect("dict");
        assert!(matches!(task.as_text(), Operand::NotText(_)));

        let parsed = data(r#"{"task": "t", "prompt": null}"#);
        let prompt = member(&parsed, "prompt").expect("dict");
        assert!(matches!(prompt.as_text(), Operand::NotText(_)));
    }

    /// A form body's `get` answers a string, and an absent key is `False`.
    #[test]
    fn a_form_body_reads_like_a_query_dict() {
        use plane_drf::request::QueryDict;
        let form = Data::Form(QueryDict::parse("task=hello&prompt=", None).expect("form"));
        let task = member(&form, "task").expect("form");
        assert!(task.truthy());
        assert!(matches!(task.as_text(), Operand::Text("hello")));
        let prompt = member(&form, "prompt").expect("form");
        assert!(!prompt.truthy());
        assert!(matches!(prompt.as_text(), Operand::Text("")));
        let absent = member(&form, "missing").expect("form");
        assert!(matches!(absent.as_text(), Operand::NotText(_)));
    }

    /// A lone surrogate survives the JSON parse and the concatenation, so the
    /// failure is the SDK's encode: the view's own 500 body.
    #[test]
    fn a_lone_surrogate_is_the_sdk_encode_error() {
        let parsed = data(r#"{"task": "\ud800", "prompt": "p"}"#);
        let task = member(&parsed, "task").expect("dict");
        assert!(task.truthy());
        assert!(matches!(task.as_text(), Operand::LoneSurrogate));
    }

    #[test]
    fn the_two_error_bodies_are_the_upstream_literals() {
        let render = |response: ViewResponse| {
            let rendered = plane_drf::view::DrfResponse::render(response).expect("render");
            String::from_utf8(rendered.body).expect("utf8")
        };
        assert_eq!(
            render(missing_config()),
            r#"{"error":"LLM provider API key and model are required"}"#
        );
        assert_eq!(render(task_required()), r#"{"error":"Task is required"}"#);
        assert_eq!(
            render(internal_error()),
            r#"{"error":"An internal error has occurred."}"#
        );
    }

    /// `if not text and error` (`external/base.py:164`, `:200`):
    ///
    /// * an exception (`error` set) is this view's 500;
    /// * a `null` content is `(None, None)` — the branch is False, and
    ///   `text.replace` raises outside it, so the family 500 (a different body)
    ///   answers;
    /// * an empty string is `("", None)` — also False, but `.replace` works, so
    ///   the view answers 200 with an empty `response`.
    #[test]
    fn the_llm_reply_maps_to_the_upstream_branches() {
        let failed = "Error occurred while generating response from openai";
        assert!(matches!(
            finish(LlmReply::Failed(failed.to_string())),
            Ok(Err(ViewResponse::Json { status: 500, .. }))
        ));
        assert!(matches!(
            finish(LlmReply::Text(None)),
            Err(ViewError::Unknown(ref message)) if message.contains("AttributeError")
        ));
        assert!(matches!(
            finish(LlmReply::Text(Some(String::new()))),
            Ok(Ok(ref text)) if text.is_empty()
        ));
        assert!(matches!(
            finish(LlmReply::Text(Some("hi".to_string()))),
            Ok(Ok(ref text)) if text == "hi"
        ));
    }
}
