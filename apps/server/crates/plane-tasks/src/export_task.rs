// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/export_task.py:1-228` — `issue_export_task`.
//!
//! The task builds one file per project (`multiple=True`) or one file for the
//! whole workspace, packs them in a ZIP, uploads the archive to the exports
//! bucket and stores a seven-day presigned URL on the `ExporterHistory` row
//! (`export_task.py:46-106,180-228`).
//!
//! # Rows
//!
//! `Issue.objects` is the plain `SoftDeletionManager`, **not** `issue_objects`
//! (`plane/db/models/issue.py:172`): drafts and triage-state issues are
//! exported, archived projects and soft-deleted issues are not. The filter Q
//! (`export_task.py:154-167`) requires the initiator's active project
//! membership and lets a guest see only their own issues unless the project
//! has `guest_view_all_features`.
//!
//! # Serializer
//!
//! `IssueExportSerializer` (`plane/utils/porters/serializers/issue.py:1-149`)
//! declares `sub_issues_count`, `link_count` and `attachment_count` as plain
//! `IntegerField(read_only=True)`. `Issue` has no such attribute and the
//! queryset does not annotate them, so DRF raises `AttributeError`, falls
//! through to `SkipField` and **omits the three keys entirely** — the header
//! row has 25 columns, not 28. Everything else is rendered here field by field
//! in declaration order.
//!
//! # Formats
//!
//! `CSVFormatter`, `JSONFormatter` and `XLSXFormatter`
//! (`plane/utils/porters/formatters.py:47-276`) define the bytes:
//! `json.dumps(data, indent=2, default=str)`; CSV with `.title()` headers,
//! flattened nested dicts, lists as `json.dumps` strings and Python's
//! `QUOTE_MINIMAL`/CRLF writer; XLSX through `openpyxl`, reproduced by
//! `plane-xlsx` with the injected clock.
//!
//! # Failure policy
//!
//! Everything is inside one `try` (`export_task.py:194-228`), whose `except`
//! marks the exporter `failed` with `str(error)` and returns. A missing
//! exporter row makes that `get` raise again, so the task fails; so does a
//! database error while marking it. Those are returned as handler errors, not
//! swallowed.
//!
//! # Ownership
//!
//! The only producer is `ExportIssuesEndpoint.post`
//! (`plane/app/views/exporter/base.py:24-62`), a Rust `/api/` route
//! (`crates/plane-http-app/src/issues/write_export.rs`), so the task moves
//! with this change.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use uuid::Uuid;

use crate::{TaskContext, args, kwargs, string_value, swallow, uuid_value};

/// The Celery task name (`plane/bgtasks/export_task.py:109`).
pub const TASK: &str = "plane.bgtasks.export_task.issue_export_task";

/// Register the handler.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(TASK, move |job| {
        let context = Arc::clone(&context);
        Box::pin(async move { handle(&context, job).await })
    });
}

/// The six positional-or-keyword arguments of `issue_export_task`.
#[derive(Debug, Clone)]
struct ExportArgs {
    provider: String,
    workspace_id: Uuid,
    project_ids: Vec<String>,
    token_id: String,
    multiple: bool,
    slug: String,
}

/// Read the arguments the way Celery's `apply()` binds them: keyword arguments
/// first, then the positional list in function order.
fn decode(payload: &serde_json::Value) -> Option<ExportArgs> {
    let kw = kwargs(payload);
    let positional = args(payload);
    let text = |name: &str, index: usize| -> Option<String> {
        kw.and_then(|map| map.get(name))
            .or_else(|| positional.and_then(|items| items.get(index)))
            .and_then(|value| string_value(Some(value)))
    };
    let boolean = |name: &str, index: usize| -> Option<bool> {
        kw.and_then(|map| map.get(name))
            .or_else(|| positional.and_then(|items| items.get(index)))
            .map(|value| match value {
                serde_json::Value::Bool(value) => *value,
                serde_json::Value::Number(number) => number.as_f64().is_some_and(|v| v != 0.0),
                serde_json::Value::String(text) => !text.is_empty(),
                serde_json::Value::Array(items) => !items.is_empty(),
                serde_json::Value::Object(map) => !map.is_empty(),
                serde_json::Value::Null => false,
            })
    };
    let project_ids = kw
        .and_then(|map| map.get("project_ids"))
        .or_else(|| positional.and_then(|items| items.get(2)))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|value| string_value(Some(value)))
                .collect()
        })?;
    Some(ExportArgs {
        provider: text("provider", 0)?,
        workspace_id: uuid_value(
            kw.and_then(|map| map.get("workspace_id"))
                .or_else(|| positional.and_then(|items| items.get(1))),
        )?,
        project_ids,
        token_id: text("token_id", 3)?,
        multiple: boolean("multiple", 4)?,
        slug: text("slug", 5)?,
    })
}

/// A failure rendered as Python's `str(exception)` for `ExporterHistory.reason`.
#[derive(Debug)]
struct Failure {
    message: String,
    /// A database error while recording the failure means Django's own
    /// `except` raises (it re-reads the exporter row and saves), so the task
    /// ends as a handler error.
    fatal: bool,
}

impl Failure {
    fn data(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fatal: false,
        }
    }
}

impl From<tokio_postgres::Error> for Failure {
    fn from(error: tokio_postgres::Error) -> Self {
        Self {
            message: error.to_string(),
            fatal: false,
        }
    }
}

impl From<plane_storage::StorageError> for Failure {
    fn from(error: plane_storage::StorageError) -> Self {
        Self {
            message: error.to_string(),
            fatal: false,
        }
    }
}

async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    let Some(arguments) = decode(&job.payload) else {
        // The outer `except` cannot even find the exporter: the task raises.
        return Err(HandlerError::fatal(
            "invalid issue_export_task payload: Celery TypeError/attribute error",
        ));
    };
    match run(context, &job.client, &arguments).await {
        Ok(()) => Ok(()),
        Err(failure) if failure.fatal => Err(HandlerError::fatal(failure.message)),
        Err(failure) => {
            // `export_task.py:222-228`: `exporter_instance.status = "failed";
            // reason = str(e); save(update_fields=["status", "reason"])`.
            if let Err(error) =
                mark_failed(&job.client, &arguments.token_id, &failure.message).await
            {
                return Err(HandlerError::fatal(format!(
                    "{} (and marking the exporter failed: {error})",
                    failure.message
                )));
            }
            swallow(failure.message, TASK);
            Ok(())
        }
    }
}

async fn run(
    context: &TaskContext,
    client: &Client,
    arguments: &ExportArgs,
) -> Result<(), Failure> {
    let row = client
        .query_opt(
            "SELECT id, initiated_by_id FROM exporters \
             WHERE token = $1 AND deleted_at IS NULL",
            &[&arguments.token_id],
        )
        .await?;
    let Some(row) = row else {
        // `DoesNotExist` is caught by the outer `except`, whose own `get`
        // raises the same exception: the task fails.
        return Err(Failure {
            message: "ExporterHistory matching query does not exist.".to_string(),
            fatal: true,
        });
    };
    let exporter_id: Uuid = row.try_get(0)?;
    let initiated_by: Uuid = row.try_get(1)?;
    // `workspace__id=workspace_id`: the argument the producer sent, not the
    // exporter row's workspace.
    let workspace_id = arguments.workspace_id;

    // `status = "processing"; save(update_fields=["status"])`.
    client
        .execute(
            "UPDATE exporters SET status = 'processing' WHERE id = $1",
            &[&exporter_id],
        )
        .await?;

    // `DataExporter(...)` raises ValueError before the query runs; that is
    // still inside the outer try, so the row is marked failed.
    if !matches!(arguments.provider.as_str(), "csv" | "json" | "xlsx") {
        return Err(Failure::data(format!(
            "Unsupported format: {}. Available: ['csv', 'json', 'xlsx']",
            arguments.provider
        )));
    }

    let mut project_ids = Vec::with_capacity(arguments.project_ids.len());
    for text in &arguments.project_ids {
        project_ids.push(
            Uuid::parse_str(text).map_err(|_| {
                Failure::data(format!("\u{201c}{text}\u{201d} is not a valid UUID."))
            })?,
        );
    }

    let storage = context
        .storage
        .as_ref()
        .ok_or_else(|| Failure::data("object storage is not configured for this worker"))?;

    let issues = fetch_issues(client, workspace_id, initiated_by, &project_ids).await?;
    let files = if arguments.multiple {
        let mut files = Vec::with_capacity(project_ids.len());
        for (index, project_id) in project_ids.iter().enumerate() {
            let subset: Vec<&IssueData> = issues
                .iter()
                .filter(|issue| issue.project_id == *project_id)
                .collect();
            let name = format!("{}-{}", arguments.slug, arguments.project_ids[index]);
            files.push(encode(arguments.provider.as_str(), &name, &subset)?);
        }
        files
    } else {
        let all: Vec<&IssueData> = issues.iter().collect();
        vec![encode(
            arguments.provider.as_str(),
            &format!("{}-{workspace_id}", arguments.slug),
            &all,
        )?]
    };

    let archive = plane_xlsx::zip(
        &files
            .iter()
            .map(|file| (file.0.as_str(), file.1.clone()))
            .collect::<Vec<_>>(),
        Utc::now(),
    );

    let now = Utc::now();
    let key = format!(
        "{}/export-{}-{}-{}.zip",
        workspace_id,
        arguments.slug,
        arguments.token_id.chars().take(6).collect::<String>(),
        now.format("%Y-%m-%d")
    );
    storage.export_upload(&key, &archive).await?;
    let presigned_url = storage.export_download_url(&key, 7 * 24 * 60 * 60).await?;

    // `if presigned_url:` — always true here; `else: status = "failed"` is the
    // unreachable branch Django keeps.
    client
        .execute(
            "UPDATE exporters SET status = 'completed', url = $2, key = $3 WHERE id = $1",
            &[&exporter_id, &presigned_url, &key],
        )
        .await?;
    Ok(())
}

async fn mark_failed(
    client: &Client,
    token: &str,
    reason: &str,
) -> Result<(), tokio_postgres::Error> {
    client
        .execute(
            "UPDATE exporters SET status = 'failed', reason = $2 \
             WHERE token = $1 AND deleted_at IS NULL",
            &[&token, &reason],
        )
        .await
        .map(|_| ())
}

/// `(filename_with_extension, bytes)`.
fn encode(provider: &str, base: &str, issues: &[&IssueData]) -> Result<(String, Vec<u8>), Failure> {
    match provider {
        "csv" => Ok((format!("{base}.csv"), render_csv(issues).into_bytes())),
        "json" => Ok((format!("{base}.json"), render_json(issues).into_bytes())),
        "xlsx" => Ok((
            format!("{base}.xlsx"),
            render_xlsx(issues).map_err(|error| Failure::data(error.to_string()))?,
        )),
        _ => Err(Failure::data(format!(
            "Unsupported format: {provider}. Available: ['csv', 'json', 'xlsx']"
        ))),
    }
}

// ---------------------------------------------------------------------------
// The query
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct IssueData {
    project_id: Uuid,
    project_name: String,
    project_identifier: String,
    parent: Option<(String, i32)>,
    sequence_id: i32,
    name: String,
    state_name: Option<String>,
    priority: String,
    created_by_name: Option<String>,
    start_date: Option<chrono::NaiveDate>,
    target_date: Option<chrono::NaiveDate>,
    completed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    archived_at: Option<chrono::NaiveDate>,
    estimate: String,
    is_draft: bool,
    // Prefetched child rows, keyed by owner issue id.
    assignees: Vec<String>,
    subscribers: Vec<String>,
    labels: Vec<String>,
    cycles: Vec<String>,
    modules: Vec<String>,
    links: Vec<(String, String)>,
    relations: Vec<(String, String, &'static str)>,
    comments: Vec<(String, String, String)>,
}

async fn fetch_issues(
    client: &Client,
    workspace_id: Uuid,
    initiated_by: Uuid,
    project_ids: &[Uuid],
) -> Result<Vec<IssueData>, Failure> {
    if project_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            "SELECT i.id, i.project_id, p.name, p.identifier, \
                    i.sequence_id, i.name, s.name, i.priority, \
                    u.first_name, u.last_name, \
                    i.start_date, i.target_date, i.completed_at, i.created_at, \
                    i.updated_at, i.archived_at, ep.value, i.is_draft, \
                    pi.sequence_id, pp.identifier \
             FROM issues i \
             JOIN projects p ON p.id = i.project_id \
             JOIN project_members pm ON pm.project_id = p.id \
                  AND pm.member_id = $2 AND pm.is_active = TRUE \
                  AND pm.deleted_at IS NULL \
             LEFT JOIN states s ON s.id = i.state_id \
             LEFT JOIN users u ON u.id = i.created_by_id \
             LEFT JOIN estimate_points ep ON ep.id = i.estimate_point_id \
             LEFT JOIN issues pi ON pi.id = i.parent_id \
             LEFT JOIN projects pp ON pp.id = pi.project_id \
             WHERE i.workspace_id = $1 AND i.project_id = ANY($3::uuid[]) \
               AND i.deleted_at IS NULL AND p.archived_at IS NULL \
               AND (pm.role > 5 OR p.guest_view_all_features OR i.created_by_id = $2) \
             ORDER BY i.created_at DESC",
            &[&workspace_id, &initiated_by, &project_ids],
        )
        .await?;
    let ids: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();
    let mut issues = Vec::with_capacity(rows.len());
    for row in &rows {
        let created_by_name = match (
            row.try_get::<_, Option<String>>(8)?,
            row.try_get::<_, Option<String>>(9)?,
        ) {
            (Some(first), Some(last)) => Some(format!("{first} {last}").trim().to_string()),
            _ => None,
        };
        let parent = match row.try_get::<_, Option<i32>>(18)? {
            Some(sequence_id) => row
                .try_get::<_, Option<String>>(19)?
                .map(|identifier| (identifier, sequence_id)),
            None => None,
        };
        issues.push(IssueData {
            project_id: row.try_get(1)?,
            project_name: row.try_get(2)?,
            project_identifier: row.try_get(3)?,
            parent,
            sequence_id: row.try_get(4)?,
            name: row.try_get(5)?,
            state_name: row.try_get(6)?,
            priority: row.try_get(7)?,
            created_by_name,
            start_date: row.try_get(10)?,
            target_date: row.try_get(11)?,
            completed_at: row.try_get(12)?,
            created_at: row.try_get(13)?,
            updated_at: row.try_get(14)?,
            archived_at: row.try_get(15)?,
            estimate: row.try_get::<_, Option<String>>(16)?.unwrap_or_default(),
            is_draft: row.try_get(17)?,
            assignees: Vec::new(),
            subscribers: Vec::new(),
            labels: Vec::new(),
            cycles: Vec::new(),
            modules: Vec::new(),
            links: Vec::new(),
            relations: Vec::new(),
            comments: Vec::new(),
        });
    }
    let mut by_id: HashMap<Uuid, usize> = HashMap::with_capacity(ids.len());
    for (index, id) in ids.iter().enumerate() {
        by_id.insert(*id, index);
    }

    // `prefetch_related("assignees")`: `IssueAssignee.objects` filters
    // `deleted_at`; the user join is a plain FK and `get_assignees` drops
    // inactive users in Python.
    for row in client
        .query(
            "SELECT ia.issue_id, u.first_name, u.last_name, u.is_active \
             FROM issue_assignees ia JOIN users u ON u.id = ia.assignee_id \
             WHERE ia.issue_id = ANY($1::uuid[]) AND ia.deleted_at IS NULL",
            &[&ids],
        )
        .await?
    {
        if !row.try_get::<_, bool>(3)? {
            continue;
        }
        let name = full_name(row.try_get(1)?, row.try_get(2)?);
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.assignees,
            name,
        );
    }

    // `prefetch_related("issue_subscribers", select_related("subscriber"))`.
    for row in client
        .query(
            "SELECT s.issue_id, u.first_name, u.last_name \
             FROM issue_subscribers s JOIN users u ON u.id = s.subscriber_id \
             WHERE s.issue_id = ANY($1::uuid[]) AND s.deleted_at IS NULL",
            &[&ids],
        )
        .await?
    {
        let name = full_name(row.try_get(1)?, row.try_get(2)?);
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.subscribers,
            name,
        );
    }

    // `obj.label_issue.all()`: `IssueLabel.Meta.ordering = ("-created_at",)`.
    for row in client
        .query(
            "SELECT il.issue_id, l.name FROM issue_labels il \
             JOIN labels l ON l.id = il.label_id \
             WHERE il.issue_id = ANY($1::uuid[]) AND il.deleted_at IS NULL \
             ORDER BY il.created_at DESC",
            &[&ids],
        )
        .await?
    {
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.labels,
            row.try_get(1)?,
        );
    }

    // `issue_cycle__cycle`: `CycleIssue.Meta.ordering = ("-created_at",)`.
    for row in client
        .query(
            "SELECT ci.issue_id, c.name FROM cycle_issues ci \
             JOIN cycles c ON c.id = ci.cycle_id \
             WHERE ci.issue_id = ANY($1::uuid[]) AND ci.deleted_at IS NULL \
             ORDER BY ci.created_at DESC",
            &[&ids],
        )
        .await?
    {
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.cycles,
            row.try_get(1)?,
        );
    }

    // `issue_module__module`: `ModuleIssue.Meta.ordering = ("-created_at",)`.
    for row in client
        .query(
            "SELECT mi.issue_id, m.name FROM module_issues mi \
             JOIN modules m ON m.id = mi.module_id \
             WHERE mi.issue_id = ANY($1::uuid[]) AND mi.deleted_at IS NULL \
             ORDER BY mi.created_at DESC",
            &[&ids],
        )
        .await?
    {
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.modules,
            row.try_get(1)?,
        );
    }

    // `issue_link`: `IssueLink.Meta.ordering = ("-created_at",)`.
    for row in client
        .query(
            "SELECT issue_id, url, title FROM issue_links \
             WHERE issue_id = ANY($1::uuid[]) AND deleted_at IS NULL \
             ORDER BY created_at DESC",
            &[&ids],
        )
        .await?
    {
        let url: String = row.try_get(1)?;
        let title: Option<String> = row.try_get(2)?;
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.links,
            (url.clone(), title.unwrap_or(url)),
        );
    }

    // `issue_relation` (outgoing) and `issue_related` (incoming);
    // `IssueRelation.Meta.ordering = ("-created_at",)`.
    for row in client
        .query(
            "SELECT ir.issue_id, ir.relation_type, ri.sequence_id, p.identifier \
             FROM issue_relations ir \
             JOIN issues ri ON ri.id = ir.related_issue_id \
             JOIN projects p ON p.id = ri.project_id \
             WHERE ir.issue_id = ANY($1::uuid[]) AND ir.deleted_at IS NULL \
             ORDER BY ir.created_at DESC",
            &[&ids],
        )
        .await?
    {
        let relation_type: String = row.try_get(1)?;
        let identifier: String = row.try_get(3)?;
        let sequence_id: i32 = row.try_get(2)?;
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.relations,
            (
                relation_type,
                format!("{identifier}-{sequence_id}"),
                "outgoing",
            ),
        );
    }
    for row in client
        .query(
            "SELECT ir.related_issue_id, ir.relation_type, i.sequence_id, p.identifier \
             FROM issue_relations ir \
             JOIN issues i ON i.id = ir.issue_id \
             JOIN projects p ON p.id = i.project_id \
             WHERE ir.related_issue_id = ANY($1::uuid[]) AND ir.deleted_at IS NULL \
             ORDER BY ir.created_at DESC",
            &[&ids],
        )
        .await?
    {
        let relation_type: String = row.try_get(1)?;
        let identifier: String = row.try_get(3)?;
        let sequence_id: i32 = row.try_get(2)?;
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.relations,
            (
                relation_type,
                format!("{identifier}-{sequence_id}"),
                "incoming",
            ),
        );
    }

    // `Prefetch("issue_comments", order_by("created_at"))`.
    for row in client
        .query(
            "SELECT c.issue_id, c.comment_stripped, u.first_name, u.last_name, c.created_at \
             FROM issue_comments c LEFT JOIN users u ON u.id = c.actor_id \
             WHERE c.issue_id = ANY($1::uuid[]) AND c.deleted_at IS NULL \
             ORDER BY c.created_at ASC",
            &[&ids],
        )
        .await?
    {
        let actor_name = match (
            row.try_get::<_, Option<String>>(2)?,
            row.try_get::<_, Option<String>>(3)?,
        ) {
            (Some(first), Some(last)) => full_name(first, last),
            _ => String::new(),
        };
        let created_at: DateTime<Utc> = row.try_get(4)?;
        push(
            &mut issues,
            &by_id,
            row.try_get(0)?,
            |issue| &mut issue.comments,
            (
                row.try_get::<_, String>(1)?,
                actor_name,
                created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            ),
        );
    }

    Ok(issues)
}

fn full_name(first: String, last: String) -> String {
    format!("{first} {last}").trim().to_string()
}

fn push<T>(
    issues: &mut [IssueData],
    by_id: &HashMap<Uuid, usize>,
    id: Uuid,
    field: impl Fn(&mut IssueData) -> &mut Vec<T>,
    value: T,
) {
    if let Some(index) = by_id.get(&id) {
        field(&mut issues[*index]).push(value);
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// The subset of Python values the serializer emits, in insertion order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PyValue {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    List(Vec<PyValue>),
    Map(Vec<(String, PyValue)>),
}

impl PyValue {
    fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    fn opt_date(value: Option<chrono::NaiveDate>) -> Self {
        value.map_or(Self::Null, |value| {
            Self::Text(value.format("%Y-%m-%d").to_string())
        })
    }

    fn opt_datetime(value: Option<DateTime<Utc>>) -> Self {
        value.map_or(Self::Null, drf_datetime)
    }
}

/// DRF `DateTimeField`: `isoformat()` with microseconds only when non-zero and
/// `+00:00` replaced by `Z` (`rest_framework/fields.py:1216-1232`).
fn drf_datetime(value: DateTime<Utc>) -> PyValue {
    let mut text = value.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = value.timestamp_subsec_micros();
    if micros != 0 {
        text.push_str(&format!(".{micros:06}"));
    }
    text.push('Z');
    PyValue::Text(text)
}

/// One serializer row, in declaration order. `sub_issues_count`, `link_count`
/// and `attachment_count` are omitted: see the module docs.
fn serialize(issue: &IssueData) -> PyValue {
    PyValue::Map(vec![
        (
            "project_name".to_string(),
            PyValue::text(&issue.project_name),
        ),
        (
            "project_identifier".to_string(),
            PyValue::text(&issue.project_identifier),
        ),
        (
            "parent".to_string(),
            PyValue::text(match &issue.parent {
                Some((identifier, sequence_id)) => format!("{identifier}-{sequence_id}"),
                None => String::new(),
            }),
        ),
        (
            "identifier".to_string(),
            PyValue::text(format!(
                "{}-{}",
                issue.project_identifier, issue.sequence_id
            )),
        ),
        (
            "sequence_id".to_string(),
            PyValue::Int(i64::from(issue.sequence_id)),
        ),
        ("name".to_string(), PyValue::text(&issue.name)),
        (
            "state_name".to_string(),
            PyValue::text(issue.state_name.clone().unwrap_or_default()),
        ),
        ("priority".to_string(), PyValue::text(&issue.priority)),
        (
            "assignees".to_string(),
            PyValue::List(issue.assignees.iter().cloned().map(PyValue::Text).collect()),
        ),
        (
            "subscribers".to_string(),
            PyValue::List(
                issue
                    .subscribers
                    .iter()
                    .cloned()
                    .map(PyValue::Text)
                    .collect(),
            ),
        ),
        (
            "created_by_name".to_string(),
            PyValue::text(issue.created_by_name.clone().unwrap_or_default()),
        ),
        (
            "start_date".to_string(),
            PyValue::opt_date(issue.start_date),
        ),
        (
            "target_date".to_string(),
            PyValue::opt_date(issue.target_date),
        ),
        (
            "completed_at".to_string(),
            PyValue::opt_datetime(issue.completed_at),
        ),
        ("created_at".to_string(), drf_datetime(issue.created_at)),
        ("updated_at".to_string(), drf_datetime(issue.updated_at)),
        (
            "archived_at".to_string(),
            PyValue::opt_date(issue.archived_at),
        ),
        ("estimate".to_string(), PyValue::text(&issue.estimate)),
        (
            "labels".to_string(),
            PyValue::List(issue.labels.iter().cloned().map(PyValue::Text).collect()),
        ),
        (
            "cycles".to_string(),
            PyValue::List(issue.cycles.iter().cloned().map(PyValue::Text).collect()),
        ),
        (
            "modules".to_string(),
            PyValue::List(issue.modules.iter().cloned().map(PyValue::Text).collect()),
        ),
        (
            "links".to_string(),
            PyValue::List(
                issue
                    .links
                    .iter()
                    .map(|(url, title)| {
                        PyValue::Map(vec![
                            ("url".to_string(), PyValue::text(url)),
                            ("title".to_string(), PyValue::text(title)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "relations".to_string(),
            PyValue::List(
                issue
                    .relations
                    .iter()
                    .map(|(kind, issue, direction)| {
                        PyValue::Map(vec![
                            ("type".to_string(), PyValue::text(kind)),
                            ("issue".to_string(), PyValue::text(issue)),
                            ("direction".to_string(), PyValue::text(*direction)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "comments".to_string(),
            PyValue::List(
                issue
                    .comments
                    .iter()
                    .map(|(comment, created_by, created_at)| {
                        PyValue::Map(vec![
                            ("comment".to_string(), PyValue::text(comment)),
                            ("created_by".to_string(), PyValue::text(created_by)),
                            ("created_at".to_string(), PyValue::text(created_at)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("is_draft".to_string(), PyValue::Bool(issue.is_draft)),
    ])
}

/// `json.dumps(data, indent=2, default=str)`.
fn render_json(issues: &[&IssueData]) -> String {
    let rows: Vec<PyValue> = issues.iter().map(|issue| serialize(issue)).collect();
    let mut out = String::new();
    write_json(&PyValue::List(rows), 0, &mut out);
    out
}

fn write_json(value: &PyValue, indent: usize, out: &mut String) {
    match value {
        PyValue::Null => out.push_str("null"),
        PyValue::Bool(true) => out.push_str("true"),
        PyValue::Bool(false) => out.push_str("false"),
        PyValue::Int(number) => out.push_str(&number.to_string()),
        PyValue::Text(text) => write_json_string(text, out),
        PyValue::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&" ".repeat(indent + 2));
                write_json(item, indent + 2, out);
            }
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push(']');
        }
        PyValue::Map(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                out.push_str(&" ".repeat(indent + 2));
                write_json_string(key, out);
                out.push_str(": ");
                write_json(item, indent + 2, out);
            }
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
    }
}

/// `json.dumps(text)` with its defaults: `ensure_ascii=True`, the short
/// escapes for the controls Python has names for, and surrogate pairs for
/// astral code points.
fn write_json_string(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(character),
            _ => {
                let code = u32::from(character);
                if code <= 0xFFFF {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    let value = code - 0x1_0000;
                    let high = 0xD800 + (value >> 10);
                    let low = 0xDC00 + (value & 0x3FF);
                    out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// `CSVFormatter.encode` (`formatters.py:98-141`): the union of keys in first
/// seen order, `.title()` headers, nested dicts flattened with `__`, lists as
/// `json.dumps` with default separators, `sanitize_csv_row` then
/// `csv.writer`'s `QUOTE_MINIMAL` and `\r\n`.
fn render_csv(issues: &[&IssueData]) -> String {
    // `CSVFormatter.encode`: `if not data: return ""`.
    if issues.is_empty() {
        return String::new();
    }
    let rows: Vec<Vec<(String, PyValue)>> = issues
        .iter()
        .map(|issue| flatten(&serialize(issue)))
        .collect();
    let mut fieldnames: Vec<String> = Vec::new();
    for row in &rows {
        for (key, _) in row {
            if !fieldnames.contains(key) {
                fieldnames.push(key.clone());
            }
        }
    }
    let mut out = String::new();
    let headers: Vec<String> = fieldnames
        .iter()
        .map(|key| sanitize(&python_title(&key.replace('_', " "))))
        .collect();
    write_csv_row(&mut out, headers.iter().map(String::as_str));
    for row in &rows {
        let values: Vec<String> = fieldnames
            .iter()
            .map(|key| {
                let value = row
                    .iter()
                    .find(|(name, _)| name == key)
                    .map_or(PyValue::Null, |(_, value)| value.clone());
                sanitize(&csv_text(&value))
            })
            .collect();
        write_csv_row(&mut out, values.iter().map(String::as_str));
    }
    out
}

fn flatten(value: &PyValue) -> Vec<(String, PyValue)> {
    let PyValue::Map(entries) = value else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, value) in entries {
        match value {
            PyValue::Map(_) => {
                for (nested, nested_value) in flatten(value) {
                    out.push((format!("{key}__{nested}"), nested_value));
                }
            }
            PyValue::List(_) => out.push((key.clone(), PyValue::text(json_compact(value)))),
            _ => out.push((key.clone(), value.clone())),
        }
    }
    out
}

/// `str()` for one flattened CSV cell: `None` is an empty field.
fn csv_text(value: &PyValue) -> String {
    match value {
        PyValue::Null => String::new(),
        PyValue::Bool(true) => "True".to_string(),
        PyValue::Bool(false) => "False".to_string(),
        PyValue::Int(number) => number.to_string(),
        PyValue::Text(text) => text.clone(),
        PyValue::List(_) | PyValue::Map(_) => py_repr(value),
    }
}

/// `csv.writer` with the excel dialect: quote only when needed, double the
/// quotes, LF/CRLF inside a field are quoted, `\r\n` terminates the row.
fn write_csv_row<'a>(out: &mut String, fields: impl Iterator<Item = &'a str>) {
    let mut first = true;
    for field in fields {
        if !first {
            out.push(',');
        }
        first = false;
        let needs_quotes = field
            .chars()
            .any(|character| matches!(character, ',' | '"' | '\r' | '\n'));
        if needs_quotes {
            out.push('"');
            for character in field.chars() {
                if character == '"' {
                    out.push('"');
                }
                out.push(character);
            }
            out.push('"');
        } else {
            out.push_str(field);
        }
    }
    out.push_str("\r\n");
}

/// `plane.utils.csv_utils.sanitize_csv_value`.
fn sanitize(value: &str) -> String {
    match value.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r' | '\n') => {
            format!("'{value}")
        }
        _ => value.to_string(),
    }
}

/// `header.replace("_", " ").title()`.
fn python_title(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_is_alpha = false;
    for character in text.chars() {
        if previous_is_alpha {
            out.extend(character.to_lowercase());
        } else {
            out.extend(character.to_uppercase());
        }
        previous_is_alpha = character.is_alphabetic();
    }
    out
}

/// `json.dumps(value)` with default separators and `ensure_ascii=True`.
fn json_compact(value: &PyValue) -> String {
    match value {
        PyValue::Null => "null".to_string(),
        PyValue::Bool(true) => "true".to_string(),
        PyValue::Bool(false) => "false".to_string(),
        PyValue::Int(number) => number.to_string(),
        PyValue::Text(text) => {
            let mut out = String::new();
            write_json_string(text, &mut out);
            out
        }
        PyValue::List(items) => {
            let parts: Vec<String> = items.iter().map(json_compact).collect();
            format!("[{}]", parts.join(", "))
        }
        PyValue::Map(entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(key, value)| {
                    let mut rendered = String::new();
                    write_json_string(key, &mut rendered);
                    format!("{rendered}: {}", json_compact(value))
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// `str(value)` as the `XLSXFormatter` join uses it: a string is itself, every
/// other value is its `repr`.
fn py_str(value: &PyValue) -> String {
    match value {
        PyValue::Text(text) => text.clone(),
        other => py_repr(other),
    }
}

/// `str(value)` for a list element in `XLSXFormatter._format_value`: Python's
/// `repr` for a dict or a list, `str` otherwise.
fn py_repr(value: &PyValue) -> String {
    match value {
        PyValue::Null => "None".to_string(),
        PyValue::Bool(true) => "True".to_string(),
        PyValue::Bool(false) => "False".to_string(),
        PyValue::Int(number) => number.to_string(),
        PyValue::Text(text) => py_str_repr(text),
        PyValue::List(items) => {
            let parts: Vec<String> = items.iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        PyValue::Map(entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(key, value)| format!("{}: {}", py_str_repr(key), py_repr(value)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// Python's `repr(str)`: single quotes, `\\`/`\'`/named controls escaped,
/// printable non-ASCII kept.
fn py_str_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7F => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if is_python_printable(c) => out.push(c),
            c => {
                let code = u32::from(c);
                if code <= 0xFFFF {
                    out.push_str(&format!("\\u{code:04x}"));
                } else {
                    out.push_str(&format!("\\U{code:08x}"));
                }
            }
        }
    }
    out.push(quote);
    out
}

/// `str.isprintable` for the code points that appear in user text: the only
/// non-printable characters without a short escape are the Unicode
/// separators, format and surrogate planes. Rust `char` cannot be a
/// surrogate, so the remaining cases are the separators and format controls.
fn is_python_printable(character: char) -> bool {
    !matches!(
        character,
        '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200f}' | '\u{2028}' | '\u{2029}'
            | '\u{202f}' | '\u{205f}' | '\u{2060}'..='\u{206f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `XLSXFormatter.encode` (`formatters.py:218-276`).
fn render_xlsx(issues: &[&IssueData]) -> Result<Vec<u8>, plane_xlsx::XlsxError> {
    // `XLSXFormatter` does not flatten: `_format_value` turns a list into its
    // `", "` join and leaves a dict as JSON.
    let rows: Vec<Vec<(String, PyValue)>> = issues
        .iter()
        .map(|issue| match serialize(issue) {
            PyValue::Map(entries) => entries,
            _ => Vec::new(),
        })
        .collect();
    let mut fieldnames: Vec<String> = Vec::new();
    for row in &rows {
        for (key, _) in row {
            if !fieldnames.contains(key) {
                fieldnames.push(key.clone());
            }
        }
    }
    let mut cells: Vec<Vec<plane_xlsx::Cell>> = Vec::with_capacity(rows.len() + 1);
    cells.push(
        fieldnames
            .iter()
            .map(|key| plane_xlsx::Cell::Text(sanitize(&python_title(&key.replace('_', " ")))))
            .collect(),
    );
    for row in &rows {
        let mut line = Vec::with_capacity(fieldnames.len());
        for key in &fieldnames {
            let value = row
                .iter()
                .find(|(name, _)| name == key)
                .map_or(PyValue::Null, |(_, value)| value.clone());
            line.push(xlsx_cell(&value));
        }
        cells.push(line);
    }
    plane_xlsx::workbook(&cells, Utc::now())
}

fn xlsx_cell(value: &PyValue) -> plane_xlsx::Cell {
    match value {
        PyValue::Null => plane_xlsx::Cell::Text(String::new()),
        PyValue::Bool(value) => plane_xlsx::Cell::Bool(*value),
        PyValue::Int(value) => plane_xlsx::Cell::Int(*value),
        PyValue::Text(text) => plane_xlsx::Cell::Text(sanitize(text)),
        PyValue::List(items) => {
            // `str(v)`, not `repr(v)`: a plain string contributes itself, a
            // dict contributes its Python repr (`formatters.py:250-256`).
            let joined: Vec<String> = items.iter().map(py_str).collect();
            plane_xlsx::Cell::Text(sanitize(&joined.join(", ")))
        }
        PyValue::Map(_) => {
            let rendered = json_compact(value);
            plane_xlsx::Cell::Text(sanitize(&rendered))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializer_omits_the_three_unannotated_count_fields() {
        let issue = IssueData {
            project_id: Uuid::nil(),
            project_name: "P".into(),
            project_identifier: "P".into(),
            parent: None,
            sequence_id: 1,
            name: "N".into(),
            state_name: None,
            priority: "none".into(),
            created_by_name: None,
            start_date: None,
            target_date: None,
            completed_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
            estimate: String::new(),
            is_draft: false,
            assignees: Vec::new(),
            subscribers: Vec::new(),
            labels: Vec::new(),
            cycles: Vec::new(),
            modules: Vec::new(),
            links: Vec::new(),
            relations: Vec::new(),
            comments: Vec::new(),
        };
        let PyValue::Map(entries) = serialize(&issue) else {
            panic!("map");
        };
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(keys.len(), 25);
        assert!(!keys.contains(&"sub_issues_count"));
        assert!(!keys.contains(&"link_count"));
        assert!(!keys.contains(&"attachment_count"));
    }

    #[test]
    fn json_matches_pythons_indent_2_ascii_output() {
        let value = PyValue::List(vec![PyValue::Map(vec![
            ("name".to_string(), PyValue::text("héllo 🚀")),
            ("n".to_string(), PyValue::Int(3)),
            ("b".to_string(), PyValue::Bool(true)),
            ("z".to_string(), PyValue::Null),
            ("list".to_string(), PyValue::List(Vec::new())),
        ])]);
        let mut out = String::new();
        write_json(&value, 0, &mut out);
        assert_eq!(
            out,
            "[\n  {\n    \"name\": \"h\\u00e9llo \\ud83d\\ude80\",\n    \"n\": 3,\n    \"b\": true,\n    \"z\": null,\n    \"list\": []\n  }\n]"
        );
    }

    #[test]
    fn csv_headers_and_lists_follow_python() {
        let issue = IssueData {
            project_id: Uuid::nil(),
            project_name: "Beta \"Mobile\", Inc.".into(),
            project_identifier: "BET".into(),
            parent: None,
            sequence_id: 1,
            name: "N".into(),
            state_name: None,
            priority: "none".into(),
            created_by_name: None,
            start_date: None,
            target_date: None,
            completed_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            archived_at: None,
            estimate: String::new(),
            is_draft: false,
            assignees: vec!["Mia".into()],
            subscribers: Vec::new(),
            labels: Vec::new(),
            cycles: Vec::new(),
            modules: Vec::new(),
            links: Vec::new(),
            relations: Vec::new(),
            comments: Vec::new(),
        };
        let csv = render_csv(&[&issue]);
        assert!(csv.starts_with("Project Name,Project Identifier,Parent,Identifier,Sequence Id,Name,State Name,Priority,Assignees,Subscribers,Created By Name,Start Date,Target Date,Completed At,Created At,Updated At,Archived At,Estimate,Labels,Cycles,Modules,Links,Relations,Comments,Is Draft\r\n"));
        assert!(csv.contains("\"Beta \"\"Mobile\"\", Inc.\",BET,,BET-1,1,N,,none,"));
        assert!(csv.contains("\"[\"\"Mia\"\"]\""));
        assert!(csv.ends_with("\r\n"));
    }

    #[test]
    fn an_empty_export_formats_are_empty() {
        assert_eq!(render_csv(&[]), "");
        assert_eq!(render_json(&[]), "[]");
        assert!(render_xlsx(&[]).expect("xlsx").starts_with(b"PK"));
    }

    #[test]
    fn titles_and_reprs_follow_python() {
        assert_eq!(python_title("created by name"), "Created By Name");
        assert_eq!(py_repr(&PyValue::text("héllo")), "'héllo'");
        assert_eq!(
            py_repr(&PyValue::Map(vec![
                ("url".to_string(), PyValue::text("https://x")),
                ("title".to_string(), PyValue::text("it's")),
            ])),
            "{'url': 'https://x', 'title': \"it's\"}"
        );
    }
}
