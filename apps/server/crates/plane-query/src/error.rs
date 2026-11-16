// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Errors the issue read path can produce.
//!
//! Two kinds matter:
//!
//! * **DRF errors** — a `ParseError` or `ValidationError` the Django view raises
//!   through `raise ParseError(...)`/`serializers.ValidationError`, which DRF
//!   renders as a 400 with `{"detail": ...}`. [`QueryError::status`] and
//!   [`QueryError::detail`] give the handler what it needs to render the same
//!   body.
//! * **Upstream 500s** — the Python code lets an exception escape (a
//!   `FieldError` from a lookup that cannot exist, a `KeyError` in the
//!   sub-grouped grouper, a `ValidationError` from a value that does not parse).
//!   Those are [`QueryError::Unsupported`]; a handler maps them to whatever its
//!   error strategy is. They are recorded per case in the golden files, so the
//!   port keeps failing where Django fails instead of silently returning a
//!   different result.

use thiserror::Error;

use crate::json::Json;

/// The outcome of a pagination request that DRF reports as a 400.
#[derive(Debug, Clone, PartialEq, Error)]
#[error("{message}")]
pub struct ApiError {
    pub status: u16,
    pub message: String,
    /// The `code` DRF would attach, when the Python sets one.
    pub code: Option<String>,
    /// `ValidationError` from a filterset nests the per-field errors here.
    pub errors: Option<Json>,
}

impl ApiError {
    pub fn parse(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
            code: None,
            errors: None,
        }
    }

    pub fn detail(&self) -> Json {
        let mut entries: Vec<(&str, Json)> = Vec::new();
        if let Some(code) = &self.code {
            entries.push(("message", Json::string(&self.message)));
            entries.push(("code", Json::string(code)));
            if let Some(errors) = &self.errors {
                entries.push(("errors", errors.clone()));
            }
            return Json::object(entries);
        }
        // A django-filter validation failure has no message of its own: DRF
        // renders the field errors dict as the body.
        if let Some(errors) = &self.errors {
            return errors.clone();
        }
        Json::string(&self.message)
    }
}

/// Everything the query layer can fail with.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum QueryError {
    /// A request Django answers with a 400 (`ParseError`, `ValidationError`).
    #[error("{0}")]
    Api(ApiError),
    /// `IssueViewSet.list` answers this with `Response({"error": …}, 400)`,
    /// not a DRF `ParseError`, so the handler must render it differently.
    #[error("{0}")]
    BadRequest(String),
    /// `django.core.exceptions.ValidationError`, which the ORM raises when a
    /// lookup value cannot be converted to its column's type — a legacy date
    /// lookup whose first `;`-segment is not a date, for example
    /// (`django/db/models/fields/__init__.py:1602-1624`). It surfaces when the
    /// queryset compiles, and the view family's `handle_exception` renders
    /// `400 {"error": "Please provide valid detail"}`
    /// (`plane/app/views/base.py:216-220`).
    #[error("Please provide valid detail")]
    Validation,
    /// A path Django answers with a 500, kept failing here.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// A database error (never a 400).
    #[error("database: {0}")]
    Db(String),
    /// A serialisation error from `plane-serde`.
    #[error("json: {0}")]
    Json(String),
}

impl QueryError {
    pub fn parse(message: impl Into<String>) -> Self {
        Self::Api(ApiError::parse(message))
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    /// The status a handler should answer with, when there is one.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api(error) => Some(error.status),
            Self::BadRequest(_) => Some(400),
            Self::Validation => Some(400),
            _ => None,
        }
    }

    /// The `detail` a DRF `APIException` would render.
    pub fn detail(&self) -> Option<Json> {
        match self {
            Self::Api(error) => Some(error.detail()),
            _ => None,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Api(error) => error.message.clone(),
            Self::BadRequest(message) => message.clone(),
            Self::Unsupported(message) => message.clone(),
            Self::Db(message) => message.clone(),
            Self::Json(message) => message.clone(),
            Self::Validation => "Please provide valid detail".to_owned(),
        }
    }
}

impl From<plane_db::DbError> for QueryError {
    fn from(error: plane_db::DbError) -> Self {
        Self::Db(error.to_string())
    }
}

impl From<tokio_postgres::Error> for QueryError {
    fn from(error: tokio_postgres::Error) -> Self {
        // `Display` for a database error is just "db error"; the message and
        // the failing SQL state are what make a golden mismatch diagnosable.
        let detail = error
            .as_db_error()
            .map(|db| format!("{} (SQLSTATE {})", db.message(), db.code().code()))
            .unwrap_or_else(|| error.to_string());
        Self::Db(detail)
    }
}

impl From<plane_serde::Error> for QueryError {
    fn from(error: plane_serde::Error) -> Self {
        Self::Json(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_errors_render_a_detail_string() {
        let error = QueryError::parse("Invalid cursor parameter.");
        assert_eq!(error.status(), Some(400));
        assert_eq!(
            error.detail().expect("detail").to_serde(),
            serde_json::json!("Invalid cursor parameter.")
        );
    }

    #[test]
    fn coded_errors_render_a_dict() {
        let error = QueryError::Api(ApiError {
            status: 400,
            message: "Filtering on field 'x' is not allowed".to_owned(),
            code: Some("invalid_filter_field".to_owned()),
            errors: None,
        });
        assert_eq!(
            error.detail().expect("detail").to_serde(),
            serde_json::json!({"message": "Filtering on field 'x' is not allowed", "code": "invalid_filter_field"})
        );
    }
}
