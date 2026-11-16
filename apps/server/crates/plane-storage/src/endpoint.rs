// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! S3 endpoint selection, reproduced from botocore's endpoint rules.
//!
//! Plane builds the client with `Config(signature_version="s3v4")` and nothing
//! else (`plane/settings/storage.py:49-66`). That combination selects
//! botocore's default S3 addressing behaviour, which is more surprising than it
//! looks:
//!
//! * **With `endpoint_url`** (MinIO, S3-compatible servers) addressing is
//!   path-style: `http://127.0.0.1:59000/uploads/<key>`. An endpoint path is
//!   kept and the bucket is appended to it (`https://example.com/base/uploads`).
//! * **Without `endpoint_url`** the endpoint rules use the *global* endpoint for
//!   the `aws` partition (`https://s3.amazonaws.com`) for **every** region when
//!   the bucket looks like a DNS label, and `fix_s3_host`
//!   (`botocore/utils.py:1344-1370`) then moves the bucket into the host:
//!   `https://uploads.s3.amazonaws.com/`. The signing region stays the client's
//!   region (`eu-central-1` in the credential scope, `s3.amazonaws.com` in the
//!   URL). Plane never sets `us_east_1_regional_endpoint`, so this is what all
//!   AWS deployments of Plane do.
//! * A bucket that is not a valid host label (uppercase, `_`, `.`, or longer
//!   than 63) keeps the bucket in the path. That path uses the **regional**
//!   endpoint when the ruleset could not use `aws-global`
//!   (`https://s3.eu-central-1.amazonaws.com/My_Bucket`), except when the
//!   bucket is a valid host label but fails boto3's stricter lowercase DNS
//!   check (`MyBucket`, `ab` traffic): the ruleset builds the global virtual
//!   URL and `fix_s3_host` falls back to `https://s3.amazonaws.com/MyBucket`.
//! * `aws-cn` and `aws-us-gov` have no global endpoint; they use
//!   `s3.<region>.amazonaws.com[.cn]`. Other partitions require `endpoint_url`
//!   and are rejected here (Plane deployments in `us-iso-*` must set one).
//!
//! All of this is pinned by `tests/golden/presign.json` (cases
//! `post_endpoint_*` / `get_endpoint_*`), generated from boto3.

use plane_core::py;

use crate::error::StorageError;
use crate::percent;

/// Where requests go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Endpoint {
    /// An explicit `endpoint_url` (or one derived from the request host).
    /// The path is the endpoint's own path, verbatim (`""`, `"/"`, `"/base"`).
    Path {
        scheme: String,
        authority: String,
        path: String,
    },
    /// botocore's default AWS endpoint selection; `bucket_url`/`object_url`
    /// need the signing region to build it.
    Aws,
}

/// A URL split into the three parts signing cares about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UrlParts {
    pub scheme: String,
    pub authority: String,
    /// Percent-encoded already, starts with `/`.
    pub path: String,
}

impl Endpoint {
    /// Parse and validate `endpoint_url` the way `botocore/endpoint.py:397`
    /// does (`is_valid_endpoint_url`, `botocore/utils.py:1276`).
    pub fn parse(url: &str) -> Result<Self, StorageError> {
        validate_endpoint_url(url)?;
        let split = py::urlsplit(url)
            .map_err(|error| StorageError::InvalidEndpoint(format!("{url}: {error}")))?;
        if !split.query.is_empty() || !split.fragment.is_empty() {
            // `_urljoin` would keep these, producing a URL whose signature
            // covers a query string S3 does not see. Plane never sets such an
            // endpoint; refuse it instead of emitting a broken URL.
            return Err(StorageError::InvalidEndpoint(url.to_string()));
        }
        Ok(Self::Path {
            scheme: split.scheme,
            authority: split.netloc,
            path: split.path,
        })
    }

    pub fn bucket_url(&self, bucket: &str, region: &str) -> Result<UrlParts, StorageError> {
        match self {
            Self::Path {
                scheme,
                authority,
                path,
            } => Ok(UrlParts {
                scheme: scheme.clone(),
                authority: authority.clone(),
                path: join_path(path, &format!("/{bucket}")),
            }),
            Self::Aws => {
                let address = AwsAddress::resolve(bucket, region)?;
                Ok(UrlParts {
                    scheme: "https".to_string(),
                    authority: address.authority,
                    path: if address.virtual_host {
                        "/".to_string()
                    } else {
                        format!("/{bucket}")
                    },
                })
            }
        }
    }

    pub fn object_url(
        &self,
        bucket: &str,
        key: &str,
        region: &str,
    ) -> Result<UrlParts, StorageError> {
        self.object_url_encoded(bucket, &percent::encode_key(key), region)
    }

    fn object_url_encoded(
        &self,
        bucket: &str,
        encoded_key: &str,
        region: &str,
    ) -> Result<UrlParts, StorageError> {
        match self {
            Self::Path {
                scheme,
                authority,
                path,
            } => Ok(UrlParts {
                scheme: scheme.clone(),
                authority: authority.clone(),
                path: join_path(path, &format!("/{bucket}/{encoded_key}")),
            }),
            Self::Aws => {
                let address = AwsAddress::resolve(bucket, region)?;
                Ok(UrlParts {
                    scheme: "https".to_string(),
                    authority: address.authority,
                    path: if address.virtual_host {
                        format!("/{encoded_key}")
                    } else {
                        format!("/{bucket}/{encoded_key}")
                    },
                })
            }
        }
    }
}

/// The host and addressing style botocore settles on for a bucket.
struct AwsAddress {
    authority: String,
    virtual_host: bool,
}

impl AwsAddress {
    fn resolve(bucket: &str, region: &str) -> Result<Self, StorageError> {
        validate_region(region)?;
        if region.is_empty() {
            // `boto3.client(region_name="")` with no endpoint_url builds
            // `https://s3..amazonaws.com` and `create_endpoint` raises.
            return Err(StorageError::InvalidEndpoint(
                "https://s3..amazonaws.com".to_string(),
            ));
        }
        let suffix = dns_suffix(region);
        let global = is_aws_partition(region);
        let regional = if global && region == "us-east-1" {
            format!("s3.{suffix}")
        } else {
            format!("s3.{region}.{suffix}")
        };

        if global && valid_host_label(bucket) {
            // Endpoint rules pick the global endpoint with the bucket in the
            // host, then `fix_s3_host` either keeps it or falls back to path
            // style with the *global* host.
            if check_dns_name(bucket) {
                return Ok(Self {
                    authority: format!("{bucket}.s3.{suffix}"),
                    virtual_host: true,
                });
            }
            return Ok(Self {
                authority: format!("s3.{suffix}"),
                virtual_host: false,
            });
        }

        if valid_host_label(bucket) && check_dns_name(bucket) {
            return Ok(Self {
                authority: format!("{bucket}.{regional}"),
                virtual_host: true,
            });
        }
        Ok(Self {
            authority: regional,
            virtual_host: false,
        })
    }
}

/// `botocore/utils.py:1311` `validate_region_name`.
///
/// `^(?![0-9]+$)(?!-)[a-zA-Z0-9-]{,63}(?<!-)$` — note that the empty string is
/// accepted (boto3 signs an empty credential scope with MinIO).
pub fn validate_region(region: &str) -> Result<(), StorageError> {
    let bytes = region.as_bytes();
    let valid = bytes.len() <= 63
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-');
    let valid = valid
        && !bytes.first().is_some_and(|byte| *byte == b'-')
        && !bytes.last().is_some_and(|byte| *byte == b'-')
        && (region.is_empty() || !region.chars().all(|ch| ch.is_ascii_digit()));
    if valid {
        Ok(())
    } else {
        Err(StorageError::InvalidRegion(region.to_string()))
    }
}

fn dns_suffix(region: &str) -> &'static str {
    if region.starts_with("cn-") {
        "amazonaws.com.cn"
    } else {
        "amazonaws.com"
    }
}

fn is_aws_partition(region: &str) -> bool {
    !(region.starts_with("cn-") || region.starts_with("us-gov-"))
}

/// `botocore/endpoint_provider.py:46` `VALID_HOST_LABEL_RE`, used to decide
/// whether the bucket can be host-addressed at the endpoint-rules level.
fn valid_host_label(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    if bytes.first() == Some(&b'-') || bytes.last() == Some(&b'-') {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

/// `botocore/utils.py:1321` `check_dns_name` — the stricter check `fix_s3_host`
/// applies before switching to virtual-host addressing.
fn check_dns_name(bucket: &str) -> bool {
    if bucket.contains('.') {
        return false;
    }
    let bytes = bucket.as_bytes();
    if bytes.len() < 3 || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    if !(last.is_ascii_lowercase() || last.is_ascii_digit()) {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// `botocore/auth.py:70-86` `_host_from_url`: lowercase the host, drop a
/// default port (`:80` for http, `:443` for https), keep IPv6 brackets, drop
/// userinfo. This is the value signed in the `host` header; the URL keeps the
/// authority as the operator wrote it.
pub(crate) fn host_header(scheme: &str, authority: &str) -> Result<String, StorageError> {
    let split = py::urlsplit(&format!("{scheme}://{authority}")).map_err(|error| {
        StorageError::InvalidEndpoint(format!("{scheme}://{authority}: {error}"))
    })?;
    let host = split
        .hostname()
        .ok_or_else(|| StorageError::InvalidEndpoint(format!("{scheme}://{authority}")))?;
    let host = if split.netloc.contains('[') {
        format!("[{host}]")
    } else {
        host
    };
    let port = split.port().map_err(|error| {
        StorageError::InvalidEndpoint(format!("{scheme}://{authority}: {error}"))
    })?;
    let default_port = match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    };
    Ok(match port {
        Some(port) if Some(port) != default_port => format!("{host}:{port}"),
        _ => host,
    })
}

/// `botocore/awsrequest.py:317-341` `_urljoin`.
fn join_path(endpoint_path: &str, request_path: &str) -> String {
    if request_path.is_empty() || request_path == "/" {
        if endpoint_path.is_empty() {
            "/".to_string()
        } else {
            endpoint_path.to_string()
        }
    } else if endpoint_path.ends_with('/') && request_path.starts_with('/') {
        format!(
            "{}{}",
            &endpoint_path[..endpoint_path.len() - 1],
            request_path
        )
    } else {
        format!("{endpoint_path}{request_path}")
    }
}

/// `is_valid_endpoint_url` + `is_valid_ipv6_endpoint_url`.
fn validate_endpoint_url(url: &str) -> Result<(), StorageError> {
    let invalid = || StorageError::InvalidEndpoint(url.to_string());
    if url.contains(['\r', '\n', '\t', ' ']) {
        return Err(invalid());
    }
    let split = py::urlsplit(url).map_err(|_| invalid())?;
    if split.scheme != "http" && split.scheme != "https" {
        return Err(invalid());
    }
    if !split.query.is_empty() || !split.fragment.is_empty() {
        return Err(invalid());
    }
    let host = split.hostname().ok_or_else(invalid)?;
    if host.len() > 255 {
        return Err(invalid());
    }
    let host = host.strip_suffix('.').unwrap_or(&host);
    if host.is_empty() {
        return Err(invalid());
    }
    // IPv6 literals (`http://[::1]:9000`) pass botocore's separate IPv6 check.
    if split.netloc.contains('[') {
        return Ok(());
    }
    if host
        .split('.')
        .all(|label| valid_host_label(label) && !label.starts_with('-'))
    {
        Ok(())
    } else {
        Err(invalid())
    }
}

/// botocore's parameter validator for `Bucket` (`botocore/validate.py` +
/// `service-2.json`): `^[a-zA-Z0-9.\-_]{1,255}$` or an ARN.
pub(crate) fn validate_bucket(bucket: Option<&str>) -> Result<&str, StorageError> {
    let Some(bucket) = bucket else {
        // boto3's `ArnParser.is_arn(None)` raises before parameter validation,
        // which is a `TypeError` in Python and propagates.
        return Err(StorageError::InvalidParameter(
            "Invalid type for parameter Bucket, value: None, valid types: <class 'str'>"
                .to_string(),
        ));
    };
    if bucket.starts_with("arn:") {
        return Err(StorageError::InvalidParameter(format!(
            "Bucket ARNs are not supported by plane-storage: {bucket}"
        )));
    }
    let valid = (1..=255).contains(&bucket.len())
        && bucket
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    if valid {
        Ok(bucket)
    } else {
        Err(StorageError::InvalidParameter(format!(
            "Invalid bucket name \"{bucket}\": Bucket name must match the regex \"^[a-zA-Z0-9.\\-_]{{1,255}}$\""
        )))
    }
}

/// The parameter validator for `Key` on `GetObject` (`min: 1`).
pub(crate) fn validate_get_key(key: &str) -> Result<(), StorageError> {
    if key.is_empty() {
        Err(StorageError::InvalidParameter(
            "Invalid length for parameter Key, value: 0, valid min length: 1".to_string(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_name_check_matches_boto3() {
        assert!(check_dns_name("uploads"));
        assert!(check_dns_name("my-bucket"));
        assert!(!check_dns_name("ab")); // too short
        assert!(!check_dns_name("My_Bucket")); // underscore and uppercase
        assert!(!check_dns_name("my.bucket")); // dot
        assert!(!check_dns_name("MyBucket")); // uppercase
        assert!(!check_dns_name("-bucket"));
        assert!(!check_dns_name("bucket-"));
        assert!(!check_dns_name("a".repeat(64).as_str()));
    }

    #[test]
    fn host_label_check_allows_uppercase() {
        assert!(valid_host_label("MyBucket"));
        assert!(!valid_host_label("My_Bucket"));
        assert!(!valid_host_label("my.bucket"));
        assert!(!valid_host_label("a".repeat(64).as_str()));
    }

    #[test]
    fn host_header_matches_botocore() {
        let host = |scheme: &str, authority: &str| host_header(scheme, authority).unwrap();
        assert_eq!(host("http", "example.com"), "example.com");
        assert_eq!(host("http", "example.com:80"), "example.com");
        assert_eq!(host("https", "example.com:443"), "example.com");
        assert_eq!(host("https", "example.com:8443"), "example.com:8443");
        assert_eq!(host("http", "127.0.0.1:59000"), "127.0.0.1:59000");
        assert_eq!(host("http", "[::1]:80"), "[::1]");
        assert_eq!(host("http", "example.com:8080"), "example.com:8080");
        // urlsplit lowercases the host; userinfo is dropped.
        assert_eq!(host("http", "EXAMPLE.com:9000"), "example.com:9000");
        assert_eq!(host("http", "user:pass@example.com"), "example.com");
        // What reqwest/url actually sends for the raw URL.
        let parsed = reqwest::Url::parse("http://example.com:80/x").unwrap();
        assert_eq!(parsed.as_str(), "http://example.com/x");
    }

    #[test]
    fn urljoin_matches_botocore() {
        assert_eq!(join_path("", "/uploads"), "/uploads");
        assert_eq!(join_path("/", "/uploads"), "/uploads");
        assert_eq!(join_path("/base", "/uploads"), "/base/uploads");
        assert_eq!(join_path("/base/", "/uploads"), "/base/uploads");
        assert_eq!(join_path("", "/"), "/");
        assert_eq!(join_path("/base", "/"), "/base");
    }

    #[test]
    fn region_validation() {
        assert!(validate_region("us-east-1").is_ok());
        assert!(validate_region("").is_ok());
        assert!(validate_region("eu-central-1").is_ok());
        assert!(validate_region("123").is_err());
        assert!(validate_region("-x").is_err());
        assert!(validate_region("x-").is_err());
        assert!(validate_region("bad_region").is_err());
        assert!(validate_region(&"a".repeat(64)).is_err());
    }

    #[test]
    fn endpoint_validation() {
        assert!(Endpoint::parse("http://127.0.0.1:59000").is_ok());
        assert!(Endpoint::parse("https://example.com/base/").is_ok());
        assert!(Endpoint::parse("https://s3.example.com:9443").is_ok());
        assert!(Endpoint::parse("http://10.0.0.5:9000/").is_ok());
        assert!(Endpoint::parse("http://[::1]:9000").is_ok());
        assert!(Endpoint::parse("localhost:9000").is_err());
        assert!(Endpoint::parse("ftp://example.com").is_err());
        assert!(Endpoint::parse("http://exa mple.com").is_err());
        assert!(Endpoint::parse("http://example.com?x=1").is_err());
    }

    #[test]
    fn aws_addressing_matches_recorded_botocore_urls() {
        let object = |bucket: &str, region: &str| {
            Endpoint::Aws
                .object_url(bucket, "a/b.png", region)
                .map(|parts| format!("{}://{}{}", parts.scheme, parts.authority, parts.path))
                .unwrap()
        };
        assert_eq!(
            object("uploads", "us-east-1"),
            "https://uploads.s3.amazonaws.com/a/b.png"
        );
        assert_eq!(
            object("uploads", "eu-central-1"),
            "https://uploads.s3.amazonaws.com/a/b.png"
        );
        assert_eq!(
            object("My_Bucket", "eu-central-1"),
            "https://s3.eu-central-1.amazonaws.com/My_Bucket/a/b.png"
        );
        assert_eq!(
            object("my.bucket", "eu-central-1"),
            "https://s3.eu-central-1.amazonaws.com/my.bucket/a/b.png"
        );
        assert_eq!(
            object("ab", "eu-central-1"),
            "https://s3.amazonaws.com/ab/a/b.png"
        );
        assert_eq!(
            object("MyBucket", "eu-central-1"),
            "https://s3.amazonaws.com/MyBucket/a/b.png"
        );
        assert_eq!(
            object("uploads", "cn-north-1"),
            "https://uploads.s3.cn-north-1.amazonaws.com.cn/a/b.png"
        );
        assert_eq!(
            object("uploads", "us-gov-west-1"),
            "https://uploads.s3.us-gov-west-1.amazonaws.com/a/b.png"
        );
    }

    #[test]
    fn bucket_validation() {
        assert!(validate_bucket(Some("uploads")).is_ok());
        assert!(validate_bucket(Some("my.bucket")).is_ok());
        assert!(validate_bucket(Some("My_Bucket")).is_ok());
        assert!(validate_bucket(Some("")).is_err());
        assert!(validate_bucket(None).is_err());
        assert!(validate_bucket(Some("a b")).is_err());
        assert!(validate_bucket(Some(&"a".repeat(256))).is_err());
        assert!(validate_bucket(Some("arn:aws:s3:us-east-1:123:accesspoint/x")).is_err());
    }
}
