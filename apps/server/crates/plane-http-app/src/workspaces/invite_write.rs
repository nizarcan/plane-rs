// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/workspace/invite.py`'s write actions — workspace-core set B.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 188 `POST …/invitations/` | `POST` | `WorkspaceInvitationsViewset.create` (`invite.py:52-128`) | [`create`] |
//! | 189 `PATCH …/invitations/{pk}/` | `PATCH` | `.partial_update` (`invite.py:130-151`) | [`partial_update`] |
//! | 189 `DELETE …/invitations/{pk}/` | `DELETE` | `.destroy` (`invite.py:153-156`) | [`destroy`] |
//! | 191 `POST …/invitations/{pk}/join/` | `POST` | `WorkspaceJoinEndpoint.post` (`invite.py:172-248`) | [`join::post`] |
//!
//! ## 188 — one invitation per requested address
//!
//! `create` reads the body raw, not through the viewset serializer
//! (`invite.py:53-114`). In order: `emails` truthiness → the caller's active
//! membership → a `int(role)` cap against the caller's own role → the workspace
//! lookup (`Workspace.objects`, so a soft-deleted one is the family 404) → the
//! "already a member" pre-check (`member__email__in`, case-**sensitive**) →
//! per-address `validate_email` and a JWT → **one** `bulk_create` →
//! `base_host` → one `workspace_invitation` publish per input object.
//!
//! Three quirks are ported rather than fixed:
//!
//! * the JWT payload embeds the **raw body element**, not the validated
//!   address (`invite.py:95-99`), so key order and unknown members change the
//!   bytes;
//! * `bulk_create(..., ignore_conflicts=True)` returns every input object,
//!   including one whose `(email, workspace)` row already existed, and the loop
//!   publishes a task for each (`invite.py:112-126`); the worker then misses
//!   its `objects.get(token=…)` and returns silently
//!   (`workspace_invitation_task.py:28,85-86`);
//! * the invalid-address message interpolates the whole raw element with
//!   Python's `repr` (`invite.py:104-110`), so a dict renders with single
//!   quotes and `None`.
//!
//! `bulk_create` is one `transaction.atomic(savepoint=False)`
//! (`django/db/models/query.py`), so the handler opens one too: a token over
//! the column's 255 characters fails the INSERT with **no** invitation written.
//!
//! ## 189 — role only
//!
//! `partial_update` rejects any body whose key set is not exactly `{"role"}`
//! before it looks the invite up (`invite.py:132-136`), caps the new role at the
//! caller's own (`:143-148`) and answers `serializer.data` (14 keys plus the
//! nested workspace and `invite_link`). `destroy` is an H03 soft delete
//! followed by the cascade publish with the model name
//! `workspacememberinvite`.
//!
//! ## 191 — the accept/reject endpoint
//!
//! The four `@invalidate_cache` decorators run before anything else, in the
//! listed order (outermost first). The view itself checks, in order: the token
//! (`invite.py:175-182`), authentication (`:188-192`, **after** the token), the
//! invitee's address (`:193-197`), an already-answered invite (`:200-248`).
//!
//! `accepted` is assigned raw and tested for Python truthiness (`:201,205`), so
//! a form `accepted=false` is the truthy string `"false"`: the membership is
//! created while the stored `accepted` is `False`. The stored column goes
//! through Django's `BooleanField` coercion, which raises `ValidationError` for
//! anything but a boolean, `t`/`True`/`1`, `f`/`False`/`0` — a 400.
//!
//! If the invitee's address is found by the **case-sensitive**
//! `User.objects.filter(email=…)` (`:207`) — the pre-check above is
//! case-insensitive — the membership is activated or created, `user.save()`
//! runs H30, and the invitation is soft-deleted. When the lookup misses, the
//! view still answers 200 "Accepted" and leaves the invitation untouched.
//! `user.last_workspace_id = …` (`:228`) is a no-op: the field lives on
//! `Profile` (`plane/db/models/user.py:236`).

use chrono::DateTime;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks::audit::Audit;
use plane_db::models::workspace::WorkspaceMember;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_serializers::generated::app_workspace::WorkSpaceMemberSerializer;
use uuid::Uuid;

use crate::common::{cache, client, lookup};
use crate::views::write_support::{Body, body_get};
use crate::workspaces::invite::InviteRecord;
use crate::workspaces::queries;
use crate::workspaces::rows::{self, UserRow};
use crate::workspaces::write_b_queries as queries_b;
use crate::workspaces::write_queries as shared_queries;
use crate::write::{self, Serializer};

/// `plane.app.views.workspace.invite.WorkspaceInvitationsViewset`.
const VIEW: &str = "plane.app.views.workspace.invite.WorkspaceInvitationsViewset";

/// `plane.app.views.workspace.invite.WorkspaceJoinEndpoint`.
const JOIN_VIEW: &str = "plane.app.views.workspace.invite.WorkspaceJoinEndpoint";

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset(VIEW)?
        .action("create", create)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    registry
        .api_view(JOIN_VIEW)?
        .method(http::Method::POST, join::post)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// `Response({"error": …}, status)` / `Response({"message": …}, status)`.
fn message_body(status: u16, key: &str, message: &str) -> Result<ViewResponse, ViewError> {
    let body = plane_serde::to_vec(
        plane_serde::Dialect::Drf,
        &plane_core::json::Object::new().field(key, message.to_owned()),
    )?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// `Response({"error": …})` as a `Result`, like `projects::member_write::fail`.
fn fail(status: u16, message: &str) -> Result<ViewResponse, ViewError> {
    Ok(write::error_body(status, message))
}

/// Python truthiness of a raw body value (`bool(value)`).
fn body_truthy(value: &Body<'_>) -> bool {
    match value {
        Body::Missing => false,
        Body::Form(text) => !text.is_empty(),
        Body::Json(Json::Null) => false,
        Body::Json(Json::Bool(flag)) => *flag,
        Body::Json(Json::Int(number)) => number.as_i64().is_some_and(|value| value != 0),
        Body::Json(Json::Float(number)) => *number != 0.0,
        Body::Json(Json::Str(text)) => text.as_str().is_some_and(|text| !text.is_empty()),
        Body::Json(Json::List(items)) => !items.is_empty(),
        Body::Json(Json::Dict(entries)) => !entries.is_empty(),
    }
}

/// `type(value).__name__` for an `AttributeError`/`TypeError` message.
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

/// `int(value)` (`member.py`'s role cap and `IntegerField.get_prep_value`).
///
/// A value Python's `int()` refuses is a 500 — the view has no `except`
/// around the list comprehension (`invite.py:62`).
fn python_int(value: &Json) -> Result<i64, ViewError> {
    match value {
        Json::Int(value) => value
            .as_i64()
            .ok_or_else(|| ViewError::Unknown("int() overflow".to_string())),
        Json::Bool(value) => Ok(i64::from(*value)),
        Json::Float(value) => Ok(*value as i64),
        Json::Str(value) => value
            .as_str()
            .and_then(|text| text.trim().parse::<i64>().ok())
            .ok_or_else(|| {
                ViewError::Unknown(format!("ValueError: invalid literal for int(): {value:?}"))
            }),
        Json::Null | Json::List(_) | Json::Dict(_) => Err(ViewError::Unknown(
            "TypeError: int() argument must be a string, bytes-like object or a real number"
                .to_string(),
        )),
    }
}

/// Django's `CharField.get_prep_value` for an `__in` element (`str(value)`).
///
/// `None` stays SQL `NULL` (an `IN (NULL)` matches nothing); everything else
/// goes through Python's `str()`, which is `repr()` for the containers.
fn char_prep(value: &Json) -> Option<String> {
    match value {
        Json::Null => None,
        Json::Str(text) => text.as_str().map(str::to_owned),
        other => Some(write::repr::python_repr(other)),
    }
}

/// `str.strip()` for the character classes CPython's `str.isspace()` accepts.
fn python_strip(value: &str) -> &str {
    value.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '\u{1c}'..='\u{1f}')
    })
}

/// `django.core.validators.validate_email(value)` as a **tri-state**: `Ok(true)`
/// valid, `Ok(false)` a `ValidationError` (the view's 400), `Err` the
/// `TypeError`/`AttributeError` inside the validator (the family 500).
///
/// `EmailValidator.__call__` (`django/core/validators.py:255-273`) starts with
/// `if not value or "@" not in value or len(value) > 320`, which for a non-string
/// is a container test, a `TypeError` (numbers are not iterable) or a
/// `AttributeError` at `value.rsplit` — never a `ValidationError`.
fn django_validate_email(value: &Json) -> Result<bool, ViewError> {
    match value {
        Json::Str(text) => Ok(write::email::validate_email(text.as_str().unwrap_or(""))),
        Json::Null => Ok(false),
        Json::Bool(false) => Ok(false),
        Json::Bool(true) => Err(ViewError::Unknown(
            "TypeError: argument of type 'bool' is not iterable".to_string(),
        )),
        Json::Int(number) => {
            if number.as_i64().is_some_and(|value| value == 0) {
                Ok(false)
            } else {
                Err(ViewError::Unknown(
                    "TypeError: argument of type 'int' is not iterable".to_string(),
                ))
            }
        }
        Json::Float(number) => {
            if *number == 0.0 {
                Ok(false)
            } else {
                Err(ViewError::Unknown(
                    "TypeError: argument of type 'float' is not iterable".to_string(),
                ))
            }
        }
        // `"@" in value` then `len(value) > 320`, then `value.rsplit("@", 1)`.
        Json::List(items) => {
            if items.is_empty()
                || !items
                    .iter()
                    .any(|item| matches!(item, Json::Str(text) if text.as_str() == Some("@")))
                || items.len() > 320
            {
                return Ok(false);
            }
            Err(ViewError::Unknown(
                "AttributeError: 'list' object has no attribute 'rsplit'".to_string(),
            ))
        }
        Json::Dict(entries) => {
            if entries.is_empty() || !entries.contains_key("@") || entries.len() > 320 {
                return Ok(false);
            }
            Err(ViewError::Unknown(
                "AttributeError: 'dict' object has no attribute 'rsplit'".to_string(),
            ))
        }
    }
}

/// Django's `BooleanField.get_prep_value` (`django/db/models/fields/__init__.py`).
///
/// `Ok(None)` is the SQL `NULL` an explicit `null` produces (a `NOT NULL`
/// column, so the family 400 "The payload is not valid"); `Err(Validation)` is
/// `ValidationError` for a value that is neither a boolean nor one of
/// `t`/`True`/`1`/`f`/`False`/`0` (the family 400 "Please provide valid
/// detail").
fn django_boolean(value: &Json) -> Result<Option<bool>, ViewError> {
    match value {
        Json::Null => Ok(None),
        Json::Bool(flag) => Ok(Some(*flag)),
        Json::Int(number) => match number.as_i64() {
            Some(1) => Ok(Some(true)),
            Some(0) => Ok(Some(false)),
            _ => Err(ViewError::Validation),
        },
        Json::Float(number) if *number == 1.0 => Ok(Some(true)),
        Json::Float(number) if *number == 0.0 => Ok(Some(false)),
        Json::Str(text) => match text.as_str() {
            Some("t" | "True" | "1") => Ok(Some(true)),
            Some("f" | "False" | "0") => Ok(Some(false)),
            _ => Err(ViewError::Validation),
        },
        Json::Float(_) | Json::List(_) | Json::Dict(_) => Err(ViewError::Validation),
    }
}

/// `plane_auth::Json` for the JWT payload.
///
/// The payload is `{"email": <raw body element>, "timestamp": <float>}`
/// (`plane/app/views/workspace/invite.py:95-99`) and the token is normalised by
/// shape in the differential harness, but the two documents must still agree on
/// everything an HS256 signature covers. A Python `int` that does not fit
/// `i64` cannot be represented in `plane_auth::Json`; it is not reachable from
/// an invitation body (the fixture's addresses are strings) and would only
/// change the token bytes, which are a placeholder.
fn auth_json(value: &Json) -> plane_auth::Json {
    use plane_auth::Json as Auth;
    match value {
        Json::Null => Auth::Null,
        Json::Bool(flag) => Auth::Bool(*flag),
        Json::Int(number) => Auth::Int(number.as_i64().unwrap_or_default()),
        Json::Float(number) => Auth::Float(*number),
        Json::Str(text) => Auth::Str(text.as_str().unwrap_or_default().to_owned()),
        Json::List(items) => Auth::Array(items.iter().map(auth_json).collect()),
        Json::Dict(entries) => Auth::Object(
            entries
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str().unwrap_or_default().to_owned(),
                        auth_json(value),
                    )
                })
                .collect(),
        ),
    }
}

/// `datetime.now().timestamp()`: local wall-clock seconds as a float.
///
/// `datetime.now()` is naive local time, so the value tracks the host's zone —
/// the token is normalised by shape, and this keeps PyJWT's payload shape.
fn local_timestamp() -> f64 {
    let now: DateTime<chrono::Local> = chrono::Local::now();
    now.timestamp_micros() as f64 / 1_000_000.0
}

/// `jwt.encode({"email": email, "timestamp": …}, settings.SECRET_KEY,
/// algorithm="HS256")` (`invite.py:95-99`).
fn invite_token(raw: &Json, secret: &str) -> String {
    let payload = plane_auth::jwt::invitation_payload(&auth_json(raw), local_timestamp());
    plane_auth::jwt::encode_hs256_value(&payload, secret)
}

/// `request.data.get("emails")` as the JSON values the comprehension iterates.
///
/// A form body is a string, and iterating it yields one-character strings with
/// no `.get` (a 500); a bare number is not iterable (a 500); a JSON object
/// iterates its keys, which are strings (a 500).
fn email_items<'a>(value: &'a Body<'a>) -> Result<Vec<&'a Json>, ViewError> {
    match value {
        Body::Json(Json::List(items)) => Ok(items.iter().collect()),
        Body::Json(Json::Dict(_)) | Body::Json(Json::Str(_)) | Body::Form(_) => Err(
            ViewError::Unknown("AttributeError: 'str' object has no attribute 'get'".to_string()),
        ),
        Body::Json(Json::Bool(_)) => Err(ViewError::Unknown(
            "TypeError: 'bool' object is not iterable".to_string(),
        )),
        Body::Json(Json::Int(_)) => Err(ViewError::Unknown(
            "TypeError: 'int' object is not iterable".to_string(),
        )),
        Body::Json(Json::Float(_)) => Err(ViewError::Unknown(
            "TypeError: 'float' object is not iterable".to_string(),
        )),
        // Falsy, so the view answered 400 before this point.
        Body::Json(Json::Null) | Body::Missing => Ok(Vec::new()),
    }
}

/// `email.get("role", 5)` and Python's `int()` for every element, in order.
fn email_roles(items: &[&Json]) -> Result<Vec<i64>, ViewError> {
    let mut roles = Vec::with_capacity(items.len());
    for item in items {
        let Some(entry) = item.as_dict() else {
            return Err(ViewError::Unknown(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                python_type_name(item)
            )));
        };
        let default = Json::from(5i64);
        let raw = entry.get("role").unwrap_or(&default);
        roles.push(python_int(raw)?);
    }
    Ok(roles)
}

// ---------------------------------------------------------------------------
// 188 — create
// ---------------------------------------------------------------------------

/// One `WorkspaceMemberInvite` the create loop built in memory.
///
/// `role` stays the `int()` value: Django does not coerce it until the INSERT
/// (`PositiveSmallIntegerField.get_prep_value`), so an out-of-range role fails
/// **after** every address was validated — and then rolls the whole
/// `bulk_create` back.
struct NewInvite {
    email: String,
    token: String,
    role: i64,
}

/// `WorkspaceInvitationsViewset.create` (`invite.py:52-128`).
fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let requester_email = ctx.user.row().and_then(|row| row.email.clone());
        let user = ctx
            .user
            .user()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let user_id = user.id;
        let client = client(ctx)?;
        let data = ctx.data()?;

        // `emails = request.data.get("emails", [])`; `if not emails: 400`
        // (`invite.py:53-56`).
        let emails_value = body_get(data, "emails")?;
        if !body_truthy(&emails_value) {
            return fail(400, "Emails are required");
        }
        // `WorkspaceMember.objects.get(workspace__slug=slug, member=request.user,
        // is_active=True)` (`invite.py:59`) runs **before** the comprehension
        // that iterates `emails`, so a missing membership outranks a
        // non-iterable body.
        let requesting_role = {
            let statement = shared_queries::WORKSPACE_MEMBER_ACTIVE
                .prepare(client)
                .await?;
            let rows = client.query(&statement, &[&user_id, &slug]).await?;
            match rows.len() {
                0 => return Err(ViewError::ObjectDoesNotExist),
                1 => rows[0].get::<_, i16>(3),
                _ => return Err(rows::multiple_objects("WorkspaceMember")),
            }
        };

        // The cap comprehension (`invite.py:62-66`).
        let items = email_items(&emails_value)?;
        let roles = email_roles(&items)?;
        if roles.iter().any(|role| *role > i64::from(requesting_role)) {
            return fail(400, "You cannot invite a user with higher role");
        }

        // `Workspace.objects.get(slug=slug)` (`invite.py:69`).
        let workspace_id = {
            let statement = queries_b::WORKSPACE_ID_BY_SLUG.prepare(client).await?;
            let rows = client.query(&statement, &[&slug]).await?;
            match rows.len() {
                0 => return Err(ViewError::ObjectDoesNotExist),
                1 => rows[0].get::<_, Uuid>(0),
                _ => return Err(rows::multiple_objects("Workspace")),
            }
        };

        // `WorkspaceMember.objects.filter(workspace_id=…,
        // member__email__in=[email.get("email") …], is_active=True)
        // .select_related("member", "member__avatar_asset")` (`invite.py:72-85`).
        let emails: Vec<String> = {
            let mut values = Vec::with_capacity(items.len());
            for item in &items {
                let Some(entry) = item.as_dict() else {
                    return Err(ViewError::Unknown(format!(
                        "AttributeError: '{}' object has no attribute 'get'",
                        python_type_name(item)
                    )));
                };
                values.extend(entry.get("email").and_then(char_prep));
            }
            values
        };

        if !emails.is_empty() {
            let statement = queries_b::MEMBERS_BY_EMAIL.prepare(client).await?;
            let rows = client.query(&statement, &[&workspace_id, &emails]).await?;
            if !rows.is_empty() {
                let mut members = Vec::with_capacity(rows.len());
                let mut users = Vec::with_capacity(rows.len());
                for row in &rows {
                    members.push(WorkspaceMember::from_row(row, 0)?);
                    users.push(UserRow::from_row(row, 17)?);
                }
                let lites: Vec<_> = users.iter().map(UserRow::lite).collect();
                let serializers: Vec<_> = members
                    .iter()
                    .zip(lites.iter())
                    .map(|(model, user)| member_serializer(model, user))
                    .collect();
                let rendered = rows::render_plain_list(ctx, &serializers)?;
                let mut body = Vec::with_capacity(rendered.len() + 96);
                body.extend_from_slice(
                    b"{\"error\":\"Some users are already member of workspace\",\"workspace_users\":",
                );
                body.extend_from_slice(&rendered);
                body.push(b'}');
                return Ok(ViewResponse::raw(400, APPLICATION_JSON, body));
            }
        }

        // The per-address loop (`invite.py:87-114`). `validate_email` and the
        // JWT run before the single `bulk_create`.
        let secret = ctx.settings.secret_key.value();
        let mut invitations = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let Some(entry) = item.as_dict() else {
                return Err(ViewError::Unknown(format!(
                    "AttributeError: '{}' object has no attribute 'get'",
                    python_type_name(item)
                )));
            };
            let missing = Json::Null;
            let email_value = entry.get("email").unwrap_or(&missing);
            if !django_validate_email(email_value)? {
                let message = format!(
                    "Invalid email - {} provided a valid email address is required to send the invite",
                    write::repr::python_repr(item)
                );
                return fail(400, &message);
            }
            // Valid means a string (see `django_validate_email`).
            let raw_email = email_value.as_str().unwrap_or_default();
            invitations.push(NewInvite {
                email: python_strip(raw_email).to_lowercase(),
                token: invite_token(item, secret),
                role: roles[index],
            });
        }

        // One `transaction.atomic(savepoint=False)` around every batch of the
        // `bulk_create` (`django/db/models/query.py:bulk_create`).
        let statement = queries_b::INVITE_INSERT.prepare(client).await?;
        client.batch_execute("BEGIN").await?;
        let mut inserted = Ok(());
        for invitation in &invitations {
            let now = write::now();
            // `PositiveSmallIntegerField.get_prep_value`: `int()` then the
            // column's range, a `DataError` inside the atomic.
            let Ok(role) = i16::try_from(invitation.role) else {
                inserted = Err(ViewError::Unknown(
                    "psycopg.errors.NumericValueOutOfRange: smallint out of range".to_string(),
                ));
                break;
            };
            if let Err(error) = client
                .execute(
                    &statement,
                    &[
                        &now,
                        &now,
                        &Some(user_id),
                        &Uuid::new_v4(),
                        &workspace_id,
                        &invitation.email,
                        &invitation.token,
                        &role,
                    ],
                )
                .await
            {
                inserted = Err(db_error(error));
                break;
            }
        }
        match inserted {
            Ok(()) => client.batch_execute("COMMIT").await?,
            Err(error) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(error);
            }
        }

        // `current_site = base_host(request=request, is_app=True)`
        // (`invite.py:116`) — **after** the insert, so a misconfigured instance
        // keeps the invitations and answers 500.
        let current_site = plane_auth::base_host(
            ctx.settings,
            plane_auth::LoginFlags {
                is_app: true,
                ..Default::default()
            },
        )
        .map_err(|error| ViewError::Unknown(error.to_string()))?;

        // One publish per input object, in request order (`invite.py:118-126`).
        let inviter = requester_email.unwrap_or_default();
        for invitation in &invitations {
            let message = plane_jobs::celery::tasks::workspace_invitation(
                invitation.email.clone(),
                workspace_id,
                invitation.token.clone(),
                current_site.clone(),
                inviter.clone(),
            );
            crate::common::jobs::dispatch_task(
                ctx,
                message.task(),
                &message,
                plane_jobs::queue::Job::new(message.task()),
            )
            .await?;
        }

        message_body(200, "message", "Emails sent successfully")
    })
}

/// Django's `IntegrityError` family (SQLSTATE class 23) is the family 400
/// "The payload is not valid"; everything else is the 500.
fn db_error(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// The 17-key `WorkSpaceMemberSerializer` the "already a member" body nests
/// (`plane/app/serializers/workspace.py:94-109`).
pub(crate) fn member_serializer<'a>(
    model: &'a WorkspaceMember,
    user: &'a plane_serializers::generated::app_user::UserLiteSerializer<'a>,
) -> WorkSpaceMemberSerializer<'a> {
    WorkSpaceMemberSerializer {
        id: model.id,
        member: Some(user),
        created_at: model.created_at,
        updated_at: model.updated_at,
        deleted_at: model.deleted_at,
        role: i64::from(model.role),
        company_role: model.company_role.as_deref(),
        view_props: Some(plane_serializers::fields::PgJson::from(&model.view_props)),
        default_props: Some(plane_serializers::fields::PgJson::from(
            &model.default_props,
        )),
        issue_props: Some(plane_serializers::fields::PgJson::from(&model.issue_props)),
        is_active: model.is_active,
        getting_started_checklist: Some(plane_serializers::fields::PgJson::from(
            &model.getting_started_checklist,
        )),
        tips: Some(plane_serializers::fields::PgJson::from(&model.tips)),
        explored_features: Some(plane_serializers::fields::PgJson::from(
            &model.explored_features,
        )),
        created_by: model.created_by_id,
        updated_by: model.updated_by_id,
        workspace: model.workspace_id,
    }
}

// ---------------------------------------------------------------------------
// 189 — partial_update / destroy
// ---------------------------------------------------------------------------

/// `set(request.data) == {"role"}` — the 189 and 194 body guards
/// (`invite.py:132`, `member.py:109`).
///
/// A `QueryDict` **is** a `dict`, so a form body passes; anything that is not a
/// mapping (a JSON list, `null`, a number) fails.
pub(crate) fn only_role(data: &Data) -> bool {
    match data {
        Data::Json(Json::Dict(entries)) => entries.len() == 1 && entries.contains_key("role"),
        Data::Form(query) => {
            // `set(request.data)`: a `QueryDict` may carry a key twice, and the
            // set collapses it (`MultiValueDict.__iter__` yields the keys).
            let mut names: Vec<&str> = query.keys().collect();
            names.sort_unstable();
            names.dedup();
            names == ["role"]
        }
        Data::Json(_) => false,
    }
}

/// `WorkspaceMember.objects.get(workspace__slug=slug, member=request.user,
/// is_active=True).role` (`invite.py:143`).
async fn requesting_role(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
) -> Result<i16, ViewError> {
    let statement = shared_queries::WORKSPACE_MEMBER_ACTIVE
        .prepare(client)
        .await?;
    let rows = client.query(&statement, &[&user, &slug]).await?;
    match rows.len() {
        0 => Err(ViewError::ObjectDoesNotExist),
        1 => Ok(rows[0].get(3)),
        _ => Err(rows::multiple_objects("WorkspaceMember")),
    }
}

/// `WorkspaceInvitationsViewset.partial_update` (`invite.py:130-151`).
fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let data = ctx.data()?;
        if !only_role(data) {
            return fail(400, "Only the role of an invitation can be updated");
        }

        let statement = queries::INVITE_BY_ID.prepare(client).await?;
        let Some(row) = client.query_opt(&statement, &[&slug, &pk]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let record = InviteRecord::from_row(&row)?;

        let mut serializer = Serializer::new(ctx, true)?;
        write::generated::work_space_member_invite_serializer::validate(&mut serializer, client)
            .await?;
        let role = match serializer.finish() {
            Ok(values) => values
                .integer("role")
                .ok_or_else(|| ViewError::Unknown("KeyError: 'role'".to_string()))?,
            Err(errors) => return Ok(errors.into_response()),
        };

        // The cap runs **after** validation, so an invalid role is the
        // serializer's 400 even for a caller who could not grant it
        // (`invite.py:139-148`).
        if role > i64::from(requesting_role(client, slug, user).await?) {
            return fail(400, "You cannot invite a user with higher role");
        }

        let now = write::now();
        let role: i16 = i16::try_from(role)
            .map_err(|_| ViewError::Unknown("smallint out of range".to_string()))?;
        let statement = queries_b::INVITE_UPDATE_ROLE.prepare(client).await?;
        client
            .execute(&statement, &[&role, &now, &Some(user), &record.model.id])
            .await
            .map_err(db_error)?;

        let mut model = record.model;
        model.role = role;
        model.updated_at = now;
        model.updated_by_id = Audit::on_update(Some(user));
        let updated = InviteRecord {
            model,
            workspace: record.workspace,
        };
        let workspace = updated.workspace.serializer();
        let body = rows::render_plain(ctx, &updated.serializer(&workspace))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceInvitationsViewset.destroy` (`invite.py:153-156`): H03 plus the
/// cascade publish with `_meta.model_name` (`workspacememberinvite`).
fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        let statement = queries::INVITE_BY_ID.prepare(client).await?;
        let Some(row) = client.query_opt(&statement, &[&slug, &pk]).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let record = InviteRecord::from_row(&row)?;

        let now = write::now();
        write::soft_delete(
            client,
            "workspace_member_invites",
            "id",
            record.model.id,
            Audit::on_update(Some(user)),
            now,
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "workspacememberinvite", record.model.id)
            .await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// 191 — join
// ---------------------------------------------------------------------------

/// `WorkspaceJoinEndpoint.post` (`invite.py:172-248`).
pub mod join {
    use super::*;

    /// The four `@invalidate_cache` decorators, outermost first
    /// (`invite.py:163-171`).
    async fn invalidate(ctx: &ViewCtx<'_>) -> Result<(), ViewError> {
        cache::invalidate_cache(ctx, Some("/api/workspaces/"), false, false, false).await?;
        cache::invalidate_cache(ctx, Some("/api/users/me/workspaces/"), false, true, true).await?;
        cache::invalidate_cache(
            ctx,
            Some("/api/workspaces/:slug/members/"),
            true,
            false,
            true,
        )
        .await?;
        cache::invalidate_cache(ctx, Some("/api/users/me/settings/"), false, true, true).await?;
        Ok(())
    }

    /// `WorkspaceJoinEndpoint.post` (`invite.py:172-248`).
    pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
        Box::pin(async move {
            invalidate(ctx).await?;

            let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
            let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
            let client = client(ctx)?;
            let data = ctx.data()?;

            // `WorkspaceMemberInvite.objects.get(pk=pk, workspace__slug=slug)`
            // (`invite.py:173`).
            let statement = queries::INVITE_BY_ID.prepare(client).await?;
            let rows = client.query(&statement, &[&slug, &pk]).await?;
            let record = match rows.len() {
                0 => return Err(ViewError::ObjectDoesNotExist),
                1 => InviteRecord::from_row(&rows[0])?,
                _ => return Err(rows::multiple_objects("WorkspaceMemberInvite")),
            };
            let invite = record.model;

            // The token check precedes authentication (`invite.py:175-182`).
            let token = body_get(data, "token")?;
            let token_matches = match &token {
                Body::Form(text) => !text.is_empty() && *text == invite.token.as_str(),
                Body::Json(Json::Str(value)) => value
                    .as_str()
                    .is_some_and(|text| !text.is_empty() && text == invite.token),
                Body::Missing | Body::Json(_) => false,
            };
            if !token_matches {
                return fail(403, "You do not have permission to join the workspace");
            }

            // `if not request.user.is_authenticated` after the token check
            // (`invite.py:188-192`).
            if !ctx.user.is_authenticated() {
                return fail(
                    401,
                    "Authentication required to accept workspace invitation",
                );
            }
            let Some(user) = ctx.user.user() else {
                return fail(
                    401,
                    "Authentication required to accept workspace invitation",
                );
            };
            let user_id = user.id;
            if user.email.to_lowercase() != invite.email.to_lowercase() {
                return fail(403, "You do not have permission to accept this invitation");
            }

            if invite.responded_at.is_some() {
                return fail(400, "You have already responded to the invitation request");
            }

            // `accepted = request.data.get("accepted", False)`; truthiness and
            // the column's own coercion are *different* (`invite.py:201-205`).
            let accepted_value = body_get(data, "accepted")?;
            let accepted = body_truthy(&accepted_value);
            let stored = match &accepted_value {
                Body::Missing => Some(false),
                Body::Form(text) => django_boolean(&Json::from(*text))?,
                Body::Json(value) => django_boolean(value)?,
            };
            let Some(stored) = stored else {
                // `NULL` in a `NOT NULL` column: IntegrityError.
                return Err(ViewError::Integrity);
            };

            let responded_at = write::now();
            let updated_at = write::now();
            let statement = queries_b::INVITE_ACCEPT_SAVE.prepare(client).await?;
            client
                .execute(
                    &statement,
                    &[
                        &stored,
                        &responded_at,
                        &updated_at,
                        &Some(user_id),
                        &invite.id,
                    ],
                )
                .await
                .map_err(db_error)?;

            if !accepted {
                return message_body(200, "message", "Workspace Invitation was not accepted");
            }

            // `User.objects.filter(email=workspace_invite.email).first()`
            // (`invite.py:207`).
            let statement = queries_b::USER_FOR_INVITE.prepare(client).await?;
            let invitee = client.query_opt(&statement, &[&invite.email]).await?;
            let Some(invitee) = invitee else {
                return message_body(200, "message", "Workspace Invitation Accepted");
            };
            let invitee_id: Uuid = invitee.get(0);
            let invitee_email: Option<String> = invitee.get(1);
            let display_name: String = invitee.get(2);
            let is_superuser: bool = invitee.get(3);
            let is_staff: bool = invitee.get(4);
            let token: String = invitee.get(5);
            let token_updated_at: Option<chrono::DateTime<chrono::Utc>> = invitee.get(6);

            // `WorkspaceMember.objects.filter(workspace=…, member=user).first()`
            // (`invite.py:212-225`).
            let statement = queries_b::MEMBER_BY_WORKSPACE_AND_USER
                .prepare(client)
                .await?;
            let membership = client
                .query_opt(&statement, &[&invite.workspace_id, &invitee_id])
                .await?;
            match membership {
                Some(row) => {
                    let member = WorkspaceMember::from_row(&row, 0)?;
                    let now = write::now();
                    let statement = queries_b::MEMBER_ACCEPT_SAVE.prepare(client).await?;
                    client
                        .execute(
                            &statement,
                            &[&now, &Some(user_id), &true, &invite.role, &member.id],
                        )
                        .await
                        .map_err(db_error)?;
                }
                None => {
                    let now = write::now();
                    let statement = crate::workspaces::write_a_queries::WORKSPACE_MEMBER_CREATE
                        .prepare(client)
                        .await?;
                    client
                        .execute(
                            &statement,
                            &[
                                &now,
                                &now,
                                &Uuid::new_v4(),
                                &invite.role,
                                &Some(user_id),
                                &invitee_id,
                                &invite.workspace_id,
                                &None::<String>,
                                &plane_db::JsonText::new(
                                    crate::projects::invite_write::WORKSPACE_DEFAULT_PROPS,
                                ),
                                &plane_db::JsonText::new(
                                    crate::projects::invite_write::WORKSPACE_DEFAULT_PROPS,
                                ),
                                &plane_db::JsonText::new(
                                    crate::projects::invite_write::WORKSPACE_ISSUE_PROPS,
                                ),
                            ],
                        )
                        .await
                        .map_err(db_error)?;
                }
            }

            // `user.last_workspace_id = …; user.save()` (`invite.py:227-229`):
            // the assignment is a no-op, H30 plus `auto_now` are the write.
            let hook_now = write::now();
            let prepared = plane_db::hooks::user::before_save(
                invitee_email.as_deref(),
                &display_name,
                is_superuser,
                is_staff,
                &token,
                token_updated_at,
                hook_now,
            )
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
            let statement = queries_b::USER_SAVE_FOR_INVITE.prepare(client).await?;
            client
                .execute(
                    &statement,
                    &[
                        &prepared.email,
                        &prepared.display_name,
                        &prepared.is_staff,
                        &prepared.token,
                        &prepared.token_updated_at,
                        &write::now(),
                        &invitee_id,
                    ],
                )
                .await
                .map_err(db_error)?;

            // `workspace_invite.delete()` (`invite.py:232`).
            let now = write::now();
            write::soft_delete(
                client,
                "workspace_member_invites",
                "id",
                invite.id,
                Audit::on_update(Some(user_id)),
                now,
            )
            .await?;
            write::publish_soft_delete_cascade(ctx, "db", "workspacememberinvite", invite.id)
                .await?;

            message_body(200, "message", "Workspace Invitation Accepted")
        })
    }
}
