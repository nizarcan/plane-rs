// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/deletion_task.py`: the soft-delete cascade and the nightly
//! hard delete.
//!
//! # `soft_delete_related_objects` (`deletion_task.py:17-105`)
//!
//! The task is a hand-written cascade, not Django's collector:
//!
//! 1. Load the instance through `all_objects` (an already-soft-deleted row
//!    still resolves; a missing one returns silently).
//! 2. For every **reverse** relation (`one_to_many`/`one_to_one`,
//!    `auto_created`, not concrete), in `_meta.get_fields()` order:
//!    * `DO_NOTHING`: skip — `issue_activities.issue`/`issue_comment` stay
//!      live when an issue or comment is soft-deleted.
//!    * `SET_NULL`: `getattr(instance, accessor).all().update(fk=None)`. The
//!      related manager subclasses the child's **default** manager, so the
//!      default manager's predicate applies (`deleted_at IS NULL`, plus
//!      `IssueManager`'/`StateManager`' extras).
//!    * everything else: resolve the related manager with
//!      `(manager="objects")` — the child's **named** `objects` manager, so
//!      `State.objects`' triage exclusion applies here — and for each row with
//!      `deleted_at IS NULL`, set `deleted_at = now()`, `save()` it (a full
//!      save: audit columns are cleared because a task has no current user,
//!      `auto_now` moves `updated_at`, and the model's own `save()` hook runs)
//!      and **recurse in process** (a direct call, not `.delay()`).
//! 3. Finally soft-delete the instance itself if it is still live.
//!
//! A failure inside one relation is printed and the loop continues
//! (`:97-100`); there is no transaction around the run, so each statement
//! commits on its own.
//!
//! # `hard_delete` (`deletion_task.py:113-193`)
//!
//! Eighteen `Model.all_objects.filter(deleted_at__lt=now - HARD_DELETE_AFTER_DAYS)`
//! steps first (`:138-180`), then the same filter for **every** model with a
//! `deleted_at` in `apps.get_models()` order (`:184-191`). `all_objects` is a
//! plain manager, so each step is a real `DELETE` with Django's collector: it
//! walks every reverse relation, cascades `CASCADE`, nulls `SET_NULL` and
//! leaves `DO_NOTHING` rows alone. One step is one transaction, so an earlier
//! step stays committed when a later one fails.
//!
//! ## The `DO_NOTHING` bug, reproduced
//!
//! `issue_activities.issue_id` and `.issue_comment_id` are `DO_NOTHING`
//! (`plane/db/models/issue.py:416,426`) and the database FK is
//! `DEFERRABLE INITIALLY DEFERRED` (verified on the dev database). Deleting a
//! soft-deleted issue whose project is still live therefore collects the issue
//! and its cascade but not its activity rows; the `DELETE` commits and the
//! deferred check raises `IntegrityError`. `hard_delete` has no `try`, so the
//! task fails, the earlier steps stay committed and steps 6-18 plus the final
//! sweep never run. `plans/CUTOVER.md` §6 says to reproduce this, not fix it;
//! the `difftest task` fixture `hard-delete-orphan.json` records the outcome.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use deadpool_postgres::Client;
use plane_db::JsonText;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use uuid::Uuid;

use crate::TaskContext;
use crate::model_graph::{self, Model};

pub const SOFT_DELETE_RELATED_OBJECTS: &str =
    "plane.bgtasks.deletion_task.soft_delete_related_objects";
pub const HARD_DELETE: &str = "plane.bgtasks.deletion_task.hard_delete";

/// The two task names this module registers.
pub const TASKS: &[&str] = &[SOFT_DELETE_RELATED_OBJECTS, HARD_DELETE];

/// `deletion_task.py:138-180`, in order.
const HARD_DELETE_STEPS: &[&str] = &[
    "db.workspace",
    "db.project",
    "db.cycle",
    "db.module",
    "db.issue",
    "db.page",
    "db.issueview",
    "db.label",
    "db.state",
    "db.issueactivity",
    "db.issuecomment",
    "db.issuelink",
    "db.issuereaction",
    "db.userfavorite",
    "db.moduleissue",
    "db.cycleissue",
    "db.estimate",
    "db.estimatepoint",
];

/// Register the two handlers.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    let soft = Arc::clone(&context);
    registry.register(SOFT_DELETE_RELATED_OBJECTS, move |job| {
        handle_soft(Arc::clone(&soft), job)
    });
    registry.register(HARD_DELETE, move |job| {
        handle_hard(Arc::clone(&context), job)
    });
}

// ---------------------------------------------------------------------------
// soft_delete_related_objects
// ---------------------------------------------------------------------------

async fn handle_soft(_context: Arc<TaskContext>, job: JobContext) -> Result<(), HandlerError> {
    // The Python task has positional parameters, but a call site may bind them
    // by name (`.delay(app_label=..., ...)` binds either way); `difftest task`
    // sends kwargs, so accept both shapes.
    let args = crate::args(&job.payload);
    let kwargs = crate::kwargs(&job.payload);
    let text_arg = |index: usize, name: &str| -> Option<String> {
        args.and_then(|args| args.get(index))
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .or_else(|| {
                kwargs
                    .and_then(|kwargs| kwargs.get(name))
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
    };
    let (Some(app_label), Some(model_name), Some(instance_pk)) = (
        text_arg(0, "app_label"),
        text_arg(1, "model_name"),
        text_arg(2, "instance_pk").and_then(|text| Uuid::parse_str(&text).ok()),
    ) else {
        return Err(HandlerError::fatal(
            "soft_delete_related_objects needs (app_label, model_name, instance_pk)",
        ));
    };
    let label = format!("{}.{}", app_label.to_lowercase(), model_name.to_lowercase());
    if model_graph::models().model(&label).is_none() {
        // `apps.get_model` raises `LookupError` before the `try` loop.
        return Err(HandlerError::fatal(format!(
            "LookupError: no installed app with label {app_label} and model {model_name}"
        )));
    }
    cascade(&job.client, &label, instance_pk)
        .await
        .map_err(|error| HandlerError::fatal(format!("{error:?}")))
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One call of the task: the instance's relations, then the instance itself.
/// The recursion is in-process (`:75-80,91-96`).
fn cascade<'a>(client: &'a Client, label: &'a str, pk: Uuid) -> BoxFuture<'a, Result<(), DbError>> {
    Box::pin(async move {
        let Some(model) = model_graph::models().model(label) else {
            return Err(DbError::UnknownModel(label.to_string()));
        };
        // `model_class.all_objects.get(pk=...)`: the row may be soft-deleted.
        let Some(deleted_at) = load_deleted_at(client, model, pk).await? else {
            // `except model_class.DoesNotExist: return`.
            return Ok(());
        };

        for child in &model.children {
            match child.on_delete.as_str() {
                // `soft_delete_related_objects` skips DO_NOTHING (`:47-48`).
                "DO_NOTHING" => continue,
                "SET_NULL" => {
                    let Some(child_model) = model_graph::models().model(&child.model) else {
                        continue;
                    };
                    let filter = child_model.default_policy().predicate();
                    if let Err(error) =
                        set_null(client, child_model, &child.column, pk, filter).await
                    {
                        tracing::warn!(
                            target: "plane.tasks",
                            relation = %child.accessor,
                            error = %error,
                            "Error handling relation; continuing"
                        );
                    }
                }
                _ => {
                    let Some(child_model) = model_graph::models().model(&child.model) else {
                        continue;
                    };
                    // Errors inside one relation are printed and skipped
                    // (`:97-100`), including errors from the recursion.
                    if let Err(error) = cascade_relation(client, child, child_model, pk).await {
                        tracing::warn!(
                            target: "plane.tasks",
                            relation = %child.accessor,
                            error = %error,
                            "Error handling relation; continuing"
                        );
                    }
                }
            }
        }

        // `if hasattr(instance, "deleted_at") and not instance.deleted_at`.
        if model.has_deleted_at && deleted_at.is_none() {
            save_soft_deleted(client, model, pk).await?;
            // The recursion happens after the save; nothing else changed.
        }
        Ok(())
    })
}

/// The CASCADE branch of the Python loop (`:63-96`).
async fn cascade_relation(
    client: &Client,
    child: &model_graph::Child,
    child_model: &Model,
    pk: Uuid,
) -> Result<(), DbError> {
    // `getattr(instance, related_name)(manager="objects").all()`: the child's
    // named `objects` manager, not its default manager.
    let filter = child_model.objects().predicate();
    let predicate = if filter.is_empty() {
        String::new()
    } else {
        format!(" AND {filter}")
    };
    let columns = if child_model.has_deleted_at {
        "id, deleted_at"
    } else {
        "id, NULL::timestamptz"
    };
    let sql = format!(
        "SELECT {columns} FROM {} WHERE {} = $1{}",
        quote_ident(&child_model.table),
        quote_ident(&child.column),
        predicate
    );
    let rows = client.query(&sql, &[&pk]).await?;
    // A row of a model without `deleted_at` is skipped without recursing
    // (`:70,86`), and a reverse one-to-one yields at most one row.
    if !child_model.has_deleted_at {
        return Ok(());
    }
    let rows = if child.one_to_one {
        rows.into_iter().take(1).collect()
    } else {
        rows
    };
    for row in rows {
        let deleted_at: Option<DateTime<Utc>> = row.get(1);
        if deleted_at.is_some() {
            continue;
        }
        let id: Uuid = row.get(0);
        save_soft_deleted(client, child_model, id).await?;
        cascade(client, &child.model, id).await?;
    }
    Ok(())
}

/// `queryset.update(fk=None)` for the `SET_NULL` branch (`:60-61`).
async fn set_null(
    client: &Client,
    child: &Model,
    column: &str,
    pk: Uuid,
    filter: &str,
) -> Result<(), DbError> {
    let predicate = if filter.is_empty() {
        String::new()
    } else {
        format!(" AND {filter}")
    };
    let sql = format!(
        "UPDATE {} SET {} = NULL WHERE {} = $1{}",
        quote_ident(&child.table),
        quote_ident(column),
        quote_ident(column),
        predicate
    );
    client.execute(&sql, &[&pk]).await?;
    Ok(())
}

async fn load_deleted_at(
    client: &Client,
    model: &Model,
    pk: Uuid,
) -> Result<Option<Option<DateTime<Utc>>>, DbError> {
    let sql = if model.has_deleted_at {
        format!(
            "SELECT deleted_at FROM {} WHERE id = $1",
            quote_ident(&model.table)
        )
    } else {
        format!(
            "SELECT NULL::timestamptz FROM {} WHERE id = $1",
            quote_ident(&model.table)
        )
    };
    let row = client.query_opt(&sql, &[&pk]).await?;
    Ok(row.map(|row| row.get(0)))
}

/// One full `save()` of a row whose `deleted_at` was just set.
///
/// A full save from a worker has no `crum` current user, so `BaseModel.save`
/// clears both audit columns (`plane/db/models/base.py:27-45`); `auto_now`
/// moves `updated_at`; every other column keeps its stored value, except the
/// model `save()` hooks that recompute a value from the row. Those run as a
/// second statement: the row's final state is the same, and none of the hooks
/// touches `updated_at` or the audit columns.
async fn save_soft_deleted(client: &Client, model: &Model, pk: Uuid) -> Result<(), DbError> {
    let table = quote_ident(&model.table);
    let now = Utc::now();
    client
        .execute(
            &format!(
                "UPDATE {table} SET deleted_at = $1, updated_at = $1, \
                 created_by_id = NULL, updated_by_id = NULL WHERE id = $2"
            ),
            &[&now, &pk],
        )
        .await?;

    match model.label.as_str() {
        "db.project" => {
            let row = client
                .query_one(
                    &format!("SELECT identifier FROM {table} WHERE id = $1"),
                    &[&pk],
                )
                .await?;
            let identifier: String = row.get(0);
            // `self.identifier = self.identifier.strip().upper()` (`:195`).
            let identifier = identifier.trim().to_uppercase();
            client
                .execute(
                    &format!("UPDATE {table} SET identifier = $1 WHERE id = $2"),
                    &[&identifier, &pk],
                )
                .await?;
        }
        "db.state" => {
            let row = client
                .query_one(&format!("SELECT name FROM {table} WHERE id = $1"), &[&pk])
                .await?;
            let name: String = row.get(0);
            // `self.slug = slugify(self.name)` runs on every save
            // (`plane/db/models/state.py:117-119`).
            let slug = plane_db::hooks::state::slugify(&name);
            client
                .execute(
                    &format!("UPDATE {table} SET slug = $1 WHERE id = $2"),
                    &[&slug, &pk],
                )
                .await?;
        }
        "db.issue"
        | "db.page"
        | "db.pageversion"
        | "db.description"
        | "db.descriptionversion"
        | "db.sticky" => {
            let row = client
                .query_one(
                    &format!("SELECT description_html FROM {table} WHERE id = $1"),
                    &[&pk],
                )
                .await?;
            let html: Option<String> = row.get(0);
            let stripped = strip(html.as_deref());
            client
                .execute(
                    &format!("UPDATE {table} SET description_stripped = $1 WHERE id = $2"),
                    &[&stripped, &pk],
                )
                .await?;
        }
        "db.issuecomment" => {
            let row = client
                .query_one(
                    &format!("SELECT comment_html FROM {table} WHERE id = $1"),
                    &[&pk],
                )
                .await?;
            let html: String = row.get(0);
            // `strip_tags(comment_html) if comment_html != "" else ""`
            // (`plane/db/models/issue.py:487`).
            let stripped = if html.is_empty() {
                String::new()
            } else {
                plane_db::hooks::html::strip_tags(&html)
            };
            client
                .execute(
                    &format!("UPDATE {table} SET comment_stripped = $1 WHERE id = $2"),
                    &[&stripped, &pk],
                )
                .await?;
        }
        "db.issueview" => {
            let row = client
                .query_one(
                    &format!("SELECT filters FROM {table} WHERE id = $1"),
                    &[&pk],
                )
                .await?;
            let filters: JsonText = row.get(0);
            // `self.query = issue_filters(self.filters, "POST") if self.filters else {}`
            // (`plane/db/models/view.py:80-81`).
            let query = issue_view_query(&filters)?;
            client
                .execute(
                    &format!("UPDATE {table} SET query = $1 WHERE id = $2"),
                    &[&query, &pk],
                )
                .await?;
        }
        "db.draftissue" => {
            let row = client
                .query_one(
                    &format!(
                        "SELECT description_html, state_id, \
                                (SELECT \"group\" FROM states WHERE states.id = draft_issues.state_id) \
                         FROM {table} WHERE id = $1"
                    ),
                    &[&pk],
                )
                .await?;
            let html: Option<String> = row.get(0);
            let state_id: Option<Uuid> = row.get(1);
            let state_group: Option<String> = row.get(2);
            let stripped = strip(html.as_deref());
            client
                .execute(
                    &format!("UPDATE {table} SET description_stripped = $1 WHERE id = $2"),
                    &[&stripped, &pk],
                )
                .await?;
            // `DraftIssue.save` (`plane/db/models/draft.py:84-129`): a null
            // state is replaced by the project's default non-triage state, and
            // a non-null state recomputes `completed_at`.
            match state_id {
                None => {
                    let default_state: Option<Uuid> = client
                        .query_opt(
                            "SELECT id FROM states WHERE project_id = \
                                     (SELECT project_id FROM draft_issues WHERE id = $1) \
                               AND NOT is_triage AND deleted_at IS NULL \
                             ORDER BY \"default\" DESC, sequence ASC LIMIT 1",
                            &[&pk],
                        )
                        .await?
                        .map(|row| row.get(0));
                    client
                        .execute(
                            &format!("UPDATE {table} SET state_id = $1 WHERE id = $2"),
                            &[&default_state, &pk],
                        )
                        .await?;
                }
                Some(_) => {
                    if state_group.as_deref() == Some("completed") {
                        client
                            .execute(
                                &format!("UPDATE {table} SET completed_at = $1 WHERE id = $2"),
                                &[&now, &pk],
                            )
                            .await?;
                    } else {
                        client
                            .execute(
                                &format!("UPDATE {table} SET completed_at = NULL WHERE id = $1"),
                                &[&pk],
                            )
                            .await?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// `html == "" or html is None ? NULL : strip_tags(html)`.
fn strip(html: Option<&str>) -> Option<String> {
    match html {
        None | Some("") => None,
        Some(html) => Some(plane_db::hooks::html::strip_tags(html)),
    }
}

/// `IssueView.save`'s `query` (`plane/db/models/view.py:80-81`).
fn issue_view_query(filters: &JsonText) -> Result<JsonText, DbError> {
    let value = plane_serde::pyjson::loads(filters.as_str().as_bytes(), "utf-8")
        .map_err(|error| DbError::Json(error.to_string()))?;
    if !plane_db::hooks::issue_view::truthy(&value) {
        return Ok(JsonText::new("{}"));
    }
    plane_db::hooks::issue_view::query_from_filters(&value, Utc::now().date_naive())
        .map_err(|error| DbError::Json(error.to_string()))
}

// ---------------------------------------------------------------------------
// hard_delete
// ---------------------------------------------------------------------------

async fn handle_hard(context: Arc<TaskContext>, job: JobContext) -> Result<(), HandlerError> {
    let days = context.settings.limits.hard_delete_after_days;
    for label in HARD_DELETE_STEPS {
        // Each line evaluates `timezone.now()` separately (`:138-180`).
        let cutoff = Utc::now() - Duration::days(days);
        delete_step(&job.client, label, cutoff)
            .await
            .map_err(|error| HandlerError::fatal(format!("hard_delete {label}: {error:?}")))?;
    }
    // `for model in apps.get_models(): if hasattr(model, "deleted_at")`
    // (`:184-191`).
    for label in model_graph::models().registry_order() {
        let Some(model) = model_graph::models().model(label) else {
            continue;
        };
        if !model.has_deleted_at {
            continue;
        }
        let cutoff = Utc::now() - Duration::days(days);
        delete_step(&job.client, &model.label, cutoff)
            .await
            .map_err(|error| HandlerError::fatal(format!("hard_delete {label}: {error:?}")))?;
    }
    Ok(())
}

/// One `Model.all_objects.filter(deleted_at__lt=cutoff).delete()`.
///
/// The collector runs in one transaction, like `Collector.delete`'s
/// `transaction.atomic(savepoint=False)`; the deferred FK checks fire at
/// commit, which is where the upstream `DO_NOTHING` bug surfaces.
async fn delete_step(client: &Client, label: &str, cutoff: DateTime<Utc>) -> Result<(), DbError> {
    let model = model_graph::models()
        .model(label)
        .ok_or_else(|| DbError::UnknownModel(label.to_string()))?;
    // `all_objects` is a plain manager, so only the explicit predicate applies.
    let rows = client
        .query(
            &format!(
                "SELECT id FROM {} WHERE deleted_at < $1",
                quote_ident(&model.table)
            ),
            &[&cutoff],
        )
        .await?;
    let roots: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();
    if roots.is_empty() {
        return Ok(());
    }

    // `Collector.delete` opens `transaction.atomic(savepoint=False)`; the
    // manual block is the same BEGIN/COMMIT around the collected statements.
    // The deferred FK checks fire at COMMIT, which is where the upstream
    // `DO_NOTHING` bug surfaces.
    client.batch_execute("BEGIN").await?;
    let result = async {
        let plan = collect(client, label, &roots).await?;
        apply_plan(client, plan).await
    }
    .await;
    match result {
        Ok(()) => {
            client.batch_execute("COMMIT").await?;
            Ok(())
        }
        Err(error) => {
            let _ = client.batch_execute("ROLLBACK").await;
            Err(error)
        }
    }
}

/// Django's `Collector.collect`: walk every reverse relation from the roots.
struct Plan {
    /// Rows to delete per table, in collection order.
    deletes: Vec<(String, Vec<Uuid>)>,
    /// `SET_NULL` column updates: `(child table, column, parent ids)`.
    nulls: Vec<(String, String, Vec<Uuid>)>,
}

async fn collect(tx: &Client, label: &str, roots: &[Uuid]) -> Result<Plan, DbError> {
    let mut collected: BTreeMap<String, Vec<Uuid>> = BTreeMap::new();
    let mut seen: BTreeMap<String, BTreeSet<Uuid>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut pending: Vec<(String, Vec<Uuid>)> = vec![(label.to_string(), roots.to_vec())];
    let mut nulls: BTreeMap<(String, String), BTreeSet<Uuid>> = BTreeMap::new();

    while let Some((label, ids)) = pending.pop() {
        let Some(model) = model_graph::models().model(&label) else {
            continue;
        };
        let fresh: Vec<Uuid> = ids
            .into_iter()
            .filter(|id| seen.entry(label.clone()).or_default().insert(*id))
            .collect();
        if fresh.is_empty() {
            continue;
        }
        if !collected.contains_key(&label) {
            order.push(label.clone());
            collected.insert(label.clone(), Vec::new());
        }
        collected
            .get_mut(&label)
            .expect("inserted above")
            .extend(fresh.iter().copied());

        for child in &model.children {
            match child.on_delete.as_str() {
                "DO_NOTHING" => continue,
                "SET_NULL" => {
                    nulls
                        .entry((child.model.clone(), child.column.clone()))
                        .or_default()
                        .extend(fresh.iter().copied());
                }
                _ => {
                    // `Collector.related_objects` uses `_base_manager`, which
                    // is unfiltered for every Plane model.
                    let Some(child_model) = model_graph::models().model(&child.model) else {
                        continue;
                    };
                    let sql = format!(
                        "SELECT id FROM {} WHERE {} = ANY($1)",
                        quote_ident(&child_model.table),
                        quote_ident(&child.column)
                    );
                    let rows = tx.query(&sql, &[&fresh]).await?;
                    let ids: Vec<Uuid> = rows.iter().map(|row| row.get(0)).collect();
                    if !ids.is_empty() {
                        pending.push((child.model.clone(), ids));
                    }
                }
            }
        }
    }

    let deletes = order
        .into_iter()
        .filter_map(|label| {
            let model = model_graph::models().model(&label)?;
            let ids = collected.remove(&label)?;
            Some((model.table.clone(), ids))
        })
        .collect();
    let nulls = nulls
        .into_iter()
        .filter_map(|((label, column), ids)| {
            let model = model_graph::models().model(&label)?;
            Some((model.table.clone(), column, ids.into_iter().collect()))
        })
        .collect();
    Ok(Plan { deletes, nulls })
}

async fn apply_plan(tx: &Client, plan: Plan) -> Result<(), DbError> {
    // Django applies `SET_NULL` updates before the deletes (the `field_updates`
    // loop in `django/db/models/deletion.py::Collector.delete`).
    for (table, column, ids) in plan.nulls {
        for batch in ids.chunks(1000) {
            let sql = format!(
                "UPDATE {} SET {} = NULL WHERE {} = ANY($1)",
                quote_ident(&table),
                quote_ident(&column),
                quote_ident(&column)
            );
            tx.execute(&sql, &[&batch.to_vec()]).await?;
        }
    }
    for (table, ids) in plan.deletes {
        for batch in ids.chunks(1000) {
            let sql = format!("DELETE FROM {} WHERE id = ANY($1)", quote_ident(&table));
            tx.execute(&sql, &[&batch.to_vec()]).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Quote a table or column from the embedded graph. The value is a constant,
/// never request data, but `"group"` alone shows why quoting is not optional.
fn quote_ident(name: &str) -> String {
    let mut quoted = String::with_capacity(name.len() + 2);
    quoted.push('"');
    for character in name.chars() {
        if character == '"' {
            quoted.push('"');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

#[derive(Debug, thiserror::Error)]
enum DbError {
    #[error(transparent)]
    Database(#[from] tokio_postgres::Error),
    #[error("unknown model {0}")]
    UnknownModel(String),
    #[error("json: {0}")]
    Json(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hard_delete_steps_are_the_python_order() {
        // The Python task lives in the oracle (`apps/api` at the UPSTREAM.toml
        // pin), not in this checkout; `plane_oracle` locates its cache.
        let source = std::fs::read_to_string(
            plane_oracle::require_api().join("plane/bgtasks/deletion_task.py"),
        )
        .expect("deletion_task.py is in the oracle");
        let mut previous = 0;
        for label in HARD_DELETE_STEPS {
            let class = match *label {
                "db.workspace" => "Workspace.all_objects",
                "db.project" => "Project.all_objects",
                "db.cycle" => "Cycle.all_objects",
                "db.module" => "Module.all_objects",
                "db.issue" => "Issue.all_objects",
                "db.page" => "Page.all_objects",
                "db.issueview" => "IssueView.all_objects",
                "db.label" => "Label.all_objects",
                "db.state" => "State.all_objects",
                "db.issueactivity" => "IssueActivity.all_objects",
                "db.issuecomment" => "IssueComment.all_objects",
                "db.issuelink" => "IssueLink.all_objects",
                "db.issuereaction" => "IssueReaction.all_objects",
                "db.userfavorite" => "UserFavorite.all_objects",
                "db.moduleissue" => "ModuleIssue.all_objects",
                "db.cycleissue" => "CycleIssue.all_objects",
                "db.estimate" => "Estimate.all_objects",
                "db.estimatepoint" => "EstimatePoint.all_objects",
                other => panic!("unmapped step {other}"),
            };
            let at = source
                .find(class)
                .unwrap_or_else(|| panic!("{class} is missing from deletion_task.py"));
            assert!(
                at >= previous,
                "step {label} is out of order in deletion_task.py"
            );
            previous = at;
        }
    }

    #[test]
    fn every_hard_delete_step_names_a_known_model() {
        for label in HARD_DELETE_STEPS {
            let model = model_graph::models().model(label).expect(label);
            assert!(model.has_deleted_at, "{label} has no deleted_at");
        }
    }

    #[test]
    fn the_issue_activity_foreign_keys_are_do_nothing() {
        let activity = model_graph::models()
            .model("db.issueactivity")
            .expect("db.issueactivity");
        for column in ["issue_id", "issue_comment_id"] {
            let parent = activity
                .parents
                .iter()
                .find(|parent| parent.column == column)
                .unwrap_or_else(|| panic!("issue_activities.{column}"));
            assert_eq!(parent.on_delete, "DO_NOTHING");
        }
    }

    #[test]
    fn stripping_matches_the_python_condition() {
        assert_eq!(strip(None), None);
        assert_eq!(strip(Some("")), None);
        assert_eq!(strip(Some("<p>hi</p>")), Some("hi".to_string()));
    }

    #[test]
    fn quoting_escapes_double_quotes() {
        assert_eq!(quote_ident("group"), "\"group\"");
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    }
}
