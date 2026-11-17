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
//! variant's difference stays visible at its call site.

use plane_auth::Json;
use plane_auth::auth_flow::error::AuthFlowError;
use plane_auth::auth_flow::redirect::{
    PyValueError, get_safe_redirect_url, settings_allowed_hosts, space_redirect_url, urljoin,
    validate_next_path,
};
use plane_auth::auth_flow::tasks::view_base_host;
use plane_auth::login::LoginFlags;
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_drf::error::ViewError;
use plane_drf::view::ViewResponse;

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

/// `base_host(request=request, is_app=…/is_space=…)`
/// (`authentication/utils/host.py:16-63`).
///
/// `None` survives for the app variant when neither `APP_BASE_URL` nor
/// `WEB_URL` is set — Python then fails on `None.rstrip("/")`, a 500 the
/// caller reproduces with [`required_base`].
pub(crate) fn base_host(
    settings: &Settings,
    variant: Variant,
) -> Result<Option<String>, ViewError> {
    view_base_host(settings, variant.login_flags())
        .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// The base host the redirect builders need as a `&str`.
pub(crate) fn required_base(base: &Option<String>) -> Result<&str, ViewError> {
    base.as_deref().ok_or_else(|| {
        ViewError::Unknown("TypeError: 'NoneType' object has no attribute 'rstrip'".to_string())
    })
}

/// `urlparse`/`urljoin`/`validate_next_path` raise `ValueError`; Django answers
/// 500 for an uncaught one (the redirect views have no handler).
pub(crate) fn py_error(error: PyValueError) -> ViewError {
    ViewError::Unknown(format!("ValueError: {}", error.message()))
}

/// `get_safe_redirect_url(base_url=…, next_path=…, params=…)`
/// (`utils/path_validator.py:157-201`).
pub(crate) fn safe_redirect(
    settings: &Settings,
    base: &str,
    next_path: Option<&str>,
    params: &[(String, String)],
) -> Result<String, ViewError> {
    let allowed = settings_allowed_hosts(settings).map_err(py_error)?;
    get_safe_redirect_url(base, next_path, params, &allowed).map_err(py_error)
}

/// The space success redirect (`views/space/email.py:98-103`): the validated
/// path concatenated onto the base, checked, else the bare base.
pub(crate) fn space_success_redirect(
    settings: &Settings,
    base: &str,
    next_path: Option<&str>,
) -> Result<String, ViewError> {
    let allowed = settings_allowed_hosts(settings).map_err(py_error)?;
    space_redirect_url(base, next_path, &allowed).map_err(py_error)
}

/// `urljoin(host, relative)`; a falsy `host` is `urljoin(None, …)`, which
/// Python treats as the empty base (`urllib/parse.py`).
pub(crate) fn join(host: Option<&str>, relative: &str) -> Result<String, ViewError> {
    urljoin(host.unwrap_or_default(), relative).map_err(py_error)
}

/// `validate_next_path(next_path)` (`path_validator.py:105-154`).
pub(crate) fn validated_next_path(next_path: Option<&str>) -> Result<String, ViewError> {
    validate_next_path(next_path).map_err(py_error)
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

/// `HttpResponseRedirect(url)`: a 302 with an empty `text/html` body.
///
/// `django/http/response.py:482-513` sets `Location = iri_to_uri(url)` and
/// rejects a URL longer than 16384 characters or one whose scheme is not
/// `http`/`https`/`ftp` (`DisallowedRedirect`, an uncaught 500). `iri_to_uri`
/// is the same percent-encoding helper the request path uses.
pub(crate) fn redirect(url: &str) -> Result<ViewResponse, ViewError> {
    if url.chars().count() > 16_384 {
        return Err(ViewError::Unknown(
            "DisallowedRedirect: unsafe redirect exceeding 16384 characters".to_string(),
        ));
    }
    let Ok(parsed) = plane_net::pyurl::urlsplit(url) else {
        return Err(ViewError::Unknown(
            "ValueError: invalid URL in HttpResponseRedirect".to_string(),
        ));
    };
    if !parsed.scheme.is_empty() && !matches!(parsed.scheme.as_str(), "http" | "https" | "ftp") {
        return Err(ViewError::Unknown(format!(
            "DisallowedRedirect: unsafe redirect to URL with protocol '{}'",
            parsed.scheme
        )));
    }
    Ok(
        ViewResponse::raw(302, plane_core::error::TEXT_HTML, Vec::new())
            .with_header("Location", plane_core::py::iri_to_uri(url)),
    )
}

/// The request context a `user_login` needs, copied from the immutable view
/// context.
///
/// Python's `login()` writes `request.user`; the Rust pipeline re-resolves
/// `request.user` from the session during the response phase
/// (`bins/plane-api/src/middleware/mod.rs`, the logger layer), so the copy only
/// feeds `device_info`'s `base_host` and the login call. Nothing observable
/// reads `ctx.user` in between.
pub(crate) fn login_context(ctx: &RequestContext) -> RequestContext {
    ctx.clone()
}
