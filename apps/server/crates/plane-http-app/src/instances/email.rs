// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/api/views/configuration.py:80-171` — route 270,
//! `POST /api/instances/email-credentials-check/`.
//!
//! The whole endpoint is one branch table over `smtplib`'s exceptions
//! (`configuration.py:122-171`). Three of its inputs are read **outside** the
//! `try` and are therefore Django 500s, not 400s:
//!
//! * `get_email_configuration()` — a database failure;
//! * `int(EMAIL_PORT)` — a non-numeric configuration value (`:110`);
//! * `get_connection(...)` — `EMAIL_USE_TLS` and `EMAIL_USE_SSL` both set is
//!   `ValueError` in `EmailBackend.__init__` (`django/core/mail/backends/
//!   smtp.py:47-51`).
//!
//! `host or settings.EMAIL_HOST` is the backend's rule (`smtp.py:33`), so an
//! unset/empty `EMAIL_HOST` sends to `localhost`, not to "no host"; and
//! `port or settings.EMAIL_PORT` turns a port of `0` into 25 (`smtp.py:34`).
//!
//! Everything inside the `try` is [`super::mail`]'s typed [`MailError`];
//! [`MailError::endpoint_message`] is the exact 400 body. The session in
//! [`super::mail`] drives `smtplib`'s commands one by one, so each exception
//! class (`SMTPSenderRefused`, `SMTPRecipientsRefused`, `SMTPDataError`, …) is
//! a separate branch here.

use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;

use super::mail::{EmailConfiguration, MailError, Message, send};
use crate::common::client;

/// Register the route this module owns.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.license.api.views.configuration.EmailCredentialCheckEndpoint")?
        .method(http::Method::POST, post)?;
    Ok(())
}

/// `EmailCredentialCheckEndpoint.post` (`configuration.py:80-171`).
fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let receiver = crate::instances::body_value(ctx, "receiver_email")?;
        let receiver = receiver
            .as_ref()
            .filter(|value| truthy(value))
            .map(python_str)
            .unwrap_or_default();
        if receiver.is_empty() {
            return Ok(error_body(400, "Receiver email is required"));
        }

        let client = client(ctx)?;
        // Outside the view's `try`: a failure here is Django's 500.
        let configuration = EmailConfiguration::load(client, ctx.settings)
            .await
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        if configuration.use_tls && configuration.use_ssl {
            // `EmailBackend.__init__` (`smtp.py:47-51`), also outside the try.
            return Err(ViewError::Unknown(
                "ValueError: EMAIL_USE_TLS/EMAIL_USE_SSL are mutually exclusive, so only set \
                 one of those settings to True."
                    .to_string(),
            ));
        }
        // `host or settings.EMAIL_HOST` / `port or settings.EMAIL_PORT`.
        let configuration = EmailConfiguration {
            host: Some(
                configuration
                    .host
                    .filter(|host| !host.is_empty())
                    .unwrap_or_else(|| "localhost".to_string()),
            ),
            port: if configuration.port == 0 {
                25
            } else {
                configuration.port
            },
            ..configuration
        };

        let message = Message::email_credentials_check(&receiver, &configuration.from);
        match send(&configuration, &message).await {
            Ok(()) => Ok(json_message(200, "message", "Email successfully sent.")),
            Err(MailError::Generic(detail))
                if detail.starts_with("EMAIL_HOST is not configured") =>
            {
                // Unreachable after the `localhost` normalisation above; kept
                // so a future mail.rs change cannot silently become a 400.
                Err(ViewError::Unknown(detail))
            }
            Err(error) => match error.endpoint_message() {
                Some(message) => Ok(error_body(400, message)),
                None => Err(ViewError::Unknown(error.to_string())),
            },
        }
    })
}

/// `Response({"message": "Email successfully sent."})`.
fn json_message(status: u16, key: &str, value: &str) -> ViewResponse {
    let body = Object::new().field(key, value);
    let bytes = plane_serde::to_vec(Dialect::Drf, &body).unwrap_or_default();
    ViewResponse::raw(status, APPLICATION_JSON, bytes)
}

/// `Response({"error": …})`, Python's single key.
fn error_body(status: u16, message: &str) -> ViewResponse {
    let body = Object::new().field("error", message);
    let bytes = plane_serde::to_vec(Dialect::Drf, &body).unwrap_or_default();
    ViewResponse::raw(status, APPLICATION_JSON, bytes)
}

/// Python truthiness for the JSON values `request.data` can hold.
fn truthy(value: &plane_serde::pyjson::Value) -> bool {
    use plane_serde::pyjson::Value as Json;
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => value.as_i64().is_some_and(|value| value != 0),
        Json::Float(value) => *value != 0.0,
        Json::Str(value) => !value.is_empty(),
        Json::List(items) => !items.is_empty(),
        Json::Dict(dict) => !dict.is_empty(),
    }
}

/// Python's `str()` for the body value Django puts in `to=[receiver_email]`.
fn python_str(value: &plane_serde::pyjson::Value) -> String {
    use plane_serde::pyjson::Value as Json;
    match value {
        Json::Null => "None".to_string(),
        Json::Bool(true) => "True".to_string(),
        Json::Bool(false) => "False".to_string(),
        Json::Int(value) => value.to_string(),
        Json::Float(value) => value.to_string(),
        Json::Str(text) => text.to_string_lossy().into_owned(),
        other => format!("{other:?}"),
    }
}
