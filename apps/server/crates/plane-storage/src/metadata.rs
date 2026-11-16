// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `S3Storage.get_object_metadata` (`plane/settings/storage.py:153-169`) and
//! the `head_object` response fields it keeps.
//!
//! The five keys and their order are part of the contract: the dict is stored in
//! `FileAsset.storage_metadata` (a `jsonb` column) and returned by the asset
//! serializers. Note that a value that has been through Postgres `jsonb` comes
//! back in jsonb's canonical key order (`ETag`, `Metadata`, `ContentType`,
//! `LastModified`, `ContentLength`), not this order — a Rust read path that
//! re-renders `storage_metadata` must preserve the order the database returns,
//! not the insertion order here. This struct uses the insertion order because
//! that is what Django writes and what a freshly-created object returns.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The five fields Plane keeps from `head_object`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMetadata {
    /// Response `Content-Type`.
    #[serde(rename = "ContentType")]
    pub content_type: Option<String>,
    /// Response `Content-Length`, as a JSON number.
    #[serde(rename = "ContentLength")]
    pub content_length: Option<i64>,
    /// `LastModified.isoformat()`, for example `2026-11-16T03:08:00+00:00`.
    #[serde(rename = "LastModified")]
    pub last_modified: Option<String>,
    /// The quoted ETag (`"\"5d41402...\""`).
    #[serde(rename = "ETag")]
    pub etag: Option<String>,
    /// `x-amz-meta-*` response headers with the prefix removed.
    #[serde(rename = "Metadata")]
    pub metadata: BTreeMap<String, String>,
}

/// Parse an HTTP `Last-Modified` value (`IMF-fixdate`, RFC 9110 §5.6.7).
///
/// S3 and MinIO always send `Sun, 06 Nov 1994 08:49:37 GMT`. boto3 parses it
/// into a timezone-aware `datetime` with `tzutc` and Plane calls `.isoformat()`,
/// which is why the result is a string with a `+00:00` offset and no fractional
/// seconds. The two obsolete HTTP date formats are not produced by S3; they are
/// rejected here rather than guessed.
pub fn format_http_date(value: &str) -> Option<String> {
    let value = value.trim();
    let mut parts = value.split_whitespace();
    let _weekday = parts.next()?;
    let day: u32 = parts.next()?.parse().ok()?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i32 = parts.next()?.parse().ok()?;
    let time = parts.next()?;
    if parts.next()? != "GMT" {
        return None;
    }
    let mut clock = time.split(':');
    let hour: u32 = clock.next()?.parse().ok()?;
    let minute: u32 = clock.next()?.parse().ok()?;
    let second: u32 = clock.next()?.parse().ok()?;
    if clock.next().is_some() {
        return None;
    }
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let time = chrono::NaiveTime::from_hms_opt(hour, minute, second)?;
    let utc = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
        chrono::NaiveDateTime::new(date, time),
        chrono::Utc,
    );
    // `datetime.isoformat()` for a UTC datetime: `+00:00`, never `Z`.
    Some(utc.format("%Y-%m-%dT%H:%M:%S+00:00").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_date_matches_python_isoformat() {
        assert_eq!(
            format_http_date("Mon, 16 Nov 2026 03:08:34 GMT").as_deref(),
            Some("2026-11-16T03:08:34+00:00")
        );
        assert_eq!(
            format_http_date("Thu, 01 Jan 1970 00:00:00 GMT").as_deref(),
            Some("1970-01-01T00:00:00+00:00")
        );
        assert_eq!(format_http_date("garbage"), None);
        assert_eq!(format_http_date("Mon, 16 Nov 2026 03:08:34 UTC"), None);
    }

    #[test]
    fn metadata_serialises_with_plane_key_names() {
        let metadata = ObjectMetadata {
            content_type: Some("text/plain".to_string()),
            content_length: Some(5),
            last_modified: Some("2026-11-16T03:08:00+00:00".to_string()),
            etag: Some("\"5d41402abc4b2a76b9719d911017c592\"".to_string()),
            metadata: BTreeMap::new(),
        };
        assert_eq!(
            serde_json::to_string(&metadata).unwrap(),
            "{\"ContentType\":\"text/plain\",\"ContentLength\":5,\
             \"LastModified\":\"2026-11-16T03:08:00+00:00\",\
             \"ETag\":\"\\\"5d41402abc4b2a76b9719d911017c592\\\"\",\"Metadata\":{}}"
        );
    }
}
