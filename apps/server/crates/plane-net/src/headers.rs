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
//! test asserts the order we observe. Keep this type until the webhook handler
//! is ported; reproducing Python's string then depends on it.

use reqwest::header::HeaderMap;

/// Ordered, case-preserving header list with Python's merge rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderList {
    entries: Vec<(String, String)>,
}

impl HeaderList {
    /// Merge an `http::HeaderMap` the way urllib3's `HTTPHeaderDict` does.
    pub fn from_map(headers: &HeaderMap) -> Self {
        let mut list = Self::default();
        for (name, value) in headers {
            list.append(name.as_ref(), &String::from_utf8_lossy(value.as_bytes()));
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

#[cfg(test)]
mod tests {
    use super::*;

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
