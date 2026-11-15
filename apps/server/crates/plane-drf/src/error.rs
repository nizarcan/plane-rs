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
