// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The exception set a handler can produce, and how each family maps it.
//!
//! A handler in Python raises whichever exception its code path raises; the
//! DRF dispatch loop converts `APIException` subclasses, and Plane's family
//! `handle_exception` converts everything else. A Rust handler returns one of
//! these variants instead, so the mapping lives in one place
//! ([`crate::policy::Family::exception`]) and a handler cannot accidentally
//! return Django's HTML 500 page where Plane returns JSON.

use plane_core::ApiError;

/// An error raised by a handler (or by the runtime before the handler).
#[derive(Debug, Clone, PartialEq)]
pub enum ViewError {
    /// A DRF exception that the DRF exception handler already maps, or an error
    /// the runtime produced itself (throttles, parse errors, `Http404`, …).
    Api(ApiError),
    /// `django.db.IntegrityError`.
    Integrity,
    /// `django.core.exceptions.ValidationError` (the Django one, not DRF's).
    Validation,
    /// `django.core.exceptions.ObjectDoesNotExist` / `get_object_or_404`'s
    /// `DoesNotExist`.
    ObjectDoesNotExist,
    /// A missing URL kwarg (`KeyError`), e.g. `kwargs["slug"]`.
    Key,
    /// Django's `TooManyFieldsSent` (`SuspiciousOperation`) from the form
    /// parser. A family handler catches it; a plain DRF view lets Django's
    /// wrapper answer the 400 page.
    TooManyFields,
    /// Any other exception. Logged by name, never rendered.
    Unknown(String),
}

impl ViewError {
    pub fn api(error: ApiError) -> Self {
        Self::Api(error)
    }

    /// The Python exception's text, for the `plane.exception` log line of a
    /// rendered 500. Django's `log_exception` writes the traceback (the
    /// exception's `str()` included); the port writes the same cause so an
    /// operator can diagnose a 500 from the log instead of an empty access
    /// line. Never rendered in a response.
    #[must_use]
    pub fn cause(&self) -> String {
        match self {
            Self::Api(error) => format!("{error:?}"),
            Self::Integrity => "django.db.utils.IntegrityError".to_string(),
            Self::Validation => "django.core.exceptions.ValidationError".to_string(),
            Self::ObjectDoesNotExist => "ObjectDoesNotExist".to_string(),
            Self::Key => "KeyError".to_string(),
            Self::TooManyFields => "TooManyFieldsSent".to_string(),
            Self::Unknown(message) => message.clone(),
        }
    }
}

impl From<ApiError> for ViewError {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

impl From<plane_permissions::Error> for ViewError {
    /// `plane_permissions::Error` names the exception the Python furnishes.
    fn from(error: plane_permissions::Error) -> Self {
        match error {
            plane_permissions::Error::MissingKeyword(_) => Self::Key,
            plane_permissions::Error::UnpreparedUser
            | plane_permissions::Error::InvalidUuid { .. } => Self::Validation,
            plane_permissions::Error::Db(_) => Self::Unknown(error.to_string()),
        }
    }
}

impl From<plane_auth::AuthError> for ViewError {
    fn from(error: plane_auth::AuthError) -> Self {
        Self::Unknown(error.to_string())
    }
}

impl From<plane_db::DbError> for ViewError {
    fn from(error: plane_db::DbError) -> Self {
        Self::Unknown(error.to_string())
    }
}

impl From<tokio_postgres::Error> for ViewError {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Unknown(error.to_string())
    }
}

impl From<plane_throttle::Error> for ViewError {
    fn from(error: plane_throttle::Error) -> Self {
        Self::Unknown(error.to_string())
    }
}

impl From<plane_serde::Error> for ViewError {
    fn from(error: plane_serde::Error) -> Self {
        Self::Unknown(error.to_string())
    }
}

impl From<plane_serializers::DrfError> for ViewError {
    /// The two render failures map to different Python exceptions — an
    /// unserialisable value is Django's `TypeError` inside `JSONRenderer`, and
    /// an unresolved `expand=` is an `AttributeError` from the injected field's
    /// `get_attribute` (`plane/app/serializers/base.py:145-190`) — but both are
    /// uncaught, so Plane's family `handle_exception` answers 500 for both
    /// (`plane/app/views/base.py:235-239`, `plane/api/views/base.py:105-109`).
    /// [`ViewError::Unknown`] is therefore the faithful mapping; the message
    /// names the field for the log.
    fn from(error: plane_serializers::DrfError) -> Self {
        Self::Unknown(error.to_string())
    }
}

impl From<plane_cache::Error> for ViewError {
    /// `django_redis` raises `ConnectionInterrupted` (a `redis` error) out of
    /// `cache.get`/`cache.set` when Redis is unreachable; Django has no
    /// special case for it, so DRF's `exception_handler` returns `None`,
    /// `APIView.handle_exception` re-raises, and Plane's family handler answers
    /// 500 with its JSON body (`plane/app/views/base.py:200-239`). The port
    /// keeps that outcome instead of degrading to a cache miss, which would be
    /// a silent behaviour change.
    fn from(error: plane_cache::Error) -> Self {
        Self::Unknown(error.to_string())
    }
}

impl From<plane_jobs::celery::PublishError> for ViewError {
    /// Celery's `Task.apply_async` retries and then raises
    /// (`celery/app/task.py:897`, `task_publish_retry`), and nothing in a view
    /// catches `OperationalError`, so a broker outage is a 500 — the same
    /// family handler path as a cache outage. The token-log middleware is the
    /// one place that *does* catch it (`plane/middleware/logger.py:160-161`).
    fn from(error: plane_jobs::celery::PublishError) -> Self {
        Self::Unknown(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cause_names_the_exception_for_the_log() {
        // A 500 rendered by the family handlers is otherwise only an access
        // line in the log; the message is what Django's `log_exception`
        // traceback carries and what the operator needs.
        assert_eq!(
            ViewError::Unknown("relation \"plane_jobs.jobs\" does not exist".to_string()).cause(),
            "relation \"plane_jobs.jobs\" does not exist"
        );
        assert_eq!(
            ViewError::Integrity.cause(),
            "django.db.utils.IntegrityError"
        );
        assert_eq!(ViewError::Key.cause(), "KeyError");
        assert_eq!(ViewError::ObjectDoesNotExist.cause(), "ObjectDoesNotExist");
    }
}
