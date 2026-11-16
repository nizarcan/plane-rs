// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The Celery protocol-2 message, without a transport.
//!
//! Every field is reproduced from Celery 5.5.3 / kombu 5.5.4 as observed in
//! `apps/server/tools/inventory/celery_messages.json` and documented in
//! `apps/server/TASKS.md` §2:
//!
//! * body `[args, kwargs, embed]`, serialised through
//!   `plane_serde::Dialect::Kombu` (`", "`/`": "`, `ensure_ascii`,
//!   `{"__type__": …}` envelopes);
//! * headers `lang`, `task`, `id`, `shadow`, `eta`, `expires`, `group`,
//!   `group_index`, `retries`, `timelimit`, `root_id`, `parent_id`,
//!   `argsrepr`, `kwargsrepr`, `origin`, `ignore_result`,
//!   `replaced_task_nesting`, `stamped_headers`, `stamps`;
//! * properties `content_type`, `content_encoding`, `delivery_mode`,
//!   `priority`, `correlation_id`, `reply_to`; published to exchange `""` with
//!   routing key `celery`.
//!
//! `countdown=` is not a body field: Celery turns it into the `eta` header at
//! publish time (`celery/app/amqp.py:300-306`), which is why [`Message`] keeps
//! it and [`Message::encode`] resolves it against the publish instant.

use chrono::{DateTime, SecondsFormat, Timelike, Utc};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use uuid::Uuid;

use crate::celery::args::{EncodeError, TaskArg, argsrepr, kwargsrepr};

/// kombu's default queue, exchange type and routing key
/// (`TASKS.md` §2.6).
pub const DEFAULT_QUEUE: &str = "celery";
/// The exchange Celery declares for the default queue. Messages are published
/// to the nameless default exchange, not to this one.
pub const DEFAULT_EXCHANGE: &str = "celery";
/// The routing key on the nameless default exchange.
pub const DEFAULT_ROUTING_KEY: &str = "celery";

/// Registered Celery task names as the Python worker knows them
/// (`TASKS.md` §3).
pub mod task_name {
    /// `plane.bgtasks.analytic_plot_export.analytic_export_task`
    pub const ANALYTIC_EXPORT_TASK: &str =
        "plane.bgtasks.analytic_plot_export.analytic_export_task";
    /// `plane.bgtasks.cleanup_task.delete_api_logs`
    pub const DELETE_API_LOGS: &str = "plane.bgtasks.cleanup_task.delete_api_logs";
    /// `plane.bgtasks.cleanup_task.delete_email_notification_logs`
    pub const DELETE_EMAIL_NOTIFICATION_LOGS: &str =
        "plane.bgtasks.cleanup_task.delete_email_notification_logs";
    /// `plane.bgtasks.cleanup_task.delete_issue_description_versions`
    pub const DELETE_ISSUE_DESCRIPTION_VERSIONS: &str =
        "plane.bgtasks.cleanup_task.delete_issue_description_versions";
    /// `plane.bgtasks.cleanup_task.delete_page_versions`
    pub const DELETE_PAGE_VERSIONS: &str = "plane.bgtasks.cleanup_task.delete_page_versions";
    /// `plane.bgtasks.cleanup_task.delete_webhook_logs`
    pub const DELETE_WEBHOOK_LOGS: &str = "plane.bgtasks.cleanup_task.delete_webhook_logs";
    /// `plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets`
    pub const COPY_S3_OBJECTS: &str =
        "plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets";
    /// `plane.bgtasks.deletion_task.soft_delete_related_objects`
    pub const SOFT_DELETE_RELATED_OBJECTS: &str =
        "plane.bgtasks.deletion_task.soft_delete_related_objects";
    /// `plane.bgtasks.deletion_task.hard_delete`
    pub const HARD_DELETE: &str = "plane.bgtasks.deletion_task.hard_delete";
    /// `plane.bgtasks.email_notification_task.stack_email_notification`
    pub const STACK_EMAIL_NOTIFICATION: &str =
        "plane.bgtasks.email_notification_task.stack_email_notification";
    /// `plane.bgtasks.email_notification_task.send_email_notification`
    pub const SEND_EMAIL_NOTIFICATION: &str =
        "plane.bgtasks.email_notification_task.send_email_notification";
    /// `plane.bgtasks.export_task.issue_export_task`
    pub const ISSUE_EXPORT_TASK: &str = "plane.bgtasks.export_task.issue_export_task";
    /// `plane.bgtasks.exporter_expired_task.delete_old_s3_link`
    pub const DELETE_OLD_S3_LINK: &str = "plane.bgtasks.exporter_expired_task.delete_old_s3_link";
    /// `plane.bgtasks.file_asset_task.delete_unuploaded_file_asset`
    pub const DELETE_UNUPLOADED_FILE_ASSET: &str =
        "plane.bgtasks.file_asset_task.delete_unuploaded_file_asset";
    /// `plane.bgtasks.forgot_password_task.forgot_password`
    pub const FORGOT_PASSWORD: &str = "plane.bgtasks.forgot_password_task.forgot_password";
    /// `plane.bgtasks.issue_activities_task.issue_activity`
    pub const ISSUE_ACTIVITY: &str = "plane.bgtasks.issue_activities_task.issue_activity";
    /// `plane.bgtasks.issue_automation_task.archive_and_close_old_issues`
    pub const ARCHIVE_AND_CLOSE_OLD_ISSUES: &str =
        "plane.bgtasks.issue_automation_task.archive_and_close_old_issues";
    /// `plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues`
    pub const UNSNOOZE_EXPIRED_INTAKE_ISSUES: &str =
        "plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues";
    /// `plane.bgtasks.issue_description_version_sync.schedule_issue_description_version`
    pub const SCHEDULE_ISSUE_DESCRIPTION_VERSION: &str =
        "plane.bgtasks.issue_description_version_sync.schedule_issue_description_version";
    /// `plane.bgtasks.issue_description_version_sync.sync_issue_description_version`
    pub const SYNC_ISSUE_DESCRIPTION_VERSION: &str =
        "plane.bgtasks.issue_description_version_sync.sync_issue_description_version";
    /// `plane.bgtasks.issue_description_version_task.issue_description_version_task`
    pub const ISSUE_DESCRIPTION_VERSION_TASK: &str =
        "plane.bgtasks.issue_description_version_task.issue_description_version_task";
    /// `plane.bgtasks.issue_version_sync.schedule_issue_version`
    pub const SCHEDULE_ISSUE_VERSION: &str =
        "plane.bgtasks.issue_version_sync.schedule_issue_version";
    /// `plane.bgtasks.issue_version_sync.sync_issue_version`
    pub const SYNC_ISSUE_VERSION: &str = "plane.bgtasks.issue_version_sync.sync_issue_version";
    /// `plane.bgtasks.logger_task.process_logs`
    pub const PROCESS_LOGS: &str = "plane.bgtasks.logger_task.process_logs";
    /// `plane.bgtasks.magic_link_code_task.magic_link`
    pub const MAGIC_LINK: &str = "plane.bgtasks.magic_link_code_task.magic_link";
    /// `plane.bgtasks.notification_task.notifications`
    pub const NOTIFICATIONS: &str = "plane.bgtasks.notification_task.notifications";
    /// `plane.bgtasks.page_transaction_task.page_transaction`
    pub const PAGE_TRANSACTION: &str = "plane.bgtasks.page_transaction_task.page_transaction";
    /// `plane.bgtasks.page_version_task.track_page_version`
    pub const TRACK_PAGE_VERSION: &str = "plane.bgtasks.page_version_task.track_page_version";
    /// `plane.bgtasks.project_add_user_email_task.project_add_user_email`
    pub const PROJECT_ADD_USER_EMAIL: &str =
        "plane.bgtasks.project_add_user_email_task.project_add_user_email";
    /// `plane.bgtasks.recent_visited_task.recent_visited_task`
    pub const RECENT_VISITED_TASK: &str = "plane.bgtasks.recent_visited_task.recent_visited_task";
    /// `plane.bgtasks.storage_metadata_task.get_asset_object_metadata`
    pub const GET_ASSET_OBJECT_METADATA: &str =
        "plane.bgtasks.storage_metadata_task.get_asset_object_metadata";
    /// `plane.bgtasks.user_activation_email_task.user_activation_email`
    pub const USER_ACTIVATION_EMAIL: &str =
        "plane.bgtasks.user_activation_email_task.user_activation_email";
    /// `plane.bgtasks.user_deactivation_email_task.user_deactivation_email`
    pub const USER_DEACTIVATION_EMAIL: &str =
        "plane.bgtasks.user_deactivation_email_task.user_deactivation_email";
    /// `plane.bgtasks.user_email_update_task.send_email_update_confirmation`
    pub const SEND_EMAIL_UPDATE_CONFIRMATION: &str =
        "plane.bgtasks.user_email_update_task.send_email_update_confirmation";
    /// `plane.bgtasks.user_email_update_task.send_email_update_magic_code`
    pub const SEND_EMAIL_UPDATE_MAGIC_CODE: &str =
        "plane.bgtasks.user_email_update_task.send_email_update_magic_code";
    /// `plane.bgtasks.webhook_task.model_activity`
    pub const MODEL_ACTIVITY: &str = "plane.bgtasks.webhook_task.model_activity";
    /// `plane.bgtasks.webhook_task.send_webhook_deactivation_email`
    pub const SEND_WEBHOOK_DEACTIVATION_EMAIL: &str =
        "plane.bgtasks.webhook_task.send_webhook_deactivation_email";
    /// `plane.bgtasks.webhook_task.webhook_activity`
    pub const WEBHOOK_ACTIVITY: &str = "plane.bgtasks.webhook_task.webhook_activity";
    /// `plane.bgtasks.webhook_task.webhook_send_task`
    pub const WEBHOOK_SEND_TASK: &str = "plane.bgtasks.webhook_task.webhook_send_task";
    /// `plane.bgtasks.work_item_link_task.crawl_work_item_link_title`
    pub const CRAWL_WORK_ITEM_LINK_TITLE: &str =
        "plane.bgtasks.work_item_link_task.crawl_work_item_link_title";
    /// `plane.bgtasks.workspace_invitation_task.workspace_invitation`
    pub const WORKSPACE_INVITATION: &str =
        "plane.bgtasks.workspace_invitation_task.workspace_invitation";
    /// `plane.bgtasks.workspace_seed_task.workspace_seed`
    pub const WORKSPACE_SEED: &str = "plane.bgtasks.workspace_seed_task.workspace_seed";
    /// `plane.license.bgtasks.telemetry_metrics.push_instance_metrics`
    pub const PUSH_INSTANCE_METRICS: &str =
        "plane.license.bgtasks.telemetry_metrics.push_instance_metrics";
}

/// A task message that has not been given an id or a publish instant yet.
#[derive(Clone, Debug)]
pub struct Message {
    task: &'static str,
    args: Vec<TaskArg>,
    kwargs: Vec<(&'static str, TaskArg)>,
    countdown: Option<i64>,
}

impl Message {
    /// Assemble a message. Typed task functions in
    /// [`crate::celery::tasks`] are the intended callers.
    #[must_use]
    pub fn new(
        task: &'static str,
        args: Vec<TaskArg>,
        kwargs: Vec<(&'static str, TaskArg)>,
    ) -> Self {
        Self {
            task,
            args,
            kwargs,
            countdown: None,
        }
    }

    /// `apply_async(countdown=seconds)`: resolved into the `eta` header at
    /// encode time. A zero countdown is falsy in Python (`if countdown:` in
    /// `celery/app/amqp.py:300`) and publishes no eta; a negative one is
    /// truthy and publishes an eta in the past, which is also reproduced.
    #[must_use]
    pub fn with_countdown(mut self, seconds: i64) -> Self {
        self.countdown = (seconds != 0).then_some(seconds);
        self
    }

    /// The registered task name.
    #[must_use]
    pub fn task(&self) -> &'static str {
        self.task
    }

    /// The positional arguments.
    #[must_use]
    pub fn args(&self) -> &[TaskArg] {
        &self.args
    }

    /// The keyword arguments, in Python insertion order.
    #[must_use]
    pub fn kwargs(&self) -> &[(&'static str, TaskArg)] {
        &self.kwargs
    }

    /// The configured countdown, if any.
    #[must_use]
    pub fn countdown(&self) -> Option<i64> {
        self.countdown
    }

    /// Render the message to its properties, headers and body bytes.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError`] if kombu's JSON encoder refuses a value.
    pub fn encode(&self, context: &EncodeContext<'_>) -> Result<EncodedMessage, EncodeError> {
        let body = encode_body(&self.args, &self.kwargs)?;
        let eta = self.countdown.map(|seconds| {
            let instant = context.now + chrono::TimeDelta::seconds(seconds);
            py_isoformat(instant)
        });
        let id = context.id.hyphenated().to_string();
        let root_id = context
            .root_id
            .unwrap_or(context.id)
            .hyphenated()
            .to_string();
        let headers = Headers {
            lang: "py",
            task: self.task,
            id: id.clone(),
            shadow: None,
            eta,
            expires: None,
            group: None,
            group_index: None,
            retries: 0,
            timelimit: [None, None],
            root_id,
            parent_id: None,
            argsrepr: argsrepr(&self.args),
            kwargsrepr: kwargsrepr(&self.kwargs),
            origin: context.origin.to_string(),
            ignore_result: false,
            replaced_task_nesting: 0,
            stamped_headers: None,
            stamps: Vec::new(),
        };
        let properties = Properties {
            content_type: "application/json",
            content_encoding: "utf-8",
            delivery_mode: 2,
            priority: 0,
            correlation_id: id,
            reply_to: context.reply_to.to_string(),
        };
        Ok(EncodedMessage {
            task: self.task,
            task_id: context.id,
            headers,
            properties,
            body,
        })
    }
}

/// What the publisher supplies for one publish: a fresh task id, the publish
/// instant (for `countdown=`), and the per-process `origin`/`reply_to`.
#[derive(Clone, Debug)]
pub struct EncodeContext<'a> {
    /// `uuid.uuid4()` per published message.
    pub id: Uuid,
    /// `app.now()`; Celery's UTC clock at the moment the message is built.
    pub now: DateTime<Utc>,
    /// `anon_nodename()`: `gen<pid>@<hostname>`.
    pub origin: &'a str,
    /// `app.thread_oid`: one value per process for Plane's purposes.
    pub reply_to: &'a str,
    /// `root_id`, which defaults to `id` when the message is not published as
    /// part of a chain (`celery/app/amqp.py:325-326`).
    pub root_id: Option<Uuid>,
}

/// The AMQP basic properties Celery sets (`TASKS.md` §2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Properties {
    /// `"application/json"`.
    pub content_type: &'static str,
    /// `"utf-8"`.
    pub content_encoding: &'static str,
    /// `2` (persistent).
    pub delivery_mode: u8,
    /// `0`.
    pub priority: u8,
    /// The task id.
    pub correlation_id: String,
    /// The per-process reply queue name; nothing consumes it.
    pub reply_to: String,
}

/// The `application_headers` table Celery builds
/// (`celery/app/amqp.py:329-349`).
#[derive(Clone, Debug)]
pub struct Headers {
    /// `"py"`.
    pub lang: &'static str,
    /// The task name.
    pub task: &'static str,
    /// The task id (same as `correlation_id`).
    pub id: String,
    /// Always null: no Plane task uses `shadow_name`.
    pub shadow: Option<String>,
    /// ISO-8601 with microseconds and `+00:00`, for `countdown=` publishes.
    pub eta: Option<String>,
    /// Always null: no Plane task passes `expires=`.
    pub expires: Option<String>,
    /// Null (no canvas).
    pub group: Option<String>,
    /// Null (no canvas).
    pub group_index: Option<i64>,
    /// `0` on publish; the worker increments it when retrying.
    pub retries: i64,
    /// `[time_limit, soft_time_limit]`, both null: Plane sets none.
    pub timelimit: [Option<i64>; 2],
    /// Equals `id` outside a chain.
    pub root_id: String,
    /// Null (no chain).
    pub parent_id: Option<String>,
    /// Celery's `saferepr(args, 1024)`.
    pub argsrepr: String,
    /// Celery's `saferepr(kwargs, 1024)`.
    pub kwargsrepr: String,
    /// `gen<pid>@<hostname>`.
    pub origin: String,
    /// `false` (Plane leaves `task_ignore_result` at its default).
    pub ignore_result: bool,
    /// `0`.
    pub replaced_task_nesting: i64,
    /// Null (no stamped headers).
    pub stamped_headers: Option<Vec<String>>,
    /// `{}` (no stamps).
    pub stamps: Vec<(String, String)>,
}

/// A fully rendered message, ready to hand to the transport.
#[derive(Clone, Debug)]
pub struct EncodedMessage {
    /// The registered task name.
    pub task: &'static str,
    /// The task id, also `correlation_id` and `root_id`.
    pub task_id: Uuid,
    /// The AMQP properties.
    pub properties: Properties,
    /// The `application_headers` values.
    pub headers: Headers,
    /// The raw body bytes.
    pub body: Vec<u8>,
}

impl EncodedMessage {
    /// The shape `tools/celery/record.py` writes: the same names the
    /// differential harness's decoder uses for a drained delivery.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "task": self.task,
            "task_id": self.task_id.hyphenated().to_string(),
            "properties": {
                "content_type": self.properties.content_type,
                "content_encoding": self.properties.content_encoding,
                "delivery_mode": self.properties.delivery_mode,
                "priority": self.properties.priority,
                "correlation_id": self.properties.correlation_id,
                "reply_to": self.properties.reply_to,
                "delivery_info": {
                    "exchange": "",
                    "routing_key": DEFAULT_ROUTING_KEY,
                },
            },
            "headers": self.headers.to_json(),
            "body_b64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &self.body,
            ),
        })
    }
}

impl Headers {
    /// The header values as JSON, for golden comparison and diagnostics.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "lang": self.lang,
            "task": self.task,
            "id": self.id,
            "shadow": self.shadow,
            "eta": self.eta,
            "expires": self.expires,
            "group": self.group,
            "group_index": self.group_index,
            "retries": self.retries,
            "timelimit": [self.timelimit[0], self.timelimit[1]],
            "root_id": self.root_id,
            "parent_id": self.parent_id,
            "argsrepr": self.argsrepr,
            "kwargsrepr": self.kwargsrepr,
            "origin": self.origin,
            "ignore_result": self.ignore_result,
            "replaced_task_nesting": self.replaced_task_nesting,
            "stamped_headers": self.stamped_headers,
            "stamps": self.stamps.iter().cloned().collect::<std::collections::BTreeMap<_, _>>(),
        })
    }
}

/// Python's `datetime.isoformat()`: microseconds only when non-zero, and
/// `+00:00` rather than `Z` (`TASKS.md` §2.3).
#[must_use]
pub fn py_isoformat(instant: DateTime<Utc>) -> String {
    let format = if instant.nanosecond() / 1_000 == 0 {
        SecondsFormat::Secs
    } else {
        SecondsFormat::Micros
    };
    instant.to_rfc3339_opts(format, false)
}

fn encode_body(
    args: &[TaskArg],
    kwargs: &[(&'static str, TaskArg)],
) -> Result<Vec<u8>, EncodeError> {
    let embed = Embed {
        callbacks: Option::<()>::None,
        errbacks: Option::<()>::None,
        chain: Option::<()>::None,
        chord: Option::<()>::None,
    };
    // Celery's body is the tuple `(args, kwargs, embed)`
    // (`celery/app/amqp.py:357-364`); a serde tuple serialises as a JSON
    // array in declaration order.
    let body = (args, Kwargs(kwargs), embed);
    plane_serde::to_vec(plane_serde::Dialect::Kombu, &body).map_err(EncodeError::Json)
}

/// The keyword-argument dict, in Python insertion order.
struct Kwargs<'a>(&'a [(&'static str, TaskArg)]);

impl Serialize for Kwargs<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// The `embed` dict, in Celery's insertion order.
#[derive(Serialize)]
struct Embed {
    callbacks: Option<()>,
    errbacks: Option<()>,
    chain: Option<()>,
    chord: Option<()>,
}
