// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! SigV4 signing, reproducing the parts of botocore that Plane's presigned
//! URLs and object requests actually exercise.
//!
//! Sources, in order of how much they matter:
//!
//! * `botocore/auth.py:224-470` — the HMAC key derivation, canonical request,
//!   string to sign and header signing.
//! * `botocore/auth.py:702-800` — `SigV4QueryAuth`/`S3SigV4QueryAuth`. S3 does
//!   **not** normalise the path (`S3SigV4QueryAuth._normalize_url_path`) and
//!   signs `UNSIGNED-PAYLOAD`. The operation parameters keep their serialization
//!   order and the `X-Amz-*` parameters follow in a fixed insertion order
//!   (`SigV4QueryAuth._modify_request_before_signing`); only the canonical
//!   query string is sorted.
//! * `botocore/auth.py:807-846` — the POST policy signature: HMAC over the
//!   base64 policy document, not over a string-to-sign.
//!
//! A deliberate non-goal: botocore also supports signing arbitrary headers the
//! caller adds. Plane never adds any header to a presigned URL, and the object
//! operations in this crate control the whole header set, so signing is done
//! from an explicit `(name, value)` list.
//!
//! The signature version is always `s3v4`: Plane passes
//! `Config(signature_version="s3v4")` in every client it builds
//! (`storage.py:49-66`, `export_task.py:51-97`), so the deprecated `s3`/sigv2
//! signers are not implemented.

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// The SHA-256 of an empty body (`botocore/auth.py:EMPTY_SHA256_HASH`).
pub const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// The constant payload marker for presigned URLs (`botocore/auth.py`).
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// A credential triple. `token` is `Some` only for temporary credentials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub token: Option<String>,
}

impl Credentials {
    /// `access_key/yyyymmdd/region/s3/aws4_request`.
    pub fn scope(&self, date: &str, region: &str) -> String {
        format!(
            "{}/{}/{}/s3/aws4_request",
            self.access_key,
            &date[..8],
            region
        )
    }

    fn signing_key(&self, date: &str, region: &str) -> [u8; 32] {
        let k_date = hmac_sha256(
            format!("AWS4{}", self.secret_key).as_bytes(),
            &date.as_bytes()[..8],
        );
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        hmac_sha256(&k_service, b"aws4_request")
    }

    /// The POST-policy signature: HMAC over the base64 policy text.
    pub fn sign_policy(&self, date: &str, region: &str, policy_base64: &str) -> String {
        let key = self.signing_key(date, region);
        to_hex(&hmac_sha256(&key, policy_base64.as_bytes()))
    }

    /// The URL signature: HMAC over the string to sign.
    fn sign_string_to_sign(&self, date: &str, region: &str, string_to_sign: &str) -> String {
        let key = self.signing_key(date, region);
        to_hex(&hmac_sha256(&key, string_to_sign.as_bytes()))
    }
}

pub(crate) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    // `new_from_slice` only fails for key sizes HMAC cannot use; HMAC accepts
    // any length, so this cannot fail.
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// `SIGV4_TIMESTAMP`: `%Y%m%dT%H%M%SZ`.
pub fn format_date(at: DateTime<Utc>) -> String {
    at.format("%Y%m%dT%H%M%SZ").to_string()
}

/// The encoded pairs of a presigned query, in the order they appear in the URL.
pub(crate) struct QueryPresign<'a> {
    pub method: &'a str,
    pub scheme: &'a str,
    pub authority: &'a str,
    /// `_host_from_url(scheme://authority)`: lowercase, no default port. The
    /// URL keeps `authority`; the signature covers this value.
    pub host: &'a str,
    /// Already percent-encoded, starts with `/`.
    pub path: &'a str,
    /// Operation parameters, encoded, in serialization order.
    pub operation: Vec<(String, String)>,
    pub date: &'a str,
    pub region: &'a str,
    pub expires_in: i64,
    pub credentials: &'a Credentials,
}

/// Build the presigned URL exactly as `SigV4QueryAuth` leaves it.
pub(crate) fn presign_query(input: QueryPresign<'_>) -> String {
    let creds = input.credentials;
    let scope = creds.scope(input.date, input.region);

    // `SigV4QueryAuth._modify_request_before_signing` builds `auth_params` in
    // this dict order; `percent_encode_sequence` keeps it and percent-encodes
    // every value (`AKID/2026...` → `AKID%2F2026...`).
    let auth: Vec<(String, String)> = {
        let mut raw: Vec<(&str, String)> = vec![
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
            ("X-Amz-Credential", scope.clone()),
            ("X-Amz-Date", input.date.to_string()),
            ("X-Amz-Expires", input.expires_in.to_string()),
            ("X-Amz-SignedHeaders", "host".to_string()),
        ];
        if let Some(token) = &creds.token {
            raw.push(("X-Amz-Security-Token", token.clone()));
        }
        raw.into_iter()
            .map(|(name, value)| (name.to_string(), crate::percent::encode_component(&value)))
            .collect()
    };

    // `canonical_query_string_url` sorts the already-encoded pairs.
    let mut canonical_pairs: Vec<(&str, &str)> =
        Vec::with_capacity(input.operation.len() + auth.len());
    for (name, value) in &input.operation {
        canonical_pairs.push((name.as_str(), value.as_str()));
    }
    for (name, value) in &auth {
        canonical_pairs.push((name.as_str(), value.as_str()));
    }
    canonical_pairs.sort_unstable();
    let canonical_query = canonical_pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");

    // `S3SigV4QueryAuth._normalize_url_path` returns the path unchanged.
    let canonical_request = format!(
        "{}\n{}\n{}\nhost:{}\n\nhost\n{}",
        input.method, input.path, canonical_query, input.host, UNSIGNED_PAYLOAD
    );
    // `SigV4Auth.string_to_sign` uses `credential_scope` (no access key), not
    // the `Credential=` value that goes into the query string.
    let credential_scope = format!("{}/{}/s3/aws4_request", &input.date[..8], input.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        input.date,
        credential_scope,
        sha256_hex(canonical_request.as_bytes())
    );
    let signature = creds.sign_string_to_sign(input.date, input.region, &string_to_sign);

    let mut url = String::with_capacity(
        input.scheme.len() + input.authority.len() + input.path.len() + canonical_query.len() + 96,
    );
    url.push_str(input.scheme);
    url.push_str("://");
    url.push_str(input.authority);
    url.push_str(input.path);
    url.push('?');
    let mut first = true;
    for (name, value) in input.operation.iter().chain(auth.iter()) {
        if !first {
            url.push('&');
        }
        first = false;
        url.push_str(name);
        url.push('=');
        url.push_str(value);
    }
    url.push_str("&X-Amz-Signature=");
    url.push_str(&signature);
    url
}

/// One header to sign and send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    /// Lowercase.
    pub name: String,
    pub value: String,
}

/// A signed header request.
pub(crate) struct HeaderPresign {
    pub url: String,
    pub headers: Vec<Header>,
}

/// A header-signed request.
pub(crate) struct HeaderSigning<'a> {
    pub method: &'a str,
    pub scheme: &'a str,
    pub authority: &'a str,
    pub path: &'a str,
    pub query: &'a [(String, String)],
    /// Every header to sign; `host`, `x-amz-date` and `x-amz-content-sha256`
    /// are added by the signer.
    pub headers: Vec<Header>,
    pub body: &'a [u8],
    pub date: &'a str,
    pub region: &'a str,
    pub credentials: &'a Credentials,
}

/// Sign a header-based S3 request (`S3SigV4Auth`, `botocore/auth.py:462-500`).
///
/// `input.headers` must already contain every header the caller wants to sign
/// *except* `host`, `x-amz-date` and `x-amz-content-sha256`, which are added
/// here. The returned list is exactly what must be sent.
pub(crate) fn sign_headers(input: HeaderSigning<'_>) -> HeaderPresign {
    let HeaderSigning {
        method,
        scheme,
        authority,
        path,
        query,
        mut headers,
        body,
        date,
        region,
        credentials,
    } = input;
    let payload_hash = if body.is_empty() {
        EMPTY_SHA256.to_string()
    } else {
        sha256_hex(body)
    };
    headers.push(Header {
        name: "host".to_string(),
        value: authority.to_string(),
    });
    headers.push(Header {
        name: "x-amz-date".to_string(),
        value: date.to_string(),
    });
    headers.push(Header {
        name: "x-amz-content-sha256".to_string(),
        value: payload_hash.clone(),
    });
    if let Some(token) = &credentials.token {
        headers.push(Header {
            name: "x-amz-security-token".to_string(),
            value: token.clone(),
        });
    }
    headers.sort_by(|left, right| left.name.cmp(&right.name));

    let canonical_headers = headers
        .iter()
        .map(|header| {
            format!(
                "{}:{}",
                header.name,
                trim_collapse_whitespace(&header.value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let signed_headers = headers
        .iter()
        .map(|header| header.name.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let mut canonical_pairs: Vec<(&str, &str)> = query
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    canonical_pairs.sort_unstable();
    let canonical_query = canonical_pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n\n{}\n{}",
        method, path, canonical_query, canonical_headers, signed_headers, payload_hash
    );
    let scope = credentials.scope(date, region);
    let credential_scope = format!("{}/{}/s3/aws4_request", &date[..8], region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        date,
        credential_scope,
        sha256_hex(canonical_request.as_bytes())
    );
    let signature = credentials.sign_string_to_sign(date, region, &string_to_sign);

    let mut url = String::with_capacity(scheme.len() + authority.len() + path.len() + 64);
    url.push_str(scheme);
    url.push_str("://");
    url.push_str(authority);
    url.push_str(path);
    if !query.is_empty() {
        url.push('?');
        url.push_str(
            &query
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("&"),
        );
    }

    headers.push(Header {
        name: "authorization".to_string(),
        value: format!(
            "AWS4-HMAC-SHA256 Credential={scope}, SignedHeaders={signed_headers}, Signature={signature}"
        ),
    });

    HeaderPresign { url, headers }
}

/// `botocore/auth.py:354-361` `_header_value`: trim and collapse whitespace.
fn trim_collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 16, 12, 34, 56).unwrap()
    }

    fn creds() -> Credentials {
        Credentials {
            access_key: "AKID".to_string(),
            secret_key: "SECRET".to_string(),
            token: None,
        }
    }

    #[test]
    fn scope_uses_first_eight_date_chars() {
        let date = format_date(at());
        assert_eq!(date, "20261116T123456Z");
        assert_eq!(
            creds().scope(&date, "eu-central-1"),
            "AKID/20261116/eu-central-1/s3/aws4_request"
        );
    }

    #[test]
    fn empty_region_produces_double_slash_scope() {
        // Observed from boto3 with AWS_REGION="": MinIO accepts it.
        let date = format_date(at());
        assert_eq!(creds().scope(&date, ""), "AKID/20261116//s3/aws4_request");
    }

    #[test]
    fn presigned_url_shape() {
        let date = format_date(at());
        let url = presign_query(QueryPresign {
            method: "GET",
            scheme: "http",
            authority: "127.0.0.1:59000",
            host: "127.0.0.1:59000",
            path: "/uploads/a/b.png",
            operation: vec![(
                "response-content-disposition".to_string(),
                "inline".to_string(),
            )],
            date: &date,
            region: "us-east-1",
            expires_in: 60,
            credentials: &creds(),
        });
        assert!(url.starts_with("http://127.0.0.1:59000/uploads/a/b.png?response-content-disposition=inline&X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=AKID%2F20261116%2Fus-east-1%2Fs3%2Faws4_request&X-Amz-Date=20261116T123456Z&X-Amz-Expires=60&X-Amz-SignedHeaders=host&X-Amz-Signature="));
        assert_eq!(url.len(), url.find("X-Amz-Signature=").unwrap() + 16 + 64);
    }

    #[test]
    fn header_signing_is_stable() {
        let date = format_date(at());
        let signed = sign_headers(HeaderSigning {
            method: "GET",
            scheme: "http",
            authority: "127.0.0.1:59000",
            path: "/uploads/key",
            query: &[],
            headers: Vec::new(),
            body: b"",
            date: &date,
            region: "us-east-1",
            credentials: &creds(),
        });
        let auth = signed
            .headers
            .iter()
            .find(|header| header.name == "authorization")
            .unwrap();
        assert!(auth.value.starts_with(
            "AWS4-HMAC-SHA256 Credential=AKID/20261116/us-east-1/s3/aws4_request, \
             SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature="
        ));
        assert_eq!(
            signed
                .headers
                .iter()
                .find(|header| header.name == "x-amz-content-sha256")
                .unwrap()
                .value,
            EMPTY_SHA256
        );
    }

    #[test]
    fn whitespace_in_header_values_is_collapsed() {
        assert_eq!(trim_collapse_whitespace("  a \t b  "), "a b");
    }
}
