// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The POST policy document, byte for byte as boto3 writes it.
//!
//! `S3Storage.generate_presigned_post` (`plane/settings/storage.py:68-102`)
//! builds a `fields` dict and a `conditions` list and passes them to boto3's
//! `generate_presigned_post` (`botocore/signers.py:723-846`). That function
//! appends its own bucket and key conditions to the list Plane supplied, then
//! `S3PostPresigner.generate_presigned_post` (`botocore/signers.py:558-620`)
//! puts them in a policy document:
//!
//! ```text
//! {"expiration": "2026-11-16T13:34:56Z", "conditions": [...]}
//! ```
//!
//! and `S3SigV4PostAuth.add_auth` (`botocore/auth.py:807-846`) appends the
//! `x-amz-*` conditions, `json.dumps`es the document with Python's *default*
//! separators (`", "` and `": "`), base64-encodes it, signs the base64 text and
//! returns the fields in insertion order.
//!
//! Two consequences are worth spelling out because they look like bugs and are
//! ported as they are:
//!
//! * `bucket` and `key` appear **twice** in `conditions`: Plane's list already
//!   contains them (`storage.py:74-85`) and boto3 appends them again.
//! * Plane checks `object_name.startswith("${filename}")` while boto3 checks
//!   `key.endswith("${filename}")` (`signers.py:833`). A key that starts with
//!   the placeholder but does not end with it gets a truncated
//!   `starts-with "$key"` condition from Plane *and* an exact `key` condition
//!   from boto3.

use chrono::{DateTime, Duration, Utc};

use crate::error::StorageError;
use crate::percent::push_json_string;

/// A single entry in `conditions`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Condition<'a> {
    /// `{"name": "value"}`
    KeyValue(&'a str, &'a str),
    /// `{"name": null}` — Plane passes `None` when the client sends `type: null`.
    KeyValueNull(&'a str),
    /// `["content-length-range", min, max]`
    LengthRange(i64, i64),
    /// `["starts-with", "$name", "prefix"]`
    StartsWith(&'a str, &'a str),
}

/// The document plus the fields that precede `policy` and `x-amz-signature`.
pub(crate) struct Policy {
    pub document: String,
    pub fields: Vec<(String, Option<String>)>,
}

/// `generate_presigned_post` inputs, after Plane's wrapper has run.
pub(crate) struct PolicyInput<'a> {
    pub bucket: &'a str,
    pub key: &'a str,
    pub file_type: Option<&'a str>,
    pub file_size: i64,
    pub expires_in: i64,
    pub now: DateTime<Utc>,
    /// `access_key/date/region/s3/aws4_request` (`SigV4Auth.scope`).
    pub credential_scope: &'a str,
    /// `20261116T123456Z` (`SIGV4_TIMESTAMP`).
    pub date: &'a str,
    pub token: Option<&'a str>,
}

const FILENAME_PLACEHOLDER: &str = "${filename}";

pub(crate) fn build(input: PolicyInput<'_>) -> Result<Policy, StorageError> {
    let expires_at = input
        .now
        .checked_add_signed(Duration::seconds(input.expires_in))
        .ok_or_else(|| StorageError::Config("expiry is out of range".to_string()))?;

    // storage.py:72 — the wrapper's fields start with Content-Type only.
    let mut fields: Vec<(String, Option<String>)> = vec![(
        "Content-Type".to_string(),
        input.file_type.map(str::to_string),
    )];

    // storage.py:74-85.
    let mut conditions = vec![
        Condition::KeyValue("bucket", input.bucket),
        Condition::LengthRange(1, input.file_size),
        match input.file_type {
            Some(value) => Condition::KeyValue("Content-Type", value),
            None => Condition::KeyValueNull("Content-Type"),
        },
    ];
    if input.key.starts_with(FILENAME_PLACEHOLDER) {
        // storage.py:81-82 uses `object_name[: -len("${filename}")]`. When the
        // key is shorter than or equal to the placeholder this is empty; when
        // it merely *starts* with the placeholder the prefix is truncated.
        let prefix = &input.key[..input.key.len() - FILENAME_PLACEHOLDER.len()];
        conditions.push(Condition::StartsWith("$key", prefix));
    } else {
        fields.push(("key".to_string(), Some(input.key.to_string())));
        conditions.push(Condition::KeyValue("key", input.key));
    }

    // botocore/signers.py:832-838 — appended again, on top of Plane's list.
    conditions.push(Condition::KeyValue("bucket", input.bucket));
    if input.key.ends_with(FILENAME_PLACEHOLDER) {
        let prefix = &input.key[..input.key.len() - FILENAME_PLACEHOLDER.len()];
        conditions.push(Condition::StartsWith("$key", prefix));
    } else {
        conditions.push(Condition::KeyValue("key", input.key));
    }
    if !fields.iter().any(|(name, _)| name == "key") {
        // `fields['key'] = key` in signers.py:842 inserts after Content-Type.
        fields.push(("key".to_string(), Some(input.key.to_string())));
    }

    // botocore/auth.py:826-840.
    fields.push((
        "x-amz-algorithm".to_string(),
        Some("AWS4-HMAC-SHA256".to_string()),
    ));
    fields.push((
        "x-amz-credential".to_string(),
        Some(input.credential_scope.to_string()),
    ));
    fields.push(("x-amz-date".to_string(), Some(input.date.to_string())));
    conditions.push(Condition::KeyValue("x-amz-algorithm", "AWS4-HMAC-SHA256"));
    conditions.push(Condition::KeyValue(
        "x-amz-credential",
        input.credential_scope,
    ));
    conditions.push(Condition::KeyValue("x-amz-date", input.date));
    if let Some(token) = input.token {
        fields.push(("x-amz-security-token".to_string(), Some(token.to_string())));
        conditions.push(Condition::KeyValue("x-amz-security-token", token));
    }

    let mut document = String::with_capacity(512);
    document.push_str("{\"expiration\": ");
    push_json_string(&mut document, &format_timestamp(expires_at));
    document.push_str(", \"conditions\": [");
    for (index, condition) in conditions.iter().enumerate() {
        if index > 0 {
            document.push_str(", ");
        }
        push_condition(&mut document, condition);
    }
    document.push_str("]}");

    Ok(Policy { document, fields })
}

fn push_condition(out: &mut String, condition: &Condition<'_>) {
    match condition {
        Condition::KeyValue(name, value) => {
            out.push('{');
            push_json_string(out, name);
            out.push_str(": ");
            push_json_string(out, value);
            out.push('}');
        }
        Condition::KeyValueNull(name) => {
            out.push('{');
            push_json_string(out, name);
            out.push_str(": null}");
        }
        Condition::LengthRange(min, max) => {
            out.push_str("[\"content-length-range\", ");
            out.push_str(&min.to_string());
            out.push_str(", ");
            out.push_str(&max.to_string());
            out.push(']');
        }
        Condition::StartsWith(name, prefix) => {
            out.push_str("[\"starts-with\", ");
            push_json_string(out, name);
            out.push_str(", ");
            push_json_string(out, prefix);
            out.push(']');
        }
    }
}

/// `strftime("%Y-%m-%dT%H:%M:%SZ")` on a UTC instant (`botocore/auth.py:ISO8601`).
pub fn format_timestamp(at: DateTime<Utc>) -> String {
    at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 16, 12, 34, 56).unwrap()
    }

    #[test]
    fn exact_key_condition_is_repeated() {
        let policy = build(PolicyInput {
            bucket: "uploads",
            key: "ws/file name.png",
            file_type: Some("image/png"),
            file_size: 5242880,
            expires_in: 3600,
            now: at(),
            credential_scope: "AKID/20261116/us-east-1/s3/aws4_request",
            date: "20261116T123456Z",
            token: None,
        })
        .unwrap();
        assert_eq!(
            policy.document,
            "{\"expiration\": \"2026-11-16T13:34:56Z\", \"conditions\": [\
             {\"bucket\": \"uploads\"}, [\"content-length-range\", 1, 5242880], \
             {\"Content-Type\": \"image/png\"}, {\"key\": \"ws/file name.png\"}, \
             {\"bucket\": \"uploads\"}, {\"key\": \"ws/file name.png\"}, \
             {\"x-amz-algorithm\": \"AWS4-HMAC-SHA256\"}, \
             {\"x-amz-credential\": \"AKID/20261116/us-east-1/s3/aws4_request\"}, \
             {\"x-amz-date\": \"20261116T123456Z\"}]}"
        );
        assert_eq!(
            policy.fields,
            vec![
                ("Content-Type".to_string(), Some("image/png".to_string())),
                ("key".to_string(), Some("ws/file name.png".to_string())),
                (
                    "x-amz-algorithm".to_string(),
                    Some("AWS4-HMAC-SHA256".to_string())
                ),
                (
                    "x-amz-credential".to_string(),
                    Some("AKID/20261116/us-east-1/s3/aws4_request".to_string())
                ),
                (
                    "x-amz-date".to_string(),
                    Some("20261116T123456Z".to_string())
                ),
            ]
        );
    }

    #[test]
    fn starts_with_suffix_quirk() {
        // Plane's startswith check passes but boto3's endswith check does not.
        let policy = build(PolicyInput {
            bucket: "uploads",
            key: "${filename}-suffix",
            file_type: None,
            file_size: 1,
            expires_in: 60,
            now: at(),
            credential_scope: "AKID/20261116/us-east-1/s3/aws4_request",
            date: "20261116T123456Z",
            token: None,
        })
        .unwrap();
        assert!(policy.document.contains(
            "{\"Content-Type\": null}, [\"starts-with\", \"$key\", \"${filen\"], \
             {\"bucket\": \"uploads\"}, {\"key\": \"${filename}-suffix\"}"
        ));
        assert_eq!(
            policy.fields[0],
            ("Content-Type".to_string(), None),
            "null Content-Type is preserved in the fields"
        );
    }

    #[test]
    fn token_adds_field_and_condition() {
        let policy = build(PolicyInput {
            bucket: "uploads",
            key: "k",
            file_type: Some("image/png"),
            file_size: 1,
            expires_in: 1,
            now: at(),
            credential_scope: "AKID/20261116/us-east-1/s3/aws4_request",
            date: "20261116T123456Z",
            token: Some("TOKEN"),
        })
        .unwrap();
        assert_eq!(
            policy
                .fields
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Content-Type",
                "key",
                "x-amz-algorithm",
                "x-amz-credential",
                "x-amz-date",
                "x-amz-security-token"
            ]
        );
        assert!(
            policy
                .document
                .contains("{\"x-amz-security-token\": \"TOKEN\"}]}")
        );
    }
}
