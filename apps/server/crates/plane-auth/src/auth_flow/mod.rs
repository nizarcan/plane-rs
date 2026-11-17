// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `/auth/` flows' shared pieces (`apps/server/plans/AUTH_FLOWS.md` §2).
//!
//! | Module | Python | Used by |
//! | --- | --- | --- |
//! | [`redirect`] | `plane/utils/path_validator.py`, `django.utils.http` | every redirect view (352-388) |
//! | [`error`] | `authentication/adapter/error.py` | every view and adapter |
//! | [`email`] | `django.core.validators.validate_email` | [`adapter`], the user write paths |
//! | [`password`] | `authentication/utils/password.py` | signup, change/reset password |
//! | [`reset_token`] | `django.contrib.auth.tokens` | 380, 382 |
//! | [`adapter`] | `authentication/adapter/base.py` | email, magic and OAuth providers |
//! | [`instance`] | `license/models/instance.py`, `license/utils/instance_value.py` | every gated view (§2i) |
//! | [`tasks`] | `plane/bgtasks/*`, `plane/utils/host.py` | the mail tasks and `current_site` (§2h) |
//!
//! The magic-code state (`§2e`) and the OAuth adapter (`§2j`) are P0b and will
//! live in `auth_flow::magic` / `auth_flow::oauth` once they land; the provider
//! trait they implement is defined in [`adapter`] today so the sets share one
//! shape.

pub mod adapter;
pub mod email;
pub mod error;
pub mod instance;
pub mod magic;
pub mod oauth;
pub mod password;
pub mod providers;
pub mod redirect;
pub mod reset_token;
pub mod tasks;

pub use adapter::{
    AdapterContext, AdapterError, AuthCallback, AuthProvider, CompletedLogin, InviteOutcome,
    PostUserAuthWorkflow, complete_login_or_signup, create_profile,
    process_workspace_project_invitations, sanitize_email, save_user_data, signup_allowed,
    validate_password,
};
pub use email::validate_email;
pub use error::{ALL as ALL_AUTH_ERROR_CODES, AuthErrorCode, AuthFlowError};
pub use instance::{
    EmailCheckConfiguration, EmailConfiguration, GiteaConfiguration, GithubConfiguration,
    GitlabConfiguration, GoogleConfiguration, MagicLinkConfiguration, email_check_configuration,
    email_configuration, email_password_login_enabled, first as first_instance, gitea, github,
    gitlab, google, is_setup_done, magic_link_configuration, require_setup, signup_enabled,
    smtp_configured, sync_configuration_key, sync_enabled,
};
pub use magic::{
    ISSUE_WINDOW_SECONDS, IssuedCode, MAX_CODES_PER_WINDOW, MAX_VERIFY_ATTEMPTS, MagicCodeFlags,
    MagicCodeProvider, TOKEN_TTL_SECONDS, generate_code, issued_key, magic_key,
    verify_attempts_key,
};
pub use oauth::{
    OAUTH_STATE_SESSION_KEY, OAuthHttp, OAuthProvider, OauthAdapter, TokenData, TokenRequest,
    create_update_account, oauth_redirect_uri, oauth_state_matches, sync_flag_name,
    validate_oauth_state,
};
pub use password::{is_password_strong, is_password_strong_async};
pub use providers::{GiteaProvider, GithubProvider, GitlabProvider, GoogleProvider};
pub use redirect::{
    PyValueError, RedirectionPath, allowed_hosts, get_safe_redirect_url, settings_allowed_hosts,
    space_redirect_url, url_has_allowed_host_and_scheme, url_has_allowed_host_and_scheme_https,
    urlencode, urljoin, validate_next_path,
};
pub use reset_token::{
    Base36Error, Base64Error, PASSWORD_RESET_TIMEOUT, TokenUser, base36_to_int, check_token,
    int_to_base36, make_token, make_token_with_timestamp, num_seconds, urlsafe_base64_decode,
    urlsafe_base64_encode,
};
pub use tasks::{current_site, forgot_password, magic_link, user_activation_email, view_base_host};
