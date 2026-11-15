// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! # `plane-permissions`
//!
//! Every DRF permission class and `allow_permission` decorator Plane enforces,
//! with the semantics of the Python implementation rather than a simplification
//! of it. A handler cannot be registered without a decision from this crate:
//! the route policy generated from `routes.json` names a
//! [`PermissionClass`] list and the `allow_permission` specs for each method,
//! and both are evaluated here.
//!
//! ## What is modelled
//!
//! | Python | Here |
//! | --- | --- |
//! | `plane/app/permissions/*.py` | [`PermissionClass::App*`] |
//! | `plane/utils/permissions/*.py` | [`PermissionClass::Utils*`] |
//! | `rest_framework.permissions.AllowAny` / `IsAuthenticated` | [`PermissionClass::AllowAny`], [`PermissionClass::IsAuthenticated`] |
//! | `plane/license/api/permissions/instance.py` | [`PermissionClass::InstanceAdminPermission`] |
//! | `allow_permission(allowed_roles, level, creator, model)` | [`allow_permission`] + [`AllowPermission`] |
//! | `issue_hidden_from_guest` | [`issue_hidden_from_guest`] |
//! | `get_permissions` overrides | [`dynamic_permission_classes`] |
//! | `APIView.check_permissions` | [`check_permissions`] |
//!
//! The app and utils packages are copies; `diff -u` over the five files shows
//! only additions in `plane/app` (`WorkspaceMemberPermission`,
//! `ProjectArchiveUnarchivePermission`, `issue_hidden_from_guest`) and two
//! comment blocks. The Rust items are aliases of one implementation, with one
//! enum variant per dotted path so `routes.json` can be applied verbatim and an
//! unknown path is a hard error at policy-generation time.
//!
//! ## DRF semantics
//!
//! * Only `has_permission` exists. No Plane class (and none of DRF's
//!   `AllowAny`/`IsAuthenticated`) implements `has_object_permission`, so
//!   `APIView.check_object_permissions` — which `get_object()` calls — always
//!   passes. Nothing object-level is modelled because nothing can deny.
//! * `SAFE_METHODS` is DRF's `("GET", "HEAD", "OPTIONS")`
//!   (`rest_framework/permissions.py:8`), **not** Django's four-method tuple:
//!   `TRACE` takes the unsafe branch. `TRACE` reaches `initial()` and therefore
//!   the permission checks before DRF answers 405.
//! * [`check_permissions`] stops at the first failing class and applies
//!   `APIView.permission_denied` (`rest_framework/views.py:174-180`): if the
//!   request configured authenticators and none succeeded the failure is
//!   `NotAuthenticated` (401), otherwise `PermissionDenied` (403). No class
//!   defines `message` or `code`, so the details are DRF's defaults.
//! * `allow_permission` does **not** raise: it returns a DRF `Response` with
//!   status 403 and body `{"error": "You don't have the required permissions."}`,
//!   which the view returns as-is ([`Decision::Deny`]).
//!
//! ## Errors
//!
//! Three Python exceptions escape the checks and are part of the contract. The
//! DRF view families render them identically (`plane/app/views/base.py:324-334`,
//! `plane/api/views/base.py:225-236`, `plane/space/views/base.py:106-112`):
//!
//! | [`Error`] | Python | HTTP |
//! | --- | --- | --- |
//! | [`Error::MissingKeyword`] | `KeyError` from `kwargs["slug"]`/`["project_id"]`/`["pk"]` | 400 `{"error":"The required key does not exist."}` |
//! | [`Error::UnpreparedUser`] | `django.core.exceptions.ValidationError` from filtering by `AnonymousUser` | 400 `{"error":"Please provide valid detail"}` |
//! | [`Error::InvalidUuid`] | the same `ValidationError`, from an unparsable UUID | 400 `{"error":"Please provide valid detail"}` |
//! | [`Error::Db`] | a database failure | 500 `{"error":"Something went wrong please try again later"}` |
//!
//! `Error::UnpreparedUser` is reachable in `allow_permission`: unlike the
//! permission classes it has no `request.user.is_anonymous` guard, so an
//! anonymous request whose path reaches a decorated handler produces the
//! `ValidationError`/400 rather than a 403. In today's routing every such
//! route also has a permission class that rejects anonymous callers first; the
//! port keeps the behaviour so a future route cannot silently change it.
//!
//! ## Query reduction (deliberate, outcome-identical)
//!
//! Django's classes issue the same `.exists()` query up to four times
//! (`ProjectBasePermission` asks for an admin project member, then any project
//! member, then an admin workspace member). This crate selects the membership
//! **role** once and evaluates the predicates in Rust. The partial unique
//! constraints (`workspace_member_unique_workspace_member_when_deleted_at_null`,
//! `project_member_unique_project_member_when_deleted_at_null`) guarantee at
//! most one matching row, so `role = 20`, `role IN (20, 15)` and "a row exists"
//! are exactly the three Django answers. No result is memoised across calls:
//! the runtime runs every check before the handler, so there is no window in
//! which a write could change the answer, but not sharing state keeps the crate
//! free of request-scoped caches to invalidate.
//!
//! A missing kwarg is still reported in Django's order. For example
//! `allow_permission(creator=True, model=…)` on a route without `project_id`
//! first raises for `project_id` (the project-membership guard) and only then
//! for `pk`, because that is the order the Python evaluates the queries in.

pub mod allow;
pub mod classes;
pub mod dynamic;
pub mod helpers;
pub mod instance;
mod membership;
pub mod page;
pub mod policy;
pub mod queries;

use http::Method;
use uuid::Uuid;

pub use allow::{AllowPermission, CreatorModel, Decision, DenyResponse, allow_permission};
pub use classes::{PermissionClass, has_permission};
pub use dynamic::dynamic_permission_classes;
pub use helpers::issue_hidden_from_guest;
pub use policy::{CheckFailure, Denial, check_permissions, permission_denied};

/// DRF's `SAFE_METHODS` (`rest_framework/permissions.py:8`).
///
/// Deliberately three methods: unlike Django's `SAFE_METHODS`, DRF does not
/// treat `TRACE` as safe, and every Plane permission class imports DRF's tuple.
pub fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// One of `plane/db/models/project.py:24` / `plane/app/permissions/base.py:13`
/// (`ROLE`), a workspace or project membership role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Admin,
    Member,
    Guest,
}

impl Role {
    /// The integer stored in `workspace_members.role` / `project_members.role`.
    pub const fn value(self) -> i16 {
        match self {
            Self::Admin => 20,
            Self::Member => 15,
            Self::Guest => 5,
        }
    }

    /// The `ROLE` member name `routes.json` records (`"ADMIN"`, …).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "ADMIN" => Some(Self::Admin),
            "MEMBER" => Some(Self::Member),
            "GUEST" => Some(Self::Guest),
            _ => None,
        }
    }

    /// A stored role value as a variant; unknown integers (which the Python
    /// simply never matches against `role__in`) stay unmatched.
    pub fn from_value(value: i16) -> Option<Self> {
        match value {
            20 => Some(Self::Admin),
            15 => Some(Self::Member),
            5 => Some(Self::Guest),
            _ => None,
        }
    }
}

/// The `level` argument of `allow_permission`.
///
/// The Python only ever compares `level == "WORKSPACE"` and `level !=
/// "WORKSPACE"`; the default is `"PROJECT"` and any other value behaves like
/// it. [`Level::parse`] keeps that behaviour instead of rejecting unknown
/// strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    Workspace,
    #[default]
    Project,
}

impl Level {
    pub fn parse(level: &str) -> Self {
        if level == "WORKSPACE" {
            Self::Workspace
        } else {
            Self::Project
        }
    }
}

/// URL kwargs by name, as DRF's `view.kwargs` holds them.
///
/// A tiny sorted-by-insertion vector: a route has at most a handful of
/// parameters, so a linear scan beats hashing and the router can build it
/// without allocating strings.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct UrlKwargs<'a> {
    entries: Vec<(&'a str, &'a str)>,
}

impl<'a> UrlKwargs<'a> {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Insert or replace one parameter.
    pub fn insert(&mut self, name: &'a str, value: &'a str) -> Option<&'a str> {
        if let Some(slot) = self.entries.iter_mut().find(|(key, _)| *key == name) {
            return Some(std::mem::replace(&mut slot.1, value));
        }
        self.entries.push((name, value));
        None
    }

    /// `view.kwargs.get(name)`.
    pub fn get(&self, name: &str) -> Option<&'a str> {
        self.entries
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    }

    /// `kwargs["name"]`, i.e. Django's `KeyError` when absent.
    pub fn require(&self, name: &'static str) -> Result<&'a str, Error> {
        self.get(name).ok_or(Error::MissingKeyword(name))
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|(key, _)| *key == name)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &'a str)> + '_ {
        self.entries.iter().copied()
    }
}

impl<'a> FromIterator<(&'a str, &'a str)> for UrlKwargs<'a> {
    fn from_iter<T: IntoIterator<Item = (&'a str, &'a str)>>(iter: T) -> Self {
        Self {
            entries: iter.into_iter().collect(),
        }
    }
}

impl<'a, const N: usize> From<[(&'a str, &'a str); N]> for UrlKwargs<'a> {
    fn from(entries: [(&'a str, &'a str); N]) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }
}

/// Everything a permission check may read from the request.
///
/// `url_name` is the resolved URL name (`routes.json`' `name`). It exists
/// because `BaseAPIView.project_id` falls back to `kwargs["pk"]` when
/// `resolve(request.path_info).url_name == "project"`
/// (`plane/app/views/base.py:259-266`, identical in the api and space base
/// views) — that is how `/api/v1/workspaces/<slug>/projects/<pk>/` scopes its
/// project checks to `pk`.
///
/// `view_project_identifier` is `view.project_identifier` for the views that
/// define that property (today only
/// `plane.api.views.issue.WorkspaceIssueAPIEndpoint`,
/// `plane/api/views/issue.py:260-262`); `None` everywhere else. It is not a URL
/// kwarg in general, and treating it as one would let a route that happens to
/// capture `project_identifier` skip the explicit project-membership branch.
pub struct PermCtx<'a> {
    /// A checked-out connection. Every check on one request runs on it in
    /// sequence, exactly as Django's connection would.
    pub client: &'a deadpool_postgres::Client,
    /// Django's `request.user`: `Anonymous` or an authenticated user.
    ///
    /// An inactive user's session authenticates as `Anonymous`
    /// (`SessionAuthentication.authenticate` returns `None` when
    /// `user.is_active` is false), so this crate never reads `is_active` — no
    /// permission class does either.
    pub user: &'a plane_auth::RequestUser,
    /// DRF's `request.method`, uppercase.
    pub method: &'a Method,
    /// DRF's `view.kwargs`.
    pub kwargs: &'a UrlKwargs<'a>,
    /// DRF's `view.action` for a ViewSet, `None` for a plain APIView.
    pub action: Option<&'a str>,
    /// The resolved URL name, for the `project_id` fallback.
    pub url_name: Option<&'a str>,
    /// `view.project_identifier`, only when the view defines the property.
    pub view_project_identifier: Option<&'a str>,
    /// `bool(request.authenticators)`. Every DRF route in `routes.json`
    /// configures at least one authenticator, so this is `true` there; the flag
    /// exists so `permission_denied` is not guessed at.
    pub authenticators_configured: bool,
}

impl<'a> PermCtx<'a> {
    /// The common case: a request on a route with authenticators, no action and
    /// no `project_identifier` property.
    pub fn new(
        client: &'a deadpool_postgres::Client,
        user: &'a plane_auth::RequestUser,
        method: &'a Method,
        kwargs: &'a UrlKwargs<'a>,
    ) -> Self {
        Self {
            client,
            user,
            method,
            kwargs,
            action: None,
            url_name: None,
            view_project_identifier: None,
            authenticators_configured: true,
        }
    }

    pub fn with_action(mut self, action: Option<&'a str>) -> Self {
        self.action = action;
        self
    }

    pub fn with_url_name(mut self, url_name: Option<&'a str>) -> Self {
        self.url_name = url_name;
        self
    }

    pub fn with_project_identifier(mut self, identifier: Option<&'a str>) -> Self {
        self.view_project_identifier = identifier;
        self
    }

    /// `bool(request.authenticators)` for a route with no authenticators (no
    /// DRF route in `routes.json`; the auth views are not DRF).
    pub fn with_authenticators_configured(mut self, configured: bool) -> Self {
        self.authenticators_configured = configured;
        self
    }

    /// `view.workspace_slug` (`kwargs.get("slug")`).
    pub fn workspace_slug(&self) -> Option<&'a str> {
        self.kwargs.get("slug")
    }

    /// `view.project_id` (`BaseAPIView.project_id`).
    pub fn project_id(&self) -> Option<&'a str> {
        match self
            .kwargs
            .get("project_id")
            .filter(|value| !value.is_empty())
        {
            Some(project_id) => Some(project_id),
            None if self.url_name == Some("project") => {
                self.kwargs.get("pk").filter(|value| !value.is_empty())
            }
            None => None,
        }
    }

    /// `view.kwargs.get("project_id")` without the `pk` fallback. The page
    /// permission uses this form (`plane/app/permissions/page.py:34`).
    pub fn raw_project_id(&self) -> Option<&'a str> {
        self.kwargs.get("project_id")
    }

    pub(crate) fn user_id(&self) -> Result<Uuid, Error> {
        self.user.id().ok_or(Error::UnpreparedUser)
    }
}

/// A failure that is not a permission decision.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `kwargs["name"]` on a route that does not provide it: Django raises
    /// `KeyError`, `BaseAPIView.handle_exception` answers 400 with
    /// `{"error": "The required key does not exist."}`.
    #[error(
        "missing URL kwarg {0:?} (Django KeyError -> 400 \"The required key does not exist.\")"
    )]
    MissingKeyword(&'static str),
    /// A queryset filter received `AnonymousUser`: Django raises
    /// `ValidationError`, rendered as 400 `{"error": "Please provide valid detail"}`.
    #[error("queryset filter used an anonymous user (Django ValidationError -> 400)")]
    UnpreparedUser,
    /// A UUID kwarg could not be parsed: Django's `UUIDField` raises
    /// `ValidationError`, rendered as 400 `{"error": "Please provide valid detail"}`.
    #[error("invalid UUID {value:?} for {field} (Django ValidationError -> 400)")]
    InvalidUuid { field: &'static str, value: String },
    /// The database failed. The runtime renders Plane's 500 body.
    #[error(transparent)]
    Db(#[from] plane_db::DbError),
}

impl From<tokio_postgres::Error> for Error {
    fn from(error: tokio_postgres::Error) -> Self {
        Self::Db(plane_db::DbError::Postgres(error))
    }
}

/// Parse a UUID kwarg the way Django's `UUIDField.to_python` accepts it.
pub(crate) fn parse_uuid(field: &'static str, value: &str) -> Result<Uuid, Error> {
    Uuid::parse_str(value).map_err(|_| Error::InvalidUuid {
        field,
        value: value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_methods_match_drf() {
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            assert!(is_safe_method(&method), "{method} is safe in DRF");
        }
        for method in [
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::TRACE,
        ] {
            assert!(!is_safe_method(&method), "{method} is not safe in DRF");
        }
    }

    #[test]
    fn roles_round_trip() {
        assert_eq!(Role::Admin.value(), 20);
        assert_eq!(Role::Member.value(), 15);
        assert_eq!(Role::Guest.value(), 5);
        assert_eq!(Role::from_name("ADMIN"), Some(Role::Admin));
        assert_eq!(Role::from_name("nope"), None);
        assert_eq!(Role::from_value(20), Some(Role::Admin));
        assert_eq!(Role::from_value(10), None);
    }

    #[test]
    fn level_only_knows_workspace() {
        assert_eq!(Level::parse("WORKSPACE"), Level::Workspace);
        assert_eq!(Level::parse("PROJECT"), Level::Project);
        // The Python treats anything else as the default.
        assert_eq!(Level::parse("workspace"), Level::Project);
    }

    #[test]
    fn kwargs_require_matches_django() {
        let kwargs = UrlKwargs::from([("slug", "s"), ("pk", "p")]);
        assert_eq!(kwargs.get("slug"), Some("s"));
        assert_eq!(kwargs.get("missing"), None);
        assert!(matches!(
            kwargs.require("project_id").unwrap_err(),
            Error::MissingKeyword("project_id")
        ));
        assert_eq!(kwargs.len(), 2);
    }
}
