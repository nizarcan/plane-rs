// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/logger_task.py` — the `process_logs` task.
//!
//! `APITokenLogMiddleware` publishes one `log_data` dict per `X-Api-Key`
//! request (`plane/middleware/logger.py:113-158`; the row is built by
//! `plane-drf`'s `token_log`, layer 11). The task body is
//! `log_to_postgres` (`logger_task.py:20-29`):
//!
//! ```python
//! try:
//!     APIActivityLog.objects.create(**log_data)
//!     return True
//! except Exception as e:
//!     log_exception(e)
//!     return False
//! ```
//!
//! Every failure — an unknown keyword (`TypeError`), a missing non-null column
//! or an invalid `ip_address` (`IntegrityError`/`DataError`) — is swallowed, so
//! the handler always returns `Ok(())` and never retries.
//!
//! `create()` is a full `BaseModel.save()`, which in a worker (no `crum`
//! current user) writes `created_by_id`/`updated_by_id` as `NULL`
//! (`plane/db/models/base.py:23-45`). `id`, `created_at` and `updated_at` come
//! from the model defaults, not from `log_data`.
//!
//! The task signature is `process_logs(log_data, **_)`: older releases passed a
//! `mongo_log` keyword, so unknown **task** keywords are ignored (they are not
//! part of `log_data`).

use std::sync::Arc;

use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use uuid::Uuid;

use crate::{TaskContext, kwargs, swallow};

pub const TASK: &str = "plane.bgtasks.logger_task.process_logs";

/// The `APIActivityLog` fields `create(**log_data)` accepts, minus the audit
/// columns the middleware never sends.
const FIELDS: &[&str] = &[
    "token_identifier",
    "path",
    "method",
    "query_params",
    "headers",
    "body",
    "response_code",
    "response_body",
    "ip_address",
    "user_agent",
];

/// Register the handler.
pub fn register(registry: &mut TaskRegistry, _context: Arc<TaskContext>) {
    registry.register(TASK, handle);
}

async fn handle(job: JobContext) -> Result<(), HandlerError> {
    let Some(kwargs) = kwargs(&job.payload) else {
        swallow("process_logs payload has no kwargs", TASK);
        return Ok(());
    };
    let Some(log_data) = kwargs.get("log_data").and_then(|value| value.as_object()) else {
        // `create(**None)` is a `TypeError` inside the task's `try`.
        swallow("process_logs log_data is not a dict", TASK);
        return Ok(());
    };
    if let Some(unknown) = log_data.keys().find(|key| !FIELDS.contains(&key.as_str())) {
        // `Model(**unknown)` raises `TypeError` before any SQL runs.
        swallow(
            format!("APIActivityLog() got an unexpected keyword argument {unknown:?}"),
            TASK,
        );
        return Ok(());
    }
    match insert(&job.client, log_data).await {
        Ok(()) => Ok(()),
        Err(error) => {
            swallow(error, TASK);
            Ok(())
        }
    }
}

/// One `APIActivityLog.objects.create(**log_data)`.
async fn insert(
    client: &Client,
    log_data: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), HandlerError> {
    let text = |name: &str| -> Result<Option<String>, HandlerError> {
        match log_data.get(name) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
            // `CharField.get_prep_value` is `str(value)`; the middleware only
            // sends strings, and a non-string would make Django insert its
            // `repr`. That value is not worth reproducing, but the failure is
            // swallowed like any other.
            Some(other) => Err(HandlerError::fatal(format!(
                "log_data[{name:?}] is not a string: {other}"
            ))),
        }
    };
    let response_code = match log_data.get("response_code") {
        Some(serde_json::Value::Number(number)) => number.as_i64(),
        Some(serde_json::Value::String(value)) => value.parse::<i64>().ok(),
        _ => None,
    };
    let token_identifier = text("token_identifier")?;
    let path = text("path")?;
    let method = text("method")?;
    let query_params = text("query_params")?;
    let headers = text("headers")?;
    let body = text("body")?;
    let response_body = text("response_body")?;
    let user_agent = text("user_agent")?;
    let ip_address = text("ip_address")?;
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    // The casts let Postgres validate `response_code`/`ip_address` exactly as
    // Django's field prep and the `inet` input function do; a bad value is a
    // swallowed error, not a handler failure.
    client
        .execute(
            "INSERT INTO api_activity_logs \
             (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
              token_identifier, path, method, query_params, headers, body, \
              response_code, response_body, ip_address, user_agent) \
             VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, $8, $9, \
                     $10::bigint::integer, $11, $12::text::inet, $13)",
            &[
                &id,
                &now,
                &now,
                &token_identifier,
                &path,
                &method,
                &query_params,
                &headers,
                &body,
                &response_code,
                &response_body,
                &ip_address,
                &user_agent,
            ],
        )
        .await
        .map_err(|error| HandlerError::fatal(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_field_the_middleware_sends_is_accepted() {
        for field in [
            "token_identifier",
            "path",
            "method",
            "query_params",
            "headers",
            "body",
            "response_code",
            "response_body",
            "ip_address",
            "user_agent",
        ] {
            assert!(FIELDS.contains(&field), "{field} is missing");
        }
        assert!(!FIELDS.contains(&"mongo_log"));
    }
}
