// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The storage client: presigned URLs and object operations.
//!
//! This is the Rust equivalent of `plane.settings.storage.S3Storage`
//! (`apps/api/plane/settings/storage.py`), plus the export client that
//! `plane/bgtasks/export_task.py` builds by hand.
//!
//! # Shape
//!
//! Django constructs `S3Storage(request=request)` inside every view; the
//! request is only used to derive the MinIO endpoint from the browser-visible
//! host (`storage.py:42-56`). A Rust handler should share one [`Storage`]
//! (connection pool + credential cache) and take a cheap per-call view with
//! [`Storage::request`] or [`Storage::worker`]:
//!
//! ```no_run
//! # async fn example(storage: &plane_storage::Storage) -> Result<(), plane_storage::StorageError> {
//! let view = storage.request(plane_storage::RequestInfo { scheme: "https", host: "plane.example.com" });
//! let post = view
//!     .presign_post(plane_storage::PresignPost {
//!         key: "ws/file.png",
//!         file_type: Some("image/png"),
//!         file_size: 5242880,
//!         expiration: None,
//!     })
//!     .await?;
//! # let _ = post;
//! # Ok(())
//! # }
//! ```
//!
//! # Error handling
//!
//! The four methods whose Python versions catch `ClientError` reproduce that:
//! [`StorageRequest::head_object`], [`StorageRequest::copy_object`],
//! [`StorageRequest::delete_objects`] and [`StorageRequest::upload_file`] turn
//! an S3 error *response* into `None`/`false`, log it on the
//! `plane.exception` target, and only return `Err` for transport and
//! configuration failures (which Python also does not catch). The presign
//! methods cannot raise `ClientError` (they never make a request), so every
//! error propagates there — in Django those are uncaught 500s.

use std::collections::BTreeMap;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use md5::Digest as _;
use plane_core::config::{EnvSource, StorageSettings};
use reqwest::Method;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::credentials::CredentialsProvider;
use crate::endpoint::{self, Endpoint};
use crate::error::StorageError;
use crate::metadata::{self, ObjectMetadata};
use crate::percent;
use crate::policy;
use crate::sign::{self, Header};

/// The MIME types `S3Storage.generate_presigned_url` is willing to serve
/// inline (`plane/settings/common.py:595-597`). Everything else — including SVG
/// and HTML — is forced to `attachment`.
pub const INLINE_SAFE_MIME_TYPES: [&str; 5] = [
    "image/jpeg",
    "image/jpg",
    "image/png",
    "image/gif",
    "image/webp",
];

/// The two request attributes the MinIO endpoint derivation reads.
#[derive(Clone, Copy, Debug)]
pub struct RequestInfo<'a> {
    /// `request.scheme`.
    pub scheme: &'a str,
    /// `request.get_host()` — host plus non-default port, as the client sent it.
    pub host: &'a str,
}

/// Inputs to `generate_presigned_post` after Plane's wrapper has run.
#[derive(Clone, Copy, Debug)]
pub struct PresignPost<'a> {
    /// `object_name`; the asset key (`<workspace>/<uuid>-<filename>`).
    pub key: &'a str,
    /// The `Content-Type` the form must carry. `None` when the client sent
    /// `type: null`, which boto3 writes into the policy as JSON `null`.
    pub file_type: Option<&'a str>,
    /// `FILE_SIZE_LIMIT`-clamped size, the upper bound of `content-length-range`.
    pub file_size: i64,
    /// `expiration`; `None` uses `SIGNED_URL_EXPIRATION`.
    pub expiration: Option<i64>,
}

/// Inputs to `generate_presigned_url` after Plane's wrapper has run.
#[derive(Clone, Copy, Debug)]
pub struct PresignGet<'a> {
    pub key: &'a str,
    pub expiration: Option<i64>,
    /// The wrapper's `disposition` argument; only the exact string `"inline"`
    /// can be honoured, and only for an [`INLINE_SAFE_MIME_TYPES`] type.
    pub disposition: &'a str,
    /// `filename` for `Content-Disposition`. `None` or `""` makes Plane use a
    /// random 32-hex name.
    pub filename: Option<&'a str>,
    /// `content_type` from the asset's attributes.
    pub content_type: Option<&'a str>,
}

/// boto3's `{"url": ..., "fields": ...}`.
///
/// `fields` is a list, not a map, because boto3 (and therefore DRF, which
/// renders dicts in insertion order) emits the form fields in insertion order:
/// `Content-Type`, `key`, `x-amz-algorithm`, `x-amz-credential`, `x-amz-date`,
/// optional `x-amz-security-token`, `policy`, `x-amz-signature`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresignedPost {
    pub url: String,
    pub fields: Vec<(String, Option<String>)>,
}

impl PresignedPost {
    /// The base64 policy document (`fields["policy"]`).
    pub fn policy_base64(&self) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == "policy")
            .and_then(|(_, value)| value.as_deref())
    }

    /// The decoded policy JSON.
    pub fn policy_document(&self) -> Option<String> {
        let encoded = self.policy_base64()?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()?;
        String::from_utf8(bytes).ok()
    }
}

/// A shared storage client.
pub struct Storage {
    settings: StorageSettings,
    bucket: Option<String>,
    /// `storage.py`'s region (`AWS_REGION` with no default, `us-east-1` when
    /// boto3 falls back because it is unset).
    region: String,
    /// `int(os.environ["SIGNED_URL_EXPIRATION"])`, parsed where Django parses it.
    signed_url_expiration: i64,
    credentials: CredentialsProvider,
    http: reqwest::Client,
}

impl Storage {
    /// Build the shared client. `env` supplies the credential-chain variables
    /// that [`StorageSettings`] does not carry (`AWS_SESSION_TOKEN`,
    /// `AWS_CONTAINER_*`, `AWS_EC2_METADATA_*`, `AWS_DEFAULT_REGION`).
    pub fn new(settings: &StorageSettings, env: &dyn EnvSource) -> Result<Self, StorageError> {
        let region = resolve_region(settings, env);
        endpoint::validate_region(&region)?;
        let signed_url_expiration = settings.signed_url_expiration().map_err(|error| {
            StorageError::Config(format!(
                "SIGNED_URL_EXPIRATION: {error} (Django calls int() when S3Storage is constructed)"
            ))
        })?;
        let credentials =
            CredentialsProvider::new(&settings.access_key_id, &settings.secret_access_key, env)?;
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|error| StorageError::Request(error.to_string()))?;
        Ok(Self {
            settings: settings.clone(),
            bucket: settings.bucket_name_storage.clone(),
            region,
            signed_url_expiration,
            credentials,
            http,
        })
    }

    /// Build from the process environment.
    pub fn from_process_env(settings: &StorageSettings) -> Result<Self, StorageError> {
        Self::new(settings, &plane_core::config::ProcessEnv)
    }

    /// A view whose MinIO endpoint comes from this request, like
    /// `S3Storage(request=request)`.
    pub fn request<'a>(&'a self, request: RequestInfo<'a>) -> StorageRequest<'a> {
        StorageRequest {
            storage: self,
            request: Some(request),
        }
    }

    /// A view without a request, like `S3Storage()` in the worker tasks.
    pub fn worker(&self) -> StorageRequest<'_> {
        StorageRequest {
            storage: self,
            request: None,
        }
    }

    fn endpoint(&self, request: Option<RequestInfo<'_>>) -> Result<Endpoint, StorageError> {
        let endpoint_url = if self.settings.use_minio_storage {
            match request {
                Some(request) => {
                    // `storage.py:42-54`: the request host wins and
                    // `AWS_S3_ENDPOINT_URL` is ignored.
                    let protocol = if self.settings.minio_endpoint_ssl {
                        "https"
                    } else {
                        request.scheme
                    };
                    Some(format!("{protocol}://{}", request.host))
                }
                None => self.settings.endpoint_url.clone(),
            }
        } else {
            self.settings.endpoint_url.clone()
        };
        match endpoint_url {
            Some(url) => Endpoint::parse(&url),
            None => Ok(Endpoint::Aws),
        }
    }

    fn bucket(&self) -> Result<&str, StorageError> {
        endpoint::validate_bucket(self.bucket.as_deref())
    }

    fn expiration(&self, requested: Option<i64>) -> i64 {
        requested.unwrap_or(self.signed_url_expiration)
    }

    /// `export_task.py:50-80` endpoint for the presigned download.
    ///
    /// The MinIO branch signs for `AWS_S3_URL_PROTOCOL//<custom domain with the
    /// literal "/uploads" stripped>/`, which is the browser-visible MinIO URL.
    /// The non-MinIO branch reuses the upload client.
    fn export_presign_endpoint(&self) -> Result<(Endpoint, String), StorageError> {
        if self.settings.use_minio_settings {
            let custom_domain = self.settings.custom_domain.as_deref().ok_or_else(|| {
                StorageError::Config(
                    "AWS_S3_CUSTOM_DOMAIN is undefined: USE_MINIO is on but AWS_S3_ENDPOINT_URL is empty"
                        .to_string(),
                )
            })?;
            let protocol = self.settings.url_protocol.as_deref().unwrap_or(":");
            // `export_task.py:69`: `str(...).replace("/uploads", "")` is a
            // hardcoded string replace, not the bucket name. A bucket other
            // than "uploads" therefore appears twice in the URL.
            let endpoint_url = format!("{protocol}//{}/", custom_domain.replace("/uploads", ""));
            Ok((Endpoint::parse(&endpoint_url)?, "us-east-1".to_string()))
        } else {
            self.export_upload_endpoint()
        }
    }

    /// `export_task.py:50-106` upload endpoint and signing region.
    fn export_upload_endpoint(&self) -> Result<(Endpoint, String), StorageError> {
        if self.settings.use_minio_settings || self.settings.aws_s3_endpoint_url.is_some() {
            // Both branches pass `endpoint_url` and no `region_name`, so
            // boto3 signs with us-east-1.
            let endpoint = match &self.settings.aws_s3_endpoint_url {
                Some(url) => Endpoint::parse(url)?,
                None => Endpoint::Aws,
            };
            Ok((endpoint, "us-east-1".to_string()))
        } else {
            // `region_name=settings.AWS_REGION`, whose default is "" and makes
            // boto3 raise `ValueError: Invalid endpoint: https://s3..amazonaws.com`.
            endpoint::validate_region(&self.settings.region)?;
            Ok((Endpoint::Aws, self.settings.region.clone()))
        }
    }

    /// Presign the export download (`export_task.py:66-80`), with a fixed
    /// expiry the caller passes (`7 * 24 * 60 * 60` in the task).
    pub async fn export_download_url(
        &self,
        key: &str,
        expires_in: i64,
    ) -> Result<String, StorageError> {
        self.export_download_url_at(Utc::now(), key, expires_in)
            .await
    }

    /// [`Storage::export_download_url`] with an injected clock.
    pub async fn export_download_url_at(
        &self,
        now: DateTime<Utc>,
        key: &str,
        expires_in: i64,
    ) -> Result<String, StorageError> {
        let (endpoint, region) = self.export_presign_endpoint()?;
        endpoint::validate_get_key(key)?;
        let bucket = self.bucket()?;
        let credentials = self.credentials.get().await?;
        let date = sign::format_date(now);
        let parts = endpoint.object_url(bucket, key, &region)?;
        let host = endpoint::host_header(&parts.scheme, &parts.authority)?;
        Ok(sign::presign_query(sign::QueryPresign {
            method: "GET",
            scheme: &parts.scheme,
            authority: &parts.authority,
            host: &host,
            path: &parts.path,
            operation: Vec::new(),
            date: &date,
            region: &region,
            expires_in,
            credentials: &credentials,
        }))
    }

    /// Upload an export archive (`export_task.py:50-106`): an `application/zip`
    /// object that is `public-read` in the MinIO branch only.
    pub async fn export_upload(&self, key: &str, body: &[u8]) -> Result<(), StorageError> {
        let (endpoint, region) = self.export_upload_endpoint()?;
        let bucket = self.bucket()?;
        let mime = Header {
            name: "content-type".to_string(),
            value: "application/zip".to_string(),
        };
        let mut headers = vec![mime];
        if self.settings.use_minio_settings {
            headers.push(Header {
                name: "x-amz-acl".to_string(),
                value: "public-read".to_string(),
            });
        }
        self.send(
            endpoint,
            &region,
            Method::PUT,
            Target::Object { bucket, key },
            &[],
            headers,
            body,
            true,
        )
        .await?;
        Ok(())
    }

    /// A signed request to an explicit endpoint, used by the object operations.
    #[allow(clippy::too_many_arguments)]
    async fn send(
        &self,
        endpoint: Endpoint,
        region: &str,
        method: Method,
        target: Target<'_>,
        query: &[(String, String)],
        headers: Vec<Header>,
        body: &[u8],
        md5: bool,
    ) -> Result<reqwest::Response, StorageError> {
        let credentials = self.credentials.get().await?;
        let parts = match target {
            Target::Bucket(bucket) => endpoint.bucket_url(bucket, region)?,
            Target::Object { bucket, key } => endpoint.object_url(bucket, key, region)?,
        };
        let date = sign::format_date(Utc::now());
        let mut headers = headers;
        if md5 {
            headers.push(Header {
                name: "content-md5".to_string(),
                value: base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(body)),
            });
        }
        // The signed `host` must be the header reqwest sends (lowercase,
        // default ports dropped); presigned URLs keep the authority verbatim
        // like boto3.
        let host = endpoint::host_header(&parts.scheme, &parts.authority)?;
        let signed = sign::sign_headers(sign::HeaderSigning {
            method: method.as_str(),
            scheme: &parts.scheme,
            authority: &host,
            path: &parts.path,
            query,
            headers,
            body,
            date: &date,
            region,
            credentials: &credentials,
        });
        let mut request = self
            .http
            .request(
                Method::from_bytes(method.as_str().as_bytes())
                    .map_err(|error| StorageError::Request(error.to_string()))?,
                &signed.url,
            )
            .body(body.to_vec());
        let mut header_map = HeaderMap::new();
        for header in &signed.headers {
            if header.name == "host" {
                // reqwest derives Host from the URL; it must not be duplicated.
                continue;
            }
            let name = HeaderName::from_bytes(header.name.as_bytes())
                .map_err(|error| StorageError::Request(error.to_string()))?;
            let value = HeaderValue::from_str(&header.value)
                .map_err(|error| StorageError::Request(error.to_string()))?;
            header_map.insert(name, value);
        }
        request = request.headers(header_map);
        request.send().await.map_err(StorageError::from)
    }
}

/// The URL a request targets: an object, or the bucket itself (`?delete`).
#[derive(Clone, Copy)]
enum Target<'a> {
    Bucket(&'a str),
    Object { bucket: &'a str, key: &'a str },
}

/// A per-call storage view, the equivalent of one `S3Storage` object.
pub struct StorageRequest<'a> {
    storage: &'a Storage,
    request: Option<RequestInfo<'a>>,
}

impl StorageRequest<'_> {
    /// `S3Storage.generate_presigned_post` (`storage.py:68-102`).
    pub async fn presign_post(
        &self,
        input: PresignPost<'_>,
    ) -> Result<PresignedPost, StorageError> {
        self.presign_post_at(Utc::now(), input).await
    }

    /// [`StorageRequest::presign_post`] with an injected clock.
    pub async fn presign_post_at(
        &self,
        now: DateTime<Utc>,
        input: PresignPost<'_>,
    ) -> Result<PresignedPost, StorageError> {
        let bucket = self.storage.bucket()?;
        let credentials = self.storage.credentials.get().await?;
        let date = sign::format_date(now);
        let scope = credentials.scope(&date, &self.storage.region);
        let policy = policy::build(policy::PolicyInput {
            bucket,
            key: input.key,
            file_type: input.file_type,
            file_size: input.file_size,
            expires_in: self.storage.expiration(input.expiration),
            now,
            credential_scope: &scope,
            date: &date,
            token: credentials.token.as_deref(),
        })?;
        let policy_base64 =
            base64::engine::general_purpose::STANDARD.encode(policy.document.as_bytes());
        let signature = credentials.sign_policy(&date, &self.storage.region, &policy_base64);

        let endpoint = self.storage.endpoint(self.request)?;
        let parts = endpoint.bucket_url(bucket, &self.storage.region)?;
        let mut fields = policy.fields;
        fields.push(("policy".to_string(), Some(policy_base64)));
        fields.push(("x-amz-signature".to_string(), Some(signature)));
        Ok(PresignedPost {
            url: format!("{}://{}{}", parts.scheme, parts.authority, parts.path),
            fields,
        })
    }

    /// `S3Storage.generate_presigned_url` (`storage.py:114-151`).
    pub async fn presign_get(&self, input: PresignGet<'_>) -> Result<String, StorageError> {
        self.presign_get_at(Utc::now(), input).await
    }

    /// [`StorageRequest::presign_get`] with an injected clock.
    pub async fn presign_get_at(
        &self,
        now: DateTime<Utc>,
        input: PresignGet<'_>,
    ) -> Result<String, StorageError> {
        endpoint::validate_get_key(input.key)?;
        let bucket = self.storage.bucket()?;
        let credentials = self.storage.credentials.get().await?;
        let date = sign::format_date(now);

        // `content_type.split(";")[0].strip().lower()`.
        let content_type = input
            .content_type
            .map(|value| value.split(';').next().unwrap_or("").trim().to_lowercase())
            .unwrap_or_default();
        let inline = input.disposition == "inline"
            && INLINE_SAFE_MIME_TYPES.contains(&content_type.as_str());

        let disposition = if inline { "inline" } else { "attachment" };
        let filename = match input.filename {
            Some(filename) if !filename.is_empty() => filename.to_string(),
            _ => uuid::Uuid::new_v4().simple().to_string(),
        };
        let filename = percent::encode_filename(&filename);
        let content_disposition = format!("{disposition}; filename*=UTF-8''{filename}");

        let mut operation = Vec::with_capacity(2);
        if inline {
            operation.push((
                "response-content-type".to_string(),
                percent::encode_component(&content_type),
            ));
        }
        operation.push((
            "response-content-disposition".to_string(),
            percent::encode_component(&content_disposition),
        ));

        let endpoint = self.storage.endpoint(self.request)?;
        let parts = endpoint.object_url(bucket, input.key, &self.storage.region)?;
        let host = endpoint::host_header(&parts.scheme, &parts.authority)?;
        Ok(sign::presign_query(sign::QueryPresign {
            method: "GET",
            scheme: &parts.scheme,
            authority: &parts.authority,
            host: &host,
            path: &parts.path,
            operation,
            date: &date,
            region: &self.storage.region,
            expires_in: self.storage.expiration(input.expiration),
            credentials: &credentials,
        }))
    }

    /// `S3Storage.get_object_metadata` (`storage.py:153-169`).
    ///
    /// A missing object is expected (the upload never completed) and returns
    /// `None` without a log; any other S3 error is logged and returns `None`
    /// too, exactly like the Python method.
    pub async fn head_object(&self, key: &str) -> Result<Option<ObjectMetadata>, StorageError> {
        let endpoint = self.storage.endpoint(self.request)?;
        let bucket = self.storage.bucket()?;
        let response = self
            .storage
            .send(
                endpoint,
                &self.storage.region,
                Method::HEAD,
                Target::Object { bucket, key },
                &[],
                Vec::new(),
                b"",
                false,
            )
            .await?;
        if response.status().is_success() {
            return Ok(Some(read_metadata(response.headers())));
        }
        let error = s3_error(response).await;
        if !matches!(error.s3_code(), Some("404" | "NoSuchKey")) {
            log_swallowed(&error);
        }
        Ok(None)
    }

    /// `S3Storage.copy_object` (`storage.py:171-183`).
    ///
    /// The Python method returns the botocore response; no caller reads it, so
    /// this returns `Some(())` on success and `None` after a logged error.
    pub async fn copy_object(
        &self,
        source_key: &str,
        destination_key: &str,
    ) -> Result<Option<()>, StorageError> {
        let endpoint = self.storage.endpoint(self.request)?;
        let bucket = self.storage.bucket()?;
        // botocore serialises `CopySource={"Bucket", "Key"}` as `bucket/key`
        // (no leading slash) with the key quoted with safe="/~".
        let source = format!("{bucket}/{}", percent::encode_key(source_key));
        let response = self
            .storage
            .send(
                endpoint,
                &self.storage.region,
                Method::PUT,
                Target::Object {
                    bucket,
                    key: destination_key,
                },
                &[],
                vec![Header {
                    name: "x-amz-copy-source".to_string(),
                    value: source,
                }],
                b"",
                false,
            )
            .await?;
        if response.status().is_success() {
            return Ok(Some(()));
        }
        log_swallowed(&s3_error(response).await);
        Ok(None)
    }

    /// `S3Storage.delete_files` (`storage.py:208-218`) — `DeleteObjects`.
    pub async fn delete_objects(&self, keys: &[&str]) -> Result<bool, StorageError> {
        let endpoint = self.storage.endpoint(self.request)?;
        let bucket = self.storage.bucket()?;
        let mut body = String::from("<Delete xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">");
        for key in keys {
            body.push_str("<Object><Key>");
            push_xml_text(&mut body, key);
            body.push_str("</Key></Object>");
        }
        body.push_str("</Delete>");
        // boto3 sends the `?delete` sub-resource with the empty value.
        let query = [("delete".to_string(), String::new())];
        let response = self
            .storage
            .send(
                endpoint,
                &self.storage.region,
                Method::POST,
                Target::Bucket(bucket),
                &query,
                Vec::new(),
                body.as_bytes(),
                true,
            )
            .await?;
        if response.status().is_success() {
            return Ok(true);
        }
        log_swallowed(&s3_error(response).await);
        Ok(false)
    }

    /// `client.delete_object`, used by `exporter_expired_task.py:49`.
    ///
    /// S3 returns 204 for a key that does not exist, so a missing object is not
    /// an error. Any real error propagates, as the uncaught `ClientError` does
    /// in the Celery task.
    pub async fn delete_object(&self, key: &str) -> Result<(), StorageError> {
        let endpoint = self.storage.endpoint(self.request)?;
        let bucket = self.storage.bucket()?;
        let response = self
            .storage
            .send(
                endpoint,
                &self.storage.region,
                Method::DELETE,
                Target::Object { bucket, key },
                &[],
                Vec::new(),
                b"",
                false,
            )
            .await?;
        ensure_success(response).await.map(|_| ())
    }

    /// `S3Storage.upload_file` (`storage.py:185-206`).
    ///
    /// A single `PutObject`. boto3's `upload_fileobj` switches to a multipart
    /// upload above 8 MiB; the stored bytes, `Content-Type` and ACL are the
    /// same, only the ETag differs (a multipart ETag has a `-<parts>` suffix).
    /// No Plane caller reads an upload's ETag, so this is not observable.
    pub async fn upload_file(
        &self,
        key: &str,
        body: &[u8],
        content_type: Option<&str>,
    ) -> Result<bool, StorageError> {
        let endpoint = self.storage.endpoint(self.request)?;
        let bucket = self.storage.bucket()?;
        let mut headers = Vec::new();
        if let Some(content_type) = content_type {
            headers.push(Header {
                name: "content-type".to_string(),
                value: content_type.to_string(),
            });
        }
        let response = self
            .storage
            .send(
                endpoint,
                &self.storage.region,
                Method::PUT,
                Target::Object { bucket, key },
                &[],
                headers,
                body,
                true,
            )
            .await?;
        if response.status().is_success() {
            return Ok(true);
        }
        log_swallowed(&s3_error(response).await);
        Ok(false)
    }

    /// `client.put_object` (`update_bucket.py:64` and the export upload).
    pub async fn put_object(
        &self,
        key: &str,
        body: &[u8],
        content_type: Option<&str>,
        acl: Option<&str>,
    ) -> Result<(), StorageError> {
        let endpoint = self.storage.endpoint(self.request)?;
        let bucket = self.storage.bucket()?;
        let mut headers = Vec::new();
        if let Some(content_type) = content_type {
            headers.push(Header {
                name: "content-type".to_string(),
                value: content_type.to_string(),
            });
        }
        if let Some(acl) = acl {
            headers.push(Header {
                name: "x-amz-acl".to_string(),
                value: acl.to_string(),
            });
        }
        let response = self
            .storage
            .send(
                endpoint,
                &self.storage.region,
                Method::PUT,
                Target::Object { bucket, key },
                &[],
                headers,
                body,
                true,
            )
            .await?;
        ensure_success(response).await.map(|_| ())
    }

    /// `client.get_object` (`update_bucket.py:54` and the workspace seed).
    pub async fn get_object(&self, key: &str) -> Result<Vec<u8>, StorageError> {
        let endpoint = self.storage.endpoint(self.request)?;
        let bucket = self.storage.bucket()?;
        let response = self
            .storage
            .send(
                endpoint,
                &self.storage.region,
                Method::GET,
                Target::Object { bucket, key },
                &[],
                Vec::new(),
                b"",
                false,
            )
            .await?;
        let response = ensure_success(response).await?;
        response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(StorageError::from)
    }
}

/// `storage.py:36` / `credentials.py`: boto3 signs `us-east-1` when no region is
/// configured; `AWS_DEFAULT_REGION` is consulted by the session, not by Plane.
fn resolve_region(settings: &StorageSettings, env: &dyn EnvSource) -> String {
    match &settings.region_storage {
        Some(region) => region.clone(),
        None => env
            .get("AWS_DEFAULT_REGION")
            .map(|value| value.into_owned())
            .unwrap_or_else(|| "us-east-1".to_string()),
    }
}

fn read_metadata(headers: &HeaderMap) -> ObjectMetadata {
    let text = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let mut metadata = BTreeMap::new();
    for (name, value) in headers {
        let name = name.as_str();
        if let Some(key) = name.strip_prefix("x-amz-meta-")
            && let Ok(value) = value.to_str()
        {
            metadata.insert(key.to_string(), value.to_string());
        }
    }
    ObjectMetadata {
        content_type: text("content-type"),
        content_length: text("content-length").and_then(|value| value.parse().ok()),
        last_modified: text("last-modified")
            .as_deref()
            .and_then(metadata::format_http_date),
        etag: text("etag"),
        metadata,
    }
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, StorageError> {
    if response.status().is_success() {
        return Ok(response);
    }
    Err(s3_error(response).await)
}

async fn s3_error(response: reqwest::Response) -> StorageError {
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .unwrap_or_default();
    crate::error::parse_s3_error(status, &body)
}

/// `log_exception` (`plane/utils/exception_logger.py:9`) on its logger target.
fn log_swallowed(error: &StorageError) {
    tracing::error!(target: "plane.exception", error = %error, "storage operation failed");
}

/// XML-escape an object key for the `DeleteObjects` body.
fn push_xml_text(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_matches_python_quote() {
        // quote(filename) then percent_encode_component, as in the golden file.
        let filename = percent::encode_filename("é 名前 #1+2%&=?.png");
        let value = percent::encode_component(&format!("attachment; filename*=UTF-8''{filename}"));
        assert_eq!(
            value,
            "attachment%3B%20filename%2A%3DUTF-8%27%27%25C3%25A9%2520%25E5%2590%258D%25E5%2589%258D%2520%25231%252B2%2525%2526%253D%253F.png"
        );
    }

    #[test]
    fn inline_detection_trims_and_lowercases() {
        // `"IMAGE/PNG ; charset=binary"` → `image/png`, which is inline.
        let raw = "IMAGE/PNG ; charset=binary";
        let value = raw.split(';').next().unwrap().trim().to_lowercase();
        assert!(INLINE_SAFE_MIME_TYPES.contains(&value.as_str()));
        let svg = "image/svg+xml";
        assert!(!INLINE_SAFE_MIME_TYPES.contains(&svg));
    }

    #[test]
    fn xml_escaping() {
        let mut out = String::new();
        push_xml_text(&mut out, "a&b<c>\"d'");
        assert_eq!(out, "a&amp;b&lt;c&gt;&quot;d&apos;");
    }
}
