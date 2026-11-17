// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-auth`: the parts of Django that make a session, a password and a CSRF
//! token portable between `apps/api` and the Rust services.
//!
//! Coexistence (`RUST_REWRITE.md`, "How the two backends coexist") needs three
//! byte- or format-compatible things:
//!
//! | Concern | Module | Python reference |
//! | --- | --- | --- |
//! | Sessions | [`session`], [`signing`], [`json`], [`cookies`] | `django.contrib.sessions.backends.db`, `django.core.signing`, `plane/db/models/session.py`, `plane/authentication/middleware/session.py` |
//! | Users | [`user`], [`login`] | `django.contrib.auth`, `django.contrib.auth.middleware.AuthenticationMiddleware`, `plane/authentication/utils/login.py` |
//! | Passwords | [`password`] | `django/contrib/auth/hashers.py` 5.2.17 |
//! | CSRF | [`csrf`] | `django/middleware/csrf.py` 5.2.17 |
//! | DRF origin check | [`origin`] | `plane/authentication/session.py` |
//! | API keys | [`api_key`] | `plane/api/middleware/api_authentication.py` |
//!
//! [`AuthUser`] and [`RequestUser`] are the types other crates take:
//! `plane-permissions` guards read a `&RequestUser`, and the DRF runtime keeps
//! it in the request state.
//!
//! Everything that touches the database goes through [`queries`], which is
//! checked against a Django-migrated schema by `tests/schema_check.rs`
//! (`plane_db::schema_check::verify`).

pub mod api_key;
pub mod cookies;
pub mod csrf;
pub mod error;
pub mod host;
pub mod json;
pub mod login;
pub mod origin;
pub mod password;
pub mod queries;
pub mod random;
pub mod session;
pub mod signing;
pub mod user;

pub use api_key::{ApiKeyError, ApiToken, INVALID_API_TOKEN};
pub use csrf::CsrfState;
pub use error::AuthError;
pub use json::Json;
pub use login::{LoginFlags, base_host, login, logout, user_login};
pub use origin::{ORIGIN_NOT_ALLOWED, enforce_csrf, is_allowed_origin};
pub use password::{
    PBKDF2_ITERATIONS, PasswordCheck, check_password, check_password_async, is_password_usable,
    make_password, make_password_async, verify_password,
};
pub use session::{SessionCookieAction, SessionSlot, SessionStore, delete_all_for_user};
pub use user::{
    AuthUser, MODEL_BACKEND, RequestUser, SessionUser, get_user, load_session_user_by_id,
};
