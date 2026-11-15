// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Small helpers shared by the differential-test harness.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::de::DeserializeOwned;
use uuid::Uuid;

/// Repository root, derived from the crate location at compile time.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A short identifier for one difftest run. Used in database and vhost names,
/// so it stays inside `[a-z0-9]`.
pub fn new_run_id() -> String {
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

pub fn now_utc() -> DateTime<Utc> {
    Utc::now()
}

pub fn format_ts(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Read and deserialize a YAML file with a path-qualified error message.
pub fn read_yaml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_yaml_ng::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Resolve a path relative to the repository root unless it is absolute.
pub fn resolve_repo_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root().join(path)
    }
}

/// Truncate long text for console output, keeping both ends.
pub fn ellipsize(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let head = max / 2;
    let tail = max - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text
        .chars()
        .rev()
        .take(tail)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{start}\n… ({} bytes elided) …\n{end}", text.len() - max)
}

/// Read a dotted path (`header.name`, `a.b.0`) out of a JSON value.
pub fn json_pointer<'a>(
    value: &'a serde_json::Value,
    pointer: &str,
) -> Result<&'a serde_json::Value> {
    let pointer = if pointer.starts_with('/') {
        pointer.to_string()
    } else {
        format!("/{}", pointer.replace('.', "/"))
    };
    value
        .pointer(&pointer)
        .ok_or_else(|| anyhow!("JSON pointer {pointer} not found"))
}

/// Validate a Postgres identifier we generate ourselves.
pub fn validate_identifier(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 63
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(anyhow!(
            "generated identifier {name:?} is not a safe lower-case Postgres identifier"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_ids_are_short_and_safe() {
        let id = new_run_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        validate_identifier(&id).expect("run id must be a safe identifier");
    }

    #[test]
    fn json_pointer_accepts_both_forms() {
        let value = serde_json::json!({"a": {"b": [10, 20]}});
        assert_eq!(json_pointer(&value, "a.b.1").expect("pointer"), &20);
        assert_eq!(json_pointer(&value, "/a/b/0").expect("pointer"), &10);
        assert!(json_pointer(&value, "a.missing").is_err());
    }

    #[test]
    fn invalid_identifiers_are_rejected() {
        assert!(validate_identifier("dt_abc_a").is_ok());
        assert!(validate_identifier("dt-abc").is_err());
        assert!(validate_identifier("").is_err());
        assert!(validate_identifier("UPPER").is_err());
    }
}
