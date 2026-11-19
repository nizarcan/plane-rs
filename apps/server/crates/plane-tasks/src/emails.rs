// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The auth and user e-mail tasks: 17, 28, 36, 37, 38, 39, 32, 45.
//!
//! Every one of them is the same shape (`TASKS.md:545-573`):
//!
//! ```python
//! try:
//!     (...) = get_email_configuration()
//!     <look up the rows the context needs>
//!     html = render_to_string("<template>", context)
//!     text = generate_plain_text_from_html(html)
//!     connection = get_connection(host=..., use_tls=EMAIL_USE_TLS == "1", ...)
//!     msg = EmailMultiAlternatives(subject, body=text, from_email=EMAIL_FROM,
//!                                  to=[...], connection=connection)
//!     msg.attach_alternative(html, "text/html")
//!     msg.send()
//! except Exception as e:
//!     log_exception(e, warning=True)   # 17/28/36-39; `log_exception(e)` for 40
//!     return
//! ```
//!
//! So a failure anywhere — a missing user, an unparseable `EMAIL_PORT`, an SMTP
//! error — is logged and the task succeeds. The handler returns `Ok(())` after
//! [`swallow`] and never retries.
//!
//! # Positional arguments
//!
//! These tasks are called positionally at every site (`TASKS.md:1120-1145`),
//! so the handler reads `payload["args"]`, not `kwargs`.
//!
//! # Python details that are part of the bytes
//!
//! * `f"{user.first_name or user.display_name or user.email}"`: an empty name
//!   falls through, and a `None` e-mail renders `"None"`.
//! * `str(user.email)` in the template context: `None` → `"None"`.
//! * 45's `workspace_member_invite.message = text_content; save()` is a bare
//!   `save()`: every column is written, `updated_at` moves and the audit users
//!   are nulled (`plane/db/models/base.py:23-50`).

use std::sync::Arc;

use deadpool_postgres::Client;
use plane_db::models::{ProjectMember, User, Workspace, WorkspaceMemberInvite, with_alias};
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_mail::{EmailConfiguration, Message};
use serde_json::Value;
use uuid::Uuid;

use crate::{TaskContext, args, string_value, swallow, uuid_value};

/// 17 — `forgot_password`.
pub const FORGOT_PASSWORD: &str = "plane.bgtasks.forgot_password_task.forgot_password";
/// 28 — `magic_link`.
pub const MAGIC_LINK: &str = "plane.bgtasks.magic_link_code_task.magic_link";
/// 32 — `project_add_user_email`.
pub const PROJECT_ADD_USER_EMAIL: &str =
    "plane.bgtasks.project_add_user_email_task.project_add_user_email";
/// 36 — `user_activation_email`.
pub const USER_ACTIVATION_EMAIL: &str =
    "plane.bgtasks.user_activation_email_task.user_activation_email";
/// 37 — `user_deactivation_email`.
pub const USER_DEACTIVATION_EMAIL: &str =
    "plane.bgtasks.user_deactivation_email_task.user_deactivation_email";
/// 38 — `send_email_update_magic_code`.
pub const SEND_EMAIL_UPDATE_MAGIC_CODE: &str =
    "plane.bgtasks.user_email_update_task.send_email_update_magic_code";
/// 39 — `send_email_update_confirmation`.
pub const SEND_EMAIL_UPDATE_CONFIRMATION: &str =
    "plane.bgtasks.user_email_update_task.send_email_update_confirmation";
/// 45 — `workspace_invitation`.
pub const WORKSPACE_INVITATION: &str =
    "plane.bgtasks.workspace_invitation_task.workspace_invitation";

pub const TASKS: &[&str] = &[
    FORGOT_PASSWORD,
    MAGIC_LINK,
    PROJECT_ADD_USER_EMAIL,
    USER_ACTIVATION_EMAIL,
    USER_DEACTIVATION_EMAIL,
    SEND_EMAIL_UPDATE_MAGIC_CODE,
    SEND_EMAIL_UPDATE_CONFIRMATION,
    WORKSPACE_INVITATION,
];

/// Register every handler.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    for task in TASKS {
        let context = Arc::clone(&context);
        registry.register(*task, move |job| {
            let context = Arc::clone(&context);
            Box::pin(async move { handle(&context, job).await })
        });
    }
}

async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    if let Err(error) = run(context, &job).await {
        // `except Exception as e: log_exception(e, warning=True); return`.
        swallow(error, job.task.as_str());
    }
    Ok(())
}

async fn run(context: &TaskContext, job: &JobContext) -> Result<(), String> {
    match job.task.as_str() {
        FORGOT_PASSWORD => forgot_password(context, &job.client, &job.payload).await,
        MAGIC_LINK => magic_link(context, &job.client, &job.payload).await,
        PROJECT_ADD_USER_EMAIL => project_add_user_email(context, &job.client, &job.payload).await,
        USER_ACTIVATION_EMAIL => user_name_email(context, &job.client, &job.payload, true).await,
        USER_DEACTIVATION_EMAIL => user_name_email(context, &job.client, &job.payload, false).await,
        SEND_EMAIL_UPDATE_MAGIC_CODE => {
            send_email_update_magic_code(context, &job.client, &job.payload).await
        }
        SEND_EMAIL_UPDATE_CONFIRMATION => {
            send_email_update_confirmation(context, &job.client, &job.payload).await
        }
        WORKSPACE_INVITATION => workspace_invitation(context, &job.client, &job.payload).await,
        other => Err(format!("{other} is not an e-mail task")),
    }
}

/// The `args` array, with each member as a string (a UUID argument included).
fn string_args(payload: &Value) -> Option<Vec<String>> {
    let items = args(payload)?;
    items
        .iter()
        .map(|item| match item {
            Value::String(text) => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Render, build and send one `EmailMultiAlternatives` with an HTML
/// alternative (`django/core/mail/message.py:454-545`).
async fn send_html(
    configuration: &EmailConfiguration,
    subject: String,
    template: &str,
    context: &Value,
    to: Vec<String>,
) -> Result<(), String> {
    let html = plane_mail::render(template, context).map_err(|error| error.to_string())?;
    let text = plane_mail::generate_plain_text_from_html(&html);
    let message = Message::with_html(subject, text, html, configuration.from.clone(), to);
    plane_mail::send(configuration, &message)
        .await
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// 17 — forgot_password
// ---------------------------------------------------------------------------

/// `plane/bgtasks/forgot_password_task.py:23-74`.
async fn forgot_password(
    context: &TaskContext,
    client: &Client,
    payload: &Value,
) -> Result<(), String> {
    let args = string_args(payload)
        .ok_or_else(|| "forgot_password expects five positional strings".to_string())?;
    let [first_name, email, uidb64, token, current_site] = args.as_slice() else {
        return Err("forgot_password expects five positional strings".to_string());
    };
    let relative_link =
        format!("/accounts/reset-password/?uidb64={uidb64}&token={token}&email={email}");
    let abs_url = format!("{current_site}{relative_link}");
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    send_html(
        &configuration,
        "A new password to your Plane account has been requested".to_string(),
        "emails/auth/forgot_password.html",
        &serde_json::json!({
            "first_name": first_name,
            "forgot_password_url": abs_url,
            "email": email,
        }),
        vec![email.clone()],
    )
    .await
}

// ---------------------------------------------------------------------------
// 28 — magic_link
// ---------------------------------------------------------------------------

/// `plane/bgtasks/magic_link_code_task.py:20-64`. `key` is not used by the body.
async fn magic_link(context: &TaskContext, client: &Client, payload: &Value) -> Result<(), String> {
    let args = string_args(payload)
        .ok_or_else(|| "magic_link expects three positional strings".to_string())?;
    let [email, _key, token] = args.as_slice() else {
        return Err("magic_link expects three positional strings".to_string());
    };
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    send_html(
        &configuration,
        format!("Your unique Plane login code is {token}"),
        "emails/auth/magic_signin.html",
        &serde_json::json!({"code": token, "email": email}),
        vec![email.clone()],
    )
    .await
}

// ---------------------------------------------------------------------------
// 36 / 37 — activation and deactivation
// ---------------------------------------------------------------------------

/// `user_activation_email_task.py:17-70` and
/// `user_deactivation_email_task.py:17-70`; the only differences are the
/// subject verb, the template and the URL.
async fn user_name_email(
    context: &TaskContext,
    client: &Client,
    payload: &Value,
    activation: bool,
) -> Result<(), String> {
    let Some(items) = args(payload) else {
        return Err("the task expects (current_site, user_id)".to_string());
    };
    let current_site =
        string_value(items.first()).ok_or_else(|| "current_site is not a string".to_string())?;
    let user_id = uuid_value(items.get(1)).ok_or_else(|| "user_id is not a UUID".to_string())?;
    let user = load_user(client, user_id)
        .await?
        .ok_or_else(|| "User matching query does not exist.".to_string())?;
    let display = python_or3(&user.first_name, &user.display_name, user.email.as_deref())
        .unwrap_or_else(|| "None".to_string());
    let (verb, template, field) = if activation {
        (
            "activated",
            "emails/user/user_activation.html",
            "profile_url",
        )
    } else {
        (
            "deactivated",
            "emails/user/user_deactivation.html",
            "login_url",
        )
    };
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    let context_value = if activation {
        serde_json::json!({
            "email": user.email.as_deref().unwrap_or("None"),
            (field): format!("{current_site}/profile"),
        })
    } else {
        serde_json::json!({
            "email": user.email.as_deref().unwrap_or("None"),
            (field): format!("{current_site}/login"),
        })
    };
    let to = user
        .email
        .clone()
        .ok_or_else(|| "recipient address is None".to_string())?;
    send_html(
        &configuration,
        format!("{display} has been {verb} on Plane"),
        template,
        &context_value,
        vec![to],
    )
    .await
}

/// Python's `a or b or c`: the first truthy value (an empty string is falsy).
fn python_or3(a: &str, b: &str, c: Option<&str>) -> Option<String> {
    if !a.is_empty() {
        return Some(a.to_string());
    }
    if !b.is_empty() {
        return Some(b.to_string());
    }
    c.map(str::to_string)
}

async fn load_user(client: &Client, id: Uuid) -> Result<Option<User>, String> {
    let columns = with_alias(User::COLUMNS, "u");
    let row = client
        .query_opt(
            &format!("SELECT {columns} FROM users u WHERE u.id = $1"),
            &[&id],
        )
        .await
        .map_err(|error| error.to_string())?;
    row.map(|row| User::from_row(&row, 0).map_err(|error| error.to_string()))
        .transpose()
}

// ---------------------------------------------------------------------------
// 38 / 39 — e-mail change
// ---------------------------------------------------------------------------

/// `plane/bgtasks/user_email_update_task.py:16-58`.
async fn send_email_update_magic_code(
    context: &TaskContext,
    client: &Client,
    payload: &Value,
) -> Result<(), String> {
    let args = string_args(payload)
        .ok_or_else(|| "send_email_update_magic_code expects (email, token)".to_string())?;
    let [email, token] = args.as_slice() else {
        return Err("send_email_update_magic_code expects (email, token)".to_string());
    };
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    send_html(
        &configuration,
        "Verify your new email address".to_string(),
        "emails/auth/magic_signin.html",
        &serde_json::json!({"code": token, "email": email}),
        vec![email.clone()],
    )
    .await
}

/// `plane/bgtasks/user_email_update_task.py:61-105`.
async fn send_email_update_confirmation(
    context: &TaskContext,
    client: &Client,
    payload: &Value,
) -> Result<(), String> {
    let args = string_args(payload)
        .ok_or_else(|| "send_email_update_confirmation expects one positional email".to_string())?;
    let [email] = args.as_slice() else {
        return Err("send_email_update_confirmation expects one positional email".to_string());
    };
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    send_html(
        &configuration,
        "Plane email address successfully updated".to_string(),
        "emails/user/email_updated.html",
        &serde_json::json!({"email": email}),
        vec![email.clone()],
    )
    .await
}

// ---------------------------------------------------------------------------
// 32 — project_add_user_email
// ---------------------------------------------------------------------------

/// `plane/bgtasks/project_add_user_email_task.py:22-85`.
async fn project_add_user_email(
    context: &TaskContext,
    client: &Client,
    payload: &Value,
) -> Result<(), String> {
    let Some(items) = args(payload) else {
        return Err("project_add_user_email expects (current_site, member, invitor)".to_string());
    };
    let current_site =
        string_value(items.first()).ok_or_else(|| "current_site is not a string".to_string())?;
    let member_id =
        uuid_value(items.get(1)).ok_or_else(|| "project_member_id is not a UUID".to_string())?;
    let invitor_id =
        uuid_value(items.get(2)).ok_or_else(|| "invitor_id is not a UUID".to_string())?;

    let invitor = load_user(client, invitor_id)
        .await?
        .ok_or_else(|| "User matching query does not exist.".to_string())?;
    let columns = with_alias(ProjectMember::COLUMNS, "pm");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns}, p.name, w.name, w.slug, u.email \
                 FROM project_members pm \
                 JOIN projects p ON p.id = pm.project_id AND p.deleted_at IS NULL \
                 JOIN workspaces w ON w.id = pm.workspace_id AND w.deleted_at IS NULL \
                 JOIN users u ON u.id = pm.member_id \
                 WHERE pm.id = $1 AND pm.deleted_at IS NULL"
            ),
            &[&member_id],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "ProjectMember matching query does not exist.".to_string())?;
    let member = ProjectMember::from_row(&row, 0).map_err(|error| error.to_string())?;
    let start = ProjectMember::COLUMN_COUNT;
    let project_name: String = row.get(start);
    let workspace_name: String = row.get(start + 1);
    let workspace_slug: String = row.get(start + 2);
    let member_email: Option<String> = row.get(start + 3);
    let member_email = member_email.ok_or_else(|| "recipient address is None".to_string())?;
    let project_url = format!(
        "{current_site}/{workspace_slug}/projects/{}/issues",
        member.project_id
    );
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    send_html(
        &configuration,
        "You have been invited to a Plane project".to_string(),
        "emails/notifications/project_addition.html",
        &serde_json::json!({
            "project_name": project_name,
            "workspace_name": workspace_name,
            "email": member_email,
            "inviter_first_name": invitor.first_name,
            "project_url": project_url,
        }),
        vec![member_email],
    )
    .await
}

// ---------------------------------------------------------------------------
// 45 — workspace_invitation
// ---------------------------------------------------------------------------

/// `plane/bgtasks/workspace_invitation_task.py:19-88`.
async fn workspace_invitation(
    context: &TaskContext,
    client: &Client,
    payload: &Value,
) -> Result<(), String> {
    let Some(items) = args(payload) else {
        return Err("workspace_invitation expects five positional arguments".to_string());
    };
    let email = string_value(items.first()).ok_or_else(|| "email is not a string".to_string())?;
    let workspace_id =
        uuid_value(items.get(1)).ok_or_else(|| "workspace_id is not a UUID".to_string())?;
    let token = string_value(items.get(2)).ok_or_else(|| "token is not a string".to_string())?;
    let current_site =
        string_value(items.get(3)).ok_or_else(|| "current_site is not a string".to_string())?;
    let inviter =
        string_value(items.get(4)).ok_or_else(|| "inviter is not a string".to_string())?;

    let inviter_user = client
        .query_opt(
            &format!(
                "SELECT {columns} FROM users u WHERE u.email = $1 ORDER BY u.id LIMIT 2",
                columns = with_alias(User::COLUMNS, "u")
            ),
            &[&inviter],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "User matching query does not exist.".to_string())?;
    let inviter_user = User::from_row(&inviter_user, 0).map_err(|error| error.to_string())?;
    let columns = with_alias(Workspace::COLUMNS, "w");
    let row = client
        .query_opt(
            &format!("SELECT {columns} FROM workspaces w WHERE w.id = $1 AND w.deleted_at IS NULL"),
            &[&workspace_id],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Workspace matching query does not exist.".to_string())?;
    let workspace = Workspace::from_row(&row, 0).map_err(|error| error.to_string())?;
    let columns = with_alias(WorkspaceMemberInvite::COLUMNS, "i");
    let row = client
        .query_opt(
            &format!(
                "SELECT {columns} FROM workspace_member_invites i \
                 WHERE i.token = $1 AND i.email = $2 AND i.deleted_at IS NULL"
            ),
            &[&token, &email],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "WorkspaceMemberInvite matching query does not exist.".to_string())?;
    let invite = WorkspaceMemberInvite::from_row(&row, 0).map_err(|error| error.to_string())?;

    let abs_url = format!(
        "{current_site}/workspace-invitations/?invitation_id={}&slug={}&token={token}",
        invite.id, workspace.slug
    );
    let configuration = context
        .email_configuration(client)
        .await
        .map_err(|error| error.to_string())?;
    let display = python_or3(
        &inviter_user.first_name,
        &inviter_user.display_name,
        inviter_user.email.as_deref(),
    )
    .unwrap_or_else(|| "None".to_string());
    let html = plane_mail::render(
        "emails/invitations/workspace_invitation.html",
        &serde_json::json!({
            "email": email,
            "first_name": display,
            "workspace_name": workspace.name,
            "abs_url": abs_url,
        }),
    )
    .map_err(|error| error.to_string())?;
    let text = plane_mail::generate_plain_text_from_html(&html);
    // `workspace_member_invite.message = text_content; workspace_member_invite.save()`:
    // a bare save writes every column, moves `updated_at` and nulls the audit
    // users (`plane/db/models/base.py:23-50`).
    client
        .execute(
            "UPDATE workspace_member_invites \
             SET message = $2, updated_at = now(), created_by_id = NULL, \
                 updated_by_id = NULL \
             WHERE id = $1",
            &[&invite.id, &text],
        )
        .await
        .map_err(|error| error.to_string())?;
    let message = Message::with_html(
        format!(
            "{display} has invited you to join them in {} on Plane",
            workspace.name
        ),
        text,
        html,
        configuration.from.clone(),
        vec![email],
    );
    plane_mail::send(&configuration, &message)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_or3_falls_through_empty_values() {
        assert_eq!(
            python_or3("Ada", "Ada A", Some("a@b.c")).as_deref(),
            Some("Ada")
        );
        assert_eq!(
            python_or3("", "Ada A", Some("a@b.c")).as_deref(),
            Some("Ada A")
        );
        assert_eq!(python_or3("", "", Some("a@b.c")).as_deref(), Some("a@b.c"));
        assert_eq!(python_or3("", "", None), None);
    }
}
