// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `auth` group (`/auth/`, `deploy/route-groups.yaml`).
//!
//! Phase 6 splits the 37 auth routes into three sets. This module is set B —
//! **magic link** — and owns routes 359-364 (`plans/AUTH_FLOWS.md` §3):
//!
//! | # | Route | Python | Handler |
//! | --- | --- | --- | --- |
//! | 359 | `POST /auth/magic-generate/` | `views/app/magic.py:36-61` | [`generate`] |
//! | 360 | `POST /auth/magic-sign-in/` | `views/app/magic.py:64-133` | [`sign_in`] |
//! | 361 | `POST /auth/magic-sign-up/` | `views/app/magic.py:136-201` | [`sign_up`] |
//! | 362 | `POST /auth/spaces/magic-generate/` | `views/space/magic.py:35-59` | [`generate_space`] |
//! | 363 | `POST /auth/spaces/magic-sign-in/` | `views/space/magic.py:62-119` | [`space_sign_in`] |
//! | 364 | `POST /auth/spaces/magic-sign-up/` | `views/space/magic.py:122-178` | [`space_sign_up`] |
//!
//! Sets A (`email`, `signout`, `csrf`, `check`, `password`) and C (`oauth`) own
//! the rest and add their own `register` calls to this module; the orchestrator
//! merges them.
//!
//! ## The two flow shapes
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
//! ## Behaviour that looks wrong and is ported as-is
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

pub mod magic;
pub mod queries;

use plane_drf::registry::{Registry, RegistryError};

/// Register set B's routes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    magic::register(registry)
}
