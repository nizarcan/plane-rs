// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `POST /api/workspaces/{slug}/export-issues/` — `ExportIssuesEndpoint.post`
//! (`plane/app/views/exporter/base.py:19-66`), route 233. Its `GET` lives in
//! [`super::list_export`], which registers both verbs.
//!
//! ## Statement order (plan §4)
//!
//! 1. `allow_permission([ADMIN, MEMBER], level="WORKSPACE")` — the runtime's
//!    policy layer, from `routes.json`.
//! 2. `Workspace.objects.get(slug=slug)` → a miss is the family
//!    `404 {"error": "The required object does not exist."}`
//!    (`plane/app/views/base.py:222-226`). Runs **before** the body is parsed.
//! 3. `request.data`; `provider in ["csv", "xlsx", "json"]` → else 400
//!    `{"error": "Provider '<str(provider)>' not found."}`.
//! 4. `if not project_ids:` → the project fallback query
//!    ([`super::write_export_queries::EXPORT_PROJECT_IDS`]), and its ids are
//!    stringified *for the task* (`base.py:33-40`).
//! 5. One `exporters` INSERT (`base.py:42-48`), with `token = uuid4().hex`.
//! 6. `issue_export_task.delay(provider, workspace_id, project_ids, token_id,
//!    multiple, slug)` (`:50-57`).
//! 7. 200 `{"message": "Once the export is ready you will be able to
//!    download it"}`.
//!
//! No transaction (`ATOMIC_REQUESTS` is unset), so a broker failure at step 6 is
//! a 500 with the row committed — the plan's "publish-after-INSERT 500 path".
//!
//! ## Raw body values
//!
//! `multiple` and `project_ids` go into the message **as the client sent
//! them** (`base.py:29-30`): a list stays a list of whatever it held, a form's
//! `"false"` is a truthy string, and an absent key is Python's `False` / `[]`.
//! Only the `project` column is converted, by the ORM: a list is converted
//! element by element through `UUIDField`, while any other value is handed to
//! psycopg2 verbatim (`django/contrib/postgres/fields/array.py:125-131`).
//! See `super::write_export_queries` for the two INSERTs that reproduce it.
//!
//! `ExportIssuesEndpoint.get`'s pagination is untouched.

use plane_core::error::ApiError;
use plane_db::hooks::audit::Audit;
use plane_db::hooks::issue_view::truthy;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks;
use plane_jobs::queue::Job;
use plane_serde::pyjson::PyStr;
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::analytics::base::message_body;
use crate::common::{client, jobs, lookup};
use crate::issues::write_export_queries as queries;
use crate::projects::write_support;
use crate::views::write_support::{Body, body_get};
use crate::write;
use crate::write::fields::{Raw, python_str};

/// The `project` value as it reaches the `exporters.project` column.
enum ProjectColumn {
    /// A converted list/tuple: bound as `uuid[]`.
    List(Vec<Option<Uuid>>),
    /// Anything else: the `str()` psycopg2 sends, parsed by Postgres as an
    /// array literal (`$7::text::uuid[]`).
    Text(String),
}

/// `ExportIssuesEndpoint.post` (`plane/app/views/exporter/base.py:23-66`).
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:26`): the default manager's
        // `deleted_at IS NULL`, so a soft-deleted workspace is the 404.
        let workspace = lookup::workspace_by_slug(client, slug)
            .await
            .map_err(ViewError::from)?;

        // `request.data` (`:28`) — parsed only now, after the workspace lookup.
        let data = ctx.data()?;
        let provider_raw = body_value(data, "provider", &Json::Bool(false))?;
        let multiple = body_value(data, "multiple", &Json::Bool(false))?;
        let project_raw = body_value(data, "project", &Json::List(Vec::new()))?;

        // `if provider in ["csv", "xlsx", "json"]` (`:32`): an equality test
        // against strings, so only a string can pass. The error message is an
        // f-string, i.e. `str(provider)` — `False` for the default, `None` for
        // an explicit null, `['csv']` for a list.
        let provider = match &provider_raw {
            Json::Str(value) => value
                .as_str()
                .filter(|text| matches!(*text, "csv" | "xlsx" | "json"))
                .map(str::to_owned),
            _ => None,
        };
        let Some(provider) = provider else {
            return Ok(write::error_body(
                400,
                &format!(
                    "Provider '{}' not found.",
                    python_str(Raw::Json(&provider_raw))
                ),
            ));
        };

        // `if not project_ids:` (`:33`) — Python truthiness, so `[]`, `""`,
        // `0`, `false`, `{}` and `null` all take the fallback.
        let (project_arg, project_column) = if truthy(&project_raw) {
            match &project_raw {
                // `ArrayField.get_db_prep_value` converts a list/tuple.
                Json::List(items) => {
                    let mut values = Vec::with_capacity(items.len());
                    for item in items {
                        values.push(uuid_element(item)?);
                    }
                    (
                        TaskArg::Json(project_raw.clone()),
                        ProjectColumn::List(values),
                    )
                }
                // Everything else keeps psycopg2's type, and a `str` is parsed
                // by Postgres as the array literal.
                _ => (
                    TaskArg::Json(project_raw.clone()),
                    ProjectColumn::Text(python_str(Raw::Json(&project_raw))),
                ),
            }
        } else {
            let statement = queries::EXPORT_PROJECT_IDS.prepare(client).await?;
            let rows = client
                .query(&statement, &[&user, &slug])
                .await
                .map_err(ViewError::from)?;
            let ids: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();
            // `project_ids = [str(project_id) for project_id in project_ids]`
            // (`:40`) — the message carries strings, the column carries uuids.
            let arg = TaskArg::List(ids.iter().map(|id| TaskArg::Str(id.to_string())).collect());
            (
                arg,
                ProjectColumn::List(ids.into_iter().map(Some).collect()),
            )
        };

        // `ExporterHistory.objects.create(...)` (`:42-48`). `auto_now_add` then
        // `auto_now`: two `timezone.now()` calls, one per field. The columns the
        // view fixes (`name`, `type`, `status`, `reason`, `key`, `url`,
        // `filters`, `rich_filters`) are the model defaults, inlined by the
        // statement; the ones that vary are bound below.
        let created_at = write::now();
        let updated_at = write::now();
        let audit = Audit::on_create(Some(user));
        let id = Uuid::new_v4();
        // `generate_token` (`plane/db/models/exporter.py:20-21`).
        let token = Uuid::new_v4().simple().to_string();

        let inserted = match &project_column {
            ProjectColumn::List(values) => {
                let statement = queries::EXPORTER_INSERT.prepare(client).await?;
                client
                    .execute(
                        &statement,
                        &[
                            &created_at,
                            &updated_at,
                            &audit.created_by_id,
                            &audit.updated_by_id,
                            &id,
                            &workspace.id,
                            values,
                            &provider,
                            &token,
                            &user,
                        ],
                    )
                    .await
            }
            ProjectColumn::Text(text) => {
                let statement = queries::EXPORTER_INSERT_PROJECT_TEXT
                    .prepare(client)
                    .await?;
                client
                    .execute(
                        &statement,
                        &[
                            &created_at,
                            &updated_at,
                            &audit.created_by_id,
                            &audit.updated_by_id,
                            &id,
                            &workspace.id,
                            text,
                            &provider,
                            &token,
                            &user,
                        ],
                    )
                    .await
            }
        };
        // psycopg2's `DataError` (a malformed array literal) is a 500, an
        // `IntegrityError` the family's 400.
        inserted.map_err(write_support::integrity)?;

        // `issue_export_task.delay(...)` (`:50-57`), in the Python keyword
        // order. Both tasks are Celery-owned during coexistence.
        let message = tasks::issue_export_task(
            provider,
            workspace.id,
            project_arg,
            token,
            TaskArg::Json(multiple),
            slug.to_owned(),
        );
        jobs::dispatch_task(ctx, message.task(), &message, Job::from_message(&message)).await?;

        Ok(message_body(
            200,
            "Once the export is ready you will be able to download it",
        ))
    })
}

/// `request.data.get(key, default)`, as the Python value the view sees.
///
/// A form value is a `str` (`QueryDict.get` returns the last one); a JSON body
/// keeps its type. A non-dict body is `AttributeError` — the family 500 — which
/// is [`body_get`]'s contract.
fn body_value(
    data: &plane_drf::parsers::Data,
    key: &str,
    default: &Json,
) -> Result<Json, ViewError> {
    Ok(match body_get(data, key)? {
        Body::Missing => default.clone(),
        Body::Json(value) => value.clone(),
        Body::Form(text) => Json::Str(PyStr::new(text)),
    })
}

/// `UUIDField.get_db_prep_value` for one `ArrayField` element
/// (`django/db/models/fields/__init__.py:2748-2773`):
///
/// * `None` stays `None` (a NULL array element);
/// * a string goes through `uuid.UUID(hex=…)`;
/// * an int (a `bool` included, `bool` subclasses `int`) through
///   `uuid.UUID(int=…)`, so a negative or `>= 2**128` value is a
///   `ValidationError` → the family 400;
/// * a float, list or dict raises `AttributeError`/`ValueError` inside
///   `uuid.UUID`, which `to_python` turns into the same `ValidationError`.
fn uuid_element(value: &Json) -> Result<Option<Uuid>, ViewError> {
    match value {
        Json::Null => Ok(None),
        Json::Bool(flag) => Ok(Some(Uuid::from_u128(u128::from(*flag)))),
        Json::Int(int) => int
            .to_decimal_string()
            .parse::<u128>()
            .map(|value| Some(Uuid::from_u128(value)))
            .map_err(|_| ViewError::Validation),
        Json::Str(text) => text.as_str().map_or(Err(ViewError::Validation), |text| {
            Uuid::parse_str(text)
                .map(Some)
                .map_err(|_| ViewError::Validation)
        }),
        Json::Float(_) | Json::List(_) | Json::Dict(_) => Err(ViewError::Validation),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_element_follows_djangos_uuid_field() {
        // `uuid.UUID(hex=…)`: hyphenated, 32 hex digits, urn and braces.
        let canonical = "0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f";
        let expected = Uuid::parse_str(canonical).expect("uuid");
        for form in [
            canonical.to_owned(),
            canonical.replace('-', ""),
            format!("urn:uuid:{canonical}"),
            format!("{{{canonical}}}"),
        ] {
            assert_eq!(
                uuid_element(&Json::Str(PyStr::new(form.as_str()))).expect("uuid"),
                Some(expected),
                "{form}"
            );
        }
        // An int is `uuid.UUID(int=…)`; `bool` is an int subclass.
        assert_eq!(
            uuid_element(&Json::from(123_i64)).expect("int"),
            Some(Uuid::from_u128(123))
        );
        assert_eq!(
            uuid_element(&Json::Bool(true)).expect("bool"),
            Some(Uuid::from_u128(1))
        );
        // `None` is a NULL element; everything else is a ValidationError.
        assert_eq!(uuid_element(&Json::Null).expect("null"), None);
        for value in [
            Json::from(-1_i64),
            Json::Float(1.5),
            Json::List(Vec::new()),
            Json::Dict(plane_serde::pyjson::PyDict::new()),
            Json::Str(PyStr::new("abc")),
        ] {
            assert_eq!(uuid_element(&value), Err(ViewError::Validation));
        }
    }
}
