// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/api/views/instance.py`: routes 259 and 269.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/instances/` | `InstanceEndpoint.get` | `instance.py:34-158` |
//! | `PATCH /api/instances/` | `InstanceEndpoint.patch` | `instance.py:160-169` |
//! | `POST /api/instances/admins/sign-up-screen-visited/` | `SignUpScreenVisitedEndpoint.post` | `instance.py:172-186` |
//!
//! ## The cache and its missing header
//!
//! `@cache_response(60 * 60 * 2, user=False)` is the **outer** decorator and
//! `@method_decorator(cache_control(private=True, max_age=12))` the inner one
//! (`instance.py:34-35`), so a hit returns a fresh `Response` built from the
//! cached `(data, status)` and never runs `cache_control`: the second request
//! for the same path loses `Cache-Control` while the first has it. Ported
//! as-is; [`get`] therefore drives `plane_cache::drf` directly instead of
//! [`crate::common::cache::cache_response`], which cannot say whether it hit.
//!
//! ## What the body reads
//!
//! Thirteen `get_configuration_value` keys, five `settings.*` values, then
//! `str(GITHUB_APP_NAME)`, `float(FILE_SIZE_LIMIT)` from the **environment**
//! (not the configuration table), `bool(...)` twice and
//! `Workspace.objects.count() >= 1` (`instance.py:49-158`).
//! `primary_owner_details` is always absent: the serializer declares it with
//! `source="primary_owner"` and the model has no such attribute, so DRF's
//! `get_attribute` raises `SkipField` (`rest_framework/fields.py:421-446`).
//! `data = serializer.data` at `:46-48` is dead code and not ported; the
//! second assignment at `:143` is the live one.

use plane_cache::PyValue;
use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_db::models::license::instance::Instance;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;
use plane_serializers::{
    DrfError, DrfSerialize, DrfUuid, DrfWriter, FilterMode, RenderCtx, drf_datetime,
    drf_datetime_opt,
};

use crate::common::cache;
use crate::common::client;
use crate::instances::queries;
use crate::write;
use crate::write::fields::Detail;
use crate::write::serializer::ValidationErrors;

/// Register the routes this module owns.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.license.api.views.instance.InstanceEndpoint")?
        .method(http::Method::GET, get)?
        .method(http::Method::PATCH, patch)?;
    registry
        .api_view("plane.license.api.views.instance.SignUpScreenVisitedEndpoint")?
        .method(http::Method::POST, sign_up_screen_visited)?;
    Ok(())
}

/// `@cache_response(60 * 60 * 2, user=False)` — the decorator's timeout.
const CACHE_SECONDS: i64 = 60 * 60 * 2;

/// `GET /api/instances/` (`instance.py:34-158`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let redis = ctx.redis.ok_or_else(|| {
            ViewError::Unknown(
                "the process has no Redis cache handle; django-redis would fail the request"
                    .to_string(),
            )
        })?;
        // `request.get_full_path()`, `user=False`: no user id in the key.
        let full_path = plane_cache::drf::full_path(ctx.path(), &ctx.ctx.query_string);
        let facts = plane_cache::drf::RequestFacts {
            full_path: &full_path,
            resolver_kwargs: &[],
            user_id: None,
        };
        let key = plane_cache::drf::response_cache_key(None, false, &facts);
        if let Some(cached) = plane_cache::drf::lookup(redis, &key).await? {
            return render_cached(cached.status, &cached.data);
        }
        let (status, data) = compute(ctx).await?;
        plane_cache::drf::store(
            redis,
            &key,
            &data,
            status,
            CACHE_SECONDS,
            ctx.settings.debug,
        )
        .await?;
        let response = render_cached(status, &data)?;
        // The inner `@cache_control(private=True, max_age=12)`.
        Ok(response.with_header("Cache-Control", "private, max-age=12"))
    })
}

/// `Response(data, status)` for the cached `(data, status)` pair.
fn render_cached(status: u16, data: &PyValue) -> Result<ViewResponse, ViewError> {
    if matches!(data, PyValue::None) {
        return Ok(ViewResponse::empty(status));
    }
    let body = plane_serde::to_vec(Dialect::Drf, data)?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// The miss path (`instance.py:39-158`).
async fn compute(ctx: &ViewCtx<'_>) -> Result<(u16, PyValue), ViewError> {
    let client = client(ctx)?;
    let Some(instance) = instance_first(client).await? else {
        // `Instance is None` (`:41-45`).
        return Ok((
            200,
            PyValue::dict(vec![
                ("is_activated", PyValue::bool(false)),
                ("is_setup_done", PyValue::bool(false)),
            ]),
        ));
    };

    // `get_configuration_value([...])` (`:49-118`): each default is the call
    // site's own `os.environ.get(key, default)`, which
    // `configuration_values` applies before the fallback.
    let values = plane_auth::instance_config::configuration_values(
        client,
        ctx.settings,
        &[
            ("ENABLE_SIGNUP", Some("0")),
            ("DISABLE_WORKSPACE_CREATION", Some("0")),
            ("IS_GOOGLE_ENABLED", Some("0")),
            ("IS_GITHUB_ENABLED", Some("0")),
            ("GITHUB_APP_NAME", Some("")),
            ("IS_GITLAB_ENABLED", Some("0")),
            ("IS_GITEA_ENABLED", Some("0")),
            ("EMAIL_HOST", Some("")),
            ("ENABLE_MAGIC_LINK_LOGIN", Some("1")),
            ("ENABLE_EMAIL_PASSWORD", Some("1")),
            ("SLACK_CLIENT_ID", None),
            ("UNSPLASH_ACCESS_KEY", Some("")),
            ("LLM_API_KEY", Some("")),
        ],
    )
    .await
    .map_err(|error| ViewError::Unknown(error.to_string()))?;
    let value = |index: usize| values.get(index).and_then(Option::as_deref);

    // `data["..."] = X == "1"` (`:121-128`).
    // The indices are the tuple's order above.
    let enable_signup = flag_index(&values, 0) == Some("1");
    let disable_workspace_creation = flag_index(&values, 1) == Some("1");
    let is_google_enabled = flag_index(&values, 2) == Some("1");
    let is_github_enabled = flag_index(&values, 3) == Some("1");
    let is_gitlab_enabled = flag_index(&values, 5) == Some("1");
    let is_gitea_enabled = flag_index(&values, 6) == Some("1");
    let is_magic_login_enabled = flag_index(&values, 8) == Some("1");
    let is_email_password_enabled = flag_index(&values, 9) == Some("1");

    // `data["github_app_name"] = str(GITHUB_APP_NAME)` (`:131`).
    let github_app_name = match value(4) {
        Some(text) => text.to_string(),
        None => "None".to_string(),
    };
    let slack_client_id: Option<&str> = value(10);
    let has_unsplash_configured = value(11).is_some_and(|text| !text.is_empty());
    let has_llm_configured = value(12).is_some_and(|text| !text.is_empty());
    let is_smtp_configured = value(7).is_some_and(|text| !text.is_empty());

    // `float(os.environ.get("FILE_SIZE_LIMIT", 5242880))` (`:140`). This reads
    // the **environment**, not instance_configurations; a non-numeric value is
    // an uncaught `ValueError` and therefore a 500 (`ENV.md` names it).
    let file_size_limit = file_size_limit()?;

    let config = PyValue::dict(vec![
        ("enable_signup", PyValue::bool(enable_signup)),
        (
            "is_workspace_creation_disabled",
            PyValue::bool(disable_workspace_creation),
        ),
        ("is_google_enabled", PyValue::bool(is_google_enabled)),
        ("is_github_enabled", PyValue::bool(is_github_enabled)),
        ("is_gitlab_enabled", PyValue::bool(is_gitlab_enabled)),
        ("is_gitea_enabled", PyValue::bool(is_gitea_enabled)),
        (
            "is_magic_login_enabled",
            PyValue::bool(is_magic_login_enabled),
        ),
        (
            "is_email_password_enabled",
            PyValue::bool(is_email_password_enabled),
        ),
        ("github_app_name", PyValue::string(github_app_name)),
        (
            "slack_client_id",
            slack_client_id.map_or(PyValue::None, PyValue::string),
        ),
        (
            "has_unsplash_configured",
            PyValue::bool(has_unsplash_configured),
        ),
        ("has_llm_configured", PyValue::bool(has_llm_configured)),
        ("file_size_limit", PyValue::Float(file_size_limit)),
        ("is_smtp_configured", PyValue::bool(is_smtp_configured)),
        (
            "admin_base_url",
            option_py(ctx.settings.urls.admin_base_url.as_deref()),
        ),
        (
            "space_base_url",
            option_py(ctx.settings.urls.space_base_url.as_deref()),
        ),
        (
            "app_base_url",
            option_py(ctx.settings.urls.app_base_url.as_deref()),
        ),
        (
            "instance_changelog_url",
            PyValue::string(ctx.settings.urls.instance_changelog_url.clone()),
        ),
        // `IS_SELF_MANAGED = True` is a constant in `settings/common.py:55`.
        ("is_self_managed", PyValue::bool(true)),
    ]);

    // `instance_data = serializer.data; instance_data["workspaces_exist"] = …`
    // (`:150-158`). `serializer.data` is a `ReturnDict`, which is what makes
    // the whole cached frame unreadable to the harness's pickle decoder; the
    // `builtins.dict` reduce is reproduced here for the same bytes.
    let count = {
        let statement = client.prepare_cached(queries::WORKSPACE_COUNT.sql).await?;
        let row = client.query_one(&statement, &[]).await?;
        row.get::<_, i64>(0)
    };
    let instance = PyValue::return_mapping(instance_fields(&instance, ctx, count >= 1));
    Ok((
        200,
        PyValue::dict(vec![("config", config), ("instance", instance)]),
    ))
}

fn flag_index(values: &[Option<String>], index: usize) -> Option<&str> {
    values.get(index).and_then(Option::as_deref)
}

fn option_py(value: Option<&str>) -> PyValue {
    value.map_or(PyValue::None, PyValue::string)
}

/// `float(os.environ.get("FILE_SIZE_LIMIT", 5242880))`.
fn file_size_limit() -> Result<f64, ViewError> {
    let raw = std::env::var("FILE_SIZE_LIMIT").ok();
    let Some(raw) = raw else {
        return Ok(5_242_880.0);
    };
    // CPython's `float(text)`: surrounding whitespace allowed and single
    // underscores between digits ignored.
    let trimmed = raw.trim();
    let cleaned: String = trimmed
        .chars()
        .filter(|character| *character != '_')
        .collect();
    cleaned.parse::<f64>().map_err(|_| {
        ViewError::Unknown(format!(
            "ValueError: could not convert string to float: {raw:?}"
        ))
    })
}

/// `Instance.objects.first()`.
pub(crate) async fn instance_first(
    client: &deadpool_postgres::Client,
) -> Result<Option<Instance>, ViewError> {
    let statement = client.prepare_cached(queries::INSTANCE_FIRST.sql).await?;
    match client.query_opt(&statement, &[]).await? {
        Some(row) => Ok(Some(Instance::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// The fields `InstanceSerializer` renders, in `_readable_fields` order, with
/// `primary_owner_details` skipped (`rest_framework/fields.py:421-446`).
pub(crate) fn instance_view<'a>(instance: &'a Instance, ctx: &'a ViewCtx<'a>) -> InstanceView<'a> {
    InstanceView {
        instance,
        tz: ctx.timezone.copied().unwrap_or(chrono_tz::UTC),
    }
}

/// One `InstanceSerializer(instance)` render.
#[derive(Debug)]
pub(crate) struct InstanceView<'a> {
    instance: &'a Instance,
    tz: chrono_tz::Tz,
}

impl DrfSerialize for InstanceView<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = self.instance;
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
        // `primary_owner_details` has no attribute on the model: SkipField.
        obj.field_dyn("created_at", ctx, &drf_datetime(row.created_at, &self.tz))?;
        obj.field_dyn("updated_at", ctx, &drf_datetime(row.updated_at, &self.tz))?;
        obj.field_dyn(
            "deleted_at",
            ctx,
            &drf_datetime_opt(row.deleted_at, &self.tz),
        )?;
        obj.field_dyn("instance_name", ctx, &row.instance_name)?;
        obj.field_dyn("whitelist_emails", ctx, &row.whitelist_emails)?;
        obj.field_dyn("instance_id", ctx, &row.instance_id)?;
        obj.field_dyn("current_version", ctx, &row.current_version)?;
        obj.field_dyn("latest_version", ctx, &row.latest_version)?;
        obj.field_dyn("edition", ctx, &row.edition)?;
        obj.field_dyn("domain", ctx, &row.domain)?;
        obj.field_dyn(
            "last_checked_at",
            ctx,
            &drf_datetime(row.last_checked_at, &self.tz),
        )?;
        obj.field_dyn("namespace", ctx, &row.namespace)?;
        obj.field_dyn("is_telemetry_enabled", ctx, &row.is_telemetry_enabled)?;
        obj.field_dyn("is_support_required", ctx, &row.is_support_required)?;
        obj.field_dyn("is_setup_done", ctx, &row.is_setup_done)?;
        obj.field_dyn(
            "is_signup_screen_visited",
            ctx,
            &row.is_signup_screen_visited,
        )?;
        obj.field_dyn("is_verified", ctx, &row.is_verified)?;
        obj.field_dyn("is_test", ctx, &row.is_test)?;
        obj.field_dyn(
            "is_current_version_deprecated",
            ctx,
            &row.is_current_version_deprecated,
        )?;
        obj.field_dyn("created_by", ctx, &row.created_by_id.map(DrfUuid))?;
        obj.field_dyn("updated_by", ctx, &row.updated_by_id.map(DrfUuid))?;
        ctx.expansions().write_injected(
            &mut obj,
            ctx,
            &[
                "id",
                "created_at",
                "updated_at",
                "deleted_at",
                "instance_name",
                "whitelist_emails",
                "instance_id",
                "current_version",
                "latest_version",
                "edition",
                "domain",
                "last_checked_at",
                "namespace",
                "is_telemetry_enabled",
                "is_support_required",
                "is_setup_done",
                "is_signup_screen_visited",
                "is_verified",
                "is_test",
                "is_current_version_deprecated",
                "created_by",
                "updated_by",
            ],
            FilterMode::None,
        )?;
        obj.end();
        Ok(())
    }
}

/// The same fields as [`InstanceView`], as the `PyValue` tree
/// `serializer.data` pickles to (for `@cache_response`).
fn instance_fields(
    row: &Instance,
    ctx: &ViewCtx<'_>,
    workspaces_exist: bool,
) -> Vec<(PyValue, PyValue)> {
    let tz = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let datetime = |value: chrono::DateTime<chrono::Utc>| {
        PyValue::string(drf_datetime(value, &tz).to_drf_string())
    };
    let datetime_opt = |value: Option<chrono::DateTime<chrono::Utc>>| {
        value.map_or(PyValue::None, |value| {
            PyValue::string(drf_datetime(value, &tz).to_drf_string())
        })
    };
    let string = |value: &str| PyValue::string(value);
    let string_opt = |value: Option<&str>| value.map_or(PyValue::None, PyValue::string);
    let uuid = |value: uuid::Uuid| PyValue::uuid(value);
    let uuid_opt = |value: Option<uuid::Uuid>| value.map_or(PyValue::None, PyValue::uuid);
    vec![
        (PyValue::string("id"), uuid(row.id)),
        (PyValue::string("created_at"), datetime(row.created_at)),
        (PyValue::string("updated_at"), datetime(row.updated_at)),
        (PyValue::string("deleted_at"), datetime_opt(row.deleted_at)),
        (PyValue::string("instance_name"), string(&row.instance_name)),
        (
            PyValue::string("whitelist_emails"),
            string_opt(row.whitelist_emails.as_deref()),
        ),
        (PyValue::string("instance_id"), string(&row.instance_id)),
        (
            PyValue::string("current_version"),
            string(&row.current_version),
        ),
        (
            PyValue::string("latest_version"),
            string_opt(row.latest_version.as_deref()),
        ),
        (PyValue::string("edition"), string(&row.edition)),
        (PyValue::string("domain"), string(&row.domain)),
        (
            PyValue::string("last_checked_at"),
            datetime(row.last_checked_at),
        ),
        (
            PyValue::string("namespace"),
            string_opt(row.namespace.as_deref()),
        ),
        (
            PyValue::string("is_telemetry_enabled"),
            PyValue::bool(row.is_telemetry_enabled),
        ),
        (
            PyValue::string("is_support_required"),
            PyValue::bool(row.is_support_required),
        ),
        (
            PyValue::string("is_setup_done"),
            PyValue::bool(row.is_setup_done),
        ),
        (
            PyValue::string("is_signup_screen_visited"),
            PyValue::bool(row.is_signup_screen_visited),
        ),
        (
            PyValue::string("is_verified"),
            PyValue::bool(row.is_verified),
        ),
        (PyValue::string("is_test"), PyValue::bool(row.is_test)),
        (
            PyValue::string("is_current_version_deprecated"),
            PyValue::bool(row.is_current_version_deprecated),
        ),
        (PyValue::string("created_by"), uuid_opt(row.created_by_id)),
        (PyValue::string("updated_by"), uuid_opt(row.updated_by_id)),
        (
            PyValue::string("workspaces_exist"),
            PyValue::bool(workspaces_exist),
        ),
    ]
}

/// `PATCH /api/instances/` (`instance.py:160-169`).
///
/// `@invalidate_cache(path="/api/instances/", user=False)` runs before the
/// body; `InstanceSerializer(instance, data=request.data, partial=True)` then
/// validates and `serializer.save()` writes every column (Django's
/// `Model.save` has no `update_fields`), with H01's `updated_at`/`updated_by`.
fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // The decorator's body.
        cache::invalidate_cache(ctx, Some("/api/instances/"), false, false, false).await?;
        let client = client(ctx)?;
        let Some(mut instance) = instance_first(client).await? else {
            // `InstanceSerializer(None, …)` — the endpoint has no `None`
            // guard, and DRF would answer 400 before writing anything.
            return Err(ViewError::Unknown(
                "InstanceEndpoint.patch with no instance row".to_string(),
            ));
        };
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let body = ctx.data()?;
        let Some(fields) = patch_fields(body) else {
            let mut errors = ValidationErrors::new(
                "non_field_errors",
                Detail::new(
                    format!(
                        "Invalid data. Expected a dictionary, but got {}.",
                        body_type_name(body)
                    ),
                    "invalid",
                ),
            );
            let order: Vec<&str> = InstanceView::FIELDS.to_vec();
            errors.sort_by_field_order(&order);
            return Ok(errors.into_response());
        };

        let mut errors = ValidationErrors::default();
        let mut values: Vec<(&'static str, PatchValue)> = Vec::new();
        for field in WRITABLE_FIELDS {
            let Some(raw) = fields.get(field) else {
                continue;
            };
            match patch_value(field, raw) {
                Ok(value) => values.push((field, value)),
                Err(detail) => errors.push(field, detail),
            }
        }
        if !errors.is_empty() {
            errors.sort_by_field_order(InstanceView::FIELDS);
            return Ok(errors.into_response());
        }
        for (field, value) in values {
            match value {
                PatchValue::Text(text) => set_text(&mut instance, field, text),
                PatchValue::OptText(text) => set_text_opt(&mut instance, field, text),
                PatchValue::Bool(value) => set_bool(&mut instance, field, value),
                PatchValue::DateTime(value) => instance.deleted_at = value,
            }
        }
        // `instance.save()`: `updated_at` (auto_now) and H01's `updated_by`
        // = the authenticated caller; `created_by` is untouched.
        instance.updated_at = write::now();
        instance.updated_by_id = Some(user);
        save_instance(client, &instance).await?;
        let body = crate::common::render(ctx, &instance_view(&instance, ctx))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

impl InstanceView<'_> {
    /// The serializer's `_readable_fields` order (`SERIALIZERS.md`).
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "primary_owner_details",
        "created_at",
        "updated_at",
        "deleted_at",
        "instance_name",
        "whitelist_emails",
        "instance_id",
        "current_version",
        "latest_version",
        "edition",
        "domain",
        "last_checked_at",
        "namespace",
        "is_telemetry_enabled",
        "is_support_required",
        "is_setup_done",
        "is_signup_screen_visited",
        "is_verified",
        "is_test",
        "is_current_version_deprecated",
        "created_by",
        "updated_by",
    ];
}

/// The writable half of [`InstanceView::FIELDS`], in that order.
/// `read_only_fields = ["id", "email", "last_checked_at", "is_setup_done"]`
/// (`license/api/serializers/instance.py:13`) plus `created_at`/`updated_at`
/// and the two audit FKs from `BaseModel`.
const WRITABLE_FIELDS: &[&str] = &[
    "deleted_at",
    "instance_name",
    "whitelist_emails",
    "instance_id",
    "current_version",
    "latest_version",
    "edition",
    "domain",
    "namespace",
    "is_telemetry_enabled",
    "is_support_required",
    "is_signup_screen_visited",
    "is_verified",
    "is_test",
    "is_current_version_deprecated",
];

/// One validated PATCH field.
enum PatchValue {
    Text(String),
    OptText(Option<String>),
    Bool(bool),
    DateTime(Option<chrono::DateTime<chrono::Utc>>),
}

fn set_text(instance: &mut Instance, field: &str, value: String) {
    match field {
        "instance_name" => instance.instance_name = value,
        "instance_id" => instance.instance_id = value,
        "current_version" => instance.current_version = value,
        "edition" => instance.edition = value,
        "domain" => instance.domain = value,
        _ => {}
    }
}

fn set_text_opt(instance: &mut Instance, field: &str, value: Option<String>) {
    match field {
        "whitelist_emails" => instance.whitelist_emails = value,
        "latest_version" => instance.latest_version = value,
        "namespace" => instance.namespace = value,
        _ => {}
    }
}

fn set_bool(instance: &mut Instance, field: &str, value: bool) {
    match field {
        "is_telemetry_enabled" => instance.is_telemetry_enabled = value,
        "is_support_required" => instance.is_support_required = value,
        "is_signup_screen_visited" => instance.is_signup_screen_visited = value,
        "is_verified" => instance.is_verified = value,
        "is_test" => instance.is_test = value,
        "is_current_version_deprecated" => instance.is_current_version_deprecated = value,
        _ => {}
    }
}

/// The request body as a field lookup, or `None` when it is not a mapping
/// (`Serializer.to_internal_value`'s first check).
fn patch_fields(data: &plane_drf::parsers::Data) -> Option<FieldMap<'_>> {
    match data {
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Dict(dict)) => {
            Some(FieldMap::Json(dict))
        }
        plane_drf::parsers::Data::Form(query) => Some(FieldMap::Form(query)),
        _ => None,
    }
}

enum FieldMap<'a> {
    Json(&'a plane_serde::pyjson::PyDict),
    Form(&'a plane_drf::QueryDict),
}

impl FieldMap<'_> {
    fn get(&self, key: &str) -> Option<FieldRaw<'_>> {
        match self {
            Self::Json(dict) => dict.get(key).map(FieldRaw::Json),
            Self::Form(query) => query.get(key).map(FieldRaw::Text),
        }
    }
}

#[derive(Clone, Copy)]
enum FieldRaw<'a> {
    Json(&'a plane_serde::pyjson::Value),
    Text(&'a str),
}

fn body_type_name(data: &plane_drf::parsers::Data) -> &'static str {
    match data {
        plane_drf::parsers::Data::Form(_) => "dict",
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Null) => "NoneType",
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Bool(_)) => "bool",
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Int(_)) => "int",
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Float(_)) => "float",
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Str(_)) => "str",
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::List(_)) => "list",
        plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Dict(_)) => "dict",
    }
}

/// One field's `to_internal_value`, for the field classes this serializer has.
fn patch_value(field: &'static str, raw: FieldRaw<'_>) -> Result<PatchValue, Detail> {
    // `deleted_at` is the only DateTimeField in the writable set.
    if field == "deleted_at" {
        return match raw {
            FieldRaw::Json(plane_serde::pyjson::Value::Null) => Ok(PatchValue::DateTime(None)),
            FieldRaw::Json(plane_serde::pyjson::Value::Str(text)) => {
                parse_datetime(text.as_str().ok_or_else(datetime_invalid)?)
                    .map(|value| PatchValue::DateTime(Some(value)))
                    .ok_or_else(datetime_invalid)
            }
            FieldRaw::Text(text) => parse_datetime(text)
                .map(|value| PatchValue::DateTime(Some(value)))
                .ok_or_else(datetime_invalid),
            _ => Err(datetime_invalid()),
        };
    }
    if BOOL_FIELDS.contains(&field) {
        return parse_bool(raw)
            .map(PatchValue::Bool)
            .ok_or_else(|| Detail::new("Must be a valid boolean.", "invalid"));
    }
    // Char/Text fields: null only where the model column is nullable.
    let nullable = matches!(field, "whitelist_emails" | "latest_version" | "namespace");
    let allow_blank = matches!(
        field,
        "whitelist_emails" | "latest_version" | "domain" | "namespace"
    );
    let text = match raw {
        FieldRaw::Json(plane_serde::pyjson::Value::Null) => {
            if nullable {
                return Ok(PatchValue::OptText(None));
            }
            return Err(Detail::new("This field may not be null.", "null"));
        }
        FieldRaw::Json(plane_serde::pyjson::Value::Str(value)) => value
            .as_str()
            .ok_or_else(|| Detail::new("Not a valid string.", "invalid"))?,
        FieldRaw::Json(plane_serde::pyjson::Value::Bool(value)) => {
            if *value {
                "True"
            } else {
                "False"
            }
        }
        FieldRaw::Json(plane_serde::pyjson::Value::Int(value)) => {
            return Ok(PatchValue::OptText(Some(value.to_string())));
        }
        FieldRaw::Json(plane_serde::pyjson::Value::Float(value)) => {
            return Ok(PatchValue::OptText(Some(value.to_string())));
        }
        FieldRaw::Text(text) => text,
        _ => return Err(Detail::new("Not a valid string.", "invalid")),
    };
    // `CharField`'s `trim_whitespace=True`.
    let text = text.trim();
    if text.is_empty() && !allow_blank {
        return Err(Detail::new("This field may not be blank.", "blank"));
    }
    if field == "instance_name" && text.chars().count() > 255 {
        return Err(Detail::new(
            "Ensure this field has no more than 255 characters.",
            "max_length",
        ));
    }
    if field == "instance_id" && text.chars().count() > 255 {
        return Err(Detail::new(
            "Ensure this field has no more than 255 characters.",
            "max_length",
        ));
    }
    if nullable && matches!(field, "whitelist_emails" | "latest_version" | "namespace") {
        Ok(PatchValue::OptText(Some(text.to_string())))
    } else {
        Ok(PatchValue::Text(text.to_string()))
    }
}

const BOOL_FIELDS: &[&str] = &[
    "is_telemetry_enabled",
    "is_support_required",
    "is_signup_screen_visited",
    "is_verified",
    "is_test",
    "is_current_version_deprecated",
];

/// `BooleanField.to_internal_value` (`rest_framework/fields.py:801-820`).
fn parse_bool(raw: FieldRaw<'_>) -> Option<bool> {
    match raw {
        FieldRaw::Json(plane_serde::pyjson::Value::Bool(value)) => Some(*value),
        FieldRaw::Json(plane_serde::pyjson::Value::Int(value)) => match value.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        FieldRaw::Json(plane_serde::pyjson::Value::Float(value)) => {
            if *value == 0.0 {
                Some(false)
            } else if *value == 1.0 {
                Some(true)
            } else {
                None
            }
        }
        FieldRaw::Json(plane_serde::pyjson::Value::Str(text)) => parse_bool_text(text.as_str()?),
        FieldRaw::Text(text) => parse_bool_text(text),
        _ => None,
    }
}

fn parse_bool_text(text: &str) -> Option<bool> {
    match text {
        "true" | "True" | "1" => Some(true),
        "false" | "False" | "0" => Some(false),
        _ => None,
    }
}

fn datetime_invalid() -> Detail {
    Detail::new(
        "Datetime has wrong format. Use one of these formats instead: ISO-8601.",
        "invalid",
    )
}

/// `DateTimeField.to_internal_value` for the ISO-8601 subset DRF accepts.
pub(crate) fn parse_datetime(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
    if let Ok(value) = DateTime::parse_from_rfc3339(text) {
        return Some(value.with_timezone(&Utc));
    }
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(value) = NaiveDateTime::parse_from_str(text, format) {
            // DRF makes a naive datetime aware in the active timezone; the
            // testtimezone is UTC and this endpoint has no other users.
            return Utc.from_local_datetime(&value).single();
        }
    }
    None
}

/// `instance.save()` (`plane/db/models/base.py:23-45` + `auto_now`).
pub(crate) async fn save_instance(
    client: &deadpool_postgres::Client,
    instance: &Instance,
) -> Result<(), ViewError> {
    let statement = client.prepare_cached(queries::INSTANCE_SAVE.sql).await?;
    client
        .execute(
            &statement,
            &[
                &instance.created_at,
                &instance.updated_at,
                &instance.created_by_id,
                &instance.updated_by_id,
                &instance.deleted_at,
                &instance.instance_name,
                &instance.whitelist_emails,
                &instance.instance_id,
                &instance.current_version,
                &instance.latest_version,
                &instance.edition,
                &instance.domain,
                &instance.last_checked_at,
                &instance.namespace,
                &instance.is_telemetry_enabled,
                &instance.is_support_required,
                &instance.is_setup_done,
                &instance.is_signup_screen_visited,
                &instance.is_verified,
                &instance.is_test,
                &instance.is_current_version_deprecated,
                &instance.id,
            ],
        )
        .await?;
    Ok(())
}

/// `POST /api/instances/admins/sign-up-screen-visited/` (`instance.py:172-186`).
///
/// `AllowAny`; `@invalidate_cache(path="/api/instances/", user=False)` runs
/// before the body. `instance.save()` with an anonymous caller clears
/// `updated_by` (`plane/db/models/base.py:31-33`) and moves `updated_at`.
fn sign_up_screen_visited<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        cache::invalidate_cache(ctx, Some("/api/instances/"), false, false, false).await?;
        let client = client(ctx)?;
        let Some(mut instance) = instance_first(client).await? else {
            return Ok(error_body(400, "Instance is not configured"));
        };
        instance.is_signup_screen_visited = true;
        instance.updated_at = write::now();
        instance.updated_by_id = None;
        save_instance(client, &instance).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `Response({"error": …}, status=…)`, built in Python's key order.
pub(crate) fn error_body(status: u16, message: &str) -> ViewResponse {
    let body = Object::new().field("error", message);
    let bytes = plane_serde::to_vec(Dialect::Drf, &body).unwrap_or_default();
    ViewResponse::raw(status, APPLICATION_JSON, bytes)
}
