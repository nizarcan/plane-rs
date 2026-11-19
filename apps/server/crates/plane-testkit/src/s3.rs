// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The S3 side of `difftest task`: a tiny SigV4 client for the dev MinIO.
//!
//! The harness must observe what a task did to object storage — the key, the
//! bytes, the `Content-Type` and the ACL (`plans/CUTOVER.md` §1 row 14: the
//! export bucket is `public-read`; `§6` "Risks": keys and ACLs are observable).
//! The server workspace already has a full client (`plane-storage`), but the
//! testkit is a separate Cargo workspace and must not depend on it
//! (`apps/server/Cargo.toml`), so this module implements the four operations
//! the harness needs: `PutObject`, `ListObjectsV2`, `GetObject`+HEAD and
//! `GetObjectAcl`, plus bucket create/delete.
//!
//! Only path-style addressing over plain HTTP is supported: the dev stack is
//! `http://127.0.0.1:59000`, and a differential run must never leave the
//! machine. The signature is the standard SigV4 `s3` scope with the body's
//! SHA-256 (S3 rejects `UNSIGNED-PAYLOAD` on a non-TLS endpoint in some
//! configurations; hashing the small fixture bodies is cheap and simpler).

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// A recorded object as both sides see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedObject {
    /// Key relative to the side's bucket.
    pub key: String,
    pub content_type: Option<String>,
    /// `permission|grantee` entries, sorted and de-duplicated.
    pub acl: Vec<String>,
    pub body: Vec<u8>,
}

/// What a case seeds into each side's bucket before the task runs.
#[derive(Debug, Clone)]
pub struct SeedObject {
    pub key: String,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub acl: Option<String>,
}

/// A dev MinIO connection plus the per-side bucket names.
#[derive(Debug, Clone)]
pub struct S3Admin {
    endpoint: String,
    access_key: String,
    secret_key: String,
    region: String,
}

impl S3Admin {
    pub fn new(
        endpoint: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> Self {
        Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            region: "us-east-1".to_string(),
        }
    }

    /// Create a bucket, first removing it if it exists so a failed earlier run
    /// cannot leak objects into this one. The caller owns the name: the harness
    /// passes the bucket names it created and deletes exactly those.
    pub async fn recreate_bucket(&self, bucket: &str) -> Result<()> {
        if self.bucket_exists(bucket).await? {
            self.empty_bucket(bucket).await?;
            self.delete_bucket(bucket).await?;
        }
        let response = self.send("PUT", &format!("/{bucket}"), "", b"").await?;
        if response.status != 200 {
            bail!(
                "creating bucket {bucket} failed: {} {}",
                response.status,
                String::from_utf8_lossy(&response.body)
            );
        }
        Ok(())
    }

    pub async fn bucket_exists(&self, bucket: &str) -> Result<bool> {
        let response = self.send("HEAD", &format!("/{bucket}"), "", b"").await?;
        match response.status {
            200 => Ok(true),
            404 => Ok(false),
            other => bail!("HEAD bucket {bucket} returned {other}"),
        }
    }

    pub async fn empty_bucket(&self, bucket: &str) -> Result<()> {
        for key in self.list_keys(bucket).await? {
            self.delete_object(bucket, &key).await?;
        }
        Ok(())
    }

    pub async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        let response = self.send("DELETE", &format!("/{bucket}"), "", b"").await?;
        if !matches!(response.status, 200 | 204) {
            bail!(
                "deleting bucket {bucket} failed: {} {}",
                response.status,
                String::from_utf8_lossy(&response.body)
            );
        }
        Ok(())
    }

    pub async fn put_object(&self, bucket: &str, seed: &SeedObject) -> Result<()> {
        let path = format!("/{bucket}/{}", percent_encode_path_segment(&seed.key));
        let mut extra: Vec<(&str, String)> = Vec::new();
        if let Some(content_type) = &seed.content_type {
            extra.push(("content-type", content_type.clone()));
        }
        if let Some(acl) = &seed.acl {
            extra.push(("x-amz-acl", acl.clone()));
        }
        let response = self.send_with("PUT", &path, "", &extra, &seed.body).await?;
        if response.status != 200 {
            bail!(
                "PUT s3://{bucket}/{} failed: {} {}",
                seed.key,
                response.status,
                String::from_utf8_lossy(&response.body)
            );
        }
        Ok(())
    }

    pub async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        let path = format!("/{bucket}/{}", percent_encode_path_segment(key));
        let response = self.send("DELETE", &path, "", b"").await?;
        if !matches!(response.status, 204 | 200) {
            bail!("DELETE s3://{bucket}/{key} failed: {}", response.status);
        }
        Ok(())
    }

    /// Every object in the bucket, sorted by key.
    pub async fn observe(&self, bucket: &str) -> Result<Vec<ObservedObject>> {
        let mut objects = Vec::new();
        for key in self.list_keys(bucket).await? {
            let path = format!("/{bucket}/{}", percent_encode_path_segment(&key));
            let response = self.send("GET", &path, "", b"").await?;
            if response.status != 200 {
                bail!("GET s3://{bucket}/{key} returned {}", response.status);
            }
            let content_type = response.header("content-type");
            let acl = self.object_acl(bucket, &key).await?;
            objects.push(ObservedObject {
                key,
                content_type,
                acl,
                body: response.body,
            });
        }
        objects.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(objects)
    }

    async fn object_acl(&self, bucket: &str, key: &str) -> Result<Vec<String>> {
        let path = format!("/{bucket}/{}", percent_encode_path_segment(key));
        let response = self.send("GET", &path, "acl=", b"").await?;
        if response.status != 200 {
            bail!("GET s3://{bucket}/{key}?acl returned {}", response.status);
        }
        let xml = String::from_utf8_lossy(&response.body);
        let mut grants = BTreeSet::new();
        for grant in xml_elements(&xml, "Grant") {
            let permission = xml_elements(grant, "Permission")
                .into_iter()
                .next()
                .unwrap_or_default()
                .to_string();
            let grantee = xml_elements(grant, "URI")
                .into_iter()
                .next()
                .or_else(|| xml_elements(grant, "ID").into_iter().next())
                .unwrap_or("")
                .to_string();
            grants.insert(format!("{permission}|{grantee}"));
        }
        Ok(grants.into_iter().collect())
    }

    /// `<Key>` values of a `ListObjectsV2` response. The dev buckets hold a
    /// handful of objects, so pagination is not implemented; a truncated
    /// response is an error rather than a silently partial observation.
    async fn list_keys(&self, bucket: &str) -> Result<Vec<String>> {
        let response = self
            .send("GET", &format!("/{bucket}"), "list-type=2", b"")
            .await?;
        if response.status != 200 {
            bail!("listing bucket {bucket} returned {}", response.status);
        }
        let xml = String::from_utf8_lossy(&response.body).to_string();
        if xml_elements(&xml, "IsTruncated")
            .first()
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
        {
            bail!("bucket {bucket} listing is truncated; the harness does not paginate");
        }
        let mut keys: Vec<String> = xml_elements(&xml, "Key")
            .into_iter()
            .map(str::to_string)
            .collect();
        keys.sort();
        Ok(keys)
    }

    async fn send(&self, method: &str, path: &str, query: &str, body: &[u8]) -> Result<S3Response> {
        self.send_with(method, path, query, &[], body).await
    }

    async fn send_with(
        &self,
        method: &str,
        path: &str,
        query: &str,
        extra_headers: &[(&str, String)],
        body: &[u8],
    ) -> Result<S3Response> {
        let url = if query.is_empty() {
            format!("{}{}", self.endpoint, path)
        } else {
            format!("{}{}?{}", self.endpoint, path, query)
        };
        let host = self
            .endpoint
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&self.endpoint)
            .to_string();
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let payload_hash = hex(&Sha256::digest(body));

        let mut headers: Vec<(String, String)> = vec![
            ("host".to_string(), host),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), amz_date.clone()),
        ];
        for (name, value) in extra_headers {
            headers.push((name.to_ascii_lowercase(), value.clone()));
        }
        headers.sort_by(|left, right| left.0.cmp(&right.0));

        let canonical_headers: String = headers
            .iter()
            .map(|(name, value)| format!("{name}:{}\n", value.trim()))
            .collect();
        let signed_headers = headers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(";");
        let canonical_query = canonical_query(query);
        let canonical_request = format!(
            "{method}\n{path}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            hex(&Sha256::digest(canonical_request.as_bytes()))
        );
        let signing_key = signing_key(&self.secret_key, &date, &self.region);
        let signature = hex(&hmac(&signing_key, string_to_sign.as_bytes()));
        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key
        );

        let mut request = reqwest::Client::new().request(
            reqwest::Method::from_bytes(method.as_bytes()).context("HTTP method")?,
            &url,
        );
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        request = request.header("authorization", authorization);
        if !body.is_empty() {
            request = request.body(body.to_vec());
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("{method} {url}"))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = response.bytes().await?.to_vec();
        Ok(S3Response {
            status,
            content_type,
            body,
        })
    }
}

struct S3Response {
    status: u16,
    content_type: Option<String>,
    body: Vec<u8>,
}

impl S3Response {
    fn header(&self, name: &str) -> Option<String> {
        if name == "content-type" {
            self.content_type.clone()
        } else {
            None
        }
    }
}

fn signing_key(secret: &str, date: &str, region: &str) -> Vec<u8> {
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    key = hmac(&key, region.as_bytes());
    key = hmac(&key, b"s3");
    hmac(&key, b"aws4_request")
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `list-type=2` and `acl=` as SigV4 canonical query strings.
fn canonical_query(query: &str) -> String {
    let mut parts: Vec<(String, String)> = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        parts.push((percent_encode(name), percent_encode(value)));
    }
    parts.sort();
    parts
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// RFC 3986 unreserved characters stay literal; a path segment is encoded
/// with `/` also encoded (the caller joins segments with `/`).
fn percent_encode_path_segment(segment: &str) -> String {
    segment
        .split('/')
        .map(percent_encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The text of every `<name>...</name>` element. The harness's fixture keys are
/// plain ASCII paths, so the five XML entities S3 may use are not decoded here;
/// a key containing `&` would be reported in its escaped form.
fn xml_elements<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    // The returned slices borrow from `xml`; entity decoding is done by the
    // caller only for keys, where `%` and `&` are rare. Keeping the raw slice
    // here avoids allocating for the ACL fields.
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let mut values = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(&close) else {
            break;
        };
        values.push(&after[..end]);
        rest = &after[end + close.len()..];
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_query_sorts_and_encodes() {
        assert_eq!(canonical_query("list-type=2"), "list-type=2");
        assert_eq!(canonical_query("acl="), "acl=");
        assert_eq!(canonical_query("b=2&a=1"), "a=1&b=2");
        assert_eq!(canonical_query("prefix=a b"), "prefix=a%20b");
    }

    #[test]
    fn path_segments_keep_slashes_between_segments() {
        assert_eq!(
            percent_encode_path_segment("ws/export-a b.zip"),
            "ws/export-a%20b.zip"
        );
        assert_eq!(percent_encode_path_segment("a+b"), "a%2Bb");
    }

    #[test]
    fn xml_elements_finds_each_key() {
        let xml = "<ListBucketResult><Contents><Key>a</Key></Contents>\
                   <Contents><Key>b</Key></Contents><IsTruncated>false</IsTruncated></ListBucketResult>";
        assert_eq!(xml_elements(xml, "Key"), vec!["a", "b"]);
        assert_eq!(xml_elements(xml, "IsTruncated"), vec!["false"]);
    }
}
