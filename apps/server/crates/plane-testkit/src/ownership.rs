// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Tasks the Rust backend no longer publishes to RabbitMQ.
//!
//! During the Phase 8 cutover a task moves one at a time
//! (`apps/server/plans/CUTOVER.md` §2): its producer inserts a `plane_jobs` row
//! instead of a Celery message. A differential scenario therefore sees one
//! Celery delivery on target A (Django) and one `plane_jobs` row on target B
//! (Rust) for every switched task. The transport is not an effect, but the
//! `(task, args, kwargs)` triple is: this module turns the two representations
//! into one [`TransportJob`] and compares them (`DECISIONS.md` D25).
//!
//! The list mirrors `plane-jobs/src/ownership.rs::RUST_TASKS`. The harness is a
//! separate Cargo workspace and cannot depend on `plane-jobs` (its features
//! would leak into the server build, `apps/server/Cargo.toml`), so the names
//! are duplicated. `plane-jobs`' `policy::every_rust_task_has_only_rust_producers`
//! fails when a task moves without its producer table, and `difftest task` is
//! the handler comparison; this list keeps the HTTP sweeps truthful for the
//! same moves.
//!
//! `plane.jobs.echo` is test-only and never published by an HTTP scenario.

use crate::celery::CeleryMessage;
use crate::compare::Difference;
use crate::db::DbDiff;

/// Registered task names whose producer is Rust-owned.
pub const RUST_OWNED_TASKS: &[&str] = &[
    "plane.jobs.echo",
    "plane.bgtasks.analytic_plot_export.analytic_export_task",
    "plane.bgtasks.cleanup_task.delete_api_logs",
    "plane.bgtasks.cleanup_task.delete_email_notification_logs",
    "plane.bgtasks.cleanup_task.delete_issue_description_versions",
    "plane.bgtasks.cleanup_task.delete_page_versions",
    "plane.bgtasks.cleanup_task.delete_webhook_logs",
    "plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets",
    "plane.bgtasks.deletion_task.hard_delete",
    "plane.bgtasks.export_task.issue_export_task",
    "plane.bgtasks.issue_automation_task.archive_and_close_old_issues",
    "plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues",
    "plane.bgtasks.logger_task.process_logs",
    "plane.bgtasks.issue_description_version_task.issue_description_version_task",
    "plane.bgtasks.page_transaction_task.page_transaction",
    "plane.bgtasks.page_version_task.track_page_version",
    "plane.bgtasks.deletion_task.soft_delete_related_objects",
    "plane.bgtasks.recent_visited_task.recent_visited_task",
    "plane.bgtasks.storage_metadata_task.get_asset_object_metadata",
    "plane.bgtasks.work_item_link_task.crawl_work_item_link_title",
    "plane.bgtasks.workspace_seed_task.workspace_seed",
    "plane.bgtasks.issue_activities_task.issue_activity",
    "plane.bgtasks.notification_task.notifications",
];

/// Whether `task` is published to `plane_jobs` rather than to RabbitMQ.
#[must_use]
pub fn rust_owned(task: &str) -> bool {
    RUST_OWNED_TASKS.contains(&task)
}

/// One Rust-owned task delivery in the form both transports share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportJob {
    pub task: String,
    pub args: serde_json::Value,
    pub kwargs: serde_json::Value,
}

/// Remove the Rust-owned deliveries from A's Celery drain and decode them into
/// [`TransportJob`]s.
///
/// `Job::from_message` (`plane-jobs/src/queue.rs`) stores Python runtime values,
/// so a `uuid.UUID` argument loses kombu's JSON envelope; [`python_value`] does
/// the same decoding here before the normaliser runs.
#[must_use]
pub fn take_owned_messages(messages: &mut Vec<CeleryMessage>) -> Vec<TransportJob> {
    let mut owned = Vec::new();
    messages.retain(|message| {
        let Some(task) = message.task.as_deref() else {
            return true;
        };
        if !rust_owned(task) {
            return true;
        }
        owned.push(TransportJob {
            task: task.to_string(),
            args: python_value(&message.args),
            kwargs: python_value(&message.kwargs),
        });
        false
    });
    owned
}

/// Take the queue tables out of a step's database diff and return the jobs B
/// inserted.
///
/// `plane_jobs` is the transport, not an effect: it never appears in the
/// generic table comparison, and its rows are compared in their decoded form by
/// [`compare_transports`].
#[must_use]
pub fn take_plane_jobs(diff: &mut DbDiff) -> Vec<TransportJob> {
    let mut jobs = Vec::new();
    if let Some(table) = diff.tables.remove("plane_jobs.jobs") {
        for row in table.inserted {
            let Some(task) = row
                .get("task")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            let payload = row
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            jobs.push(TransportJob {
                task,
                args: payload
                    .get("args")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                kwargs: payload
                    .get("kwargs")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            });
        }
    }
    diff.tables
        .retain(|table, _| !table.starts_with("plane_jobs."));
    jobs
}

/// Compare the Rust-owned deliveries of the two transports.
///
/// `a` is what Django's producer published to RabbitMQ, `b` is what B's
/// producer inserted into `plane_jobs.jobs`. Each side must deliver the same
/// multiset of `(task, canonical args, canonical kwargs)`; object key order is
/// not part of the payload (`jsonb` reorders it on B, Python keeps insertion
/// order on A), so objects are canonicalised by key.
#[must_use]
pub fn compare_transports(a: &[TransportJob], b: &[TransportJob]) -> Vec<Difference> {
    let mut unused: Vec<Option<&TransportJob>> = b.iter().map(Some).collect();
    let mut missing = Vec::new();
    for job in a {
        let key = transport_key(job);
        match unused
            .iter_mut()
            .find(|candidate| candidate.is_some_and(|candidate| transport_key(candidate) == key))
        {
            Some(slot) => *slot = None,
            None => missing.push(job),
        }
    }
    let mut differences = Vec::new();
    for job in missing {
        differences.push(
            Difference::new(
                "celery",
                format!("task:{}", job.task),
                "A published the task but B enqueued no matching plane_jobs row",
            )
            .with_values(pretty(job), String::new()),
        );
    }
    for job in unused.into_iter().flatten() {
        differences.push(
            Difference::new(
                "celery",
                format!("task:{}", job.task),
                "B enqueued a plane_jobs row but A published no matching task",
            )
            .with_values(String::new(), pretty(job)),
        );
    }
    differences
}

fn transport_key(job: &TransportJob) -> String {
    format!(
        "{}\u{0}{}\u{0}{}",
        job.task,
        canonical(&job.args),
        canonical(&job.kwargs)
    )
}

fn canonical(value: &serde_json::Value) -> String {
    serde_json::to_string(&sorted(value)).unwrap_or_default()
}

fn sorted(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(sorted).collect())
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = serde_json::Map::new();
            for key in keys {
                if let Some(value) = map.get(key) {
                    out.insert(key.clone(), sorted(value));
                }
            }
            serde_json::Value::Object(out)
        }
        other => other.clone(),
    }
}

fn pretty(job: &TransportJob) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "args": job.args,
        "kwargs": job.kwargs,
    }))
    .unwrap_or_default()
}

/// Decode kombu's JSON type envelope: `uuid.UUID` is
/// `{"__type__": "uuid", "__value__": {"hex": "…"}}`; its Python value is the
/// plain hyphenated string, which is what `Job::from_message` stores.
#[must_use]
pub fn python_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(python_value).collect())
        }
        serde_json::Value::Object(map) => {
            if map.get("__type__").and_then(serde_json::Value::as_str) == Some("uuid")
                && let Some(hex) = map
                    .get("__value__")
                    .and_then(|value| value.get("hex"))
                    .and_then(serde_json::Value::as_str)
                && let Ok(id) = uuid::Uuid::parse_str(hex)
            {
                return serde_json::Value::String(id.to_string());
            }
            serde_json::Value::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), python_value(value)))
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn empty_message(task: &str) -> CeleryMessage {
        CeleryMessage {
            queue: "celery".to_string(),
            task: Some(task.to_string()),
            headers: BTreeMap::new(),
            properties: BTreeMap::new(),
            args: serde_json::Value::Array(Vec::new()),
            kwargs: serde_json::Value::Object(serde_json::Map::new()),
            body_extra: None,
        }
    }

    fn message(task: &str, args: serde_json::Value, kwargs: serde_json::Value) -> CeleryMessage {
        let mut message = empty_message(task);
        message.args = args;
        message.kwargs = kwargs;
        message
    }

    #[test]
    fn a_uuid_envelope_decodes_to_its_python_string() {
        let id = "11111111-1111-1111-1111-111111111111";
        assert_eq!(
            python_value(&serde_json::json!({
                "entity": {"__type__": "uuid", "__value__": {"hex": id.replace('-', "")}},
                "nested": [{"__type__": "uuid", "__value__": {"hex": id.replace('-', "")}}],
            })),
            serde_json::json!({"entity": id, "nested": [id]})
        );
    }

    #[test]
    fn a_rust_owned_delivery_matches_across_the_two_transports() {
        let id = "11111111-1111-1111-1111-111111111111";
        let mut messages = vec![
            message(
                "plane.bgtasks.recent_visited_task.recent_visited_task",
                serde_json::json!([]),
                serde_json::json!({
                    "slug": "acme",
                    "user_id": {"__type__": "uuid", "__value__": {"hex": id.replace('-', "")}},
                }),
            ),
            empty_message("plane.bgtasks.webhook_task.webhook_send_task"),
        ];
        let owned = take_owned_messages(&mut messages);
        assert_eq!(owned.len(), 1);
        assert_eq!(
            messages.len(),
            1,
            "Celery-owned messages stay for the drain comparison"
        );
        let jobs = vec![TransportJob {
            task: "plane.bgtasks.recent_visited_task.recent_visited_task".to_string(),
            args: serde_json::json!([]),
            // `jsonb` reorders keys; equality is by key, not by order.
            kwargs: serde_json::json!({"user_id": id, "slug": "acme"}),
        }];
        assert!(compare_transports(&owned, &jobs).is_empty());
    }

    #[test]
    fn a_payload_difference_is_reported() {
        let owned = vec![TransportJob {
            task: "plane.bgtasks.recent_visited_task.recent_visited_task".to_string(),
            args: serde_json::json!([]),
            kwargs: serde_json::json!({"slug": "acme"}),
        }];
        let jobs = vec![TransportJob {
            task: "plane.bgtasks.recent_visited_task.recent_visited_task".to_string(),
            args: serde_json::json!([]),
            kwargs: serde_json::json!({"slug": "other"}),
        }];
        let differences = compare_transports(&owned, &jobs);
        assert_eq!(differences.len(), 2, "one missing on B, one extra on B");
    }

    #[test]
    fn plane_jobs_leaves_the_generic_database_diff() {
        let mut diff = DbDiff::default();
        let mut table = crate::db::TableDiff::default();
        table.inserted.push(serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "task": "plane.bgtasks.logger_task.process_logs",
            "payload": {"args": [], "kwargs": {"log_data": {"path": "/"}}},
        }));
        diff.tables.insert("plane_jobs.jobs".to_string(), table);
        let mut ticks = crate::db::TableDiff::default();
        ticks
            .inserted
            .push(serde_json::json!({"schedule_name": "x"}));
        diff.tables.insert("plane_jobs.ticks".to_string(), ticks);
        let jobs = take_plane_jobs(&mut diff);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].task, "plane.bgtasks.logger_task.process_logs");
        assert_eq!(
            jobs[0].kwargs,
            serde_json::json!({"log_data": {"path": "/"}})
        );
        assert!(diff.tables.is_empty());
    }
}
