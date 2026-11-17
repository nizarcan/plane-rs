// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `AUTH_FLOWS.md` §2i — the instance row and the configuration flags every
//! `/auth/` view reads.
//!
//! Two layers:
//!
//! * [`first`], [`is_setup_done`] and [`require_setup`] are
//!   `Instance.objects.first()` and the `instance is None or not
//!   instance.is_setup_done` gate (`plane/authentication/views/*`: routes
//!   352-361 and 377-379).
//! * one function per Python `get_configuration_value([...])` **call site**,
//!   so the per-call-site defaults and predicates stay visible instead of
//!   collapsing into one "flag" per key. The database row beats the environment
//!   and `NULL` is not `""` (see [`crate::instance_config`]).
//!
//! ## Seeded defaults that contradict the read sites
//!
//! `ENV.md` §1.9/§5 and `plane/utils/instance_config_variables/core.py`:
//!
//! * `ENABLE_MAGIC_LINK_LOGIN` is seeded `"0"` (`core.py:22-23`) while every
//!   read site falls back to `"1"` (`magic_code.py:62`, `check.py:49`), so a
//!   fresh install has magic links **off** even though the code default is on.
//! * `ENABLE_SIGNUP` is seeded `"1"` (`core.py:10-11`) and the adapter falls
//!   back to `"1"` (`base.py:104`), but `license/api/views/instance.py:68`
//!   reports a `"0"` fallback. The auth surface only sees the first two.
//! * `EMAIL_HOST` is seeded `""` (`ENV.md:634`), which is falsy and therefore
//!   "SMTP not configured".
//! * `ENABLE_EMAIL_PASSWORD` is seeded `"1"` (`core.py:16-17`) while
//!   `email.py:30` falls back to `None`; both mean "enabled" (`None != "0"`).
//!
//! ## What the `/auth/` surface does **not** read
//!
//! `IS_GOOGLE_ENABLED`, `IS_GITHUB_ENABLED`, `IS_GITLAB_ENABLED` and
//! `IS_GITEA_ENABLED` are never read under `plane/authentication/`: the only
//! reader is the instance-admin endpoint (`license/api/views/instance.py:122`,
//! Phase 7). The OAuth providers gate on their client id/secret/host instead,
//! which is why those four reads are here ([`google`], [`github`], [`gitlab`],
//! [`gitea`]) and the `IS_*` flags are not.

use deadpool_postgres::Client;
use plane_core::config::Settings;
use plane_db::models::Instance;

use crate::error::AuthError;
use crate::instance_config::configuration_values;

use super::adapter::AdapterError;
use super::error::{AuthErrorCode, AuthFlowError};

// ---------------------------------------------------------------------------
// Instance.objects.first()
// ---------------------------------------------------------------------------

/// `Instance.objects.first()`: the newest row (`Meta.ordering =
/// ("-created_at",)`, `license/models/instance.py:44`), soft-deleted rows
/// excluded by the default manager (`plane/db/mixins.py:57`).
///
/// # Errors
///
/// A database failure as [`AuthError::Database`].
pub async fn first(client: &Client) -> Result<Option<Instance>, AuthError> {
    let statement = crate::queries::LOAD_FIRST_INSTANCE.prepare(client).await?;
    let row = client.query_opt(&statement, &[]).await?;
    match row {
        Some(row) => Instance::from_row(&row, 0)
            .map(Some)
            .map_err(|error| AuthError::Database(error.to_string())),
        None => Ok(None),
    }
}

/// `instance is not None and instance.is_setup_done` — the half of the gate the
/// views test after loading the row.
///
/// # Errors
///
/// See [`first`].
pub async fn is_setup_done(client: &Client) -> Result<bool, AuthError> {
    Ok(first(client)
        .await?
        .is_some_and(|instance| instance.is_setup_done))
}

/// ```python
/// instance = Instance.objects.first()
/// if instance is None or not instance.is_setup_done:
///     exc = AuthenticationException(
///         error_code=AUTHENTICATION_ERROR_CODES["INSTANCE_NOT_CONFIGURED"],
///         error_message="INSTANCE_NOT_CONFIGURED",
///     )
///     return Response(exc.get_error_dict(), status=status.HTTP_400_BAD_REQUEST)
/// ```
///
/// (`views/app/{magic,email,check,password_management}.py`,
/// `views/space/{magic,email,check,password_management}.py`.)
///
/// # Errors
///
/// [`AdapterError::Auth`] holding `INSTANCE_NOT_CONFIGURED` — the view's 400 —
/// or [`AdapterError::Internal`] for a database failure (Python's 500).
pub async fn require_setup(client: &Client) -> Result<(), AdapterError> {
    match is_setup_done(client).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(AdapterError::Auth(AuthFlowError::new(
            AuthErrorCode::InstanceNotConfigured,
        ))),
        Err(error) => Err(AdapterError::Internal(error.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Python predicates
// ---------------------------------------------------------------------------

/// `bool(value)` for `get_configuration_value`'s `str | None`.
fn truthy(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// ENABLE_SIGNUP
// ---------------------------------------------------------------------------

/// `(ENABLE_SIGNUP,)` from `Adapter.__check_signup` (`base.py:103-106`):
/// `default=os.environ.get("ENABLE_SIGNUP", "1")`.
///
/// The gate disables signup only for the exact string `"0"` (`base.py:108`), so
/// any other value — `"1"`, `"true"`, an empty string, `NULL` with no
/// environment fallback — leaves it enabled. Feed the result to
/// [`super::adapter::signup_allowed`], which takes the same boolean.
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn signup_enabled(client: &Client, settings: &Settings) -> Result<bool, AuthError> {
    let values = configuration_values(client, settings, &[("ENABLE_SIGNUP", Some("1"))]).await?;
    Ok(values[0].as_deref() != Some("0"))
}

// ---------------------------------------------------------------------------
// ENABLE_EMAIL_PASSWORD
// ---------------------------------------------------------------------------

/// `(ENABLE_EMAIL_PASSWORD,)` from `EmailProvider.__init__`
/// (`provider/credentials/email.py:27-38`), whose default is
/// `os.environ.get("ENABLE_EMAIL_PASSWORD")` — `None`, not `"1"`. Disabled iff
/// the value is exactly `"0"`, so `None` means enabled.
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn email_password_login_enabled(
    client: &Client,
    settings: &Settings,
) -> Result<bool, AuthError> {
    let values = configuration_values(client, settings, &[("ENABLE_EMAIL_PASSWORD", None)]).await?;
    Ok(values[0].as_deref() != Some("0"))
}

// ---------------------------------------------------------------------------
// EMAIL_HOST + ENABLE_MAGIC_LINK_LOGIN (magic-code provider)
// ---------------------------------------------------------------------------

/// The two keys `MagicCodeProvider.__init__` reads in **one**
/// `get_configuration_value` call (`provider/credentials/magic_code.py:57-65`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MagicLinkConfiguration {
    /// `EMAIL_HOST`: `None` for a `NULL` row with no environment fallback.
    pub email_host: Option<String>,
    /// `ENABLE_MAGIC_LINK_LOGIN`, raw.
    pub enable_magic_link_login: Option<String>,
}

impl MagicLinkConfiguration {
    /// `if not (EMAIL_HOST)` (`magic_code.py:67`) — an empty string is not
    /// configured either.
    #[must_use]
    pub fn smtp_configured(&self) -> bool {
        truthy(self.email_host.as_deref())
    }

    /// `if ENABLE_MAGIC_LINK_LOGIN == "0"` (`magic_code.py:74`), the provider's
    /// `MAGIC_LINK_LOGIN_DISABLED` gate.
    #[must_use]
    pub fn login_disabled(&self) -> bool {
        self.enable_magic_link_login.as_deref() == Some("0")
    }
}

/// `(EMAIL_HOST, ENABLE_MAGIC_LINK_LOGIN)` from `MagicCodeProvider.__init__`
/// (`provider/credentials/magic_code.py:57-65`): `EMAIL_HOST` defaults to
/// `os.environ.get("EMAIL_HOST")` and the flag to
/// `os.environ.get("ENABLE_MAGIC_LINK_LOGIN", "1")`.
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn magic_link_configuration(
    client: &Client,
    settings: &Settings,
) -> Result<MagicLinkConfiguration, AuthError> {
    let values = configuration_values(
        client,
        settings,
        &[("EMAIL_HOST", None), ("ENABLE_MAGIC_LINK_LOGIN", Some("1"))],
    )
    .await?;
    let mut values = values.into_iter();
    Ok(MagicLinkConfiguration {
        email_host: values.next().flatten(),
        enable_magic_link_login: values.next().flatten(),
    })
}

// ---------------------------------------------------------------------------
// EMAIL_HOST + ENABLE_MAGIC_LINK_LOGIN (email-check views)
// ---------------------------------------------------------------------------

/// `POST /auth/email-check/` and `/auth/spaces/email-check/`
/// (`views/app/check.py:44-55`, `views/space/check.py:44-55`): one
/// `get_configuration_value` call for the same two keys, with
/// `default=os.environ.get("EMAIL_HOST", "")` and
/// `default=os.environ.get("ENABLE_MAGIC_LINK_LOGIN", "1")`.
///
/// The `EMAIL_HOST` default differs from the magic provider's `None`, but it is
/// only read through `bool()`, where `None` and `""` are the same.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmailCheckConfiguration {
    /// `bool(EMAIL_HOST)` — `smtp_configured` in the response.
    pub smtp_configured: bool,
    /// `ENABLE_MAGIC_LINK_LOGIN == "1"` — `is_magic_login_enabled` in the
    /// response. Note the exact-`"1"` comparison, unlike the provider's
    /// exact-`"0"` gate.
    pub magic_link_login_enabled: bool,
}

/// The email-check view's two keys (`check.py:44-55`).
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn email_check_configuration(
    client: &Client,
    settings: &Settings,
) -> Result<EmailCheckConfiguration, AuthError> {
    let values = configuration_values(
        client,
        settings,
        &[
            ("EMAIL_HOST", Some("")),
            ("ENABLE_MAGIC_LINK_LOGIN", Some("1")),
        ],
    )
    .await?;
    Ok(EmailCheckConfiguration {
        smtp_configured: truthy(values[0].as_deref()),
        magic_link_login_enabled: values[1].as_deref() == Some("1"),
    })
}

// ---------------------------------------------------------------------------
// EMAIL_HOST (forgot-password views)
// ---------------------------------------------------------------------------

/// `(EMAIL_HOST,)` from `ForgotPasswordEndpoint.post`
/// (`views/app/password_management.py:62`) with the default
/// `os.environ.get("EMAIL_HOST")`; the view's gate is `if not (EMAIL_HOST)`
/// (`:64`).
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn smtp_configured(client: &Client, settings: &Settings) -> Result<bool, AuthError> {
    let values = configuration_values(client, settings, &[("EMAIL_HOST", None)]).await?;
    Ok(truthy(values[0].as_deref()))
}

/// The three keys `ForgotPasswordSpaceEndpoint.post` reads in one call
/// (`views/space/password_management.py:62-74`), each defaulting to
/// `os.environ.get(...)`.
///
/// Only `EMAIL_HOST` is used by the view (`:76`); the user and password are
/// read but never inspected there. They are kept because the query is the same
/// one call in Python, and a future SMTP path needs them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmailConfiguration {
    pub host: Option<String>,
    pub host_user: Option<String>,
    pub host_password: Option<String>,
}

impl EmailConfiguration {
    /// `if not (EMAIL_HOST)` (`space/password_management.py:76`).
    #[must_use]
    pub fn smtp_configured(&self) -> bool {
        truthy(self.host.as_deref())
    }
}

/// `(EMAIL_HOST, EMAIL_HOST_USER, EMAIL_HOST_PASSWORD)` for the space
/// forgot-password view (`views/space/password_management.py:62-74`).
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn email_configuration(
    client: &Client,
    settings: &Settings,
) -> Result<EmailConfiguration, AuthError> {
    let values = configuration_values(
        client,
        settings,
        &[
            ("EMAIL_HOST", None),
            ("EMAIL_HOST_USER", None),
            ("EMAIL_HOST_PASSWORD", None),
        ],
    )
    .await?;
    let mut values = values.into_iter();
    Ok(EmailConfiguration {
        host: values.next().flatten(),
        host_user: values.next().flatten(),
        host_password: values.next().flatten(),
    })
}

// ---------------------------------------------------------------------------
// ENABLE_<PROVIDER>_SYNC
// ---------------------------------------------------------------------------

/// The provider → key map in `Adapter.check_sync_enabled` (`base.py:124-134`).
///
/// Returns `None` for any other provider name: Python's
/// `provider_config_map.get(self.provider)` is `None`, `config_key` is falsy
/// and the method answers `False` **without querying**.
#[must_use]
pub fn sync_configuration_key(provider: &str) -> Option<&'static str> {
    match provider {
        "google" => Some("ENABLE_GOOGLE_SYNC"),
        "github" => Some("ENABLE_GITHUB_SYNC"),
        "gitlab" => Some("ENABLE_GITLAB_SYNC"),
        "gitea" => Some("ENABLE_GITEA_SYNC"),
        _ => None,
    }
}

/// `Adapter.check_sync_enabled` (`base.py:122-134`):
/// `get_configuration_value([{"key": key, "default": os.environ.get(key, "0")}])[0]
/// == "1"`, and `False` for an unknown provider.
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn sync_enabled(
    client: &Client,
    settings: &Settings,
    provider: &str,
) -> Result<bool, AuthError> {
    let Some(key) = sync_configuration_key(provider) else {
        return Ok(false);
    };
    let values = configuration_values(client, settings, &[(key, Some("0"))]).await?;
    Ok(values[0].as_deref() == Some("1"))
}

// ---------------------------------------------------------------------------
// OAuth providers
// ---------------------------------------------------------------------------

/// `(GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET)` from
/// `GoogleOAuthProvider.__init__` (`provider/oauth/google.py:28-45`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoogleConfiguration {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

impl GoogleConfiguration {
    /// `if not (GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET)`
    /// (`google.py:41`) — `GOOGLE_NOT_CONFIGURED`.
    #[must_use]
    pub fn configured(&self) -> bool {
        truthy(self.client_id.as_deref()) && truthy(self.client_secret.as_deref())
    }
}

/// `(GITHUB_CLIENT_ID, GITHUB_CLIENT_SECRET, GITHUB_ORGANIZATION_ID)`
/// (`provider/oauth/github.py:34-47`). The organization id is optional: a
/// truthy value adds the `read:org` scope and enables the membership check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GithubConfiguration {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub organization_id: Option<String>,
}

impl GithubConfiguration {
    /// `if not (GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET)` (`github.py:49`).
    #[must_use]
    pub fn configured(&self) -> bool {
        truthy(self.client_id.as_deref()) && truthy(self.client_secret.as_deref())
    }

    /// `if self.organization_id:` (`github.py:58`).
    #[must_use]
    pub fn organization_scoped(&self) -> bool {
        truthy(self.organization_id.as_deref())
    }
}

/// `(GITLAB_CLIENT_ID, GITLAB_CLIENT_SECRET, GITLAB_HOST)`
/// (`provider/oauth/gitlab.py:26-47`). `GITLAB_HOST` defaults to
/// `os.environ.get("GITLAB_HOST", "https://gitlab.com")` — the literal is the
/// call site's default, so an unset environment keeps `gitlab.com`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitlabConfiguration {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub host: Option<String>,
}

impl GitlabConfiguration {
    /// `if not (GITLAB_CLIENT_ID and GITLAB_CLIENT_SECRET and GITLAB_HOST)`
    /// (`gitlab.py:47`).
    #[must_use]
    pub fn configured(&self) -> bool {
        truthy(self.client_id.as_deref())
            && truthy(self.client_secret.as_deref())
            && truthy(self.host.as_deref())
    }
}

/// `(GITEA_CLIENT_ID, GITEA_CLIENT_SECRET, GITEA_HOST)`
/// (`provider/oauth/gitea.py:25-45`). No host default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GiteaConfiguration {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub host: Option<String>,
}

impl GiteaConfiguration {
    /// `if not (GITEA_CLIENT_ID and GITEA_CLIENT_SECRET and GITEA_HOST)`
    /// (`gitea.py:42`).
    #[must_use]
    pub fn configured(&self) -> bool {
        truthy(self.client_id.as_deref())
            && truthy(self.client_secret.as_deref())
            && truthy(self.host.as_deref())
    }
}

/// `GoogleOAuthProvider.__init__`'s configuration (`google.py:28-39`).
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn google(
    client: &Client,
    settings: &Settings,
) -> Result<GoogleConfiguration, AuthError> {
    let values = configuration_values(
        client,
        settings,
        &[("GOOGLE_CLIENT_ID", None), ("GOOGLE_CLIENT_SECRET", None)],
    )
    .await?;
    let mut values = values.into_iter();
    Ok(GoogleConfiguration {
        client_id: values.next().flatten(),
        client_secret: values.next().flatten(),
    })
}

/// `GithubOAuthProvider.__init__`'s configuration (`github.py:34-46`).
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn github(
    client: &Client,
    settings: &Settings,
) -> Result<GithubConfiguration, AuthError> {
    let values = configuration_values(
        client,
        settings,
        &[
            ("GITHUB_CLIENT_ID", None),
            ("GITHUB_CLIENT_SECRET", None),
            ("GITHUB_ORGANIZATION_ID", None),
        ],
    )
    .await?;
    let mut values = values.into_iter();
    Ok(GithubConfiguration {
        client_id: values.next().flatten(),
        client_secret: values.next().flatten(),
        organization_id: values.next().flatten(),
    })
}

/// `GitLabOAuthProvider.__init__`'s configuration (`gitlab.py:26-40`).
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn gitlab(
    client: &Client,
    settings: &Settings,
) -> Result<GitlabConfiguration, AuthError> {
    let values = configuration_values(
        client,
        settings,
        &[
            ("GITLAB_CLIENT_ID", None),
            ("GITLAB_CLIENT_SECRET", None),
            ("GITLAB_HOST", Some("https://gitlab.com")),
        ],
    )
    .await?;
    let mut values = values.into_iter();
    Ok(GitlabConfiguration {
        client_id: values.next().flatten(),
        client_secret: values.next().flatten(),
        host: values.next().flatten(),
    })
}

/// `GiteaOAuthProvider.__init__`'s configuration (`gitea.py:25-39`).
///
/// # Errors
///
/// See [`crate::instance_config::configuration_values`].
pub async fn gitea(client: &Client, settings: &Settings) -> Result<GiteaConfiguration, AuthError> {
    let values = configuration_values(
        client,
        settings,
        &[
            ("GITEA_CLIENT_ID", None),
            ("GITEA_CLIENT_SECRET", None),
            ("GITEA_HOST", None),
        ],
    )
    .await?;
    let mut values = values.into_iter();
    Ok(GiteaConfiguration {
        client_id: values.next().flatten(),
        client_secret: values.next().flatten(),
        host: values.next().flatten(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_predicates_are_the_python_comparisons() {
        // `magic_code.py:74`: disabled only for the exact string `"0"`.
        assert!(
            MagicLinkConfiguration {
                enable_magic_link_login: Some("0".to_string()),
                ..MagicLinkConfiguration::default()
            }
            .login_disabled()
        );
        for value in ["1", "true", ""] {
            assert!(
                !MagicLinkConfiguration {
                    enable_magic_link_login: Some(value.to_string()),
                    ..MagicLinkConfiguration::default()
                }
                .login_disabled(),
                "{value:?} is not \"0\""
            );
        }
        assert!(!MagicLinkConfiguration::default().login_disabled());

        // `bool(EMAIL_HOST)`: `None` and `""` are falsy, `"0"` is truthy.
        assert!(!truthy(Some("")));
        assert!(!truthy(None));
        assert!(truthy(Some("0")));

        // `check.py:55` compares with `"1"`, not `"0"`.
        let check = |value: Option<&str>| EmailCheckConfiguration {
            smtp_configured: truthy(value),
            magic_link_login_enabled: value == Some("1"),
        };
        assert!(!check(Some("")).smtp_configured);
        assert!(!check(None).smtp_configured);
        assert!(check(Some("0")).smtp_configured);
        assert!(check(Some("1")).magic_link_login_enabled);
        assert!(!check(Some("0")).magic_link_login_enabled);
    }

    #[test]
    fn sync_keys_are_the_four_providers() {
        assert_eq!(sync_configuration_key("google"), Some("ENABLE_GOOGLE_SYNC"));
        assert_eq!(sync_configuration_key("github"), Some("ENABLE_GITHUB_SYNC"));
        assert_eq!(sync_configuration_key("gitlab"), Some("ENABLE_GITLAB_SYNC"));
        assert_eq!(sync_configuration_key("gitea"), Some("ENABLE_GITEA_SYNC"));
        assert_eq!(sync_configuration_key("magic-code"), None);
        assert_eq!(sync_configuration_key("email"), None);
    }

    #[test]
    fn oauth_configuration_gates_match_the_providers() {
        assert!(
            GoogleConfiguration {
                client_id: Some("id".to_string()),
                client_secret: Some("secret".to_string()),
            }
            .configured()
        );
        assert!(
            !GoogleConfiguration {
                client_id: Some("id".to_string()),
                client_secret: Some(String::new()),
            }
            .configured()
        );
        // GitHub does not require the organization id.
        assert!(
            GithubConfiguration {
                client_id: Some("id".to_string()),
                client_secret: Some("secret".to_string()),
                organization_id: None,
            }
            .configured()
        );
        // GitLab's host default makes it configured with an id and secret.
        assert!(
            GitlabConfiguration {
                client_id: Some("id".to_string()),
                client_secret: Some("secret".to_string()),
                host: Some("https://gitlab.com".to_string()),
            }
            .configured()
        );
        // Gitea has no host default.
        assert!(
            !GiteaConfiguration {
                client_id: Some("id".to_string()),
                client_secret: Some("secret".to_string()),
                host: None,
            }
            .configured()
        );
    }
}
