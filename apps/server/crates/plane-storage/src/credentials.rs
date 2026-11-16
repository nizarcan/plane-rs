// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! S3 credentials: env, container role and instance role.
//!
//! `S3Storage` passes `aws_access_key_id=os.environ.get("AWS_ACCESS_KEY_ID")` and
//! `aws_secret_access_key=os.environ.get("AWS_SECRET_ACCESS_KEY")` to
//! `boto3.client` (`plane/settings/storage.py:49-66`) and **never** passes
//! `aws_session_token`. That has two consequences this module reproduces:
//!
//! 1. When both env variables are present, boto3's explicit-credential provider
//!    wins and the token is empty — `AWS_SESSION_TOKEN` is ignored, which breaks
//!    temporary credentials from the environment. Only one of the two being set
//!    is `PartialCredentialsError` at the first presign.
//! 2. When both are absent, boto3 resolves through its chain. For Plane that
//!    means the shared credentials file (deliberately not ported, see
//!    `PROGRESS.md` open question 3), the container endpoint
//!    (`AWS_CONTAINER_CREDENTIALS_*`) and IMDSv2. The env part of the chain is
//!    unreachable in practice because Plane already read the same variables.
//!
//! Credentials are resolved lazily on the first operation and cached until 15
//! minutes before their `Expiration` (`botocore/credentials.py:61`,
//! `_DEFAULT_ADVISORY_REFRESH_TIMEOUT`). One lock covers both the cache and the
//! refresh so concurrent requests perform one fetch.

use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use plane_core::config::EnvSource;
use tokio::sync::Mutex;

use crate::error::StorageError;
use crate::sign::Credentials;

/// Refresh this long before `Expiration` (`botocore/credentials.py:61`).
const ADVISORY_REFRESH_SECONDS: i64 = 15 * 60;

/// IMDSv2 and container metadata requests (`AWS_METADATA_SERVICE_TIMEOUT`
/// default 1 second, one attempt).
const METADATA_TIMEOUT: Duration = Duration::from_secs(1);

/// How credentials are obtained.
enum Mode {
    /// `aws_access_key_id` / `aws_secret_access_key` were passed explicitly.
    Explicit {
        access_key: Option<String>,
        secret_key: Option<String>,
    },
    /// Both were `None`, so boto3's own chain runs.
    Chain(Chain),
}

/// The parts of boto3's credential chain that a Plane deployment can rely on.
struct Chain {
    env_access_key: Option<String>,
    env_secret_key: Option<String>,
    env_token: Option<String>,
    container_relative_uri: Option<String>,
    container_full_uri: Option<String>,
    container_authorization_token: Option<String>,
    container_authorization_token_file: Option<PathBuf>,
    imds_disabled: bool,
    imds_endpoint: String,
}

struct Cached {
    credentials: Credentials,
    expires_at: Option<DateTime<Utc>>,
}

/// Lazily resolved, cached credentials.
pub(crate) struct CredentialsProvider {
    mode: Mode,
    http: reqwest::Client,
    cached: Mutex<Option<Cached>>,
}

impl CredentialsProvider {
    pub fn new(
        access_key: &Option<String>,
        secret_key: &Option<String>,
        env: &dyn EnvSource,
    ) -> Result<Self, StorageError> {
        let get = |name: &str| env.get(name).map(|value| value.into_owned());
        let mode = if access_key.is_some() || secret_key.is_some() {
            Mode::Explicit {
                access_key: access_key.clone(),
                secret_key: secret_key.clone(),
            }
        } else {
            Mode::Chain(Chain {
                env_access_key: get("AWS_ACCESS_KEY_ID"),
                env_secret_key: get("AWS_SECRET_ACCESS_KEY"),
                env_token: get("AWS_SESSION_TOKEN"),
                container_relative_uri: get("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI"),
                container_full_uri: get("AWS_CONTAINER_CREDENTIALS_FULL_URI"),
                container_authorization_token: get("AWS_CONTAINER_AUTHORIZATION_TOKEN"),
                container_authorization_token_file: get("AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE")
                    .map(PathBuf::from),
                imds_disabled: get("AWS_EC2_METADATA_DISABLED")
                    .map(|value| value.to_lowercase() == "true")
                    .unwrap_or(false),
                imds_endpoint: imds_endpoint(&get),
            })
        };
        let http = reqwest::Client::builder()
            .connect_timeout(METADATA_TIMEOUT)
            .timeout(METADATA_TIMEOUT)
            .build()
            .map_err(|error| StorageError::CredentialRetrieval(error.to_string()))?;
        Ok(Self {
            mode,
            http,
            cached: Mutex::new(None),
        })
    }

    pub async fn get(&self) -> Result<Credentials, StorageError> {
        let mut cached = self.cached.lock().await;
        if let Some(current) = cached.as_ref()
            && !needs_refresh(current.expires_at)
        {
            return Ok(current.credentials.clone());
        }
        match self.resolve().await {
            Ok((credentials, expires_at)) => {
                *cached = Some(Cached {
                    credentials: credentials.clone(),
                    expires_at,
                });
                Ok(credentials)
            }
            Err(error) => {
                // `botocore/credentials.py:508-535`: a failed *advisory* refresh
                // keeps using the still-valid credentials; only inside the
                // mandatory window (10 minutes) does the failure surface.
                if let Some(current) = cached.as_ref()
                    && !within_mandatory_refresh(current.expires_at)
                {
                    tracing::warn!(
                        target: "plane.exception",
                        error = %error,
                        "metadata credential refresh failed; using cached credentials"
                    );
                    return Ok(current.credentials.clone());
                }
                Err(error)
            }
        }
    }

    async fn resolve(&self) -> Result<(Credentials, Option<DateTime<Utc>>), StorageError> {
        match &self.mode {
            Mode::Explicit {
                access_key,
                secret_key,
            } => match (access_key, secret_key) {
                (Some(access_key), Some(secret_key)) => Ok((
                    Credentials {
                        access_key: access_key.clone(),
                        secret_key: secret_key.clone(),
                        token: None,
                    },
                    None,
                )),
                (None, _) => Err(StorageError::PartialCredentials {
                    location: "explicit".to_string(),
                    missing: "aws_access_key_id".to_string(),
                }),
                (_, None) => Err(StorageError::PartialCredentials {
                    location: "explicit".to_string(),
                    missing: "aws_secret_access_key".to_string(),
                }),
            },
            Mode::Chain(chain) => chain.resolve(&self.http).await,
        }
    }
}

impl Chain {
    async fn resolve(
        &self,
        http: &reqwest::Client,
    ) -> Result<(Credentials, Option<DateTime<Utc>>), StorageError> {
        if self.env_access_key.is_some() || self.env_secret_key.is_some() {
            let access_key =
                self.env_access_key
                    .clone()
                    .ok_or_else(|| StorageError::PartialCredentials {
                        location: "env".to_string(),
                        missing: "aws_access_key_id".to_string(),
                    })?;
            let secret_key =
                self.env_secret_key
                    .clone()
                    .ok_or_else(|| StorageError::PartialCredentials {
                        location: "env".to_string(),
                        missing: "aws_secret_access_key".to_string(),
                    })?;
            return Ok((
                Credentials {
                    access_key,
                    secret_key,
                    token: self.env_token.clone(),
                },
                None,
            ));
        }

        if let Some(uri) = self.container_uri() {
            let mut request = http.get(uri);
            if let Some(token) = self.container_token()? {
                request = request.header(reqwest::header::AUTHORIZATION, token);
            }
            let body = send_metadata(request, "container credentials").await?;
            return parse_credential_document(&body, "container");
        }

        if self.imds_disabled {
            return Err(StorageError::NoCredentials);
        }
        imds(http, &self.imds_endpoint).await
    }

    fn container_uri(&self) -> Option<String> {
        if let Some(relative) = &self.container_relative_uri {
            return Some(format!("http://169.254.170.2{relative}"));
        }
        self.container_full_uri.clone()
    }

    fn container_token(&self) -> Result<Option<String>, StorageError> {
        if let Some(path) = &self.container_authorization_token_file {
            let token = std::fs::read_to_string(path).map_err(|error| {
                StorageError::CredentialRetrieval(format!(
                    "could not read AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE: {error}"
                ))
            })?;
            return validate_token(token).map(Some);
        }
        self.container_authorization_token
            .clone()
            .map(validate_token)
            .transpose()
    }
}

/// `botocore/credentials.py:1959-1963`: an auth token with CR/LF is refused.
fn validate_token(token: String) -> Result<String, StorageError> {
    if token.contains('\r') || token.contains('\n') {
        return Err(StorageError::CredentialRetrieval(
            "Auth token value is not a legal header value".to_string(),
        ));
    }
    Ok(token)
}

/// IMDSv2: token, role name, role credentials.
async fn imds(
    http: &reqwest::Client,
    endpoint: &str,
) -> Result<(Credentials, Option<DateTime<Utc>>), StorageError> {
    let token = http
        .put(format!("{endpoint}/latest/api/token"))
        .header(
            "x-aws-ec2-metadata-token-ttl-seconds",
            // `botocore/utils.py` uses a 6-hour token.
            "21600",
        )
        .send()
        .await
        .map_err(|error| StorageError::CredentialRetrieval(format!("IMDSv2 token: {error}")))?
        .error_for_status()
        .map_err(|error| StorageError::CredentialRetrieval(format!("IMDSv2 token: {error}")))?
        .text()
        .await
        .map_err(|error| StorageError::CredentialRetrieval(format!("IMDSv2 token: {error}")))?;

    let role = http
        .get(format!(
            "{endpoint}/latest/meta-data/iam/security-credentials/"
        ))
        .header("x-aws-ec2-metadata-token", &token)
        .send()
        .await
        .map_err(|error| StorageError::CredentialRetrieval(format!("IMDSv2 role name: {error}")))?
        .error_for_status()
        .map_err(|error| StorageError::CredentialRetrieval(format!("IMDSv2 role name: {error}")))?
        .text()
        .await
        .map_err(|error| StorageError::CredentialRetrieval(format!("IMDSv2 role name: {error}")))?;
    let role = role.lines().next().unwrap_or("").trim();
    if role.is_empty() {
        return Err(StorageError::CredentialRetrieval(
            "IMDSv2 returned no IAM role".to_string(),
        ));
    }

    let body = http
        .get(format!(
            "{endpoint}/latest/meta-data/iam/security-credentials/{role}"
        ))
        .header("x-aws-ec2-metadata-token", token)
        .send()
        .await
        .map_err(|error| StorageError::CredentialRetrieval(format!("IMDSv2 credentials: {error}")))?
        .error_for_status()
        .map_err(|error| StorageError::CredentialRetrieval(format!("IMDSv2 credentials: {error}")))?
        .bytes()
        .await
        .map_err(|error| {
            StorageError::CredentialRetrieval(format!("IMDSv2 credentials: {error}"))
        })?;
    parse_credential_document(&body, "instance")
}

/// `AWS_EC2_METADATA_SERVICE_ENDPOINT` wins over the mode; the default is the
/// IPv4 link-local address (`botocore/utils.py:93`).
fn imds_endpoint(get: &impl Fn(&str) -> Option<String>) -> String {
    if let Some(endpoint) = get("AWS_EC2_METADATA_SERVICE_ENDPOINT")
        && !endpoint.is_empty()
    {
        return endpoint.trim_end_matches('/').to_string();
    }
    if get("AWS_EC2_METADATA_SERVICE_ENDPOINT_MODE")
        .map(|mode| mode.eq_ignore_ascii_case("ipv6"))
        .unwrap_or(false)
    {
        return "http://[fd00:ec2::254]".to_string();
    }
    "http://169.254.169.254".to_string()
}

async fn send_metadata(
    request: reqwest::RequestBuilder,
    what: &str,
) -> Result<Vec<u8>, StorageError> {
    let response = request
        .send()
        .await
        .map_err(|error| StorageError::CredentialRetrieval(format!("{what}: {error}")))?
        .error_for_status()
        .map_err(|error| StorageError::CredentialRetrieval(format!("{what}: {error}")))?;
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| StorageError::CredentialRetrieval(format!("{what}: {error}")))
}

/// Parse the JSON document both metadata providers return.
///
/// `botocore/credentials.py:_parse_creds` requires `AccessKeyId`,
/// `SecretAccessKey`, `Token` and `Expiration`; a missing field is an error, not
/// a fall-through to the next provider.
fn parse_credential_document(
    body: &[u8],
    source: &str,
) -> Result<(Credentials, Option<DateTime<Utc>>), StorageError> {
    let document: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| StorageError::CredentialRetrieval(format!("{source}: {error}")))?;
    let field = |name: &str| {
        document
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| StorageError::CredentialRetrieval(format!("{source}: missing {name}")))
    };
    let access_key = field("AccessKeyId")?;
    let secret_key = field("SecretAccessKey")?;
    let token = field("Token")?;
    let expiration = field("Expiration")?;
    let expires_at = DateTime::parse_from_rfc3339(&expiration)
        .map_err(|error| {
            StorageError::CredentialRetrieval(format!(
                "{source}: invalid Expiration {expiration:?}: {error}"
            ))
        })?
        .with_timezone(&Utc);
    Ok((
        Credentials {
            access_key,
            secret_key,
            token: Some(token),
        },
        Some(expires_at),
    ))
}

fn needs_refresh(expires_at: Option<DateTime<Utc>>) -> bool {
    match expires_at {
        // Static credentials never expire.
        None => false,
        Some(expires_at) => (expires_at - Utc::now()).num_seconds() < ADVISORY_REFRESH_SECONDS,
    }
}

/// Inside the mandatory window (`botocore/credentials.py:62`, 10 minutes) a
/// failed refresh must fail the request.
fn within_mandatory_refresh(expires_at: Option<DateTime<Utc>>) -> bool {
    match expires_at {
        None => false,
        Some(expires_at) => (expires_at - Utc::now()).num_seconds() < 10 * 60,
    }
}

#[cfg(test)]
mod tests {
    use plane_core::config::EnvMap;

    use super::*;

    fn provider(env: &EnvMap) -> CredentialsProvider {
        CredentialsProvider::new(&None, &None, env).unwrap()
    }

    #[tokio::test]
    async fn explicit_credentials_ignore_the_session_token() {
        let env = EnvMap::new().set("AWS_SESSION_TOKEN", "TOKEN");
        let provider =
            CredentialsProvider::new(&Some("AKID".into()), &Some("SECRET".into()), &env).unwrap();
        let credentials = provider.get().await.unwrap();
        assert_eq!(credentials.access_key, "AKID");
        assert_eq!(credentials.secret_key, "SECRET");
        assert_eq!(credentials.token, None);
    }

    #[tokio::test]
    async fn explicit_partial_credentials_fail_at_resolution() {
        let provider =
            CredentialsProvider::new(&Some("AKID".into()), &None, &EnvMap::new()).unwrap();
        match provider.get().await {
            Err(StorageError::PartialCredentials { missing, .. }) => {
                assert_eq!(missing, "aws_secret_access_key")
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn env_chain_uses_the_token() {
        let env = EnvMap::new()
            .set("AWS_ACCESS_KEY_ID", "CHAINAK")
            .set("AWS_SECRET_ACCESS_KEY", "CHAINSECRET")
            .set("AWS_SESSION_TOKEN", "TOKEN");
        let credentials = provider(&env).get().await.unwrap();
        assert_eq!(credentials.access_key, "CHAINAK");
        assert_eq!(credentials.token.as_deref(), Some("TOKEN"));
    }

    #[tokio::test]
    async fn imds_disabled_and_no_container_means_no_credentials() {
        let env = EnvMap::new().set("AWS_EC2_METADATA_DISABLED", "true");
        match provider(&env).get().await {
            Err(StorageError::NoCredentials) => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn container_full_uri_is_used_with_the_auth_token() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buffer = [0_u8; 4096];
            let read = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: token-value"),
                "{request}"
            );
            let body = "{\"AccessKeyId\":\"CONTAINERAK\",\"SecretAccessKey\":\"CONTAINERSECRET\",\"Token\":\"CONTAINERTOKEN\",\"Expiration\":\"2030-01-01T00:00:00Z\"}";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let env = EnvMap::new()
            .set(
                "AWS_CONTAINER_CREDENTIALS_FULL_URI",
                &format!("http://{address}/creds"),
            )
            .set("AWS_CONTAINER_AUTHORIZATION_TOKEN", "token-value");
        let credentials = provider(&env).get().await.unwrap();
        assert_eq!(credentials.access_key, "CONTAINERAK");
        assert_eq!(credentials.token.as_deref(), Some("CONTAINERTOKEN"));
    }

    #[tokio::test]
    async fn imds_v2_is_followed_and_cached() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen = requests.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = [0_u8; 4096];
                let read = socket.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                seen.lock().unwrap().push(request.clone());
                let (status, body) = if request.starts_with("PUT /latest/api/token") {
                    ("200 OK", "IMDSTOKEN".to_string())
                } else if request.starts_with("GET /latest/meta-data/iam/security-credentials/") {
                    if request
                        .starts_with("GET /latest/meta-data/iam/security-credentials/ HTTP/1.1")
                    {
                        ("200 OK", "role-name\n".to_string())
                    } else {
                        (
                            "200 OK",
                            "{\"AccessKeyId\":\"IMDSAK\",\"SecretAccessKey\":\"IMDSSECRET\",\"Token\":\"IMDSTOKENCRED\",\"Expiration\":\"2030-01-01T00:00:00Z\"}".to_string(),
                        )
                    }
                } else {
                    ("404 Not Found", String::new())
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
                let _ = socket.shutdown().await;
            }
        });
        let env = EnvMap::new()
            .set(
                "AWS_EC2_METADATA_SERVICE_ENDPOINT",
                &format!("http://{address}"),
            )
            .set("AWS_EC2_METADATA_DISABLED", "false");
        let provider = provider(&env);
        let first = provider.get().await.unwrap();
        assert_eq!(first.access_key, "IMDSAK");
        let second = provider.get().await.unwrap();
        assert_eq!(second.access_key, "IMDSAK");
        // Cached: three requests total (token, role name, credentials), not six.
        assert_eq!(requests.lock().unwrap().len(), 3);
    }
}
