// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! One typed constructor per Celery task a Rust handler may enqueue.
//!
//! The argument types are the ones each **Python call site** passes, not the
//! task's nominal signature: Plane enqueues the same task with a `UUID` in
//! some places and its `str()` in others (`TASKS.md` §8.3), and the two
//! serialise differently into the body. Fields that vary use [`TaskArg`] so
//! the Rust caller states which call site it mirrors; fields that are the
//! same everywhere use a concrete type.
//!
//! Keyword arguments are emitted in the Python call expression's order (a
//! Python dict keeps insertion order, and Celery serialises the `kwargs` dict
//! in it). Where a task has several distinct call-site orderings — notably
//! `issue_activity` — the constructors below name the shape and cite the call
//! sites. The worker does not care about key order; a byte comparison of
//! drained messages does.
//!
//! Tasks deliberately absent:
//!
//! * `plane.bgtasks.analytic_plot_export.export_analytics_to_csv_email`,
//!   `plane.bgtasks.issue_version_sync.issue_task` and
//!   `plane.bgtasks.dummy_data_task.create_dummy_data` have no enqueue site
//!   (`TASKS.md` §8.2); the last is called directly by a management command.
//! * `plane.bgtasks.project_invitation_task.project_invitation` is not
//!   registered on any worker and its only caller raises (`TASKS.md` §8.1).

use uuid::Uuid;

use crate::celery::args::TaskArg;
use crate::celery::message::{Message, task_name};

// ---------------------------------------------------------------------------
// issue_activity
// ---------------------------------------------------------------------------

/// The seven fields every `issue_activity` call site passes first.
///
/// `requested_data` and `current_instance` are `str` values produced by
/// `json.dumps(…, cls=DjangoJSONEncoder)`, `None`, or (for
/// `current_instance` in one delete path) the empty dict; `issue_id` and
/// `project_id` are sometimes `str()` and sometimes a `UUID` object.
#[derive(Clone, Debug)]
pub struct IssueActivityBase {
    /// `type=`.
    pub activity_type: String,
    /// `requested_data=`.
    pub requested_data: TaskArg,
    /// `actor_id=`; every site passes `str(user.id)`.
    pub actor_id: String,
    /// `issue_id=`.
    pub issue_id: TaskArg,
    /// `project_id=`.
    pub project_id: TaskArg,
    /// `current_instance=`.
    pub current_instance: TaskArg,
    /// `epoch=int(timezone.now().timestamp())`.
    pub epoch: i64,
}

impl IssueActivityBase {
    /// Assemble the common fields.
    #[must_use]
    pub fn new(
        activity_type: impl Into<String>,
        requested_data: TaskArg,
        actor_id: impl Into<String>,
        issue_id: TaskArg,
        project_id: TaskArg,
        current_instance: TaskArg,
        epoch: i64,
    ) -> Self {
        Self {
            activity_type: activity_type.into(),
            requested_data,
            actor_id: actor_id.into(),
            issue_id,
            project_id,
            current_instance,
            epoch,
        }
    }
}

/// Which Python call-site keyword order an [`IssueActivityCall`] uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueActivityShape {
    /// `type, requested_data, actor_id, issue_id, project_id,
    /// current_instance, epoch, notification, origin` — 42 sites.
    View,
    /// The same without `notification`/`origin` — 22 sites.
    Bare,
    /// The view shape plus `intake` last — 4 sites.
    ViewWithIntake,
    /// `…, current_instance, subscriber, epoch, notification` — the two
    /// automation sites (`issue_automation_task.py:74,137`).
    Automation,
    /// `type, requested_data, current_instance, issue_id, actor_id,
    /// project_id, epoch` — the two bulk-date sites
    /// (`app/views/issue/base.py:1157,1170`).
    BulkDate,
    /// The bare shape plus `intake` last — 2 sites.
    WithIntake,
    /// The bare shape plus `origin` last — 2 sites.
    WithOrigin,
    /// The view shape plus `subscriber` last (issue delete,
    /// `app/views/issue/base.py:730`).
    ViewWithSubscriber,
    /// `type, requested_data, issue_id, project_id, actor_id,
    /// current_instance, epoch` (v1 link create,
    /// `api/views/issue.py:1286`).
    LinkCreated,
}

/// A typed `issue_activity` message.
#[derive(Clone, Debug)]
pub struct IssueActivityCall {
    /// The fields common to every shape.
    pub base: IssueActivityBase,
    /// `notification=`.
    pub notification: Option<bool>,
    /// `origin=`.
    pub origin: Option<String>,
    /// `subscriber=`.
    pub subscriber: Option<bool>,
    /// `intake=`.
    pub intake: Option<String>,
    shape: IssueActivityShape,
}

impl IssueActivityCall {
    fn from_parts(
        base: IssueActivityBase,
        shape: IssueActivityShape,
        notification: Option<bool>,
        origin: Option<String>,
        subscriber: Option<bool>,
        intake: Option<String>,
    ) -> Self {
        Self {
            base,
            notification,
            origin,
            subscriber,
            intake,
            shape,
        }
    }

    /// The dominant view shape: `notification=True`, `origin` last.
    /// 42 sites, e.g. `plane/app/views/issue/base.py:689`.
    #[must_use]
    pub fn view(base: IssueActivityBase, origin: impl Into<String>) -> Self {
        Self::from_parts(
            base,
            IssueActivityShape::View,
            Some(true),
            Some(origin.into()),
            None,
            None,
        )
    }

    /// View shape plus `intake` last. 4 sites, e.g.
    /// `plane/app/views/intake/base.py:281`; `notification` varies by site
    /// (`intake/base.py:465` and `api/views/intake.py:440` pass `False`).
    #[must_use]
    pub fn view_with_intake(
        base: IssueActivityBase,
        origin: impl Into<String>,
        notification: bool,
        intake: impl Into<String>,
    ) -> Self {
        Self::from_parts(
            base,
            IssueActivityShape::ViewWithIntake,
            Some(notification),
            Some(origin.into()),
            None,
            Some(intake.into()),
        )
    }

    /// The bare shape. 22 sites, e.g. `plane/api/views/issue.py:936`.
    #[must_use]
    pub fn bare(base: IssueActivityBase) -> Self {
        Self::from_parts(base, IssueActivityShape::Bare, None, None, None, None)
    }

    /// Bare shape plus `intake` last. 2 sites,
    /// `plane/api/views/intake.py:218,419`.
    #[must_use]
    pub fn with_intake(base: IssueActivityBase, intake: impl Into<String>) -> Self {
        Self::from_parts(
            base,
            IssueActivityShape::WithIntake,
            None,
            None,
            None,
            Some(intake.into()),
        )
    }

    /// Bare shape plus `origin` last. 2 sites,
    /// `plane/api/views/module.py:563,771`.
    #[must_use]
    pub fn with_origin(base: IssueActivityBase, origin: impl Into<String>) -> Self {
        Self::from_parts(
            base,
            IssueActivityShape::WithOrigin,
            None,
            Some(origin.into()),
            None,
            None,
        )
    }

    /// The two automation sites: `subscriber=False` before `epoch`,
    /// `notification=True` last (`plane/bgtasks/issue_automation_task.py:74,137`).
    #[must_use]
    pub fn automation(base: IssueActivityBase) -> Self {
        Self::from_parts(
            base,
            IssueActivityShape::Automation,
            Some(true),
            None,
            Some(false),
            None,
        )
    }

    /// The two bulk-date sites (`plane/app/views/issue/base.py:1157,1170`).
    #[must_use]
    pub fn bulk_date(base: IssueActivityBase) -> Self {
        Self::from_parts(base, IssueActivityShape::BulkDate, None, None, None, None)
    }

    /// Issue delete with `subscriber=False` last
    /// (`plane/app/views/issue/base.py:730`).
    #[must_use]
    pub fn view_with_subscriber(base: IssueActivityBase, origin: impl Into<String>) -> Self {
        Self::from_parts(
            base,
            IssueActivityShape::ViewWithSubscriber,
            Some(true),
            Some(origin.into()),
            Some(false),
            None,
        )
    }

    /// v1 link create (`plane/api/views/issue.py:1286`).
    #[must_use]
    pub fn link_created(base: IssueActivityBase) -> Self {
        Self::from_parts(
            base,
            IssueActivityShape::LinkCreated,
            None,
            None,
            None,
            None,
        )
    }

    /// The shape this call serialises with.
    #[must_use]
    pub fn shape(&self) -> IssueActivityShape {
        self.shape
    }
}

/// `plane.bgtasks.issue_activities_task.issue_activity`.
#[must_use]
pub fn issue_activity(call: IssueActivityCall) -> Message {
    let mut kwargs: Vec<(&'static str, TaskArg)> = Vec::with_capacity(11);
    let base = &call.base;
    let push_common = |kwargs: &mut Vec<(&'static str, TaskArg)>| {
        kwargs.push(("type", TaskArg::str(&base.activity_type)));
        kwargs.push(("requested_data", base.requested_data.clone()));
        kwargs.push(("actor_id", TaskArg::str(&base.actor_id)));
        kwargs.push(("issue_id", base.issue_id.clone()));
        kwargs.push(("project_id", base.project_id.clone()));
        kwargs.push(("current_instance", base.current_instance.clone()));
    };
    match call.shape {
        IssueActivityShape::View => {
            push_common(&mut kwargs);
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
            kwargs.push(("notification", TaskArg::Bool(true)));
            if let Some(origin) = &call.origin {
                kwargs.push(("origin", TaskArg::str(origin)));
            }
        }
        IssueActivityShape::Bare => {
            push_common(&mut kwargs);
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
        }
        IssueActivityShape::ViewWithIntake => {
            push_common(&mut kwargs);
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
            if let Some(notification) = call.notification {
                kwargs.push(("notification", TaskArg::Bool(notification)));
            }
            if let Some(origin) = &call.origin {
                kwargs.push(("origin", TaskArg::str(origin)));
            }
            if let Some(intake) = &call.intake {
                kwargs.push(("intake", TaskArg::str(intake)));
            }
        }
        IssueActivityShape::Automation => {
            push_common(&mut kwargs);
            kwargs.push(("subscriber", TaskArg::Bool(false)));
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
            kwargs.push(("notification", TaskArg::Bool(true)));
        }
        IssueActivityShape::BulkDate => {
            kwargs.push(("type", TaskArg::str(&base.activity_type)));
            kwargs.push(("requested_data", base.requested_data.clone()));
            kwargs.push(("current_instance", base.current_instance.clone()));
            kwargs.push(("issue_id", base.issue_id.clone()));
            kwargs.push(("actor_id", TaskArg::str(&base.actor_id)));
            kwargs.push(("project_id", base.project_id.clone()));
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
        }
        IssueActivityShape::WithIntake => {
            push_common(&mut kwargs);
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
            if let Some(intake) = &call.intake {
                kwargs.push(("intake", TaskArg::str(intake)));
            }
        }
        IssueActivityShape::WithOrigin => {
            push_common(&mut kwargs);
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
            if let Some(origin) = &call.origin {
                kwargs.push(("origin", TaskArg::str(origin)));
            }
        }
        IssueActivityShape::ViewWithSubscriber => {
            push_common(&mut kwargs);
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
            kwargs.push(("notification", TaskArg::Bool(true)));
            if let Some(origin) = &call.origin {
                kwargs.push(("origin", TaskArg::str(origin)));
            }
            kwargs.push(("subscriber", TaskArg::Bool(false)));
        }
        IssueActivityShape::LinkCreated => {
            kwargs.push(("type", TaskArg::str(&base.activity_type)));
            kwargs.push(("requested_data", base.requested_data.clone()));
            kwargs.push(("issue_id", base.issue_id.clone()));
            kwargs.push(("project_id", base.project_id.clone()));
            kwargs.push(("actor_id", TaskArg::str(&base.actor_id)));
            kwargs.push(("current_instance", base.current_instance.clone()));
            kwargs.push(("epoch", TaskArg::Int(base.epoch)));
        }
    }
    Message::new(task_name::ISSUE_ACTIVITY, Vec::new(), kwargs)
}

// ---------------------------------------------------------------------------
// recent_visited_task
// ---------------------------------------------------------------------------

/// The two keyword orders used by `recent_visited_task`'s ten call sites.
#[derive(Clone, Debug)]
pub enum RecentVisitedTask {
    /// `slug, project_id, entity_name, entity_identifier, user_id` — five
    /// sites (`app/views/issue/base.py:165,304`;
    /// `app/views/project/base.py:247`; `app/views/view/base.py:140,376`).
    /// `project_id` is `None` for a workspace-level view.
    ProjectKeyed {
        /// `slug=`.
        slug: String,
        /// `project_id=`.
        project_id: Option<TaskArg>,
        /// `entity_name=`.
        entity_name: String,
        /// `entity_identifier=`.
        entity_identifier: TaskArg,
        /// `user_id=`.
        user_id: TaskArg,
    },
    /// `slug, entity_name, entity_identifier, user_id, project_id` — five
    /// sites (`app/views/cycle/base.py:469`; `app/views/issue/base.py:617,1359`;
    /// `app/views/module/base.py:642`; `app/views/page/base.py:232`).
    EntityKeyed {
        /// `slug=`.
        slug: String,
        /// `entity_name=`.
        entity_name: String,
        /// `entity_identifier=`.
        entity_identifier: TaskArg,
        /// `user_id=`.
        user_id: TaskArg,
        /// `project_id=`; UUID, string or `None` depending on the site.
        project_id: Option<TaskArg>,
    },
}

/// `plane.bgtasks.recent_visited_task.recent_visited_task`.
#[must_use]
pub fn recent_visited_task(call: RecentVisitedTask) -> Message {
    let mut kwargs: Vec<(&'static str, TaskArg)> = Vec::with_capacity(5);
    match call {
        RecentVisitedTask::ProjectKeyed {
            slug,
            project_id,
            entity_name,
            entity_identifier,
            user_id,
        } => {
            kwargs.push(("slug", TaskArg::str(slug)));
            kwargs.push(("project_id", project_id.unwrap_or(TaskArg::Null)));
            kwargs.push(("entity_name", TaskArg::str(entity_name)));
            kwargs.push(("entity_identifier", entity_identifier));
            kwargs.push(("user_id", user_id));
        }
        RecentVisitedTask::EntityKeyed {
            slug,
            entity_name,
            entity_identifier,
            user_id,
            project_id,
        } => {
            kwargs.push(("slug", TaskArg::str(slug)));
            kwargs.push(("entity_name", TaskArg::str(entity_name)));
            kwargs.push(("entity_identifier", entity_identifier));
            kwargs.push(("user_id", user_id));
            kwargs.push(("project_id", project_id.unwrap_or(TaskArg::Null)));
        }
    }
    Message::new(task_name::RECENT_VISITED_TASK, Vec::new(), kwargs)
}

// ---------------------------------------------------------------------------
// Webhook tasks
// ---------------------------------------------------------------------------

/// `plane.bgtasks.webhook_task.model_activity`, with the call-site order
/// `model_name, model_id, requested_data, current_instance, actor_id, slug,
/// origin` (22 sites).
#[derive(Clone, Debug)]
pub struct ModelActivityCall {
    /// `model_name=`; `"project"`, `"issue"`, `"issue_comment"`, `"cycle"`,
    /// `"module"`.
    pub model_name: String,
    /// `model_id=`; always `str(...)` at the call sites.
    pub model_id: String,
    /// `requested_data=request.data` (a dict, serialised inline).
    pub requested_data: TaskArg,
    /// `current_instance=`; a `json.dumps(…, cls=DjangoJSONEncoder)` string
    /// or `None`.
    pub current_instance: Option<String>,
    /// `actor_id=request.user.id`; a `UUID` at every site.
    pub actor_id: Uuid,
    /// `slug=`.
    pub slug: String,
    /// `origin=base_host(request=request, is_app=True)`.
    pub origin: String,
}

/// `plane.bgtasks.webhook_task.model_activity`.
#[must_use]
pub fn model_activity(call: ModelActivityCall) -> Message {
    let kwargs = vec![
        ("model_name", TaskArg::str(call.model_name)),
        ("model_id", TaskArg::str(call.model_id)),
        ("requested_data", call.requested_data),
        (
            "current_instance",
            call.current_instance
                .map_or(TaskArg::Null, TaskArg::JsonText),
        ),
        ("actor_id", TaskArg::uuid(call.actor_id)),
        ("slug", TaskArg::str(call.slug)),
        ("origin", TaskArg::str(call.origin)),
    ];
    Message::new(task_name::MODEL_ACTIVITY, Vec::new(), kwargs)
}

/// `plane.bgtasks.webhook_task.webhook_activity`, with the call-site order
/// `event, verb, field, old_value, new_value, actor_id, slug, current_site,
/// event_id, old_identifier, new_identifier` (4 sites).
#[derive(Clone, Debug)]
pub struct WebhookActivityCall {
    /// `event=`.
    pub event: String,
    /// `verb=`; `"created"`, `"updated"` or `"deleted"`.
    pub verb: String,
    /// `field=`.
    pub field: Option<String>,
    /// `old_value=`.
    pub old_value: TaskArg,
    /// `new_value=`.
    pub new_value: TaskArg,
    /// `actor_id=`; a `UUID` at every site.
    pub actor_id: Uuid,
    /// `slug=`.
    pub slug: String,
    /// `current_site=`.
    pub current_site: String,
    /// `event_id=`; a `UUID` at the two project-delete sites, a string when
    /// forwarded by `model_activity`.
    pub event_id: TaskArg,
    /// `old_identifier=`.
    pub old_identifier: Option<String>,
    /// `new_identifier=`.
    pub new_identifier: Option<String>,
}

/// `plane.bgtasks.webhook_task.webhook_activity`.
#[must_use]
pub fn webhook_activity(call: WebhookActivityCall) -> Message {
    let kwargs = vec![
        ("event", TaskArg::str(call.event)),
        ("verb", TaskArg::str(call.verb)),
        ("field", call.field.map_or(TaskArg::Null, TaskArg::Str)),
        ("old_value", call.old_value),
        ("new_value", call.new_value),
        ("actor_id", TaskArg::uuid(call.actor_id)),
        ("slug", TaskArg::str(call.slug)),
        ("current_site", TaskArg::str(call.current_site)),
        ("event_id", call.event_id),
        (
            "old_identifier",
            call.old_identifier.map_or(TaskArg::Null, TaskArg::Str),
        ),
        (
            "new_identifier",
            call.new_identifier.map_or(TaskArg::Null, TaskArg::Str),
        ),
    ];
    Message::new(task_name::WEBHOOK_ACTIVITY, Vec::new(), kwargs)
}

/// `plane.bgtasks.webhook_task.webhook_send_task`, with the call-site order
/// `webhook_id, slug, event, event_data, action, current_site, activity`
/// (`webhook_task.py:451`).
#[derive(Clone, Debug)]
pub struct WebhookSendTaskCall {
    /// `webhook_id=webhook.id`; a `UUID`.
    pub webhook_id: Uuid,
    /// `slug=`.
    pub slug: String,
    /// `event=`.
    pub event: String,
    /// `event_data=`.
    pub event_data: TaskArg,
    /// `action=verb`.
    pub action: String,
    /// `current_site=`.
    pub current_site: String,
    /// `activity=`.
    pub activity: TaskArg,
}

/// `plane.bgtasks.webhook_task.webhook_send_task`.
#[must_use]
pub fn webhook_send_task(call: WebhookSendTaskCall) -> Message {
    let kwargs = vec![
        ("webhook_id", TaskArg::uuid(call.webhook_id)),
        ("slug", TaskArg::str(call.slug)),
        ("event", TaskArg::str(call.event)),
        ("event_data", call.event_data),
        ("action", TaskArg::str(call.action)),
        ("current_site", TaskArg::str(call.current_site)),
        ("activity", call.activity),
    ];
    Message::new(task_name::WEBHOOK_SEND_TASK, Vec::new(), kwargs)
}

/// `plane.bgtasks.webhook_task.send_webhook_deactivation_email`
/// (`webhook_task.py:359`).
#[must_use]
pub fn send_webhook_deactivation_email(
    webhook_id: Uuid,
    receiver_id: Option<Uuid>,
    reason: String,
    current_site: String,
) -> Message {
    let kwargs = vec![
        ("webhook_id", TaskArg::uuid(webhook_id)),
        (
            "receiver_id",
            receiver_id.map_or(TaskArg::Null, TaskArg::uuid),
        ),
        ("reason", TaskArg::str(reason)),
        ("current_site", TaskArg::str(current_site)),
    ];
    Message::new(
        task_name::SEND_WEBHOOK_DEACTIVATION_EMAIL,
        Vec::new(),
        kwargs,
    )
}

// ---------------------------------------------------------------------------
// Notification and email chain
// ---------------------------------------------------------------------------

/// `plane.bgtasks.notification_task.notifications`, with the producer's
/// order (`issue_activities_task.py:1633`):
/// `type, issue_id, actor_id, project_id, subscriber,
/// issue_activities_created, requested_data, current_instance`.
#[derive(Clone, Debug)]
pub struct NotificationsCall {
    /// `type=`; the originating activity type.
    pub activity_type: String,
    /// `issue_id=`; the value `issue_activity` received (string, UUID or
    /// `None`).
    pub issue_id: TaskArg,
    /// `actor_id=`; string or UUID.
    pub actor_id: TaskArg,
    /// `project_id=`; string or UUID.
    pub project_id: TaskArg,
    /// `subscriber=`.
    pub subscriber: bool,
    /// `issue_activities_created=json.dumps(serializer.data, cls=DjangoJSONEncoder)`.
    pub issue_activities_created: TaskArg,
    /// `requested_data=`.
    pub requested_data: TaskArg,
    /// `current_instance=`.
    pub current_instance: TaskArg,
}

/// `plane.bgtasks.notification_task.notifications`.
#[must_use]
pub fn notifications(call: NotificationsCall) -> Message {
    let kwargs = vec![
        ("type", TaskArg::str(call.activity_type)),
        ("issue_id", call.issue_id),
        ("actor_id", call.actor_id),
        ("project_id", call.project_id),
        ("subscriber", TaskArg::Bool(call.subscriber)),
        ("issue_activities_created", call.issue_activities_created),
        ("requested_data", call.requested_data),
        ("current_instance", call.current_instance),
    ];
    Message::new(task_name::NOTIFICATIONS, Vec::new(), kwargs)
}

/// `plane.bgtasks.email_notification_task.send_email_notification`, the
/// message `stack_email_notification` produces (`email_notification_task.py:76`).
///
/// `issue_id` is a `UUID` (it comes from `EmailNotificationLog.entity_identifier`),
/// `receiver_id` a `str`, `email_notification_ids` a `list[UUID]`, and
/// `notification_data` the nested payload described in the task docstring.
#[must_use]
pub fn send_email_notification(
    issue_id: Uuid,
    notification_data: TaskArg,
    receiver_id: String,
    email_notification_ids: Vec<Uuid>,
) -> Message {
    let kwargs = vec![
        ("issue_id", TaskArg::uuid(issue_id)),
        ("notification_data", notification_data),
        ("receiver_id", TaskArg::str(receiver_id)),
        (
            "email_notification_ids",
            TaskArg::list(email_notification_ids.into_iter().map(TaskArg::uuid)),
        ),
    ];
    Message::new(task_name::SEND_EMAIL_NOTIFICATION, Vec::new(), kwargs)
}

/// `plane.bgtasks.email_notification_task.stack_email_notification`.
#[must_use]
pub fn stack_email_notification() -> Message {
    Message::new(task_name::STACK_EMAIL_NOTIFICATION, Vec::new(), Vec::new())
}

/// `plane.bgtasks.forgot_password_task.forgot_password`; both call sites pass
/// five positional strings (`authentication/views/{app,space}/password_management.py`).
#[must_use]
pub fn forgot_password(
    first_name: String,
    email: String,
    uidb64: String,
    token: String,
    current_site: String,
) -> Message {
    let args = vec![
        TaskArg::str(first_name),
        TaskArg::str(email),
        TaskArg::str(uidb64),
        TaskArg::str(token),
        TaskArg::str(current_site),
    ];
    Message::new(task_name::FORGOT_PASSWORD, args, Vec::new())
}

/// `plane.bgtasks.magic_link_code_task.magic_link`; both call sites pass
/// `(email, key, token)` positionally.
#[must_use]
pub fn magic_link(email: String, key: String, token: String) -> Message {
    let args = vec![TaskArg::str(email), TaskArg::str(key), TaskArg::str(token)];
    Message::new(task_name::MAGIC_LINK, args, Vec::new())
}

/// `plane.bgtasks.user_activation_email_task.user_activation_email`
/// (`authentication/adapter/base.py:248`): `(current_site, user.id)`, the id a
/// `UUID`.
#[must_use]
pub fn user_activation_email(current_site: String, user_id: Uuid) -> Message {
    let args = vec![TaskArg::str(current_site), TaskArg::uuid(user_id)];
    Message::new(task_name::USER_ACTIVATION_EMAIL, args, Vec::new())
}

/// `plane.bgtasks.user_deactivation_email_task.user_deactivation_email`
/// (`app/views/user/base.py:370`): `(current_site, user.id)`, the id a `UUID`.
#[must_use]
pub fn user_deactivation_email(current_site: String, user_id: Uuid) -> Message {
    let args = vec![TaskArg::str(current_site), TaskArg::uuid(user_id)];
    Message::new(task_name::USER_DEACTIVATION_EMAIL, args, Vec::new())
}

/// `plane.bgtasks.user_email_update_task.send_email_update_confirmation`
/// (`app/views/user/base.py:270,272`): one positional email.
#[must_use]
pub fn send_email_update_confirmation(email: String) -> Message {
    Message::new(
        task_name::SEND_EMAIL_UPDATE_CONFIRMATION,
        vec![TaskArg::str(email)],
        Vec::new(),
    )
}

/// `plane.bgtasks.user_email_update_task.send_email_update_magic_code`
/// (`app/views/user/base.py:178`): `(new_email, token)` positional.
#[must_use]
pub fn send_email_update_magic_code(email: String, token: String) -> Message {
    let args = vec![TaskArg::str(email), TaskArg::str(token)];
    Message::new(task_name::SEND_EMAIL_UPDATE_MAGIC_CODE, args, Vec::new())
}

// ---------------------------------------------------------------------------
// Project and workspace
// ---------------------------------------------------------------------------

/// `plane.bgtasks.project_add_user_email_task.project_add_user_email`
/// (`app/views/project/member.py:154`):
/// `(current_site, project_member.id, request.user.id)`, both ids `UUID`.
#[must_use]
pub fn project_add_user_email(
    current_site: String,
    project_member_id: Uuid,
    invitor_id: Uuid,
) -> Message {
    let args = vec![
        TaskArg::str(current_site),
        TaskArg::uuid(project_member_id),
        TaskArg::uuid(invitor_id),
    ];
    Message::new(task_name::PROJECT_ADD_USER_EMAIL, args, Vec::new())
}

/// `plane.bgtasks.workspace_invitation_task.workspace_invitation`
/// (`app/views/workspace/invite.py:120`):
/// `(email, workspace.id, token, current_site, inviter)`; the workspace id is
/// a `UUID` object, the rest strings.
#[must_use]
pub fn workspace_invitation(
    email: String,
    workspace_id: Uuid,
    token: String,
    current_site: String,
    inviter: String,
) -> Message {
    let args = vec![
        TaskArg::str(email),
        TaskArg::uuid(workspace_id),
        TaskArg::str(token),
        TaskArg::str(current_site),
        TaskArg::str(inviter),
    ];
    Message::new(task_name::WORKSPACE_INVITATION, args, Vec::new())
}

/// `plane.bgtasks.workspace_seed_task.workspace_seed`
/// (`app/views/workspace/base.py:138`): one positional argument,
/// `serializer.data["id"]`, a **string**.
#[must_use]
pub fn workspace_seed(workspace_id: String) -> Message {
    Message::new(
        task_name::WORKSPACE_SEED,
        vec![TaskArg::str(workspace_id)],
        Vec::new(),
    )
}

/// `plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets`
/// (`app/views/page/base.py:617`): `entity_name` is a string; the page id,
/// project id and user id are `UUID` objects.
#[must_use]
pub fn copy_s3_objects_of_description_and_assets(
    entity_name: String,
    entity_identifier: Uuid,
    project_id: Uuid,
    slug: String,
    user_id: Uuid,
) -> Message {
    let kwargs = vec![
        ("entity_name", TaskArg::str(entity_name)),
        ("entity_identifier", TaskArg::uuid(entity_identifier)),
        ("project_id", TaskArg::uuid(project_id)),
        ("slug", TaskArg::str(slug)),
        ("user_id", TaskArg::uuid(user_id)),
    ];
    Message::new(task_name::COPY_S3_OBJECTS, Vec::new(), kwargs)
}

// ---------------------------------------------------------------------------
// Issue and page tasks
// ---------------------------------------------------------------------------

/// `plane.bgtasks.issue_description_version_task.issue_description_version_task`.
///
/// Create sites (`app/views/issue/base.py:484`,
/// `app/views/intake/base.py:294`) pass `is_creating=True`; update sites omit
/// it, so the kwarg is only emitted for `true`.
#[must_use]
pub fn issue_description_version_task(
    updated_issue: TaskArg,
    issue_id: String,
    user_id: Uuid,
    is_creating: bool,
) -> Message {
    let mut kwargs = vec![
        ("updated_issue", updated_issue),
        ("issue_id", TaskArg::str(issue_id)),
        ("user_id", TaskArg::uuid(user_id)),
    ];
    if is_creating {
        kwargs.push(("is_creating", TaskArg::Bool(true)));
    }
    Message::new(
        task_name::ISSUE_DESCRIPTION_VERSION_TASK,
        Vec::new(),
        kwargs,
    )
}

/// The two shapes of the issue-version backfill tasks.
#[derive(Clone, Debug)]
pub enum BackfillBatch {
    /// `apply_async(kwargs={'batch_size': …, 'offset': …, 'countdown': …},
    /// countdown=…)`: the self re-enqueue inside the task.
    Continue {
        /// The batch size (an `int`).
        batch_size: i64,
        /// The next offset (an `int`).
        offset: i64,
        /// The delay in seconds, present both as a task kwarg and as the
        /// message's `eta` countdown.
        countdown: i64,
    },
    /// `delay(batch_size=…, countdown=…)`: the management command's entry
    /// point and `schedule_*`'s enqueue.
    Start {
        /// The batch size; a `str` from `input()` in the management command,
        /// an `int` inside the task. [`TaskArg`] keeps that distinction.
        batch_size: TaskArg,
        /// The delay in seconds; also the `eta` countdown.
        countdown: i64,
    },
}

fn backfill_message(task: &'static str, batch: BackfillBatch) -> Message {
    match batch {
        BackfillBatch::Continue {
            batch_size,
            offset,
            countdown,
        } => {
            let kwargs = vec![
                ("batch_size", TaskArg::Int(batch_size)),
                ("offset", TaskArg::Int(offset)),
                ("countdown", TaskArg::Int(countdown)),
            ];
            Message::new(task, Vec::new(), kwargs).with_countdown(countdown)
        }
        BackfillBatch::Start {
            batch_size,
            countdown,
        } => {
            let kwargs = vec![
                ("batch_size", batch_size),
                ("countdown", TaskArg::Int(countdown)),
            ];
            // The `Start` sites use `.delay(batch_size=…, countdown=…)`: the
            // `countdown` is a task argument, not a publish option, so there
            // is no `eta` header.
            Message::new(task, Vec::new(), kwargs)
        }
    }
}

/// `plane.bgtasks.issue_version_sync.schedule_issue_version`
/// (`db/management/commands/sync_issue_version.py:19`).
#[must_use]
pub fn schedule_issue_version(batch_size: TaskArg, countdown: i64) -> Message {
    backfill_message(
        task_name::SCHEDULE_ISSUE_VERSION,
        BackfillBatch::Start {
            batch_size,
            countdown,
        },
    )
}

/// `plane.bgtasks.issue_version_sync.sync_issue_version`.
#[must_use]
pub fn sync_issue_version(batch: BackfillBatch) -> Message {
    backfill_message(task_name::SYNC_ISSUE_VERSION, batch)
}

/// `plane.bgtasks.issue_description_version_sync.schedule_issue_description_version`
/// (`db/management/commands/sync_issue_description_version.py:21`).
#[must_use]
pub fn schedule_issue_description_version(batch_size: TaskArg, countdown: i64) -> Message {
    backfill_message(
        task_name::SCHEDULE_ISSUE_DESCRIPTION_VERSION,
        BackfillBatch::Start {
            batch_size,
            countdown,
        },
    )
}

/// `plane.bgtasks.issue_description_version_sync.sync_issue_description_version`.
#[must_use]
pub fn sync_issue_description_version(batch: BackfillBatch) -> Message {
    backfill_message(task_name::SYNC_ISSUE_DESCRIPTION_VERSION, batch)
}

/// `plane.bgtasks.page_version_task.track_page_version`
/// (`app/views/page/base.py:565`): `page_id` and `user_id` are `UUID`;
/// `existing_instance` is a `json.dumps(…, cls=DjangoJSONEncoder)` string.
#[must_use]
pub fn track_page_version(page_id: Uuid, existing_instance: TaskArg, user_id: Uuid) -> Message {
    let kwargs = vec![
        ("page_id", TaskArg::uuid(page_id)),
        ("existing_instance", existing_instance),
        ("user_id", TaskArg::uuid(user_id)),
    ];
    Message::new(task_name::TRACK_PAGE_VERSION, Vec::new(), kwargs)
}

/// `plane.bgtasks.page_transaction_task.page_transaction`.
///
/// `old_description_html` is always present, `None` on create/duplicate.
/// `page_id` is a string at the create site (`serializer.data["id"]`) and a
/// `UUID` at the other three.
#[must_use]
pub fn page_transaction(
    new_description_html: String,
    old_description_html: Option<String>,
    page_id: TaskArg,
) -> Message {
    let kwargs = vec![
        ("new_description_html", TaskArg::str(new_description_html)),
        (
            "old_description_html",
            old_description_html.map_or(TaskArg::Null, TaskArg::Str),
        ),
        ("page_id", page_id),
    ];
    Message::new(task_name::PAGE_TRANSACTION, Vec::new(), kwargs)
}

/// `plane.bgtasks.work_item_link_task.crawl_work_item_link_title`.
///
/// Four positional calls: `serializer.instance.id` (a `UUID`) at
/// `api/views/issue.py:1284`, and `serializer.data.get("id")` (a string) at
/// the other three.
#[must_use]
pub fn crawl_work_item_link_title(id: TaskArg, url: String) -> Message {
    Message::new(
        task_name::CRAWL_WORK_ITEM_LINK_TITLE,
        vec![id, TaskArg::str(url)],
        Vec::new(),
    )
}

// ---------------------------------------------------------------------------
// Storage, export and analytics
// ---------------------------------------------------------------------------

/// `plane.bgtasks.storage_metadata_task.get_asset_object_metadata`, keyword
/// form (`asset_id=str(asset_id)`): six call sites, e.g.
/// `app/views/asset/v2.py:219`.
#[must_use]
pub fn get_asset_object_metadata(asset_id: String) -> Message {
    Message::new(
        task_name::GET_ASSET_OBJECT_METADATA,
        Vec::new(),
        vec![("asset_id", TaskArg::str(asset_id))],
    )
}

/// The positional form of `get_asset_object_metadata`:
/// `get_asset_object_metadata.delay(str(issue_attachment.id))`, four sites
/// (`app/views/issue/attachment.py:277`; `api/views/issue.py:2254,2415`;
/// `space/views/asset.py:197`).
#[must_use]
pub fn get_asset_object_metadata_positional(asset_id: String) -> Message {
    Message::new(
        task_name::GET_ASSET_OBJECT_METADATA,
        vec![TaskArg::str(asset_id)],
        Vec::new(),
    )
}

/// `plane.bgtasks.export_task.issue_export_task`
/// (`app/views/exporter/base.py:50`): the workspace id is a `UUID`, the
/// project ids are a `list[str]`.
#[must_use]
pub fn issue_export_task(
    provider: String,
    workspace_id: Uuid,
    project_ids: Vec<String>,
    token_id: String,
    multiple: bool,
    slug: String,
) -> Message {
    let kwargs = vec![
        ("provider", TaskArg::str(provider)),
        ("workspace_id", TaskArg::uuid(workspace_id)),
        (
            "project_ids",
            TaskArg::list(project_ids.into_iter().map(TaskArg::Str)),
        ),
        ("token_id", TaskArg::str(token_id)),
        ("multiple", TaskArg::Bool(multiple)),
        ("slug", TaskArg::str(slug)),
    ];
    Message::new(task_name::ISSUE_EXPORT_TASK, Vec::new(), kwargs)
}

/// `plane.bgtasks.analytic_plot_export.analytic_export_task`
/// (`app/views/analytic/base.py:256`): `data=request.data` is a dict, and
/// `user_id` is `str(...)`.
#[must_use]
pub fn analytic_export_task(
    email: String,
    data: TaskArg,
    slug: String,
    user_id: String,
) -> Message {
    let kwargs = vec![
        ("email", TaskArg::str(email)),
        ("data", data),
        ("slug", TaskArg::str(slug)),
        ("user_id", TaskArg::str(user_id)),
    ];
    Message::new(task_name::ANALYTIC_EXPORT_TASK, Vec::new(), kwargs)
}

// ---------------------------------------------------------------------------
// Logging and deletion
// ---------------------------------------------------------------------------

/// `plane.bgtasks.logger_task.process_logs` (`middleware/logger.py:158`):
/// `log_data=log_data`, the dict built by `APITokenLogMiddleware`.
#[must_use]
pub fn process_logs(log_data: TaskArg) -> Message {
    Message::new(
        task_name::PROCESS_LOGS,
        Vec::new(),
        vec![("log_data", log_data)],
    )
}

/// `plane.bgtasks.deletion_task.soft_delete_related_objects`
/// (`db/mixins.py:78`): three positional values plus `using=` (which is
/// always passed, possibly `None`).
#[must_use]
pub fn soft_delete_related_objects(
    app_label: String,
    model_name: String,
    instance_pk: Uuid,
    using: Option<String>,
) -> Message {
    let args = vec![
        TaskArg::str(app_label),
        TaskArg::str(model_name),
        TaskArg::uuid(instance_pk),
    ];
    let kwargs = vec![("using", using.map_or(TaskArg::Null, TaskArg::Str))];
    Message::new(task_name::SOFT_DELETE_RELATED_OBJECTS, args, kwargs)
}

// ---------------------------------------------------------------------------
// Beat-only tasks
// ---------------------------------------------------------------------------

macro_rules! no_argument_task {
    ($(#[$meta:meta])* $name:ident => $task:expr) => {
        $(#[$meta])*
        #[must_use]
        pub fn $name() -> Message {
            Message::new($task, Vec::new(), Vec::new())
        }
    };
}

no_argument_task!(
    /// `plane.bgtasks.cleanup_task.delete_api_logs`.
    delete_api_logs => task_name::DELETE_API_LOGS
);
no_argument_task!(
    /// `plane.bgtasks.cleanup_task.delete_email_notification_logs`.
    delete_email_notification_logs => task_name::DELETE_EMAIL_NOTIFICATION_LOGS
);
no_argument_task!(
    /// `plane.bgtasks.cleanup_task.delete_issue_description_versions`.
    delete_issue_description_versions => task_name::DELETE_ISSUE_DESCRIPTION_VERSIONS
);
no_argument_task!(
    /// `plane.bgtasks.cleanup_task.delete_page_versions`.
    delete_page_versions => task_name::DELETE_PAGE_VERSIONS
);
no_argument_task!(
    /// `plane.bgtasks.cleanup_task.delete_webhook_logs`.
    delete_webhook_logs => task_name::DELETE_WEBHOOK_LOGS
);
no_argument_task!(
    /// `plane.bgtasks.deletion_task.hard_delete`.
    hard_delete => task_name::HARD_DELETE
);
no_argument_task!(
    /// `plane.bgtasks.exporter_expired_task.delete_old_s3_link`. Scheduled
    /// twice (`TASKS.md` §7.1).
    delete_old_s3_link => task_name::DELETE_OLD_S3_LINK
);
no_argument_task!(
    /// `plane.bgtasks.file_asset_task.delete_unuploaded_file_asset`.
    delete_unuploaded_file_asset => task_name::DELETE_UNUPLOADED_FILE_ASSET
);
no_argument_task!(
    /// `plane.bgtasks.issue_automation_task.archive_and_close_old_issues`.
    archive_and_close_old_issues => task_name::ARCHIVE_AND_CLOSE_OLD_ISSUES
);
no_argument_task!(
    /// `plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues`.
    unsnooze_expired_intake_issues => task_name::UNSNOOZE_EXPIRED_INTAKE_ISSUES
);
no_argument_task!(
    /// `plane.license.bgtasks.telemetry_metrics.push_instance_metrics`.
    ///
    /// Included for completeness: `docker-entrypoint-api.sh` enqueues it via
    /// `manage.py register_instance` on every API container start. Owner
    /// decision 5 assumes the metric push itself is not ported.
    push_instance_metrics => task_name::PUSH_INSTANCE_METRICS
);
