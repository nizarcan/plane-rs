// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/api/views/workspace.py`: routes 271 and 272.
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET /api/instances/workspace-slug-check/` | `InstanceWorkSpaceAvailabilityCheckEndpoint.get` | `workspace.py:17-34` |
//! | `GET /api/instances/workspaces/` | `InstanceWorkSpaceEndpoint.get` | `workspace.py:37-70` |
//! | `POST /api/instances/workspaces/` | `InstanceWorkSpaceEndpoint.post` | `workspace.py:72-112` |
//!
//! ## Quirks ported as-is
//!
//! * 271 compares `slug__iexact` in the database but `slug in
//!   RESTRICTED_WORKSPACE_SLUGS` **case-sensitively** (`workspace.py:31`), so
//!   `"API"` is available while `"api"` is not.
//! * 272's paginator is `max_per_page=10, default_per_page=10`
//!   (`workspace.py:63-69`), so `?per_page=11` is a 400 and an absent
//!   `per_page` is 10.
//! * `POST` runs `serializer.is_valid(raise_exception=True)` and *then*
//!   creates the member row, so the `serializer.errors` branch at
//!   `workspace.py:100-103` is dead: an invalid body is DRF's 400 from the
//!   raise. The `except IntegrityError` only handles `"already exists"`, and
//!   anything else (including the implicit `return None` for a duplicate slug
//!   that trips the DB constraint after DRF's case-sensitive `UniqueValidator`
//!   passed) is a 500.

use std::collections::HashMap;

use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_db::JsonText;
use plane_db::models::workspace::Workspace;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;
use plane_serializers::FieldValue;
use plane_serializers::generated::license_api_workspace::WorkspaceSerializer;
use uuid::Uuid;

use super::admin::{AdminUser, load_admin_users};
use super::queries;
use crate::common::client;
use crate::workspaces::base::RESTRICTED_WORKSPACE_SLUGS;
use crate::workspaces::rows;
use crate::write;
use crate::write::fields::Detail;
use crate::write::serializer::ValidationErrors;

/// Register the routes this module owns.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.license.api.views.workspace.InstanceWorkSpaceAvailabilityCheckEndpoint")?
        .method(http::Method::GET, availability_check)?;
    registry
        .api_view("plane.license.api.views.workspace.InstanceWorkSpaceEndpoint")?
        .method(http::Method::GET, list)?
        .method(http::Method::POST, create)?;
    Ok(())
}

/// `InstanceWorkSpaceAvailabilityCheckEndpoint.get` (`workspace.py:21-34`).
fn availability_check<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.query().get("slug").unwrap_or("");
        if slug.is_empty() {
            return Ok(super::instance::error_body(
                400,
                "Workspace Slug is required",
            ));
        }
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::WORKSPACE_SLUG_IEXACT_EXISTS.sql)
            .await?;
        let row = client.query_one(&statement, &[&slug]).await?;
        let exists: bool = row.get(0);
        let taken = exists || RESTRICTED_WORKSPACE_SLUGS.contains(&slug);
        let body = Object::new().field("status", !taken);
        let bytes = plane_serde::to_vec(Dialect::Drf, &body)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// One page row: the workspace, the two annotations and `logo_url` resolved.
struct WorkspaceRow {
    model: Workspace,
    logo_url: Option<String>,
    total_projects: i64,
    total_members: i64,
}

/// `InstanceWorkSpaceEndpoint.get` (`workspace.py:41-69`).
fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let pagination = Pagination::parse(ctx)?;
        let client = client(ctx)?;
        let search = ctx.query().get("search").filter(|value| !value.is_empty());
        let pattern = search.map(like_contains);
        let pattern = pattern.as_deref();

        let count_statement = client.prepare_cached(queries::WORKSPACES_COUNT.sql).await?;
        let count_row = client.query_one(&count_statement, &[&pattern]).await?;
        let total: i64 = count_row.get(0);

        let limit = i64::from(pagination.per_page);
        let offset = pagination.offset();
        let statement = client.prepare_cached(queries::WORKSPACES_PAGE.sql).await?;
        // `queryset[offset:offset + limit + 1]` (`paginator.py:161-189`).
        let fetch = limit + 1;
        let rows = client
            .query(&statement, &[&pattern, &fetch, &offset])
            .await?;

        let mut records = Vec::with_capacity(rows.len().min(usize::try_from(limit).unwrap_or(0)));
        let mut owner_ids = Vec::new();
        for row in &rows {
            let model = Workspace::from_row(row, 0)?;
            let entity_type: Option<String> = row.try_get(16)?;
            let total_projects: i64 = row.try_get(14)?;
            let total_members: i64 = row.try_get(15)?;
            if !owner_ids.contains(&model.owner_id) {
                owner_ids.push(model.owner_id);
            }
            records.push(WorkspaceRow {
                logo_url: rows::logo_url(
                    model.logo_asset_id,
                    entity_type.as_deref(),
                    model.logo.as_deref(),
                ),
                model,
                total_projects,
                total_members,
            });
        }
        // `next_cursor.has_results = page_results.count() > limit`, then
        // `results = results[:limit]` (`paginator.py:179-189`).
        let has_next = records.len() > usize::try_from(limit).unwrap_or(usize::MAX);
        records.truncate(usize::try_from(limit).unwrap_or(0));

        let owners: HashMap<Uuid, AdminUser> = load_admin_users(client, &owner_ids).await?;
        let owner_views: Vec<
            Option<plane_serializers::generated::license_api_user::UserLiteSerializer<'_>>,
        > = records
            .iter()
            .map(|record| owners.get(&record.model.owner_id).map(AdminUser::user_lite))
            .collect();
        let prepared = crate::common::RequestRender::from_ctx(ctx);
        let context = prepared.render_ctx();
        let serializers: Vec<WorkspaceSerializer<'_>> = records
            .iter()
            .zip(owner_views.iter())
            .map(|(record, owner)| WorkspaceSerializer {
                id: record.model.id,
                owner: owner.as_ref(),
                logo_url: record.logo_url.as_deref(),
                total_projects: FieldValue::from_attribute(Some(record.total_projects)),
                total_members: FieldValue::from_attribute(Some(record.total_members)),
                created_at: record.model.created_at,
                updated_at: record.model.updated_at,
                deleted_at: record.model.deleted_at,
                name: &record.model.name,
                logo: record.model.logo.as_deref(),
                slug: &record.model.slug,
                organization_size: record.model.organization_size.as_deref(),
                timezone: &record.model.timezone,
                background_color: &record.model.background_color,
                created_by: record.model.created_by_id,
                updated_by: record.model.updated_by_id,
                logo_asset: record.model.logo_asset_id,
            })
            .collect();
        let results = crate::common::render_list(&serializers, &context)?;

        // `BasePaginator.paginate`'s envelope (`plane/utils/paginator.py:739-755`).
        let page = pagination.page;
        let has_prev = page > 0;
        let max_hits = if limit == 0 {
            0
        } else {
            (total + limit - 1) / limit
        };
        let count = i64::try_from(records.len()).unwrap_or(0);
        let prefix = format!(
            "{{\"grouped_by\":null,\"sub_grouped_by\":null,\"total_count\":{total},\
             \"next_cursor\":\"{limit}:{}:0\",\"prev_cursor\":\"{limit}:{}:1\",\
             \"next_page_results\":{has_next},\"prev_page_results\":{has_prev},\
             \"count\":{count},\"total_pages\":{max_hits},\"total_results\":{total},\
             \"extra_stats\":null,\"results\":",
            page + 1,
            page - 1
        );
        let mut body = Vec::with_capacity(prefix.len() + results.len() + 1);
        body.extend_from_slice(prefix.as_bytes());
        body.extend_from_slice(&results);
        body.push(b'}');
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `BasePaginator.get_per_page` + `Cursor.from_string` for this view
/// (`plane/utils/paginator.py:642-693`).
struct Pagination {
    per_page: u32,
    page: i64,
}

impl Pagination {
    fn parse(ctx: &ViewCtx<'_>) -> Result<Self, ViewError> {
        let raw = ctx.query().get("per_page").unwrap_or("10");
        let per_page = raw.parse::<i64>().map_err(|_| {
            ViewError::Api(plane_core::ApiError::ParseError(
                "Invalid per_page parameter.".to_string(),
            ))
        })?;
        let max_per_page = 10;
        if per_page <= 0 {
            return Err(ViewError::Api(plane_core::ApiError::ParseError(
                "Invalid per_page value. Must be greater than zero.".to_string(),
            )));
        }
        if per_page > max_per_page {
            return Err(ViewError::Api(plane_core::ApiError::ParseError(format!(
                "Invalid per_page value. Cannot exceed {max_per_page}."
            ))));
        }
        let default_cursor = format!("{per_page}:0:0");
        let cursor = ctx.query().get("cursor").unwrap_or(&default_cursor);
        let (value, page) = parse_cursor(cursor)?;
        if !(0.0..=max_per_page as f64).contains(&value) || page < 0 {
            return Err(ViewError::Api(plane_core::ApiError::ParseError(
                "Invalid cursor parameter.".to_string(),
            )));
        }
        Ok(Self {
            per_page: u32::try_from(per_page).unwrap_or(10),
            page,
        })
    }

    fn offset(&self) -> i64 {
        self.page * i64::from(self.per_page)
    }
}

/// `Cursor.from_string` (`plane/utils/paginator.py:55-64`): three
/// colon-separated parts; the first is a number, the second an int, the third
/// an int used as a bool. Any failure is the view's `ParseError`.
fn parse_cursor(text: &str) -> Result<(f64, i64), ViewError> {
    let invalid = || {
        ViewError::Api(plane_core::ApiError::ParseError(
            "Invalid cursor parameter.".to_string(),
        ))
    };
    let bits: Vec<&str> = text.split(':').collect();
    if bits.len() != 3 {
        return Err(invalid());
    }
    let value = if bits[0].contains('.') {
        bits[0].parse::<f64>().map_err(|_| invalid())?
    } else {
        bits[0].parse::<i64>().map_err(|_| invalid())? as f64
    };
    let offset = bits[1].parse::<i64>().map_err(|_| invalid())?;
    // `bool(int(bits[2]))` — any integer is accepted.
    bits[2].parse::<i64>().map_err(|_| invalid())?;
    Ok((value, offset))
}

/// Django's `icontains` pattern: `%` + the `LIKE`-escaped text + `%`.
fn like_contains(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len() + 2);
    escaped.push('%');
    for character in text.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push('%');
    escaped
}

/// `InstanceWorkSpaceEndpoint.post` (`workspace.py:72-112`).
///
/// `WorkspaceSerializer(data=request.data)` in declaration order, then
/// `serializer.is_valid(raise_exception=True)` (so the dead branch at
/// `workspace.py:100-103` never runs), `save(owner=request.user)` and one
/// `WorkspaceMember.objects.create(...)`.
fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let data = ctx.data()?;
        let Some(fields) = BodyFields::new(data) else {
            // `Serializer.to_internal_value`: a non-Mapping body.
            let mut errors = ValidationErrors::new(
                "non_field_errors",
                Detail::new(
                    format!(
                        "Invalid data. Expected a dictionary, but got {}.",
                        body_type_name(data)
                    ),
                    "invalid",
                ),
            );
            errors.sort_by_field_order(FIELD_ORDER);
            return Ok(errors.into_response());
        };
        let user_id = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;

        let mut errors = ValidationErrors::default();
        let deleted_at = validate_datetime_field(&fields, "deleted_at", &mut errors);
        let name = validate_name(&fields, &mut errors);
        let logo = validate_char(&fields, "logo", false, true, None, &mut errors);
        let slug = validate_slug(&fields, client, &mut errors).await?;
        let organization_size = validate_char(
            &fields,
            "organization_size",
            false,
            true,
            Some(20),
            &mut errors,
        );
        let timezone = validate_timezone(&fields, &mut errors);
        let background_color = validate_char(
            &fields,
            "background_color",
            false,
            false,
            Some(255),
            &mut errors,
        );
        if !errors.is_empty() {
            errors.sort_by_field_order(FIELD_ORDER);
            return Ok(errors.into_response());
        }

        let name = name.unwrap_or_default();
        let slug = slug.unwrap_or_default();
        let timezone = timezone.unwrap_or_else(|| "UTC".to_string());
        let background_color = background_color.unwrap_or_else(random_color);

        let now = write::now();
        let workspace = Workspace {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user_id),
            updated_by_id: None,
            deleted_at,
            id: Uuid::new_v4(),
            name,
            logo,
            logo_asset_id: None,
            owner_id: user_id,
            slug,
            organization_size,
            timezone,
            background_color,
        };
        let statement = client
            .prepare_cached(crate::workspaces::write_a_queries::WORKSPACE_INSERT.sql)
            .await?;
        if let Err(error) = client
            .execute(
                &statement,
                &[
                    &workspace.created_at,
                    &workspace.updated_at,
                    &workspace.created_by_id,
                    &workspace.updated_by_id,
                    &workspace.deleted_at,
                    &workspace.id,
                    &workspace.name,
                    &workspace.logo,
                    &workspace.logo_asset_id,
                    &workspace.owner_id,
                    &workspace.slug,
                    &workspace.organization_size,
                    &workspace.timezone,
                    &workspace.background_color,
                ],
            )
            .await
        {
            return Ok(create_db_error(error));
        }

        // `WorkspaceMember.objects.create(workspace_id=serializer.data["id"],
        // member=request.user, role=20,
        // company_role=request.data.get("company_role", ""))`
        // (`workspace.py:94-99`). `serializer.data` is computed first, which
        // only matters for its queries.
        let member_id = Uuid::new_v4();
        let member_role: i16 = 20;
        let company_role = company_role(&fields);
        let default_props = JsonText::new(crate::projects::invite_write::WORKSPACE_DEFAULT_PROPS);
        let issue_props = JsonText::new(crate::projects::invite_write::WORKSPACE_ISSUE_PROPS);
        let statement = client
            .prepare_cached(crate::workspaces::write_a_queries::WORKSPACE_MEMBER_CREATE.sql)
            .await?;
        if let Err(error) = client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &member_id,
                    &member_role,
                    &Some(user_id),
                    &user_id,
                    &workspace.id,
                    &company_role,
                    &default_props,
                    &default_props,
                    &issue_props,
                ],
            )
            .await
        {
            return Ok(create_db_error(error));
        }

        let owner = super::admin::current_user(ctx).await?;
        let logo_entity_type =
            super::admin::load_asset_entity_type(client, workspace.logo_asset_id).await?;
        let logo_url = rows::logo_url(
            workspace.logo_asset_id,
            logo_entity_type.as_deref(),
            workspace.logo.as_deref(),
        );
        let owner_lite = plane_serializers::generated::license_api_user::UserLiteSerializer {
            id: owner.id,
            email: owner.email.as_deref(),
            first_name: &owner.first_name,
            last_name: &owner.last_name,
        };
        let serializer = WorkspaceSerializer {
            id: workspace.id,
            owner: Some(&owner_lite),
            logo_url: logo_url.as_deref(),
            // Not annotated on a create response: `SkipField`.
            total_projects: FieldValue::absent(),
            total_members: FieldValue::absent(),
            created_at: workspace.created_at,
            updated_at: workspace.updated_at,
            deleted_at: workspace.deleted_at,
            name: &workspace.name,
            logo: workspace.logo.as_deref(),
            slug: &workspace.slug,
            organization_size: workspace.organization_size.as_deref(),
            timezone: &workspace.timezone,
            background_color: &workspace.background_color,
            created_by: workspace.created_by_id,
            updated_by: workspace.updated_by_id,
            logo_asset: workspace.logo_asset_id,
        };
        let body = crate::common::render(ctx, &serializer)?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// The two unique/database failures: `except IntegrityError as e: if "already
/// exists" in str(e): 409` else the implicit `None` (a 500)
/// (`workspace.py:105-112`).
fn create_db_error(error: tokio_postgres::Error) -> ViewResponse {
    let text = error.to_string();
    if text.contains("already exists") {
        let body = Object::new().field("slug", "The workspace with the slug already exists");
        let bytes = plane_serde::to_vec(Dialect::Drf, &body).unwrap_or_default();
        ViewResponse::raw(409, APPLICATION_JSON, bytes)
    } else {
        let body = Object::new().field("error", "Something went wrong please try again later");
        let bytes = plane_serde::to_vec(Dialect::Drf, &body).unwrap_or_default();
        ViewResponse::raw(500, APPLICATION_JSON, bytes)
    }
}

const FIELD_ORDER: &[&str] = &[
    "id",
    "owner",
    "logo_url",
    "total_projects",
    "total_members",
    "created_at",
    "updated_at",
    "deleted_at",
    "name",
    "logo",
    "slug",
    "organization_size",
    "timezone",
    "background_color",
    "created_by",
    "updated_by",
    "logo_asset",
];

/// The writable fields, in `_readable_fields` order.
enum BodyFields<'a> {
    Json(&'a plane_serde::pyjson::PyDict),
    Form(&'a plane_drf::QueryDict),
}

impl<'a> BodyFields<'a> {
    fn new(data: &'a plane_drf::parsers::Data) -> Option<Self> {
        match data {
            plane_drf::parsers::Data::Json(plane_serde::pyjson::Value::Dict(dict)) => {
                Some(Self::Json(dict))
            }
            plane_drf::parsers::Data::Form(query) => Some(Self::Form(query)),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<BodyValue<'_>> {
        match self {
            Self::Json(dict) => dict.get(key).map(BodyValue::Json),
            Self::Form(query) => query.get(key).map(BodyValue::Text),
        }
    }
}

#[derive(Clone, Copy)]
enum BodyValue<'a> {
    Json(&'a plane_serde::pyjson::Value),
    Text(&'a str),
}

impl BodyValue<'_> {
    fn is_null(&self) -> bool {
        matches!(self, Self::Json(plane_serde::pyjson::Value::Null))
    }

    /// DRF's `CharField.to_internal_value`: a string, or a number through
    /// Python's `str()`; a bool and any container are `Not a valid string.`
    fn into_string(self) -> Option<String> {
        use plane_serde::pyjson::Value as Json;
        match self {
            Self::Text(text) => Some(text.to_string()),
            Self::Json(Json::Str(text)) => Some(text.to_string_lossy().into_owned()),
            Self::Json(Json::Int(value)) => Some(value.to_string()),
            Self::Json(Json::Float(value)) => Some(value.to_string()),
            _ => None,
        }
    }
}

fn body_type_name(data: &plane_drf::parsers::Data) -> &'static str {
    use plane_serde::pyjson::Value as Json;
    match data {
        plane_drf::parsers::Data::Form(_) => "dict",
        plane_drf::parsers::Data::Json(Json::Null) => "NoneType",
        plane_drf::parsers::Data::Json(Json::Bool(_)) => "bool",
        plane_drf::parsers::Data::Json(Json::Int(_)) => "int",
        plane_drf::parsers::Data::Json(Json::Float(_)) => "float",
        plane_drf::parsers::Data::Json(Json::Str(_)) => "str",
        plane_drf::parsers::Data::Json(Json::List(_)) => "list",
        plane_drf::parsers::Data::Json(Json::Dict(_)) => "dict",
    }
}

/// One `CharField`/`TextField` (`rest_framework/fields.py:619-700`).
fn validate_char(
    fields: &BodyFields<'_>,
    name: &'static str,
    required: bool,
    allow_null: bool,
    max_length: Option<usize>,
    errors: &mut ValidationErrors,
) -> Option<String> {
    let Some(raw) = fields.get(name) else {
        if required {
            errors.push(name, Detail::new("This field is required.", "required"));
        }
        return None;
    };
    if raw.is_null() {
        if allow_null {
            return None;
        }
        errors.push(name, Detail::new("This field may not be null.", "null"));
        return None;
    }
    let Some(text) = raw.into_string() else {
        errors.push(name, Detail::new("Not a valid string.", "invalid"));
        return None;
    };
    // `CharField` trims whitespace by default.
    let text = text.trim().to_string();
    if text.is_empty() && !allow_null {
        // `allow_blank=False` (the model marks `logo` and
        // `organization_size` blank; `name`/`slug`/`background_color` not).
        if matches!(name, "logo" | "organization_size") {
            return Some(text);
        }
        errors.push(name, Detail::new("This field may not be blank.", "blank"));
        return None;
    }
    if let Some(max_length) = max_length
        && text.chars().count() > max_length
    {
        errors.push(
            name,
            Detail::new(
                format!("Ensure this field has no more than {max_length} characters."),
                "max_length",
            ),
        );
        return None;
    }
    Some(text)
}

fn validate_datetime_field(
    fields: &BodyFields<'_>,
    name: &'static str,
    errors: &mut ValidationErrors,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = fields.get(name)?;
    if raw.is_null() {
        return None;
    }
    let Some(text) = raw.into_string() else {
        errors.push(
            name,
            Detail::new(
                "Datetime has wrong format. Use one of these formats instead: \
                 YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].",
                "invalid",
            ),
        );
        return None;
    };
    match super::instance::parse_datetime(&text) {
        Some(value) => Some(value),
        None => {
            errors.push(
                name,
                Detail::new(
                    "Datetime has wrong format. Use one of these formats instead: \
                     YYYY-MM-DDThh:mm[:ss[.uuuuuu]][+HH:MM|-HH:MM|Z].",
                    "invalid",
                ),
            );
            None
        }
    }
}

/// `name`: a `CharField` plus `validate_name`'s two rules
/// (`license/api/serializers/workspace.py:20-32`). `validate_name` runs inside
/// `to_internal_value`, so its error is reported even when another field also
/// fails.
fn validate_name(fields: &BodyFields<'_>, errors: &mut ValidationErrors) -> Option<String> {
    let name = validate_char(fields, "name", true, false, Some(80), errors)?;
    if crate::write::email::contains_url(&name) {
        errors.push("name", Detail::new("Name must not contain URLs", "invalid"));
        return None;
    }
    if !name.chars().any(char::is_alphanumeric) {
        errors.push(
            "name",
            Detail::new("Name must contain at least one letter or number", "invalid"),
        );
        return None;
    }
    Some(name)
}

/// `SlugField`: the model's `UniqueValidator`, DRF's regex, then
/// `validate_slug`'s restricted and case-insensitive checks
/// (`license/api/serializers/workspace.py:34-44`).
async fn validate_slug(
    fields: &BodyFields<'_>,
    client: &deadpool_postgres::Client,
    errors: &mut ValidationErrors,
) -> Result<Option<String>, ViewError> {
    let Some(raw) = fields.get("slug") else {
        errors.push("slug", Detail::new("This field is required.", "required"));
        return Ok(None);
    };
    if raw.is_null() {
        errors.push("slug", Detail::new("This field may not be null.", "null"));
        return Ok(None);
    }
    let Some(text) = raw.into_string() else {
        errors.push("slug", Detail::new("Not a valid string.", "invalid"));
        return Ok(None);
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        errors.push("slug", Detail::new("This field may not be blank.", "blank"));
        return Ok(None);
    }
    if text.chars().count() > 48 {
        errors.push(
            "slug",
            Detail::new(
                "Ensure this field has no more than 48 characters.",
                "max_length",
            ),
        );
        return Ok(None);
    }
    // `UniqueValidator` (exact, manager predicate).
    let statement = client
        .prepare_cached(crate::workspaces::queries::WORKSPACE_SLUG_EXISTS.sql)
        .await?;
    let row = client.query_one(&statement, &[&text]).await?;
    let taken: bool = row.get(0);
    if taken {
        errors.push(
            "slug",
            Detail::new("Workspace with this slug already exists.", "unique"),
        );
        return Ok(None);
    }
    // DRF `SlugField`'s regex validator.
    if !text
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        errors.push(
            "slug",
            Detail::new(
                "Enter a valid \"slug\" consisting of letters, numbers, underscores or hyphens.",
                "invalid",
            ),
        );
        return Ok(None);
    }
    // `validate_slug`.
    if RESTRICTED_WORKSPACE_SLUGS.contains(&text.as_str()) {
        errors.push("slug", Detail::new("Slug is not valid", "invalid"));
        return Ok(None);
    }
    let statement = client
        .prepare_cached(queries::WORKSPACE_SLUG_IN_USE.sql)
        .await?;
    let row = client.query_one(&statement, &[&text]).await?;
    let in_use: bool = row.get(0);
    if in_use {
        errors.push("slug", Detail::new("Slug is already in use", "invalid"));
        return Ok(None);
    }
    Ok(Some(text))
}

/// `ChoiceField` against `pytz.common_timezones`
/// (`plane/db/models/workspace.py:120`).
fn validate_timezone(fields: &BodyFields<'_>, errors: &mut ValidationErrors) -> Option<String> {
    let raw = fields.get("timezone")?;
    if raw.is_null() {
        errors.push(
            "timezone",
            Detail::new("This field may not be null.", "null"),
        );
        return None;
    }
    let Some(text) = raw.into_string() else {
        errors.push(
            "timezone",
            Detail::new("\"\" is not a valid choice.", "invalid_choice"),
        );
        return None;
    };
    // The 433-entry `pytz.common_timezones` table lives in `plane-serializers`
    // only as a generated write table for the app serializer
    // (`write/generated/work_space_serializer.rs`); reuse it instead of
    // duplicating the list.
    let valid = crate::write::generated::work_space_serializer::FIELDS
        .iter()
        .find(|field| field.name == "timezone")
        .is_some_and(|field| field.choices.iter().any(|(_, value)| *value == text));
    if !valid {
        errors.push(
            "timezone",
            Detail::new(
                format!("\"{text}\" is not a valid choice."),
                "invalid_choice",
            ),
        );
        return None;
    }
    Some(text)
}

/// `request.data.get("company_role", "")`: a `TextField`, so `str(value)` for
/// a non-string and `None` for an explicit null.
fn company_role(fields: &BodyFields<'_>) -> Option<String> {
    match fields.get("company_role") {
        None => Some(String::new()),
        Some(value) if value.is_null() => None,
        Some(value) => value.into_string(),
    }
}

/// `plane.utils.color.get_random_color` (`color.py:9`).
fn random_color() -> String {
    format!(
        "#{}",
        plane_auth::random::get_random_string(6, "0123456789abcdefABCDEF")
    )
}
