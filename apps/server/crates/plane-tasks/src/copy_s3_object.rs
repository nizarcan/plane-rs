// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/copy_s3_object.py:117-153` —
//! `copy_s3_objects_of_description_and_assets`.
//!
//! Duplicating a page (route 133) copies the page's `description_html` but not
//! its attachments; this task duplicates the `FileAsset` rows referenced by
//! `<image-component src="…">` and rewrites their `src`:
//!
//! 1. `extract_asset_ids` (`:41-49`) parses the HTML with
//!    `BeautifulSoup(html, "html.parser")` and keeps every `src` that is a
//!    **version-4** UUID (`plane/utils/uuid.py:8-13`), so external URLs are
//!    ignored.
//! 2. `copy_assets` (`:88-115`) filters `file_assets` by workspace, project and
//!    `id__in` (the soft-delete manager hides `deleted_at` rows), inserts a new
//!    row per match with a fresh UUID and `asset = f"{workspace_id}/\
//!    {uuid4().hex}-{attributes.get('name')}"`, copies the S3 object
//!    (`S3Storage.copy_object`, `storage.py:171-183`), then flips
//!    `is_uploaded` on the new rows in one `UPDATE`. **Upstream bug kept:**
//!    `created_by_id=user_id` is passed to `objects.create()`, i.e. to the
//!    model constructor; the following `save()` sees no current user and
//!    overwrites it with NULL (`plane/db/models/base.py:29-33`), so the copied
//!    rows have neither an author nor a modifier.
//! 3. `update_description` (`:73-76`) re-parses the HTML, rewrites each
//!    matching `src` and saves the entity — **always**, even with no assets,
//!    so the stored HTML is normalised by bs4's serialiser. `Page.save`/
//!    `Issue.save` recompute `description_stripped` with
//!    `plane.utils.html_processor.strip_tags` and, because a task has no
//!    current user, `BaseModel.save` writes `created_by_id`/`updated_by_id` as
//!    NULL (`plane/db/models/base.py:23-44`).
//! 4. `sync_with_external_service` (`:79-99`) POSTs `{description_html,
//!    variant}` to `LIVE_URL/convert-document/` (`requests.post(json=…)`,
//!    preset `LIVE_CONVERT`). A `200` stores the returned `description_json`
//!    and base64-decoded `description_binary`; any other status or a missing
//!    `LIVE_URL` stores nothing.
//!
//! Everything is inside `try`/`except Exception` (`:150-153`): the handler
//! logs and succeeds. A `KeyError`/`TypeError` after the HTML save (a
//! `description_binary` that is missing or not a string) therefore leaves the
//! earlier writes in place, exactly as Django does.
//!
//! **Upstream bug kept:** `get_entity_id_field` maps `WORKSPACE_LOGO` to
//! `workspace_id` and `PROJECT_COVER` to `project_id` (`:26-33`), but
//! `FileAsset.objects.create(..., workspace=workspace, project_id=project_id,
//! **…)` already passes both keywords, so such an asset raises `TypeError`
//! (multiple values) and the task aborts after the first copy. The port fails
//! the same way. `entity_identifier` is still passed to the row's mapped
//! column for every other type.
//!
//! Ownership: the only producer is `PageDuplicateEndpoint.post`
//! (`plane/app/views/page/base.py:617`), a Rust `/api/` route (133), so the
//! task moves with this change.

use std::sync::Arc;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_net::{Method, OutboundRequest, SsrfPolicy, preset};
use plane_serde::Dialect;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{TaskContext, bs4, kwargs, string_value, swallow, uuid_value};

/// The Celery task name (`plane/bgtasks/copy_s3_object.py:117`).
pub const TASK: &str = "plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets";

/// The tag whose `src` is rewritten (`:41`, `:60`, `:145`).
const TAG: &str = "image-component";

/// Register the handler; the worker's registry owns the closure.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(TASK, move |job| {
        let context = Arc::clone(&context);
        Box::pin(async move { handle(&context, job).await })
    });
}

#[derive(Debug, Clone)]
struct Args {
    entity_name: String,
    entity_identifier: Uuid,
    project_id: Option<Uuid>,
}

fn decode(payload: &Value) -> Option<Args> {
    let kw = kwargs(payload)?;
    Some(Args {
        entity_name: string_value(kw.get("entity_name"))?,
        entity_identifier: uuid_value(kw.get("entity_identifier"))?,
        // `filter(project_id=…)` takes None as IS NULL; a missing kwarg is a
        // TypeError in Python, which `uuid_value(None)` also rejects.
        project_id: uuid_value(kw.get("project_id")),
    })
    .filter(|args| {
        // `created_by_id=user_id` is still a required kwarg in Python: a
        // missing or malformed value is a `ValidationError` inside the task.
        let _ = args;
        kw.get("user_id")
            .and_then(|value| uuid_value(Some(value)))
            .is_some()
    })
}

async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    let Some(args) = decode(&job.payload) else {
        swallow("invalid copy_s3_objects payload", TASK);
        return Ok(());
    };
    if let Err(error) = run(context, &job.client, &args).await {
        swallow(error, TASK);
    }
    Ok(())
}

type Failure = String;

/// One `file_assets` row the task duplicates.
struct OriginalAsset {
    id: Uuid,
    attributes: Value,
    asset: String,
    size: f64,
    entity_type: Option<String>,
    storage_metadata: Option<Value>,
}

struct Entity {
    workspace_id: Uuid,
    description_html: String,
    /// Only `issues` carry these; `pages` leave them `None`.
    state_id: Option<Uuid>,
    completed_at: Option<DateTime<Utc>>,
}

async fn run(context: &TaskContext, client: &Client, args: &Args) -> Result<(), Failure> {
    let (entity, is_page) = load_entity(client, &args.entity_name, args.entity_identifier).await?;

    // `extract_asset_ids(entity.description_html, "image-component")`.
    let soup = bs4::Soup::parse(&entity.description_html);
    let mut asset_ids: Vec<Uuid> = Vec::new();
    for src in soup.src_values(TAG) {
        if let Ok(id) = Uuid::parse_str(&src)
            && id.get_version_num() == 4
        {
            asset_ids.push(id);
        }
    }

    let Some(storage) = context.storage.as_ref() else {
        return Err("object storage is not configured for this worker".to_string());
    };

    let originals =
        original_assets(client, entity.workspace_id, args.project_id, &asset_ids).await?;

    let mut duplicated: Vec<(String, String)> = Vec::new();
    let mut new_ids: Vec<Uuid> = Vec::new();
    for original in &originals {
        // `get_entity_id_field` duplicates a keyword for these two types.
        if matches!(
            original.entity_type.as_deref(),
            Some("WORKSPACE_LOGO" | "PROJECT_COVER")
        ) {
            return Err("TypeError: got multiple values for keyword argument".to_string());
        }
        let new_id = Uuid::new_v4();
        // `uuid.uuid4().hex` is 32 lowercase hex digits, no dashes.
        let key = format!(
            "{}/{}-{}",
            entity.workspace_id,
            Uuid::new_v4().simple(),
            py_str(original.attributes.get("name").unwrap_or(&Value::Null))
        );
        insert_asset(client, args, &entity, original, new_id, &key).await?;
        // `S3Storage.copy_object` logs a `ClientError` and returns None; the
        // row stays behind. A transport failure propagates in Python (it is
        // not a `ClientError`) and aborts the task.
        storage
            .worker()
            .copy_object(&original.asset, &key)
            .await
            .map_err(|error| error.to_string())?;
        duplicated.push((original.id.to_string(), new_id.to_string()));
        new_ids.push(new_id);
    }
    if !new_ids.is_empty() {
        client
            .execute(
                "UPDATE file_assets SET is_uploaded = true WHERE id = ANY($1::uuid[])",
                &[&new_ids],
            )
            .await
            .map_err(|error| error.to_string())?;
    }

    // `update_description(entity, duplicated_assets, "image-component")`.
    let mut soup = bs4::Soup::parse(&entity.description_html);
    for (old, new) in &duplicated {
        soup.replace_src(TAG, old, new);
    }
    let updated_html = soup.serialize();
    let stripped = if updated_html.is_empty() {
        None
    } else {
        Some(bs4::strip_tags(&updated_html))
    };
    save_description(
        client,
        args,
        &entity,
        is_page,
        &updated_html,
        stripped.as_deref(),
    )
    .await?;

    // `sync_with_external_service`.
    let Some(external) = convert_document(context, &args.entity_name, &updated_html).await else {
        return Ok(());
    };
    let description_json = external.get("description_json").cloned();
    let binary = match external.get("description_binary").and_then(Value::as_str) {
        Some(text) => decode_base64(text)?,
        // `base64.b64decode(None)` is a TypeError inside the task.
        None => return Err("TypeError: argument should be a bytes-like object".to_string()),
    };
    // A second full `save()`: `description_json` is NOT NULL, so a missing
    // key is an IntegrityError, as in Python.
    let sql = if is_page {
        "UPDATE pages SET description_json = $2, description_binary = $3, \
                updated_at = now(), created_by_id = NULL, updated_by_id = NULL \
         WHERE id = $1"
    } else {
        "UPDATE issues SET description_json = $2, description_binary = $3, \
                updated_at = now(), created_by_id = NULL, updated_by_id = NULL \
         WHERE id = $1"
    };
    client
        .execute(sql, &[&args.entity_identifier, &description_json, &binary])
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn load_entity(
    client: &Client,
    entity_name: &str,
    entity_identifier: Uuid,
) -> Result<(Entity, bool), Failure> {
    match entity_name {
        "PAGE" => {
            let row = client
                .query_opt(
                    "SELECT workspace_id, description_html FROM pages \
                     WHERE id = $1 AND deleted_at IS NULL",
                    &[&entity_identifier],
                )
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Page matching query does not exist.".to_string())?;
            Ok((
                Entity {
                    workspace_id: row.get(0),
                    description_html: row.get(1),
                    state_id: None,
                    completed_at: None,
                },
                true,
            ))
        }
        "ISSUE" => {
            let row = client
                .query_opt(
                    "SELECT workspace_id, description_html, state_id, completed_at \
                     FROM issues WHERE id = $1 AND deleted_at IS NULL",
                    &[&entity_identifier],
                )
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Issue matching query does not exist.".to_string())?;
            Ok((
                Entity {
                    workspace_id: row.get(0),
                    description_html: row.get(1),
                    state_id: row.get(2),
                    completed_at: row.get(3),
                },
                false,
            ))
        }
        other => Err(format!("ValueError: Unsupported entity_name: {other}")),
    }
}

/// `FileAsset.objects.filter(workspace=…, project_id=…, id__in=…)`, ordered by
/// the model's `("-created_at",)`.
async fn original_assets(
    client: &Client,
    workspace_id: Uuid,
    project_id: Option<Uuid>,
    asset_ids: &[Uuid],
) -> Result<Vec<OriginalAsset>, Failure> {
    if asset_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            "SELECT id, attributes, asset, size, entity_type, storage_metadata \
             FROM file_assets \
             WHERE workspace_id = $1 AND project_id IS NOT DISTINCT FROM $2 \
               AND id = ANY($3::uuid[]) AND deleted_at IS NULL \
             ORDER BY created_at DESC",
            &[&workspace_id, &project_id, &asset_ids],
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| OriginalAsset {
            id: row.get(0),
            attributes: row.get(1),
            asset: row.get(2),
            size: row.get(3),
            entity_type: row.get(4),
            storage_metadata: row.get(5),
        })
        .collect())
}

/// `FileAsset.objects.create(...)` (`:99-111`).
async fn insert_asset(
    client: &Client,
    args: &Args,
    entity: &Entity,
    original: &OriginalAsset,
    id: Uuid,
    key: &str,
) -> Result<(), Failure> {
    // `{"name": …, "type": …, "size": …}`: three keys, raw values, `None` for
    // a missing member.
    let mut attributes = serde_json::Map::new();
    for name in ["name", "type", "size"] {
        attributes.insert(
            name.to_string(),
            original
                .attributes
                .get(name)
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    let attributes = Value::Object(attributes);

    let (issue_id, page_id, comment_id, user_id, draft_issue_id) = match original
        .entity_type
        .as_deref()
    {
        Some("ISSUE_ATTACHMENT" | "ISSUE_DESCRIPTION") => {
            (Some(args.entity_identifier), None, None, None, None)
        }
        Some("PAGE_DESCRIPTION") => (None, Some(args.entity_identifier), None, None, None),
        Some("COMMENT_DESCRIPTION") => (None, None, Some(args.entity_identifier), None, None),
        Some("DRAFT_ISSUE_DESCRIPTION") => (None, None, None, None, Some(args.entity_identifier)),
        Some("USER_AVATAR" | "USER_COVER") => {
            (None, None, None, Some(args.entity_identifier), None)
        }
        _ => (None, None, None, None, None),
    };
    let now = Utc::now();
    client
        .execute(
            "INSERT INTO file_assets (created_at, updated_at, id, attributes, asset, \
             created_by_id, updated_by_id, workspace_id, is_deleted, deleted_at, \
             is_archived, comment_id, entity_type, external_id, external_source, \
             is_uploaded, issue_id, page_id, project_id, size, storage_metadata, \
             user_id, draft_issue_id, entity_identifier) VALUES \
             ($1, $2, $3, $4, $5, NULL, NULL, $6, false, NULL, false, $7, $8, NULL, \
              NULL, false, $9, $10, $11, $12, $13, $14, $15, NULL)",
            &[
                &now,
                &now,
                &id,
                &attributes,
                &key,
                &entity.workspace_id,
                &comment_id,
                &original.entity_type,
                &issue_id,
                &page_id,
                &args.project_id,
                &original.size,
                &original.storage_metadata,
                &user_id,
                &draft_issue_id,
            ],
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// `entity.description_html = updated_html; entity.save()`.
async fn save_description(
    client: &Client,
    args: &Args,
    entity: &Entity,
    is_page: bool,
    updated_html: &str,
    stripped: Option<&str>,
) -> Result<(), Failure> {
    if is_page {
        client
            .execute(
                "UPDATE pages SET description_html = $2, description_stripped = $3, \
                        updated_at = now(), created_by_id = NULL, updated_by_id = NULL \
                 WHERE id = $1",
                &[&args.entity_identifier, &updated_html, &stripped],
            )
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    // `Issue._ensure_default_state` / `_sync_completed_at`
    // (`plane/db/models/issue.py:180-256`): a null state is replaced by the
    // project's default, and because `state_id` then changed, `completed_at`
    // follows the new state's group.
    let (state_id, completed_at) = match entity.state_id {
        Some(state) => (Some(state), entity.completed_at),
        None => match default_state(client, args.entity_identifier).await? {
            Some((state, completed)) => {
                (Some(state), if completed { Some(Utc::now()) } else { None })
            }
            None => (None, entity.completed_at),
        },
    };
    client
        .execute(
            "UPDATE issues SET description_html = $2, description_stripped = $3, \
                    state_id = $4, completed_at = $5, updated_at = now(), \
                    created_by_id = NULL, updated_by_id = NULL WHERE id = $1",
            &[
                &args.entity_identifier,
                &updated_html,
                &stripped,
                &state_id,
                &completed_at,
            ],
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// `State.objects.filter(~Q(is_triage=True), project=…, default=True).first()`
/// then the first non-triage state; `(id, group == "completed")`.
async fn default_state(client: &Client, issue_id: Uuid) -> Result<Option<(Uuid, bool)>, Failure> {
    let select = "SELECT s.id, s.\"group\" FROM states s \
                  WHERE s.project_id = (SELECT project_id FROM issues WHERE id = $1) \
                    AND s.deleted_at IS NULL AND s.is_triage = false";
    // `filter(..., default=True).first()` (`ordering = ("sequence",)`), then
    // the first non-triage state when no default exists.
    for filter in [" AND s.\"default\" = true", ""] {
        let rows = client
            .query(
                &format!("{select}{filter} ORDER BY s.sequence LIMIT 1"),
                &[&issue_id],
            )
            .await
            .map_err(|error| error.to_string())?;
        if let Some(row) = rows.into_iter().next() {
            return Ok(Some((row.get(0), row.get::<_, String>(1) == "completed")));
        }
    }
    Ok(None)
}

/// `sync_with_external_service` (`:79-99`). `None` is Python's `{}`.
async fn convert_document(
    context: &TaskContext,
    entity_name: &str,
    description_html: &str,
) -> Option<Value> {
    let live = context.settings.urls.live_url.as_deref()?;
    let url = normalize_url_path(&format!("{live}/convert-document/"));
    let variant = if entity_name == "PAGE" {
        "rich"
    } else {
        "document"
    };
    // `requests.post(json=…)`: `json.dumps` defaults (", "/": ", ensure_ascii)
    // and the JSON content type.
    let body = plane_serde::to_vec(
        Dialect::Django,
        &json!({"description_html": description_html, "variant": variant}),
    )
    .ok()?;
    let request = OutboundRequest::new()
        .header("Content-Type", "application/json")
        .body(body);
    let response = match context
        .http
        .fetch(
            &preset::LIVE_CONVERT,
            Method::POST,
            &SsrfPolicy::strict(),
            &url,
            &request,
        )
        .await
    {
        Ok((response, _)) => response,
        Err(error) => {
            tracing::warn!(target: "plane.tasks", task = TASK, %error, "convert-document request failed");
            return None;
        }
    };
    if response.status != 200 {
        return None;
    }
    match serde_json::from_slice(response.bytes()) {
        Ok(value) => Some(value),
        Err(error) => {
            // `response.json()` raises `ValueError`, which the task's outer
            // `except` swallows after the HTML save.
            tracing::warn!(target: "plane.tasks", task = TASK, %error, "convert-document response is not JSON");
            None
        }
    }
}

/// `plane.utils.url.normalize_url_path`: collapse runs of `/` in the path only.
fn normalize_url_path(url: &str) -> String {
    let (prefix, rest) = match url.find("://") {
        Some(index) => {
            let after = index + 3;
            match rest_path_start(&url[after..]) {
                Some(offset) => (&url[..after + offset], &url[after + offset..]),
                None => (url, ""),
            }
        }
        None => ("", url),
    };
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let (path, suffix) = rest.split_at(end);
    let mut normalized = String::with_capacity(path.len());
    let mut last_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if last_slash {
                continue;
            }
            last_slash = true;
        } else {
            last_slash = false;
        }
        normalized.push(ch);
    }
    format!("{prefix}{normalized}{suffix}")
}

fn rest_path_start(after_authority: &str) -> Option<usize> {
    after_authority.find('/')
}

/// Python's `str()` for a JSON value, used by the destination key's f-string.
fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

/// `base64.b64decode(value)` with `validate=False`.
fn decode_base64(text: &str) -> Result<Vec<u8>, Failure> {
    let cleaned: String = text
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '/' | '='))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned)
        .map_err(|error| format!("binascii.Error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_normalisation_collapses_only_the_path() {
        assert_eq!(
            normalize_url_path("http://127.0.0.1:18080/live//convert-document/"),
            "http://127.0.0.1:18080/live/convert-document/"
        );
        assert_eq!(
            normalize_url_path("https://x.test//a///b?q=1//2#f//g"),
            "https://x.test/a/b?q=1//2#f//g"
        );
    }

    #[test]
    fn python_str_for_the_destination_key() {
        assert_eq!(py_str(&Value::Null), "None");
        assert_eq!(py_str(&json!("a b")), "a b");
        assert_eq!(py_str(&json!(5)), "5");
        assert_eq!(py_str(&json!(true)), "True");
    }

    #[test]
    fn base64_ignores_non_alphabet_characters() {
        assert_eq!(decode_base64("aGVs\nbG8=").expect("decoded"), b"hello");
    }
}
