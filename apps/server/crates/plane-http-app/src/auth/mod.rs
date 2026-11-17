// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `auth` (`deploy/route-groups.yaml`): the `/auth/` surface.
//!
//! | Rust | Python | Routes |
//! | --- | --- | --- |
//! | [`email`] | `authentication/views/{app,space}/email.py` | 352-355 |
//! | [`signout`] | `authentication/views/{app,space}/signout.py` | 356-357 |
//! | [`csrf`] | `authentication/views/common.py:29-36` (`CSRFTokenEndpoint`) | 358 |
//! | [`magic`] | `authentication/views/{app,space}/magic.py` | 359-364 |
//! | [`oauth`] | `authentication/views/{app,space}/{google,github,gitlab,gitea}.py` | 365-376, 385-388 |
//! | [`check`] | `authentication/views/{app,space}/check.py` | 377-378 |
//! | [`password`] | `authentication/views/{app,space}/password_management.py`, `views/common.py:48-139` | 379-384 |
//!
//! The group is split by flow, one module per plan set (`plans/AUTH_FLOWS.md`
//! §3): set A owns the session routes, set B the magic-code routes and set C
//! the OAuth routes. Each set adds its module and its line to [`register`];
//! the pieces more than one set needs live in [`support`] (one copy, not one
//! per set).
//!
//! ## Magic link (set B)
//!
//! ### The two flow shapes
//!
//! * **359/362** are DRF `APIView`s: `AllowAny` + `AuthenticationThrottle`, a
//!   JSON body and a JSON answer. The view's whole body is one `try` whose
//!   `except AuthenticationException` answers 400 with `get_error_dict()`.
//! * **360/361/363/364** are plain `django.views.View`s: Django's CSRF
//!   middleware (layer 5) guards them, `@throttle_auth_redirect`
//!   (`authentication/rate_limit.py:62-95`) runs `AuthenticationThrottle` by
//!   hand *before* the handler, the body arrives as `request.POST` (form or
//!   multipart), and every answer is a `302` to the app or space base URL.
//!
//! ### Behaviour that looks wrong and is ported as-is
//!
//! * **An invalid or empty e-mail is a 500 on 359/362.** The view calls
//!   `django.core.validators.validate_email` (`views/app/magic.py:53`) outside
//!   any handler for `ValidationError`; DRF's `exception_handler` does not map
//!   it either, so the process answers Django's 500 page. `/auth/email-check/`
//!   answers the same input with `INVALID_EMAIL`, which makes the difference
//!   easy to trip over. This is why `test_without_data`/`test_email_validity`
//!   only pass in-process (they catch the exception).
//! * `{"email": null}` is `AttributeError: 'NoneType' object has no attribute
//!   'strip'` (`:51`), also a 500.
//! * The verification path builds the provider with `key=f"magic_{email}"`
//!   (`:102`), so the `SMTP_NOT_CONFIGURED`/`MAGIC_LINK_LOGIN_DISABLED` payload
//!   carries `magic_<email>`, while the generate path carries the bare address
//!   (`magic_code.py:71,78`).
//! * `get_redirection_path` returns bare slugs and `"onboarding"`
//!   (`redirection_path.py:14,30,46`), which `validate_next_path` drops for
//!   lack of a leading `/`: every app sign-in/up without a `next_path` lands on
//!   the base URL, never on a workspace.
//! * The space success redirect is `base_host(...).rstrip("/") +
//!   validate_next_path(next_path)` (`space/magic.py:106`), so an absent
//!   `next_path` produces the base **without** its trailing slash.
//! * `login()` rotates the CSRF secret (`django/contrib/auth/__init__.py:217`),
//!   so every successful sign-in/up answers with a fresh `csrftoken` cookie even
//!   when the request carried none.
//! * Sign-up answers `USER_ALREADY_EXIST` for an address that differs only in
//!   case from a stored one only when `H30` has already lower-cased it; the
//!   lookup itself is exact (`magic.py:157`).

pub mod check;
pub mod csrf;
pub mod email;
pub mod magic;
pub mod oauth;
pub mod password;
pub mod queries;
pub mod signout;
pub(crate) mod support;

use plane_drf::registry::{Registry, RegistryError};

/// Register every `/auth/` route this crate owns, in `routes.json`'s order.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    email::register(registry)?;
    signout::register(registry)?;
    csrf::register(registry)?;
    magic::register(registry)?;
    oauth::register(registry)?;
    check::register(registry)?;
    password::register(registry)?;
    Ok(())
}
