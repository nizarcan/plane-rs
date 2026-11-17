// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/adapter/error.py`: the auth error codes and the
//! `AuthenticationException` payload.
//!
//! ```python
//! AUTHENTICATION_ERROR_CODES = { "INSTANCE_NOT_CONFIGURED": 5000, ... }
//!
//! class AuthenticationException(Exception):
//!     error_code = None
//!     error_message = None
//!     payload = {}
//!     def __init__(self, error_code, error_message, payload={}):
//!         ...
//!     def get_error_dict(self):
//!         error = {"error_code": self.error_code, "error_message": self.error_message}
//!         for key in self.payload:
//!             error[key] = self.payload[key]
//!         return error
//! ```
//!
//! Every call site passes the dictionary key as `error_message`
//! (`error_message="INVALID_EMAIL"`), so [`AuthErrorCode::message`] is the
//! variant name and [`AuthFlowError`] has no separate message.
//!
//! `get_error_dict` keeps Python's insertion order: the two fixed keys first,
//! then the payload in the order the call site built it. The JSON views render
//! it through [`Object`](plane_core::json::Object) and the redirect views
//! through [`AuthFlowError::redirect_params`], which stringifies every value the
//! way `urllib.parse.urlencode` does (`str(None) == "None"`).

use plane_core::json::Object;
use serde_json::Value;

/// `AUTHENTICATION_ERROR_CODES` (`error.py:5-82`), in file order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthErrorCode {
    // Global
    InstanceNotConfigured,
    InvalidEmail,
    EmailRequired,
    SignupDisabled,
    MagicLinkLoginDisabled,
    BotUserLoginForbidden,
    PasswordLoginDisabled,
    UserAccountDeactivated,
    // Password strength
    InvalidPassword,
    PasswordTooWeak,
    SmtpNotConfigured,
    // Sign up
    UserAlreadyExist,
    AuthenticationFailedSignUp,
    RequiredEmailPasswordSignUp,
    InvalidEmailSignUp,
    InvalidEmailMagicSignUp,
    MagicSignUpEmailCodeRequired,
    EmailPasswordAuthenticationDisabled,
    // Sign in
    UserDoesNotExist,
    AuthenticationFailedSignIn,
    RequiredEmailPasswordSignIn,
    InvalidEmailSignIn,
    InvalidEmailMagicSignIn,
    MagicSignInEmailCodeRequired,
    // Both sign in and sign up for magic
    InvalidMagicCodeSignIn,
    InvalidMagicCodeSignUp,
    ExpiredMagicCodeSignIn,
    ExpiredMagicCodeSignUp,
    EmailCodeAttemptExhaustedSignIn,
    EmailCodeAttemptExhaustedSignUp,
    // OAuth
    OauthNotConfigured,
    GoogleNotConfigured,
    GithubNotConfigured,
    GithubUserNotInOrg,
    GitlabNotConfigured,
    GiteaNotConfigured,
    GoogleOauthProviderError,
    GithubOauthProviderError,
    GitlabOauthProviderError,
    GiteaOauthProviderError,
    OauthProviderUnverifiedEmail,
    // Reset password
    InvalidPasswordToken,
    ExpiredPasswordToken,
    // Change password
    IncorrectOldPassword,
    MissingPassword,
    InvalidNewPassword,
    // Set password
    PasswordAlreadySet,
    // Admin
    AdminAlreadyExist,
    RequiredAdminEmailPasswordFirstName,
    InvalidAdminEmail,
    InvalidAdminPassword,
    RequiredAdminEmailPassword,
    AdminAuthenticationFailed,
    AdminUserAlreadyExist,
    AdminUserDoesNotExist,
    AdminUserDeactivated,
    // Rate limit
    RateLimitExceeded,
    // Unknown
    AuthenticationFailed,
}

impl AuthErrorCode {
    /// The integer in `AUTHENTICATION_ERROR_CODES`.
    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::InstanceNotConfigured => 5000,
            Self::InvalidEmail => 5005,
            Self::EmailRequired => 5010,
            Self::SignupDisabled => 5015,
            Self::MagicLinkLoginDisabled => 5016,
            Self::BotUserLoginForbidden => 5017,
            Self::PasswordLoginDisabled => 5018,
            Self::UserAccountDeactivated => 5019,
            Self::InvalidPassword => 5020,
            Self::PasswordTooWeak => 5021,
            Self::SmtpNotConfigured => 5025,
            Self::UserAlreadyExist => 5030,
            Self::AuthenticationFailedSignUp => 5035,
            Self::RequiredEmailPasswordSignUp => 5040,
            Self::InvalidEmailSignUp => 5045,
            Self::InvalidEmailMagicSignUp => 5050,
            Self::MagicSignUpEmailCodeRequired => 5055,
            Self::EmailPasswordAuthenticationDisabled => 5056,
            Self::UserDoesNotExist => 5060,
            Self::AuthenticationFailedSignIn => 5065,
            Self::RequiredEmailPasswordSignIn => 5070,
            Self::InvalidEmailSignIn => 5075,
            Self::InvalidEmailMagicSignIn => 5080,
            Self::MagicSignInEmailCodeRequired => 5085,
            Self::InvalidMagicCodeSignIn => 5090,
            Self::InvalidMagicCodeSignUp => 5092,
            Self::ExpiredMagicCodeSignIn => 5095,
            Self::ExpiredMagicCodeSignUp => 5097,
            Self::EmailCodeAttemptExhaustedSignIn => 5100,
            Self::EmailCodeAttemptExhaustedSignUp => 5102,
            Self::OauthNotConfigured => 5104,
            Self::GoogleNotConfigured => 5105,
            Self::GithubNotConfigured => 5110,
            Self::GithubUserNotInOrg => 5122,
            Self::GitlabNotConfigured => 5111,
            Self::GiteaNotConfigured => 5112,
            Self::GoogleOauthProviderError => 5115,
            Self::GithubOauthProviderError => 5120,
            Self::GitlabOauthProviderError => 5121,
            Self::GiteaOauthProviderError => 5123,
            Self::OauthProviderUnverifiedEmail => 5124,
            Self::InvalidPasswordToken => 5125,
            Self::ExpiredPasswordToken => 5130,
            Self::IncorrectOldPassword => 5135,
            Self::MissingPassword => 5138,
            Self::InvalidNewPassword => 5140,
            Self::PasswordAlreadySet => 5145,
            Self::AdminAlreadyExist => 5150,
            Self::RequiredAdminEmailPasswordFirstName => 5155,
            Self::InvalidAdminEmail => 5160,
            Self::InvalidAdminPassword => 5165,
            Self::RequiredAdminEmailPassword => 5170,
            Self::AdminAuthenticationFailed => 5175,
            Self::AdminUserAlreadyExist => 5180,
            Self::AdminUserDoesNotExist => 5185,
            Self::AdminUserDeactivated => 5190,
            Self::RateLimitExceeded => 5900,
            Self::AuthenticationFailed => 5999,
        }
    }

    /// `error_message`: the dictionary key, always its own text.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::InstanceNotConfigured => "INSTANCE_NOT_CONFIGURED",
            Self::InvalidEmail => "INVALID_EMAIL",
            Self::EmailRequired => "EMAIL_REQUIRED",
            Self::SignupDisabled => "SIGNUP_DISABLED",
            Self::MagicLinkLoginDisabled => "MAGIC_LINK_LOGIN_DISABLED",
            Self::BotUserLoginForbidden => "BOT_USER_LOGIN_FORBIDDEN",
            Self::PasswordLoginDisabled => "PASSWORD_LOGIN_DISABLED",
            Self::UserAccountDeactivated => "USER_ACCOUNT_DEACTIVATED",
            Self::InvalidPassword => "INVALID_PASSWORD",
            Self::PasswordTooWeak => "PASSWORD_TOO_WEAK",
            Self::SmtpNotConfigured => "SMTP_NOT_CONFIGURED",
            Self::UserAlreadyExist => "USER_ALREADY_EXIST",
            Self::AuthenticationFailedSignUp => "AUTHENTICATION_FAILED_SIGN_UP",
            Self::RequiredEmailPasswordSignUp => "REQUIRED_EMAIL_PASSWORD_SIGN_UP",
            Self::InvalidEmailSignUp => "INVALID_EMAIL_SIGN_UP",
            Self::InvalidEmailMagicSignUp => "INVALID_EMAIL_MAGIC_SIGN_UP",
            Self::MagicSignUpEmailCodeRequired => "MAGIC_SIGN_UP_EMAIL_CODE_REQUIRED",
            Self::EmailPasswordAuthenticationDisabled => "EMAIL_PASSWORD_AUTHENTICATION_DISABLED",
            Self::UserDoesNotExist => "USER_DOES_NOT_EXIST",
            Self::AuthenticationFailedSignIn => "AUTHENTICATION_FAILED_SIGN_IN",
            Self::RequiredEmailPasswordSignIn => "REQUIRED_EMAIL_PASSWORD_SIGN_IN",
            Self::InvalidEmailSignIn => "INVALID_EMAIL_SIGN_IN",
            Self::InvalidEmailMagicSignIn => "INVALID_EMAIL_MAGIC_SIGN_IN",
            Self::MagicSignInEmailCodeRequired => "MAGIC_SIGN_IN_EMAIL_CODE_REQUIRED",
            Self::InvalidMagicCodeSignIn => "INVALID_MAGIC_CODE_SIGN_IN",
            Self::InvalidMagicCodeSignUp => "INVALID_MAGIC_CODE_SIGN_UP",
            Self::ExpiredMagicCodeSignIn => "EXPIRED_MAGIC_CODE_SIGN_IN",
            Self::ExpiredMagicCodeSignUp => "EXPIRED_MAGIC_CODE_SIGN_UP",
            Self::EmailCodeAttemptExhaustedSignIn => "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_IN",
            Self::EmailCodeAttemptExhaustedSignUp => "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP",
            Self::OauthNotConfigured => "OAUTH_NOT_CONFIGURED",
            Self::GoogleNotConfigured => "GOOGLE_NOT_CONFIGURED",
            Self::GithubNotConfigured => "GITHUB_NOT_CONFIGURED",
            Self::GithubUserNotInOrg => "GITHUB_USER_NOT_IN_ORG",
            Self::GitlabNotConfigured => "GITLAB_NOT_CONFIGURED",
            Self::GiteaNotConfigured => "GITEA_NOT_CONFIGURED",
            Self::GoogleOauthProviderError => "GOOGLE_OAUTH_PROVIDER_ERROR",
            Self::GithubOauthProviderError => "GITHUB_OAUTH_PROVIDER_ERROR",
            Self::GitlabOauthProviderError => "GITLAB_OAUTH_PROVIDER_ERROR",
            Self::GiteaOauthProviderError => "GITEA_OAUTH_PROVIDER_ERROR",
            Self::OauthProviderUnverifiedEmail => "OAUTH_PROVIDER_UNVERIFIED_EMAIL",
            Self::InvalidPasswordToken => "INVALID_PASSWORD_TOKEN",
            Self::ExpiredPasswordToken => "EXPIRED_PASSWORD_TOKEN",
            Self::IncorrectOldPassword => "INCORRECT_OLD_PASSWORD",
            Self::MissingPassword => "MISSING_PASSWORD",
            Self::InvalidNewPassword => "INVALID_NEW_PASSWORD",
            Self::PasswordAlreadySet => "PASSWORD_ALREADY_SET",
            Self::AdminAlreadyExist => "ADMIN_ALREADY_EXIST",
            Self::RequiredAdminEmailPasswordFirstName => "REQUIRED_ADMIN_EMAIL_PASSWORD_FIRST_NAME",
            Self::InvalidAdminEmail => "INVALID_ADMIN_EMAIL",
            Self::InvalidAdminPassword => "INVALID_ADMIN_PASSWORD",
            Self::RequiredAdminEmailPassword => "REQUIRED_ADMIN_EMAIL_PASSWORD",
            Self::AdminAuthenticationFailed => "ADMIN_AUTHENTICATION_FAILED",
            Self::AdminUserAlreadyExist => "ADMIN_USER_ALREADY_EXIST",
            Self::AdminUserDoesNotExist => "ADMIN_USER_DOES_NOT_EXIST",
            Self::AdminUserDeactivated => "ADMIN_USER_DEACTIVATED",
            Self::RateLimitExceeded => "RATE_LIMIT_EXCEEDED",
            Self::AuthenticationFailed => "AUTHENTICATION_FAILED",
        }
    }

    /// The inverse of [`Self::message`]; `None` for an unknown name.
    #[must_use]
    pub fn from_message(message: &str) -> Option<Self> {
        // Match on the stable text so the table above stays the only source.
        ALL.iter().copied().find(|code| code.message() == message)
    }
}

/// Every variant, in `AUTHENTICATION_ERROR_CODES` order.
pub const ALL: [AuthErrorCode; 58] = [
    AuthErrorCode::InstanceNotConfigured,
    AuthErrorCode::InvalidEmail,
    AuthErrorCode::EmailRequired,
    AuthErrorCode::SignupDisabled,
    AuthErrorCode::MagicLinkLoginDisabled,
    AuthErrorCode::BotUserLoginForbidden,
    AuthErrorCode::PasswordLoginDisabled,
    AuthErrorCode::UserAccountDeactivated,
    AuthErrorCode::InvalidPassword,
    AuthErrorCode::PasswordTooWeak,
    AuthErrorCode::SmtpNotConfigured,
    AuthErrorCode::UserAlreadyExist,
    AuthErrorCode::AuthenticationFailedSignUp,
    AuthErrorCode::RequiredEmailPasswordSignUp,
    AuthErrorCode::InvalidEmailSignUp,
    AuthErrorCode::InvalidEmailMagicSignUp,
    AuthErrorCode::MagicSignUpEmailCodeRequired,
    AuthErrorCode::EmailPasswordAuthenticationDisabled,
    AuthErrorCode::UserDoesNotExist,
    AuthErrorCode::AuthenticationFailedSignIn,
    AuthErrorCode::RequiredEmailPasswordSignIn,
    AuthErrorCode::InvalidEmailSignIn,
    AuthErrorCode::InvalidEmailMagicSignIn,
    AuthErrorCode::MagicSignInEmailCodeRequired,
    AuthErrorCode::InvalidMagicCodeSignIn,
    AuthErrorCode::InvalidMagicCodeSignUp,
    AuthErrorCode::ExpiredMagicCodeSignIn,
    AuthErrorCode::ExpiredMagicCodeSignUp,
    AuthErrorCode::EmailCodeAttemptExhaustedSignIn,
    AuthErrorCode::EmailCodeAttemptExhaustedSignUp,
    AuthErrorCode::OauthNotConfigured,
    AuthErrorCode::GoogleNotConfigured,
    AuthErrorCode::GithubNotConfigured,
    AuthErrorCode::GithubUserNotInOrg,
    AuthErrorCode::GitlabNotConfigured,
    AuthErrorCode::GiteaNotConfigured,
    AuthErrorCode::GoogleOauthProviderError,
    AuthErrorCode::GithubOauthProviderError,
    AuthErrorCode::GitlabOauthProviderError,
    AuthErrorCode::GiteaOauthProviderError,
    AuthErrorCode::OauthProviderUnverifiedEmail,
    AuthErrorCode::InvalidPasswordToken,
    AuthErrorCode::ExpiredPasswordToken,
    AuthErrorCode::IncorrectOldPassword,
    AuthErrorCode::MissingPassword,
    AuthErrorCode::InvalidNewPassword,
    AuthErrorCode::PasswordAlreadySet,
    AuthErrorCode::AdminAlreadyExist,
    AuthErrorCode::RequiredAdminEmailPasswordFirstName,
    AuthErrorCode::InvalidAdminEmail,
    AuthErrorCode::InvalidAdminPassword,
    AuthErrorCode::RequiredAdminEmailPassword,
    AuthErrorCode::AdminAuthenticationFailed,
    AuthErrorCode::AdminUserAlreadyExist,
    AuthErrorCode::AdminUserDoesNotExist,
    AuthErrorCode::AdminUserDeactivated,
    AuthErrorCode::RateLimitExceeded,
    AuthErrorCode::AuthenticationFailed,
];

/// Python's `AuthenticationException`.
///
/// `payload` is Python's dict: `{"email": "<value>"}` at every call site found
/// in `plane/authentication`. Values are kept as [`Value`] so the JSON views
/// can emit `null` where Python does (`payload={"email": None}` in
/// `adapter/base.py:66-70`) and the redirect views can stringify it as
/// `"None"`, as `urlencode` does.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthFlowError {
    pub code: AuthErrorCode,
    pub payload: Vec<(String, Value)>,
    /// `AuthenticationException(error_message=…)` when the call site passes
    /// something other than the dictionary key. Three Gitea branches do
    /// (`gitea.py:118-155`): `GITEA_OAUTH_PROVIDER_ERROR: Failed to fetch
    /// emails`, `…: No emails found` and `…: Exception occurred while fetching
    /// emails`. Every other call site passes the code's own name, which is
    /// [`None`] here.
    pub message: Option<String>,
}

impl AuthFlowError {
    #[must_use]
    pub fn new(code: AuthErrorCode) -> Self {
        Self {
            code,
            payload: Vec::new(),
            message: None,
        }
    }

    /// `AuthenticationException(error_message=<message>)`, kept verbatim. The
    /// redirect views put it in the query string, so it is observable.
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// `payload={"email": email}` (`adapter/base.py:66-70` and its callers).
    #[must_use]
    pub fn with_email(code: AuthErrorCode, email: impl Into<String>) -> Self {
        Self {
            code,
            payload: vec![("email".to_string(), Value::String(email.into()))],
            message: None,
        }
    }

    /// A payload key holding `None`, which JSON renders as `null`.
    #[must_use]
    pub fn with_null_email(code: AuthErrorCode) -> Self {
        Self {
            code,
            payload: vec![("email".to_string(), Value::Null)],
            message: None,
        }
    }

    /// Append one payload entry (Python's `payload[key] = value`).
    #[must_use]
    pub fn field(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.payload.push((key.into(), value.into()));
        self
    }

    #[must_use]
    pub fn code(&self) -> i64 {
        self.code.code()
    }

    #[must_use]
    pub fn message(&self) -> &str {
        match &self.message {
            Some(message) => message,
            None => self.code.message(),
        }
    }

    /// `AuthenticationException.get_error_dict()`: `error_code`,
    /// `error_message`, then the payload, in insertion order.
    #[must_use]
    pub fn get_error_dict(&self) -> Object {
        let mut object = Object::new()
            .field("error_code", Value::from(self.code()))
            .field("error_message", Value::from(self.message()));
        for (key, value) in &self.payload {
            object = object.field(key.as_str(), value.clone());
        }
        object
    }

    /// The same dict as `urlencode` input: Python's `str()` of every value.
    ///
    /// `str(None)` is `"None"`, `str(True)` is `"True"` and an integer is its
    /// decimal text, which is why the `stringify` helper is not `to_string` on
    /// a `serde_json::Value`.
    #[must_use]
    pub fn redirect_params(&self) -> Vec<(String, String)> {
        let mut params = vec![
            ("error_code".to_string(), self.code().to_string()),
            ("error_message".to_string(), self.message().to_string()),
        ];
        for (key, value) in &self.payload {
            params.push((key.clone(), python_str(value)));
        }
        params
    }
}

/// Python's `str()` for the value types an auth payload can hold.
fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        // Lists and objects never appear in an auth payload; `str()` on them
        // would be their `repr`, which `urlencode` never sees at a call site.
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::Dialect;

    #[test]
    fn every_code_and_message_is_unique() {
        let mut codes = std::collections::BTreeSet::new();
        let mut messages = std::collections::BTreeSet::new();
        for code in ALL {
            assert!(codes.insert(code.code()), "duplicate code {}", code.code());
            assert!(
                messages.insert(code.message()),
                "duplicate message {}",
                code.message()
            );
            assert_eq!(AuthErrorCode::from_message(code.message()), Some(code));
        }
        assert_eq!(ALL.len(), 58);
    }

    #[test]
    fn the_error_dict_has_planes_order_and_bytes() {
        let error = AuthFlowError::with_email(AuthErrorCode::InvalidEmail, "a@b.com");
        let bytes = plane_serde::to_vec(Dialect::Drf, &error.get_error_dict()).expect("render");
        assert_eq!(
            String::from_utf8(bytes).expect("utf-8"),
            r#"{"error_code":5005,"error_message":"INVALID_EMAIL","email":"a@b.com"}"#
        );
    }

    #[test]
    fn a_null_email_renders_null_in_json_and_none_in_a_query() {
        let error = AuthFlowError::with_null_email(AuthErrorCode::InvalidEmail);
        let bytes = plane_serde::to_vec(Dialect::Drf, &error.get_error_dict()).expect("render");
        assert_eq!(
            String::from_utf8(bytes).expect("utf-8"),
            r#"{"error_code":5005,"error_message":"INVALID_EMAIL","email":null}"#
        );
        assert_eq!(
            error.redirect_params(),
            vec![
                ("error_code".to_string(), "5005".to_string()),
                ("error_message".to_string(), "INVALID_EMAIL".to_string()),
                ("email".to_string(), "None".to_string()),
            ]
        );
    }

    #[test]
    fn codes_are_the_django_table() {
        // Spot checks across the table, including the two non-sequential rows
        // (`GITHUB_USER_NOT_IN_ORG` before `GITLAB_NOT_CONFIGURED`) and 5900.
        assert_eq!(AuthErrorCode::InstanceNotConfigured.code(), 5000);
        assert_eq!(AuthErrorCode::GithubNotConfigured.code(), 5110);
        assert_eq!(AuthErrorCode::GithubUserNotInOrg.code(), 5122);
        assert_eq!(AuthErrorCode::GitlabNotConfigured.code(), 5111);
        assert_eq!(AuthErrorCode::GiteaNotConfigured.code(), 5112);
        assert_eq!(AuthErrorCode::RateLimitExceeded.code(), 5900);
        assert_eq!(AuthErrorCode::AuthenticationFailed.code(), 5999);
    }
}
