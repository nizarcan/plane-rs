// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DRF's `check_permissions` loop and the 401-vs-403 decision.
//!
//! `APIView.check_permissions` (`rest_framework/views.py:331-342`) walks the
//! effective permission list in order and stops at the first class whose
//! `has_permission` is falsy:
//!
//! ```python
//! for permission in self.get_permissions():
//!     if not permission.has_permission(request, self):
//!         self.permission_denied(
//!             request,
//!             message=getattr(permission, 'message', None),
//!             code=getattr(permission, 'code', None),
//!         )
//! ```
//!
//! and `permission_denied` (`views.py:174-180`) raises `NotAuthenticated` when
//! `request.authenticators and not request.successful_authenticator`, otherwise
//! `PermissionDenied(detail=message, code=code)`. No class in this crate (or in
//! DRF's two) sets `message` or `code`, so both are always `None` and the
//! details are DRF's defaults:
//!
//! * `NotAuthenticated`: 401 `{"detail": "Authentication credentials were not provided."}`
//!   (Plane's exception handler forces 401; DRF alone would use 403 because
//!   Plane's authenticators return no `WWW-Authenticate` header);
//! * `PermissionDenied`: 403 `{"detail": "You do not have permission to perform this action."}`.

use crate::{Error, PermCtx, PermissionClass};

/// The exception `permission_denied` raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    NotAuthenticated,
    PermissionDenied {
        /// `getattr(permission, "message", None)`; always `None` today.
        message: Option<&'static str>,
        /// `getattr(permission, "code", None)`; always `None` today.
        code: Option<&'static str>,
    },
}

impl Denial {
    /// DRF's status code before Plane's handler adjusts `NotAuthenticated`.
    pub const fn status(self) -> u16 {
        match self {
            Self::NotAuthenticated => 401,
            Self::PermissionDenied { .. } => 403,
        }
    }

    /// The `default_detail` DRF renders for the exception.
    pub const fn detail(self) -> &'static str {
        match self {
            Self::NotAuthenticated => "Authentication credentials were not provided.",
            Self::PermissionDenied { message, .. } => match message {
                Some(message) => message,
                None => "You do not have permission to perform this action.",
            },
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::NotAuthenticated => "not_authenticated",
            Self::PermissionDenied { code, .. } => match code {
                Some(code) => code,
                None => "permission_denied",
            },
        }
    }
}

impl From<Denial> for plane_core::ApiError {
    /// The DRF-compatible error type the runtime renders. `PermissionDenied`
    /// without a custom message becomes `ApiError::permission_denied()`, whose
    /// detail is the same literal.
    fn from(denial: Denial) -> Self {
        match denial {
            Denial::NotAuthenticated => Self::NotAuthenticated,
            Denial::PermissionDenied { message: None, .. } => Self::permission_denied(),
            Denial::PermissionDenied {
                message: Some(message),
                ..
            } => Self::PermissionDenied(message.to_string()),
        }
    }
}

/// `APIView.permission_denied` for one failing class.
pub fn permission_denied(ctx: &PermCtx<'_>) -> Denial {
    if ctx.authenticators_configured && !ctx.user.is_authenticated() {
        Denial::NotAuthenticated
    } else {
        Denial::PermissionDenied {
            message: None,
            code: None,
        }
    }
}

/// A failed `check_permissions`: either a permission decision or a query error.
#[derive(Debug, thiserror::Error)]
pub enum CheckFailure {
    #[error("permission denied: {}", .0.detail())]
    Denied(Denial),
    #[error(transparent)]
    Query(#[from] Error),
}

/// `APIView.check_permissions`.
///
/// First failure wins. `Ok(())` means every class allowed the request.
pub async fn check_permissions(
    classes: &[PermissionClass],
    ctx: &PermCtx<'_>,
) -> Result<(), CheckFailure> {
    for class in classes {
        if !class.check(ctx).await? {
            return Err(CheckFailure::Denied(permission_denied(ctx)));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denial_statuses_and_details() {
        let not_authenticated = Denial::NotAuthenticated;
        assert_eq!(not_authenticated.status(), 401);
        assert_eq!(
            not_authenticated.detail(),
            "Authentication credentials were not provided."
        );
        assert_eq!(not_authenticated.code(), "not_authenticated");

        let denied = Denial::PermissionDenied {
            message: None,
            code: None,
        };
        assert_eq!(denied.status(), 403);
        assert_eq!(
            denied.detail(),
            "You do not have permission to perform this action."
        );
        assert_eq!(denied.code(), "permission_denied");

        let custom = Denial::PermissionDenied {
            message: Some("no"),
            code: Some("nope"),
        };
        assert_eq!(custom.detail(), "no");
        assert_eq!(custom.code(), "nope");
    }

    #[test]
    fn maps_to_plane_core_api_error() {
        use plane_core::ApiError;

        assert_eq!(
            ApiError::from(Denial::NotAuthenticated),
            ApiError::NotAuthenticated
        );
        assert_eq!(
            ApiError::from(Denial::PermissionDenied {
                message: None,
                code: None
            }),
            ApiError::permission_denied()
        );
        assert_eq!(
            ApiError::from(Denial::PermissionDenied {
                message: Some("no"),
                code: None
            }),
            ApiError::PermissionDenied("no".to_string())
        );
        // The literals in this crate and `plane-core` must not drift.
        assert_eq!(
            Denial::NotAuthenticated.detail(),
            plane_core::error::NOT_AUTHENTICATED_DETAIL
        );
        assert_eq!(
            Denial::PermissionDenied {
                message: None,
                code: None
            }
            .detail(),
            plane_core::error::PERMISSION_DENIED_DETAIL
        );
    }
}
