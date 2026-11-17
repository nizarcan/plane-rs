// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `QuickLinkViewSet`'s write actions (`plane/app/views/workspace/quick_link.py:
//! 24-58`) — workspace-core set C, routes 218 and 219.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 218 `POST …/quick-links/` | `POST` | `create` (`quick_link.py:24-33`) | [`create`] |
//! | 219 `PATCH …/quick-links/{pk}/` | `PATCH` | `partial_update` (`:35-44`) | [`partial_update`] |
//! | 219 | `DELETE` | `destroy` (`:54-58`) | [`destroy`] |
//!
//! ## The URL rewrite
//!
//! `WorkspaceUserLinkSerializer.to_internal_value` prefixes a scheme-less URL
//! **in place** before delegating to DRF (`serializers/workspace.py:194-199`),
//! so every later step — the `validate_url` run, the duplicate check and the
//! stored column — sees `"http://" + url`. A body that is not a mapping has no
//! `.get`, and a truthy non-string `url` has no `.startswith`: both are the
//! family 500, **before** any field error is reported.
//!
//! `validate_url` (`:201-208`) wraps Django's `URLValidator` and raises
//! `ValidationError({"error": "Invalid URL format."})`, which DRF nests under
//! the field: `{"url": {"error": "Invalid URL format."}}` — a dict, not the
//! usual list of messages.
//!
//! ## The duplicate check runs against the **URL** workspace
//!
//! `create` filters on `validated_data["workspace_id"]` (`:210-224`), which is
//! the `save(workspace_id=…)` kwarg — the workspace in the URL — while
//! `WorkspaceBaseModel.save` then re-homes the row to the body project's
//! workspace (`plane/db/models/workspace.py:192-195`). A project in another
//! workspace therefore writes a link the check cannot see, and a second POST
//! with the same URL succeeds: there is no unique index on `url`. The check
//! itself raises `ValidationError({"error": "URL already exists for this
//! workspace and owner"})` from inside `save()`, i.e. the 400 body is the flat
//! `{"error": …}`.
//!
//! `update` checks the same tuple with `.exclude(pk=instance.pk)` against
//! `instance.workspace_id` — again before the re-home (`:226-238`).
//!
//! ## The three different misses
//!
//! `partial_update` uses `.filter(...).first()` and misses with this view's
//! `404 {"detail": "Quick link not found."}`; `destroy` uses `.get(...)`, whose
//! `DoesNotExist` is the family's
//! `404 {"error": "The required object does not exist."}` (`quick_link.py:43`,
//! `:56`). Both run **after** the `@allow_permission` decorator, so a
//! non-member is a 403 with the decorator's own body.

use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks::audit::Audit;
use plane_db::models::workspace::WorkspaceUserLink;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json as Doc;
use plane_serde::pyjson::{PyStr, Value as Json};
use plane_serializers::fields::PgJson;
use plane_serializers::generated::app_workspace::WorkspaceUserLinkSerializer;
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::workspaces::rows;
use crate::workspaces::write_c_queries as queries;
use crate::write::{self, Serializer};

/// `plane.app.views.workspace.quick_link.QuickLinkViewSet`.
const VIEW: &str = "plane.app.views.workspace.quick_link.QuickLinkViewSet";

/// `WorkspaceUserLinkSerializer`'s writable fields in `self.fields` order — the
/// order DRF reports `serializer.errors` in. `workspace` and `owner` are
/// read-only (`Meta.read_only_fields`).
const FIELD_ORDER: &[&str] = &[
    "deleted_at",
    "title",
    "url",
    "metadata",
    "created_by",
    "updated_by",
    "project",
];

/// Register the write verbs of routes 218 and 219.
///
/// `quick_link::register` already registers `list` and `retrieve` for this
/// viewset; the registry accumulates actions per view.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset(VIEW)?
        .action("create", create)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    Ok(())
}

/// `QuickLinkViewSet.create` (`quick_link.py:24-33`).
fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:25`).
        let workspace = lookup::workspace_by_slug(client, slug).await?;

        // `WorkspaceUserLinkSerializer(data=request.data)` (`:26`).
        let data = ctx.data()?;
        let prepared = prepare_body(data)?;
        let mut serializer: Serializer<'_> = match &prepared {
            PreparedBody::Original => Serializer::new(ctx, false)?,
            PreparedBody::Json(value) => Serializer::from_value(ctx, value, false),
            PreparedBody::Form(query) => Serializer::from_form(ctx, query, false),
        };
        write::generated::workspace_user_link_serializer::validate(&mut serializer, client).await?;
        validate_url(&mut serializer);
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(mut errors) => {
                errors.sort_by_field_order(FIELD_ORDER);
                return Ok(errors.into_response());
            }
        };

        let url = values.text("url").unwrap_or_default().to_owned();

        // `WorkspaceUserLink.objects.filter(url=url, workspace_id=…,
        // owner_id=…).exists()` (`:210-224`).
        if url_taken(client, &url, workspace.id, user, None).await? {
            return rows::error_body(400, "URL already exists for this workspace and owner");
        }

        // `serializer.save(workspace_id=workspace.id, owner_id=request.user.id)`
        // (`:30`), then `WorkspaceBaseModel.save`'s re-home.
        let project_id = values.uuid("project");
        let workspace_id = match project_id {
            Some(project_id) => project_workspace(client, project_id).await?,
            None => workspace.id,
        };
        let now = write::now();
        let link = WorkspaceUserLink {
            created_at: now,
            updated_at: now,
            // H01 on create: the caller wins over a body `created_by`, and
            // `updated_by` is NULL (`plane/db/models/base.py:36-42`).
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: values.datetime("deleted_at"),
            id: Uuid::new_v4(),
            workspace_id,
            project_id,
            title: values.text("title").map(str::to_owned),
            url,
            metadata: values
                .json("metadata")
                .cloned()
                .unwrap_or_else(|| JsonText::new("{}")),
            owner_id: user,
        };

        let statement = queries::QUICK_LINK_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &link.created_at,
                    &link.updated_at,
                    &link.created_by_id,
                    &link.updated_by_id,
                    &link.deleted_at,
                    &link.id,
                    &link.workspace_id,
                    &link.project_id,
                    &link.title,
                    &link.url,
                    &link.metadata,
                    &link.owner_id,
                ],
            )
            .await?;

        render(ctx, &link, 201)
    })
}

/// `QuickLinkViewSet.partial_update` (`quick_link.py:35-44`).
fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;

        // `.filter(pk=pk, workspace__slug=slug, owner=request.user).first()`
        // (`:36`): a miss is this view's own detail 404, before the body.
        let Some(mut link) = get_link(ctx, client, user).await? else {
            return rows::detail_body(404, "Quick link not found.");
        };

        let data = ctx.data()?;
        let prepared = prepare_body(data)?;
        let mut serializer: Serializer<'_> = match &prepared {
            PreparedBody::Original => Serializer::new(ctx, true)?,
            PreparedBody::Json(value) => Serializer::from_value(ctx, value, true),
            PreparedBody::Form(query) => Serializer::from_form(ctx, query, true),
        };
        write::generated::workspace_user_link_serializer::validate(&mut serializer, client).await?;
        validate_url(&mut serializer);
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(mut errors) => {
                errors.sort_by_field_order(FIELD_ORDER);
                return Ok(errors.into_response());
            }
        };

        // The duplicate check uses the **stored** workspace, before the body's
        // project can re-home the row (`:226-238`).
        if let Some(url) = values.text("url")
            && url_taken(client, url, link.workspace_id, link.owner_id, Some(link.id)).await?
        {
            return rows::error_body(400, "URL already exists for this workspace and owner");
        }

        // `ModelSerializer.update`: assign every validated field, then `save()`.
        if values.contains("deleted_at") {
            link.deleted_at = values.datetime("deleted_at");
        }
        if values.is_null("title") {
            link.title = None;
        } else if let Some(title) = values.text("title") {
            link.title = Some(title.to_owned());
        }
        if let Some(url) = values.text("url") {
            link.url = url.to_owned();
        }
        if let Some(metadata) = values.json("metadata") {
            link.metadata = metadata.clone();
        }
        if values.contains("created_by") {
            link.created_by_id = values.uuid("created_by");
        }
        // `updated_by` is writable — DRF validates it, H01 then overwrites it.
        if values.contains("project") {
            link.project_id = values.uuid("project");
        }

        // `WorkspaceBaseModel.save`: `self.workspace = self.project.workspace`
        // whenever the FK is set — a body project in another workspace moves
        // the row (`plane/db/models/workspace.py:192-195`).
        if let Some(project_id) = link.project_id {
            link.workspace_id = project_workspace(client, project_id).await?;
        }

        // H01 on update: `updated_at` from `auto_now`, `updated_by` from the
        // user; `created_at` and `created_by` keep their stored values.
        link.updated_at = write::now();
        link.updated_by_id = Audit::on_update(Some(user));

        let statement = queries::QUICK_LINK_SAVE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &link.created_at,
                    &link.updated_at,
                    &link.created_by_id,
                    &link.updated_by_id,
                    &link.deleted_at,
                    &link.workspace_id,
                    &link.project_id,
                    &link.title,
                    &link.url,
                    &link.metadata,
                    &link.owner_id,
                    &link.id,
                ],
            )
            .await?;

        render(ctx, &link, 200)
    })
}

/// `QuickLinkViewSet.destroy` (`quick_link.py:54-58`).
fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;

        // `WorkspaceUserLink.objects.get(pk=pk, workspace__slug=slug,
        // owner=request.user)` (`:55`): `DoesNotExist` is the family 404.
        let Some(link) = get_link(ctx, client, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // H03: `deleted_at = now`, a full save whose observable columns are
        // `updated_at`/`updated_by`, then the cascade publish for
        // `("db", "workspaceuserlink")` (`plane/db/mixins.py:65-77`).
        let now = write::now();
        write::soft_delete(
            client,
            "workspace_user_links",
            "id",
            link.id,
            Audit::on_update(Some(user)),
            now,
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "workspaceuserlink", link.id).await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `WorkspaceUserLink.objects.filter(pk=pk, workspace__slug=slug,
/// owner=request.user).first()` for both 219 verbs.
///
/// The statement is the read route's [`crate::workspaces::queries::
/// QUICK_LINK_BY_ID`]: a primary key makes `.first()`'s `ORDER BY created_at
/// DESC` unobservable, and `LIMIT 1` matches at most one row.
///
/// An unparsable `pk` is Django's `ValidationError` from `UUIDField`, i.e. the
/// family 400; the router's `<uuid:pk>` converter makes it unreachable in
/// practice, as the read route documents.
async fn get_link(
    ctx: &ViewCtx<'_>,
    client: &deadpool_postgres::Client,
    user: Uuid,
) -> Result<Option<WorkspaceUserLink>, ViewError> {
    let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
    let Some(pk) = ctx.kwarg("pk") else {
        return Err(ViewError::Key);
    };
    let Ok(pk) = Uuid::parse_str(pk) else {
        return Err(ViewError::Validation);
    };
    let statement = crate::workspaces::queries::QUICK_LINK_BY_ID
        .prepare(client)
        .await?;
    match client.query_opt(&statement, &[&slug, &user, &pk]).await? {
        Some(row) => Ok(Some(WorkspaceUserLink::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

/// The duplicate check of `create`/`update` (`serializers/workspace.py:210-238`).
///
/// `excluded` is the instance's own pk on an update and `None` on a create.
async fn url_taken(
    client: &deadpool_postgres::Client,
    url: &str,
    workspace_id: Uuid,
    owner_id: Uuid,
    excluded: Option<Uuid>,
) -> Result<bool, ViewError> {
    let statement = queries::QUICK_LINK_URL_TAKEN.prepare(client).await?;
    let row = client
        .query_one(&statement, &[&url, &workspace_id, &owner_id, &excluded])
        .await?;
    Ok(row.get(0))
}

/// `self.workspace = self.project.workspace` for a body `project`
/// (`plane/db/models/workspace.py:192-195`).
///
/// The FK dereference uses `Project._base_manager`, so a soft-deleted project
/// still resolves; a missing row is `Project.DoesNotExist`, which the family
/// handler answers as `ObjectDoesNotExist`.
async fn project_workspace(
    client: &deadpool_postgres::Client,
    project_id: Uuid,
) -> Result<Uuid, ViewError> {
    plane_db::hooks::scope::workspace_from_project(client, project_id)
        .await
        .map_err(ViewError::from)?
        .ok_or(ViewError::ObjectDoesNotExist)
}

/// The request body after `to_internal_value`'s rewrite, **owned** so it
/// outlives the serializer that borrows it.
#[derive(Debug)]
enum PreparedBody {
    /// Nothing to rewrite: the serializer reads the request's own body.
    Original,
    Json(Json),
    /// A **mutable** form (multipart with a file part) after the in-place
    /// rewrite: the caller owns the copy so it outlives the serializer that
    /// borrows it.
    Form(plane_drf::QueryDict),
}

/// `WorkspaceUserLinkSerializer.to_internal_value` (`serializers/workspace.py:
/// 194-199`):
///
/// ```python
/// url = data.get("url", "")
/// if url and not url.startswith(("http://", "https://")):
///     data["url"] = "http://" + url
/// ```
///
/// The shapes reachable here, each with its own outcome:
///
/// * a JSON object with a truthy string `url` that has no scheme is rewritten;
/// * a JSON body that is not a mapping has no `.get` → family 500;
/// * a truthy non-string `url` has no `.startswith` → family 500;
/// * a **urlencoded** body with a scheme-less `url` is the 500
///   `AttributeError: This QueryDict instance is immutable` — DRF hands the
///   parser's immutable `QueryDict` as `request.data`, so the in-place write
///   raises. A form URL that already carries a scheme, or has none at all,
///   never writes and is validated normally;
/// * a **multipart** body is immutable too unless it carries a file part: DRF
///   then copies the parsed dict (`QueryDict.copy()` is mutable), so the write
///   succeeds and the prefixed value is validated.
///
/// A `null` body never reaches `to_internal_value` (DRF answers
/// `{"non_field_errors": ["No data provided"]}` from `run_validation`), so it
/// is [`PreparedBody::Original`].
fn prepare_body(data: &Data) -> Result<PreparedBody, ViewError> {
    // The shared `to_internal_value` guard: `data.get`/`.startswith`, a form's
    // immutable `QueryDict` and a file part named `url` all raise before DRF
    // reads a field.
    crate::write::url::guard_url_rewrite(data)?;
    match data {
        Data::Form(query) => {
            let Some(url) = query.get("url") else {
                return Ok(PreparedBody::Original);
            };
            if url.is_empty() || url.starts_with("http://") || url.starts_with("https://") {
                return Ok(PreparedBody::Original);
            }
            if !query.is_mutable() {
                // `data["url"] = …` on the request's immutable `QueryDict`
                // (urlencoded, or multipart without a file part).
                return Err(ViewError::Unknown(
                    "AttributeError: This QueryDict instance is immutable".to_string(),
                ));
            }
            // A multipart body with a file part reaches the view as a mutable
            // copy: the write replaces the field's values and the serializer
            // validates the copy.
            let mut rewritten = query.clone();
            rewritten.set("url", format!("http://{url}"));
            Ok(PreparedBody::Form(rewritten))
        }
        Data::Json(Json::Dict(entries)) => {
            let Some(value) = entries.get("url") else {
                return Ok(PreparedBody::Original);
            };
            match value {
                // Falsy values skip the rewrite and fail field validation.
                Json::Null => Ok(PreparedBody::Original),
                Json::Bool(false) => Ok(PreparedBody::Original),
                Json::Int(number) if number.as_i64() == Some(0) => Ok(PreparedBody::Original),
                Json::Float(number) if *number == 0.0 => Ok(PreparedBody::Original),
                Json::List(items) if items.is_empty() => Ok(PreparedBody::Original),
                Json::Dict(entries) if entries.is_empty() => Ok(PreparedBody::Original),
                Json::Str(text) => {
                    let url = text.as_str().unwrap_or_default();
                    if url.is_empty() || url.starts_with("http://") || url.starts_with("https://") {
                        return Ok(PreparedBody::Original);
                    }
                    let mut object = entries.clone();
                    object.insert(
                        PyStr::new("url"),
                        Json::Str(PyStr::from_string(format!("http://{url}"))),
                    );
                    Ok(PreparedBody::Json(Json::Dict(object)))
                }
                // A truthy non-string has no `.startswith`.
                other => Err(ViewError::Unknown(format!(
                    "AttributeError: '{}' object has no attribute 'startswith'",
                    python_type_name(other)
                ))),
            }
        }
        // `null` is DRF's "No data provided", not an `AttributeError`.
        Data::Json(Json::Null) => Ok(PreparedBody::Original),
        Data::Json(other) => Err(ViewError::Unknown(format!(
            "AttributeError: '{}' object has no attribute 'get'",
            python_type_name(other)
        ))),
    }
}

/// `WorkspaceUserLinkSerializer.validate_url` (`serializers/workspace.py:201-
/// 208`): Django's `URLValidator` over the (already prefixed) value.
///
/// The raised `ValidationError({"error": …})` is DRF's nested-dict field
/// error, so the body is `{"url": {"error": "Invalid URL format."}}`.
fn validate_url(serializer: &mut Serializer<'_>) {
    let Some(url) = serializer.value("url").and_then(|value| match value {
        write::Value::Text(text) => Some(text.clone()),
        _ => None,
    }) else {
        return;
    };
    if !write::url::django_url_valid(&url) {
        serializer.push_body(
            "url",
            Doc::object(vec![("error", Doc::string("Invalid URL format."))]),
        );
    }
}

/// `serializer.data` — the 12 keys of `WorkspaceUserLinkSerializer`
/// (`plane/app/serializers/workspace.py:188-192`) in `_readable_fields` order.
fn render(
    ctx: &ViewCtx<'_>,
    link: &WorkspaceUserLink,
    status: u16,
) -> Result<ViewResponse, ViewError> {
    let serializer = WorkspaceUserLinkSerializer {
        id: link.id,
        created_at: link.created_at,
        updated_at: link.updated_at,
        deleted_at: link.deleted_at,
        title: link.title.as_deref(),
        url: &link.url,
        metadata: Some(PgJson::from(&link.metadata)),
        created_by: link.created_by_id,
        updated_by: link.updated_by_id,
        workspace: link.workspace_id,
        project: link.project_id,
        owner: link.owner_id,
    };
    let body = rows::render_plain(ctx, &serializer)?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// `type(value).__name__` for the two `AttributeError`s above.
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

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson;

    fn json(text: &str) -> Data {
        Data::Json(pyjson::loads(text.as_bytes(), "utf-8").expect("json"))
    }

    #[test]
    fn a_scheme_less_url_gets_http() {
        let data = json(r#"{"url": "example.com/x"}"#);
        match prepare_body(&data).expect("prepared") {
            PreparedBody::Json(Json::Dict(ref entries)) => {
                assert_eq!(
                    entries.get("url").and_then(Json::as_str),
                    Some("http://example.com/x")
                );
            }
            other => panic!("expected a rewritten body, got {other:?}"),
        }
    }

    #[test]
    fn a_scheme_is_case_sensitive() {
        // `"HTTP://x"` does not start with the lowercase prefix, so it becomes
        // `"http://HTTP://x"` and the URL validator rejects it.
        let data = json(r#"{"url": "HTTP://x"}"#);
        match prepare_body(&data).expect("prepared") {
            PreparedBody::Json(Json::Dict(ref entries)) => {
                assert_eq!(
                    entries.get("url").and_then(Json::as_str),
                    Some("http://HTTP://x")
                );
            }
            other => panic!("expected a rewritten body, got {other:?}"),
        }
    }

    #[test]
    fn a_non_mapping_body_and_a_non_string_url_are_500s() {
        assert!(matches!(
            prepare_body(&json("[]")),
            Err(ViewError::Unknown(_))
        ));
        assert!(matches!(
            prepare_body(&json(r#"{"url": 7}"#)),
            Err(ViewError::Unknown(_))
        ));
        // `null` and `""` are falsy: no rewrite.
        assert!(matches!(
            prepare_body(&json(r#"{"url": null}"#)).expect("prepared"),
            PreparedBody::Original
        ));
        assert!(matches!(
            prepare_body(&json(r#"{"url": ""}"#)).expect("prepared"),
            PreparedBody::Original
        ));
    }

    #[test]
    fn a_scheme_less_url_is_rewritten_on_a_mutable_multipart_form() {
        let mut query =
            plane_drf::QueryDict::parse("url=probe.example/m&title=Link", None).expect("form");
        // A file part is what makes DRF copy the parsed dict into a mutable
        // `request.data`.
        query.mark_file(Some("attachment"));
        match prepare_body(&Data::Form(query)).expect("prepared") {
            PreparedBody::Form(query) => {
                assert_eq!(query.get("url"), Some("http://probe.example/m"));
                // The rewrite replaces the value in place, as `__setitem__` does.
                assert_eq!(query.get("title"), Some("Link"));
            }
            other => panic!("expected a rewritten form, got {other:?}"),
        }
        let immutable = plane_drf::QueryDict::parse("url=probe.example/m", None).expect("form");
        assert!(matches!(
            prepare_body(&Data::Form(immutable)),
            Err(ViewError::Unknown(_))
        ));
    }

    #[test]
    fn field_order_matches_the_generated_table() {
        let generated: Vec<&str> = write::generated::workspace_user_link_serializer::FIELDS
            .iter()
            .map(|field| field.name)
            .collect();
        assert_eq!(generated, FIELD_ORDER);
    }
}
