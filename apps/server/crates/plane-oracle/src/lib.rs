// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Locate the Django oracle at the upstream commit pinned in `UPSTREAM.toml`.
//!
//! `apps/api` is not part of this repository. The oracle is that subtree
//! exported from the pinned upstream commit by `tools/upstream/oracle.sh` into
//!
//! ```text
//! ${PLANE_RS_ORACLE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/plane-rs/oracle/<commit>}/
//!     apps/api/
//!         .venv/bin/python
//! ```
//!
//! `PLANE_RS_ORACLE_DIR` overrides the commit directory (it still contains
//! `apps/api/`); `PLANE_RS_CACHE_DIR` overrides only the cache root. The Python
//! side of the same lookup is `tools/upstream/oracle.py`; the shell side is
//! `tools/upstream/oracle.sh`. No path is hard-coded in a test or a tool.

use std::path::{Path, PathBuf};

/// The repository root, derived from this crate's manifest location.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The commit `UPSTREAM.toml` pins.
///
/// The file is small and its schema is ours, so this is a section-aware line
/// scanner rather than a TOML dependency: only `commit` inside `[upstream]`
/// counts, and a missing value is a loud error (every caller needs the oracle).
pub fn upstream_commit() -> String {
    let path = repo_root().join("UPSTREAM.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    parse_commit(&text)
        .unwrap_or_else(|| panic!("[upstream] commit is missing from {}", path.display()))
}

/// `commit = "..."` inside the `[upstream]` table.
fn parse_commit(text: &str) -> Option<String> {
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim().to_string();
            continue;
        }
        if section != "upstream" {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "commit" {
            continue;
        }
        let value = value.trim().trim_matches('"');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// The cache root: `PLANE_RS_CACHE_DIR`, else `$XDG_CACHE_HOME/plane-rs`, else
/// `$HOME/.cache/plane-rs`.
pub fn cache_root() -> PathBuf {
    if let Some(explicit) = std::env::var_os("PLANE_RS_CACHE_DIR") {
        return PathBuf::from(explicit);
    }
    if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("plane-rs");
    }
    home().join(".cache").join("plane-rs")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The materialised upstream tree root (contains `apps/api/`).
pub fn oracle_root() -> PathBuf {
    if let Some(explicit) = std::env::var_os("PLANE_RS_ORACLE_DIR") {
        return PathBuf::from(explicit);
    }
    cache_root().join("oracle").join(upstream_commit())
}

/// The oracle's `apps/api`: the path every `ported from` citation names.
pub fn api_dir() -> PathBuf {
    oracle_root().join("apps").join("api")
}

/// The oracle's virtualenv interpreter.
pub fn python() -> PathBuf {
    api_dir().join(".venv").join("bin").join("python")
}

/// `api_dir()`, or a panic that names the exact command that fetches it.
///
/// Tests and tools that need Django call this instead of constructing a path;
/// a missing oracle is a setup error, not a silent skip.
pub fn require_api() -> PathBuf {
    let api = api_dir();
    if !api.join("manage.py").is_file() || !python().is_file() {
        panic!(
            "the Django oracle for {} is not at {}; run \
             apps/server/tools/upstream/oracle.sh fetch",
            upstream_commit(),
            oracle_root().display()
        );
    }
    api
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pinned_commit_is_a_full_sha() {
        let commit = upstream_commit();
        assert_eq!(commit.len(), 40, "commit {commit:?}");
        assert!(commit.chars().all(|c| c.is_ascii_hexdigit()), "{commit:?}");
    }

    #[test]
    fn only_the_upstream_section_supplies_the_commit() {
        let text = r#"
            [other]
            commit = "not-this-one"
            [upstream]
            describe = "v1.2.3"
            commit = "0123456789abcdef0123456789abcdef01234567"
        "#;
        assert_eq!(
            parse_commit(text).as_deref(),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
        assert_eq!(parse_commit("[upstream]\ndescribe = \"x\""), None);
    }

    #[test]
    fn the_default_layout_matches_the_python_helper() {
        let api = api_dir();
        assert!(api.ends_with("apps/api"), "{}", api.display());
        assert!(
            python().ends_with(".venv/bin/python"),
            "{}",
            python().display()
        );
    }
}
