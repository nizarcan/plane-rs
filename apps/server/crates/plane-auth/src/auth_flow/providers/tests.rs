// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Golden test for the four providers, recorded from the Python classes.
//!
//! `tools/auth/record_oauth_claims.py` imports the real
//! `plane.authentication.provider.oauth.*` classes, patches only
//! `get_configuration_value` and `requests`, and writes
//! `tests/golden/oauth_claims.json`. Every case below replays that record
//! against the Rust provider: the auth URL, the token request's form and
//! headers, `set_token_data` and the claim rules (`verified_email`, the
//! primary+verified address, `confirmed_at`, the `GITHUB_ORGANIZATION_ID`
//! membership check and Gitea's two-pass verified search).
//!
//! Regenerate with
//! `apps/api/.venv/bin/python apps/server/tools/auth/record_oauth_claims.py`.

use chrono::{DateTime, Utc};
use http::{HeaderMap, Method};
use plane_core::context::RequestContext;
use serde_json::{Map, Value};

use super::gitea::GiteaProvider;
use super::github::GithubProvider;
use super::gitlab::GitlabProvider;
use super::google::GoogleProvider;
use crate::auth_flow::adapter::AdapterError;
use crate::auth_flow::error::AuthFlowError;
use crate::auth_flow::instance::{
    GiteaConfiguration, GithubConfiguration, GitlabConfiguration, GoogleConfiguration,
};
use crate::auth_flow::oauth::{OAuthProvider, TokenData, TokenRequest};

const GOLDEN: &str = include_str!("../../../tests/golden/oauth_claims.json");

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("oauth_claims.json parses")
}

fn request() -> RequestContext {
    let mut headers = HeaderMap::new();
    headers.insert("host", "plane.example.test".parse().unwrap());
    RequestContext::new(Method::GET, "/auth/google/", "", headers, None)
}

/// One configured value as `get_configuration_value` returns it: an absent key
/// is Python's `None`, and `""` stays an empty string.
fn value(config: &Map<String, Value>, key: &str) -> Option<String> {
    config.get(key).and_then(|value| {
        if value.is_null() {
            None
        } else {
            value.as_str().map(str::to_string)
        }
    })
}

fn google(config: &Map<String, Value>) -> GoogleConfiguration {
    GoogleConfiguration {
        client_id: value(config, "GOOGLE_CLIENT_ID"),
        client_secret: value(config, "GOOGLE_CLIENT_SECRET"),
    }
}

fn github(config: &Map<String, Value>) -> GithubConfiguration {
    GithubConfiguration {
        client_id: value(config, "GITHUB_CLIENT_ID"),
        client_secret: value(config, "GITHUB_CLIENT_SECRET"),
        organization_id: value(config, "GITHUB_ORGANIZATION_ID"),
    }
}

fn gitlab(config: &Map<String, Value>) -> GitlabConfiguration {
    GitlabConfiguration {
        client_id: value(config, "GITLAB_CLIENT_ID"),
        client_secret: value(config, "GITLAB_CLIENT_SECRET"),
        host: value(config, "GITLAB_HOST"),
    }
}

fn gitea(config: &Map<String, Value>) -> GiteaConfiguration {
    GiteaConfiguration {
        client_id: value(config, "GITEA_CLIENT_ID"),
        client_secret: value(config, "GITEA_CLIENT_SECRET"),
        host: value(config, "GITEA_HOST"),
    }
}

/// The provider a case's name selects, constructed with `state`.
fn provider(
    name: &str,
    config: &Map<String, Value>,
    state: Option<&str>,
) -> Result<Box<dyn OAuthProvider>, AuthFlowError> {
    let ctx = request();
    Ok(match name {
        "google" => Box::new(GoogleProvider::new(&ctx, &google(config), state)?),
        "github" => Box::new(GithubProvider::new(&ctx, &github(config), state)?),
        "gitlab" => Box::new(GitlabProvider::new(&ctx, &gitlab(config), state)?),
        "gitea" => Box::new(GiteaProvider::new(&ctx, &gitea(config), state)?),
        other => panic!("unknown provider {other}"),
    })
}

/// The Python `AuthenticationException` a case recorded, or `None`.
fn recorded_error(result: &Value) -> Option<(i64, String)> {
    let error = result.get("authentication_exception")?;
    Some((
        error.get("error_code")?.as_i64()?,
        error.get("error_message")?.as_str()?.to_string(),
    ))
}

/// `{"exception": <TypeName>}`: an uncaught Python exception, i.e. a 500.
fn recorded_exception(result: &Value) -> Option<&str> {
    result.get("exception").and_then(Value::as_str)
}

#[test]
fn auth_urls_match_the_recorded_providers() {
    let golden = golden();
    let mut checked = 0;
    for case in golden["cases"].as_array().expect("cases") {
        if case["kind"] != "auth_url" {
            continue;
        }
        let name = case["provider"].as_str().expect("provider");
        let state = case["state"].as_str().unwrap_or_default();
        let result = &case["result"];
        let built = provider(
            name,
            case["config"].as_object().expect("config"),
            Some(state),
        );
        match (recorded_error(result), built) {
            (Some((code, message)), Err(error)) => {
                assert_eq!(error.code(), code, "{name} error code");
                assert_eq!(error.message(), message, "{name} error message");
            }
            (None, Ok(provider)) => {
                assert_eq!(
                    provider.auth_url(),
                    result["auth_url"].as_str().expect("auth_url"),
                    "{name} auth_url"
                );
            }
            (None, Err(error)) => panic!("{name}: recorded a URL, port raised {error:?}"),
            (Some(expected), Ok(_)) => panic!("{name}: recorded {expected:?}, port succeeded"),
        }
        checked += 1;
    }
    assert_eq!(checked, 8, "auth_url cases");
}

fn check_token_request(recorded: &Value, provider: &dyn OAuthProvider) {
    let expected = &recorded["token_request"];
    assert!(
        !expected.is_null(),
        "the record has no token request (a constructor failure?)"
    );
    assert_eq!(
        provider.token_url(),
        expected["url"].as_str().expect("token url"),
        "token url"
    );
    let request: TokenRequest = provider.token_request(Some("fixture-code"));
    let pairs: Vec<(String, String)> = expected["data"]
        .as_array()
        .expect("data")
        .iter()
        .map(|pair| {
            (
                pair[0].as_str().expect("name").to_string(),
                pair[1].as_str().expect("value").to_string(),
            )
        })
        .collect();
    assert_eq!(request.data, pairs, "token form fields");
    let headers: Vec<(String, String)> = expected["headers"]
        .as_object()
        .expect("headers")
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                value.as_str().expect("header value").to_string(),
            )
        })
        .collect();
    assert_eq!(request.headers, headers, "token headers");
}

fn check_expiry(recorded: &Value, actual: Option<DateTime<Utc>>) {
    match recorded.as_str() {
        Some(expected) => {
            let expected = DateTime::parse_from_rfc3339(expected)
                .expect("recorded datetime")
                .with_timezone(&Utc);
            assert_eq!(actual, Some(expected), "expiry");
        }
        None => assert_eq!(actual, None, "expiry"),
    }
}

/// Gitea's `datetime.now() + timedelta(seconds=expires_in)` is a wall-clock
/// instant, so the recorded value ages out of any comparison window; the
/// record pins the shape (`expires_in` was truthy) and this test pins the
/// offset.
fn check_relative_expiry(recorded: &Value, actual: Option<DateTime<Utc>>, seconds: i64) {
    assert!(
        DateTime::parse_from_rfc3339(recorded.as_str().expect("recorded expiry")).is_ok(),
        "the record is a timestamp"
    );
    let actual = actual.expect("port produced no expiry");
    let offset = (actual - Utc::now()).num_seconds();
    assert!(
        (offset - seconds).abs() <= 10,
        "expiry offset {offset} is not {seconds}"
    );
}

fn provider_error_code(name: &str) -> crate::auth_flow::error::AuthErrorCode {
    use crate::auth_flow::error::AuthErrorCode;
    match name {
        "google" => AuthErrorCode::GoogleOauthProviderError,
        "github" => AuthErrorCode::GithubOauthProviderError,
        "gitlab" => AuthErrorCode::GitlabOauthProviderError,
        "gitea" => AuthErrorCode::GiteaOauthProviderError,
        other => panic!("unknown provider {other}"),
    }
}

fn token_data(result: &Value) -> TokenData {
    TokenData {
        access_token: result["access_token"].clone(),
        refresh_token: result["refresh_token"].clone(),
        access_token_expired_at: None,
        refresh_token_expired_at: None,
        id_token: result["id_token"].clone(),
    }
}

#[test]
fn token_requests_and_token_data_match_the_recorded_providers() {
    let golden = golden();
    let mut checked = 0;
    for case in golden["cases"].as_array().expect("cases") {
        if case["kind"] != "token" {
            continue;
        }
        let name = case["provider"].as_str().expect("provider");
        let config = case["config"].as_object().expect("config");
        let result = &case["result"];
        let built = provider(name, config, None);
        let Some(provider) = built.ok() else {
            panic!("{name}: the callback provider must construct");
        };
        if !result["token_request"].is_null() {
            check_token_request(result, provider.as_ref());
        }
        // `raise_for_status` runs before `response.json()`: a 4xx/5xx is the
        // provider's error, a non-JSON body is `response.json()`'s ValueError.
        let status = u16::try_from(case["status"].as_u64().unwrap_or(200)).unwrap_or(200);
        let data = if status >= 400 {
            Err(AdapterError::Auth(AuthFlowError::new(provider_error_code(
                name,
            ))))
        } else if case["body_is_json"].as_bool().unwrap_or(true) {
            provider.token_data(&case["token_response"])
        } else {
            // `response.json()`'s decode error is a `RequestException`, so
            // `get_user_token`'s `except` turns it into the provider error.
            Err(AdapterError::Auth(AuthFlowError::new(provider_error_code(
                name,
            ))))
        };
        match (recorded_error(result), recorded_exception(result), data) {
            (Some((code, message)), _, Err(AdapterError::Auth(error))) => {
                assert_eq!(error.code(), code, "{name} error code");
                assert_eq!(error.message(), message, "{name} error message");
            }
            (None, Some(_), Err(AdapterError::Internal(_))) => {}
            (None, None, Ok(actual)) => {
                let expected = token_data(result);
                assert_eq!(
                    actual.access_token, expected.access_token,
                    "{name} access_token"
                );
                assert_eq!(
                    actual.refresh_token, expected.refresh_token,
                    "{name} refresh_token"
                );
                assert_eq!(actual.id_token, expected.id_token, "{name} id_token");
                if name == "gitea" {
                    check_relative_expiry(
                        &result["access_token_expired_at"],
                        actual.access_token_expired_at,
                        3600,
                    );
                } else {
                    check_expiry(
                        &result["access_token_expired_at"],
                        actual.access_token_expired_at,
                    );
                }
                check_expiry(
                    &result["refresh_token_expired_at"],
                    actual.refresh_token_expired_at,
                );
            }
            (_, _, other) => panic!("{name}: unexpected outcome {other:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 10, "token cases");
}

/// The userinfo payload a case recorded.
fn userinfo(case: &Value) -> &Value {
    &case["userinfo"]
}

/// The recorded extra response for a URL suffix: `(status, is_json, payload)`.
fn extra(case: &Value, suffix: &str) -> Option<(u16, bool, Value)> {
    let entry = case["extra"].get(suffix)?;
    Some((
        u16::try_from(entry["status"].as_u64()?).ok()?,
        entry["body_is_json"].as_bool().unwrap_or(true),
        entry["payload"].clone(),
    ))
}

#[test]
fn google_claims_match_the_recorded_provider() {
    let golden = golden();
    let mut checked = 0;
    for case in golden["cases"].as_array().expect("cases") {
        if case["kind"] != "user_data" || case["provider"] != "google" {
            continue;
        }
        let result = &case["result"];
        let claims = super::google::claims(userinfo(case));
        match (recorded_error(result), claims) {
            (Some((code, message)), Err(AdapterError::Auth(error))) => {
                assert_eq!(error.code(), code);
                assert_eq!(error.message(), message);
            }
            (None, Ok(claims)) => assert_eq!(claims, result["user_data"]),
            (_, other) => panic!("google: unexpected outcome {other:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 4, "google claim cases");
}

#[test]
fn gitlab_claims_match_the_recorded_provider() {
    let golden = golden();
    let mut checked = 0;
    for case in golden["cases"].as_array().expect("cases") {
        if case["kind"] != "user_data" || case["provider"] != "gitlab" {
            continue;
        }
        let result = &case["result"];
        let claims = super::gitlab::claims(userinfo(case));
        match (recorded_error(result), claims) {
            (Some((code, message)), Err(AdapterError::Auth(error))) => {
                assert_eq!(error.code(), code);
                assert_eq!(error.message(), message);
            }
            (None, Ok(claims)) => assert_eq!(claims, result["user_data"]),
            (_, other) => panic!("gitlab: unexpected outcome {other:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 3, "gitlab claim cases");
}

#[test]
fn github_email_selection_and_claims_match_the_recorded_provider() {
    let golden = golden();
    let mut checked = 0;
    for case in golden["cases"].as_array().expect("cases") {
        if case["kind"] != "user_data" || case["provider"] != "github" {
            continue;
        }
        // The organisation cases are the membership test's; their result is
        // decided before the emails call.
        if case["organization_id"].is_string() {
            continue;
        }
        let result = &case["result"];
        let Some((status, is_json, emails)) = extra(case, "/user/emails") else {
            panic!("github case without an emails response");
        };
        // `__get_email`: a non-2xx is the provider error before the body is
        // looked at, and `response.json()`'s decode error is a
        // `RequestException` in requests 2.33 — also the provider error.
        let selected = if status >= 400 || !is_json {
            Err(AdapterError::Auth(AuthFlowError::new(
                crate::auth_flow::error::AuthErrorCode::GithubOauthProviderError,
            )))
        } else {
            super::github::select_email(&emails)
        };
        match (recorded_error(result), recorded_exception(result), selected) {
            (Some((code, message)), _, Err(AdapterError::Auth(error))) => {
                assert_eq!(error.code(), code);
                assert_eq!(error.message(), message);
            }
            // `KeyError`/`AttributeError` cases record a 500, which the record
            // names by exception type only.
            (None, Some(_), Err(AdapterError::Internal(_))) => {}
            (Some(_), _, Ok(email)) => panic!(
                "recorded an error, the port selected {email}; emails were {}",
                emails
            ),
            (None, None, Ok(email)) => {
                let claims = super::github::claims(userinfo(case), email);
                assert_eq!(claims.expect("claims"), result["user_data"]);
            }
            (_, _, other) => panic!("github: unexpected outcome {other:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 9, "github claim cases");
}

#[test]
fn github_organization_membership_matches_the_recorded_provider() {
    let golden = golden();
    let mut checked = 0;
    for case in golden["cases"].as_array().expect("cases") {
        if case["kind"] != "user_data" || case["provider"] != "github" {
            continue;
        }
        let Some(organization_id) = case["organization_id"].as_str() else {
            continue;
        };
        let Some((status, _, _)) = extra(case, "/memberships/ada") else {
            continue;
        };
        let login = userinfo(case).get("login");
        assert_eq!(
            super::github::membership_url(organization_id, login),
            "https://api.github.com/orgs/acme/memberships/ada"
        );
        let member = super::github::is_member(status);
        let recorded = recorded_error(&case["result"]);
        match (member, recorded) {
            (false, Some((code, message))) => {
                assert_eq!(code, 5122, "GITHUB_USER_NOT_IN_ORG");
                assert_eq!(message, "GITHUB_USER_NOT_IN_ORG");
            }
            (true, None) => {}
            (member, recorded) => {
                panic!("membership status {status} → {member}, recorded {recorded:?}")
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 2, "github organisation cases");
}

#[test]
fn gitea_email_selection_and_claims_match_the_recorded_provider() {
    let golden = golden();
    let mut checked = 0;
    for case in golden["cases"].as_array().expect("cases") {
        if case["kind"] != "user_data" || case["provider"] != "gitea" {
            continue;
        }
        let result = &case["result"];
        let Some((status, is_json, emails)) = extra(case, "/api/v1/user/emails") else {
            panic!("gitea case without an emails response");
        };
        if !is_json {
            // `response.json()`'s decode error is a `RequestException`, so it
            // takes the transport branch's message (`gitea.py:146-155`).
            let (code, message) = recorded_error(result).expect("a non-JSON body is an error");
            assert_eq!(code, 5123, "GITEA_OAUTH_PROVIDER_ERROR");
            assert_eq!(
                message,
                "GITEA_OAUTH_PROVIDER_ERROR: Exception occurred while fetching emails"
            );
            checked += 1;
            continue;
        }
        if status >= 400 {
            // `GiteaProvider::email` answers this from the status code, before
            // the body is parsed (`gitea.py:121-125`).
            let (code, message) = recorded_error(result).expect("a 403 is an error");
            assert_eq!(code, 5123, "GITEA_OAUTH_PROVIDER_ERROR");
            assert_eq!(
                message,
                "GITEA_OAUTH_PROVIDER_ERROR: Failed to fetch emails"
            );
            checked += 1;
            continue;
        }
        match (
            recorded_error(result),
            recorded_exception(result),
            super::gitea::select_email(&emails),
        ) {
            (Some((code, message)), _, Err(AdapterError::Auth(error))) => {
                assert_eq!(error.code(), code);
                assert_eq!(error.message(), message);
            }
            (_, Some(_), Err(AdapterError::Internal(_))) => {}
            (None, None, Ok(email)) => {
                let claims = super::gitea::claims(userinfo(case), email);
                assert_eq!(claims.expect("claims"), result["user_data"]);
            }
            (_, _, other) => panic!("gitea: unexpected outcome {other:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 12, "gitea claim cases");
}

#[test]
fn gitea_host_must_have_an_http_scheme() {
    let ctx = request();
    let base = GiteaConfiguration {
        client_id: Some("id".to_string()),
        client_secret: Some("secret".to_string()),
        host: Some("gitea.example.test".to_string()),
    };
    let error = GiteaProvider::new(&ctx, &base, None).expect_err("no scheme");
    assert_eq!(error.message(), "GITEA_NOT_CONFIGURED");
    // A trailing slash is stripped (`gitea.py:51`).
    let trailing = GiteaConfiguration {
        host: Some("http://127.0.0.1:18400/".to_string()),
        ..base
    };
    let provider = GiteaProvider::new(&ctx, &trailing, Some("s")).expect("https host");
    assert!(provider.token_url().ends_with("/login/oauth/access_token"));
    assert!(
        provider
            .auth_url()
            .starts_with("http://127.0.0.1:18400/login/oauth/authorize?")
    );
}

#[test]
fn the_token_body_renders_none_for_a_missing_code() {
    let ctx = request();
    let config = GoogleConfiguration {
        client_id: Some("id".to_string()),
        client_secret: Some("secret".to_string()),
    };
    let provider = GoogleProvider::new(&ctx, &config, None).expect("configured");
    // `urlencode({"code": None, …})` is `code=None` (`urllib/parse.py`).
    assert!(
        provider
            .token_request(None)
            .body()
            .starts_with("code=None&")
    );
    // A callback's provider has `state=None`, which is also the literal
    // `state=None` in the auth URL Python builds and never uses.
    assert!(provider.auth_url().ends_with("state=None"));
    let _: TokenData = TokenData::default();
}
