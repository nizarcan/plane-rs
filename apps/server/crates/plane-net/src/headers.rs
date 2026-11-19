// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Response headers as Python exposes them to the webhook log.
//!
//! `webhook_task.save_webhook_log` stores `str(response.headers)` in
//! `webhook_logs.response_headers`. In Python that is a
//! `requests.structures.CaseInsensitiveDict` built from urllib3's
//! `HTTPHeaderDict`:
//!
//! * names keep the casing they were received with;
//! * repeated names are merged into one entry, values joined with `", "`, at
//!   the position the name first appeared;
//! * the rendered string is an ordinary `dict` repr: `{'Server': 'nginx'}`.
//!
//! `reqwest`/`http` do not promise iteration order, so this list is
//! best-effort: hyper parses headers in order and appends them, and the live
//! test asserts the order we observe.
//!
//! # The received name case cannot be recovered (2026-11-19)
//!
//! Python keeps the spelling that arrived on the wire; `http::HeaderMap` does
//! not. `HeaderName` is lower-case by construction, and there is no supported
//! way to read the original bytes through the client stack D9 pins:
//!
//! * `reqwest` 0.13.5's `ClientBuilder` exposes `http1_title_case_headers`
//!   (which affects *requests*), `http1_allow_obsolete_multiline_headers_in_responses`,
//!   `http1_ignore_invalid_headers_in_responses`,
//!   `http1_allow_spaces_after_header_name_in_responses` and
//!   `http1_max_headers` — no `preserve_header_case` (`src/async_impl/client.rs`).
//! * `hyper` 1.12.0's `client::conn::http1::Builder::preserve_header_case`
//!   exists but stores the spellings in `hyper::ext::HeaderCaseMap`, which is
//!   `pub(crate)`; its own documentation says "the relevant extension is
//!   still private, there is no way to interact with the original cases"
//!   (`src/ext/mod.rs:150-161`, `src/client/conn/http1.rs:475-488`).
//!
//! Recovering the case would mean parsing the HTTP/1 response bytes ourselves
//! on the pinned TLS stream — a second client stack outside D9, duplicating
//! hyper's parser for one text column. The title-case restoration is therefore
//! a recorded difference: it is observable only when a server's spelling is
//! not RFC 7230 canonical (`ETag` → `Etag`, nginx's `WWW-Authenticate` →
//! `Www-Authenticate`, Cloudflare's lower-case `cf-*`), and then only in
//! `webhook_logs.response_headers`; values, order, merge rules and everything
//! the task does with the response are unaffected.

use reqwest::header::HeaderMap;

/// Ordered, case-preserving header list with Python's merge rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderList {
    entries: Vec<(String, String)>,
}

impl HeaderList {
    /// Merge an `http::HeaderMap` the way urllib3's `HTTPHeaderDict` does.
    ///
    /// `http::HeaderMap` lowercases every name and does not keep the wire
    /// casing, so the name is restored to the canonical `Title-Case` form that
    /// every mainstream server sends (nginx, hyper, uvicorn, Caddy). Python's
    /// `str(response.headers)` carries whatever casing arrived, so a server
    /// that sends `content-type` in lower case differs in this one column.
    pub fn from_map(headers: &HeaderMap) -> Self {
        let mut list = Self::default();
        for (name, value) in headers {
            let name = canonical_name(name.as_ref());
            list.append(&name, &String::from_utf8_lossy(value.as_bytes()));
        }
        list
    }

    /// Append a value, joining duplicates with `", "` at the first position.
    pub fn append(&mut self, name: &str, value: &str) {
        let lower = name.to_ascii_lowercase();
        if let Some((_, existing)) = self
            .entries
            .iter_mut()
            .find(|(existing_name, _)| existing_name.eq_ignore_ascii_case(&lower))
        {
            existing.push_str(", ");
            existing.push_str(value);
        } else {
            self.entries.push((name.to_string(), value.to_string()));
        }
    }

    /// Case-insensitive lookup, first match wins (values are already merged).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(entry_name, _)| entry_name.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `str(headers)` in Python: a `dict` repr in insertion order.
    pub fn python_dict_repr(&self) -> String {
        let mut out = String::from("{");
        for (index, (name, value)) in self.entries.iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            out.push_str(&crate::pyurl::repr_str(name));
            out.push_str(": ");
            out.push_str(&crate::pyurl::repr_str(value));
        }
        out.push('}');
        out
    }
}

/// RFC 7230's canonical casing: capitalise each `-`-separated token.
fn canonical_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut start = true;
    for character in name.chars() {
        if character == '-' {
            out.push('-');
            start = true;
            continue;
        }
        if start {
            out.extend(character.to_uppercase());
            start = false;
        } else {
            out.extend(character.to_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_names_are_canonicalised() {
        assert_eq!(canonical_name("content-type"), "Content-Type");
        assert_eq!(canonical_name("x-plane-delivery"), "X-Plane-Delivery");
        assert_eq!(canonical_name("etag"), "Etag");
    }

    #[test]
    fn a_non_canonical_wire_spelling_is_recorded_as_title_case() {
        // nginx sends `ETag`; Python's `str(response.headers)` keeps it, this
        // list restores `Etag` (see the module documentation for why the wire
        // spelling is unreachable through reqwest 0.13.5 / hyper 1.12.0).
        assert_eq!(canonical_name(&"ETag".to_ascii_lowercase()), "Etag");
    }

    #[test]
    fn duplicates_merge_at_first_position() {
        let mut list = HeaderList::default();
        list.append("Server", "nginx");
        list.append("Content-Type", "text/html");
        list.append("server", "envoy");
        assert_eq!(
            list.python_dict_repr(),
            "{'Server': 'nginx, envoy', 'Content-Type': 'text/html'}"
        );
        assert_eq!(list.get("SERVER"), Some("nginx, envoy"));
    }

    #[test]
    fn repr_escapes_quotes_like_python() {
        let mut list = HeaderList::default();
        list.append("X-Test", "a'b");
        assert_eq!(list.python_dict_repr(), "{'X-Test': \"a'b\"}");
    }
}
