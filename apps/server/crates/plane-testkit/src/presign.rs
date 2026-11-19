// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! SigV4 verification for presigned S3 GET URLs (DECISIONS.md D17).
//!
//! Django (boto3) and `plane-storage` sign a presigned URL at the current
//! second, so the two targets of a differential step legitimately differ in
//! `X-Amz-Date` and therefore in `X-Amz-Signature`. Everything else in the URL
//! is part of the contract. The harness therefore verifies each side's
//! signature against that side's own credentials and date instead of comparing
//! the bytes.
//!
//! The canonical request is the one `plane-storage` builds
//! (`crates/plane-storage/src/sign.rs:129-205`) and boto3 builds before it:
//!
//! ```text
//! GET
//! <url path, already percent-encoded>
//! <encoded query pairs, sorted, minus X-Amz-Signature>
//! host:<lowercase host, default port stripped>
//!
//! host
//! UNSIGNED-PAYLOAD
//! ```
//!
//! signed with `AWS4-HMAC-SHA256`, the scope from `X-Amz-Credential`
//! (`<date>/<region>/s3/aws4_request`) and the date from `X-Amz-Date`. Only
//! `host` is ever a signed header in Plane (`sign.rs:142`); a URL that signs
//! anything else is reported, not guessed at.

use chrono::{DateTime, NaiveDateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// The dev stack's object-storage credentials, as the target process gets them.
#[derive(Clone)]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print the secret: `NormalizerConfig` derives Debug.
        f.debug_struct("Credentials")
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"<redacted>")
            .finish()
    }
}

impl Credentials {
    /// Read `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` from an environment
    /// map. Both must be present and non-empty.
    pub fn from_env(
        env: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, &'static str> {
        let access_key_id = env
            .get("AWS_ACCESS_KEY_ID")
            .filter(|value| !value.is_empty())
            .ok_or("AWS_ACCESS_KEY_ID is not set")?;
        let secret_access_key = env
            .get("AWS_SECRET_ACCESS_KEY")
            .filter(|value| !value.is_empty())
            .ok_or("AWS_SECRET_ACCESS_KEY is not set")?;
        Ok(Self {
            access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
        })
    }
}

/// A parsed presigned GET URL, its query pairs still percent-encoded and in
/// wire order.
#[derive(Clone, Debug)]
pub struct PresignedGet {
    scheme: String,
    authority: String,
    path: String,
    pairs: Vec<(String, String)>,
}

impl PresignedGet {
    pub fn parse(url: &str) -> Result<Self, String> {
        let (scheme, rest) = url
            .split_once("://")
            .ok_or_else(|| "no scheme".to_string())?;
        let (authority, rest) = rest
            .split_once('/')
            .map(|(authority, rest)| (authority, format!("/{rest}")))
            .ok_or_else(|| "no path".to_string())?;
        if authority.is_empty() {
            return Err("empty authority".to_string());
        }
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path.to_string(), query),
            None => return Err("no query string".to_string()),
        };
        let pairs = query
            .split('&')
            .filter(|item| !item.is_empty())
            .map(|item| match item.split_once('=') {
                Some((name, value)) => (name.to_string(), value.to_string()),
                None => (item.to_string(), String::new()),
            })
            .collect();
        Ok(Self {
            scheme: scheme.to_string(),
            authority: authority.to_string(),
            path,
            pairs,
        })
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn required(&self, name: &str) -> Result<&str, String> {
        self.param(name).ok_or_else(|| format!("missing {name}"))
    }

    /// The signed host: lowercase, default port stripped (`_host_from_url`).
    fn host(&self) -> String {
        let mut host = self.authority.to_ascii_lowercase();
        if (self.scheme == "http" && host.ends_with(":80"))
            || (self.scheme == "https" && host.ends_with(":443"))
        {
            let port = if self.scheme == "http" { ":80" } else { ":443" };
            host.truncate(host.len() - port.len());
        }
        host
    }

    pub fn date(&self) -> Result<DateTime<Utc>, String> {
        let raw = self.required("X-Amz-Date")?;
        let naive = NaiveDateTime::parse_from_str(raw, "%Y%m%dT%H%M%SZ")
            .map_err(|error| format!("X-Amz-Date {raw:?}: {error}"))?;
        Ok(naive.and_utc())
    }

    pub fn signature(&self) -> Result<&str, String> {
        self.required("X-Amz-Signature")
    }

    /// `<date>/<region>` from `X-Amz-Credential`
    /// (`<access>/<date>/<region>/s3/aws4_request`).
    fn scope(&self) -> Result<(String, String), String> {
        let credential = percent_decode(self.required("X-Amz-Credential")?);
        let parts: Vec<&str> = credential.split('/').collect();
        if parts.len() != 5 || parts[3] != "s3" || parts[4] != "aws4_request" {
            return Err(format!(
                "X-Amz-Credential {credential:?} is not <key>/<date>/<region>/s3/aws4_request"
            ));
        }
        Ok((parts[1].to_string(), parts[2].to_string()))
    }

    /// The canonical query: every encoded pair except the signature, sorted by
    /// (name, value) — `canonical_query_string_url`.
    fn canonical_query(&self) -> String {
        let mut pairs: Vec<(&str, &str)> = self
            .pairs
            .iter()
            .filter(|(name, _)| name != "X-Amz-Signature")
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        pairs.sort_unstable();
        pairs
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("&")
    }

    /// Recompute the signature with `credentials` and the URL's own
    /// `X-Amz-Date`, and compare it with `X-Amz-Signature`.
    pub fn verify(&self, credentials: &Credentials) -> Result<(), String> {
        let signed_headers = self.required("X-Amz-SignedHeaders")?;
        if signed_headers != "host" {
            return Err(format!(
                "X-Amz-SignedHeaders {signed_headers:?}: only `host` can be verified"
            ));
        }
        let date = self.date()?;
        let (scope_date, region) = self.scope()?;
        if scope_date != date.format("%Y%m%d").to_string() {
            return Err(format!(
                "X-Amz-Credential date {scope_date} does not match X-Amz-Date {date}"
            ));
        }
        let expected = signature(
            credentials,
            &self.host(),
            &self.path,
            &self.canonical_query(),
            &date.format("%Y%m%dT%H%M%SZ").to_string(),
            &region,
        );
        let actual = self.signature()?;
        if !expected.eq_ignore_ascii_case(actual) {
            return Err(format!(
                "X-Amz-Signature does not verify (expected {expected}, got {actual})"
            ));
        }
        Ok(())
    }

    /// The comparable form: the date becomes `<presigned-date:<epoch>>` and the
    /// signature `<presigned-signature>`; every other byte stays as wire.
    pub fn canonical(&self) -> String {
        let mut out = String::with_capacity(self.authority.len() + self.path.len() + 256);
        out.push_str(&self.scheme);
        out.push_str("://");
        out.push_str(&self.authority);
        out.push_str(&self.path);
        out.push('?');
        for (index, (name, value)) in self.pairs.iter().enumerate() {
            if index > 0 {
                out.push('&');
            }
            out.push_str(name);
            out.push('=');
            if name == "X-Amz-Date" {
                match self.date() {
                    Ok(date) => out.push_str(&format!("<presigned-date:{}>", date.timestamp())),
                    Err(_) => out.push_str(value),
                }
            } else if name == "X-Amz-Signature" {
                out.push_str("<presigned-signature>");
            } else {
                out.push_str(value);
            }
        }
        out
    }
}

/// `AWS4-HMAC-SHA256` over the canonical request.
fn signature(
    credentials: &Credentials,
    host: &str,
    path: &str,
    canonical_query: &str,
    date: &str,
    region: &str,
) -> String {
    let path = if path.is_empty() { "/" } else { path };
    let canonical_request =
        format!("GET\n{path}\n{canonical_query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD");
    let scope = format!("{}/{}/s3/aws4_request", &date[..8], region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );
    let secret = format!("AWS4{}", credentials.secret_access_key);
    let k_date = hmac(secret.as_bytes(), &date[..8]);
    let k_region = hmac(&k_date, region);
    let k_service = hmac(&k_region, "s3");
    let k_signing = hmac(&k_service, "aws4_request");
    hex(&hmac(&k_signing, &string_to_sign))
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Build a presigned GET URL the way `plane-storage`/boto3 do, for the
/// harness's own tests.
#[allow(clippy::too_many_arguments)]
pub fn sign_presigned_get(
    credentials: &Credentials,
    scheme: &str,
    authority: &str,
    path: &str,
    operation: &[(&str, &str)],
    date: &str,
    region: &str,
    expires_in: i64,
) -> String {
    let scope = format!("{}/{region}/s3/aws4_request", &date[..8.min(date.len())]);
    let encoded_scope = percent_encode(&format!("{}/{scope}", credentials.access_key_id));
    let mut auth: Vec<(String, String)> = vec![
        ("X-Amz-Algorithm".into(), "AWS4-HMAC-SHA256".into()),
        ("X-Amz-Credential".into(), encoded_scope),
        ("X-Amz-Date".into(), date.into()),
        ("X-Amz-Expires".into(), expires_in.to_string()),
        ("X-Amz-SignedHeaders".into(), "host".into()),
    ];
    let mut canonical: Vec<(String, String)> = operation
        .iter()
        .map(|(name, value)| ((*name).to_string(), percent_encode(value)))
        .collect();
    canonical.extend(auth.iter().cloned());
    canonical.sort_unstable();
    let canonical_query = canonical
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&");

    let host = {
        let mut host = authority.to_ascii_lowercase();
        if (scheme == "http" && host.ends_with(":80"))
            || (scheme == "https" && host.ends_with(":443"))
        {
            let port = if scheme == "http" { ":80" } else { ":443" };
            host.truncate(host.len() - port.len());
        }
        host
    };
    let signature = signature(credentials, &host, path, &canonical_query, date, region);
    let mut url = String::new();
    url.push_str(scheme);
    url.push_str("://");
    url.push_str(authority);
    url.push_str(path);
    url.push('?');
    for (index, (name, value)) in operation.iter().enumerate() {
        if index > 0 {
            url.push('&');
        }
        url.push_str(name);
        url.push('=');
        url.push_str(&percent_encode(value));
    }
    for (index, (name, value)) in auth.iter_mut().enumerate() {
        if index > 0 || !operation.is_empty() {
            url.push('&');
        }
        url.push_str(name);
        url.push('=');
        url.push_str(value);
    }
    url.push_str("&X-Amz-Signature=");
    url.push_str(&signature);
    url
}

/// boto3's `percent_encode`: unreserved characters stay, everything else is
/// `%XX` (uppercase hex).
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let text = &value[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(text, 16) {
                out.push(byte as char);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index] as char);
        index += 1;
    }
    out
}

/// Two canonical presigned URLs: identical except for the date, and at most
/// two seconds apart (DECISIONS.md D17).
///
/// `None` when either value is not in the canonical form, so the caller falls
/// back to the exact comparison.
pub fn equivalent(a: &str, b: &str) -> Option<bool> {
    let (a_rest, a_date) = split_date(a)?;
    let (b_rest, b_date) = split_date(b)?;
    Some(a_rest == b_rest && (a_date - b_date).abs() <= 2)
}

/// Replace every `<presigned-date:<epoch>>` in `text` with
/// `<presigned-date>`.
///
/// The HTTP body comparison verifies each signed URL and allows the two dates
/// to be within two seconds ([`equivalent`]). A `process_logs` message whose
/// body is byte-identical to that response needs the same equivalence, but the
/// transport comparison is a plain value comparison; drop the epoch there so a
/// one-second boundary cannot turn one delivery into two.
#[must_use]
pub fn generic_dates(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<presigned-date:") {
        out.push_str(&rest[..start]);
        let after = start + "<presigned-date:".len();
        match rest[after..].find('>') {
            Some(end) => {
                out.push_str("<presigned-date>");
                rest = &rest[after + end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn split_date(value: &str) -> Option<(String, i64)> {
    let start = value.find("<presigned-date:")?;
    let after = start + "<presigned-date:".len();
    let end = value[after..].find('>')? + after;
    let date: i64 = value[after..end].parse().ok()?;
    let mut rest = String::with_capacity(value.len());
    rest.push_str(&value[..start]);
    rest.push_str("<presigned-date>");
    rest.push_str(&value[end + 1..]);
    Some((rest, date))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credentials() -> Credentials {
        Credentials {
            access_key_id: "AKIDEXAMPLE".to_string(),
            secret_access_key: "SECRETEXAMPLE".to_string(),
        }
    }

    fn url(date: &str) -> String {
        sign_presigned_get(
            &credentials(),
            "http",
            "plane-fixture.test",
            "/uploads/a91c3d80/47efd89a-spec.pdf",
            &[(
                "response-content-disposition",
                "attachment; filename=\"spec.pdf\"",
            )],
            date,
            "us-east-1",
            60,
        )
    }

    #[test]
    fn a_signed_url_verifies_and_canonicalises() {
        let signed = url("20261116T123456Z");
        let parsed = PresignedGet::parse(&signed).expect("parse");
        parsed.verify(&credentials()).expect("verify");
        let canonical = parsed.canonical();
        assert!(canonical.contains("X-Amz-Date=<presigned-date:"));
        assert!(canonical.contains("X-Amz-Signature=<presigned-signature>"));
        assert!(!canonical.contains("X-Amz-Signature=e"));
    }

    #[test]
    fn a_tampered_signature_does_not_verify() {
        let signed = url("20261116T123456Z");
        let tampered = signed.replace("X-Amz-Signature=", "X-Amz-Signature=0");
        let parsed = PresignedGet::parse(&tampered).expect("parse");
        assert!(parsed.verify(&credentials()).is_err());
    }

    #[test]
    fn the_dates_of_two_canonical_urls_are_compared_within_two_seconds() {
        let one = PresignedGet::parse(&url("20261116T123456Z"))
            .expect("parse")
            .canonical();
        let two = PresignedGet::parse(&url("20261116T123457Z"))
            .expect("parse")
            .canonical();
        assert_eq!(equivalent(&one, &two), Some(true));
        let five = PresignedGet::parse(&url("20261116T123501Z"))
            .expect("parse")
            .canonical();
        assert_eq!(equivalent(&one, &five), Some(false));
    }

    #[test]
    fn a_transport_copy_drops_the_date_epoch() {
        let canonical = PresignedGet::parse(&url("20261116T123456Z"))
            .expect("parse")
            .canonical();
        let generic = generic_dates(&canonical);
        assert!(generic.contains("X-Amz-Date=<presigned-date>"));
        assert!(!generic.contains("<presigned-date:"));
        // Everything else, including the signature placeholder, is unchanged.
        assert!(generic.contains("X-Amz-Signature=<presigned-signature>"));
    }
}
