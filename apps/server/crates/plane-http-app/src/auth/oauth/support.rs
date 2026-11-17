// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shared pieces of the sixteen OAuth views.
//!
//! The four providers' app and space views are textually almost identical but
//! differ in exactly the ways `AUTH_FLOWS.md` §5 warns about:
//!
//! * the space variants store **no** `next_path` at initiation (Gitea is the
//!   exception: both variants store the *validated* path, `gitea.py:35-38`);
//! * the space variants never run `post_user_auth_workflow`;
//! * the space success redirect is `base.rstrip("/") + validated_path` checked
//!   with `url_has_allowed_host_and_scheme`, not `get_safe_redirect_url`
//!   (`views/space/email.py:98-103`);
//! * Gitea builds every redirect with `urljoin`, so the app variant keeps the
//!   session `host` as the base (`views/app/gitea.py:45,76,101`);
//! * the space Gitea error redirects append the validated `next_path` to the
//!   base with `?` while the success redirect concatenates it with no
//!   separator (`views/space/gitea.py:49-51,101-103`).
//!
//! Everything here is deliberately one function per Python expression so a
//! variant's difference stays visible at its call site. The helpers every
//! `/auth/` view shares — `base_host`, `request.POST`, the redirect response,
//! the throttle, `user_login`, `get_redirection_path` — live in
//! [`crate::auth::support`] and are used from here rather than duplicated.

use plane_auth::Json;
use plane_auth::auth_flow::error::AuthFlowError;
use plane_auth::auth_flow::redirect::{urljoin, validate_next_path};
use plane_auth::login::LoginFlags;
use plane_drf::error::ViewError;

use crate::auth::support::py_value_error;

/// Which half of the Python view module this handler is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
    /// `plane/authentication/views/app/*.py`.
    App,
    /// `plane/authentication/views/space/*.py`.
    Space,
}

/// The four providers, as `provider/oauth/*.py` names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderKind {
    Google,
    Github,
    Gitlab,
    Gitea,
}

impl ProviderKind {
    /// `self.provider`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Gitea => "gitea",
        }
    }
}

impl Variant {
    /// `user_login(..., is_app=True)` / `is_space=True`.
    pub(crate) fn login_flags(self) -> LoginFlags {
        match self {
            Self::App => LoginFlags {
                is_app: true,
                is_admin: false,
                is_space: false,
            },
            Self::Space => LoginFlags {
                is_app: false,
                is_admin: false,
                is_space: true,
            },
        }
    }

    /// The session key is shared by both variants; only the base host differs.
    pub(crate) fn is_app(self) -> bool {
        self == Self::App
    }
}

/// `request.GET.get(name)`: the last value, `None` when absent (`""` when
/// present but empty, which is falsy in Python).
pub(crate) fn query_get<'a>(ctx: &'a plane_drf::view::ViewCtx<'a>, name: &str) -> Option<&'a str> {
    ctx.query().get(name)
}

/// `urljoin(host, relative)`; a falsy `host` is `urljoin(None, …)`, which
/// Python treats as the empty base (`urllib/parse.py`).
pub(crate) fn join(host: Option<&str>, relative: &str) -> Result<String, ViewError> {
    urljoin(host.unwrap_or_default(), relative).map_err(py_value_error)
}

/// `validate_next_path(next_path)` (`path_validator.py:105-154`).
pub(crate) fn validated_next_path(next_path: Option<&str>) -> Result<String, ViewError> {
    validate_next_path(next_path).map_err(py_value_error)
}

/// `AuthenticationException.get_error_dict()`'s `error_code`/`error_message`
/// pair, plus the call site's extra keys, in Python's insertion order.
pub(crate) fn error_params(error: &AuthFlowError) -> Vec<(String, String)> {
    error.redirect_params()
}

/// Append `next_path` to the error parameters, as the Gitea variants do
/// (`views/app/gitea.py:45`, `:85`; `views/space/gitea.py:49`).
pub(crate) fn with_next_path(
    mut params: Vec<(String, String)>,
    next_path: Option<&str>,
) -> Vec<(String, String)> {
    if let Some(next_path) = next_path {
        params.push(("next_path".to_string(), next_path.to_string()));
    }
    params
}

/// Python truthiness for a session value (`if next_path:`).
pub(crate) fn json_truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => *value != 0,
        Json::Float(value) => *value != 0.0,
        Json::Str(value) => !value.is_empty(),
        Json::Array(items) => !items.is_empty(),
        Json::Object(entries) => !entries.is_empty(),
    }
}
