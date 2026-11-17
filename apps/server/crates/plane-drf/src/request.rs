// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The request object DRF builds, and Django's `QueryDict`.
//!
//! DRF's `Request` wraps Django's `HttpRequest` (`rest_framework/request.py`).
//! The state a view can observe is:
//!
//! | DRF | Here |
//! | --- | --- |
//! | `request.method`, `request.path`, `request.headers` | [`RequestContext`] |
//! | `request.query_params` | [`ViewRequest::query`] (a [`QueryDict`]) |
//! | `view.kwargs` | [`ViewRequest::kwargs`] |
//! | `request.data` (lazy parsers) | [`ViewRequest::data`] |
//! | `request.accepted_renderer` | negotiated in [`crate::negotiate`] |
//! | `view.fields` / `view.expand` | [`ViewRequest::fields`] / [`ViewRequest::expand`] |

use std::collections::BTreeMap;
use std::sync::OnceLock;

use http::Method;
use plane_core::context::RequestContext;
use plane_core::py;

use crate::error::ViewError;
use crate::parsers::{self, Data};
use crate::policy::{MethodPolicy, RoutePolicy};

/// Django's `DATA_UPLOAD_MAX_NUMBER_FIELDS` default
/// (`django/conf/global_settings.py`). Plane does not override it, and it is not
/// an environment variable in this tree (`ENV.md` has no entry), so it is a
/// constant rather than a setting: adding the setting would be a separate
/// change and must not silently move the limit.
pub const DATA_UPLOAD_MAX_NUMBER_FIELDS: usize = 1000;

/// Django's `DEFAULT_CHARSET` (`django/conf/global_settings.py`).
pub const DEFAULT_CHARSET: &str = "utf-8";

/// `django.http.QueryDict`'s parsing failure: `TooManyFieldsSent`, a
/// `SuspiciousOperation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyFields;

/// A `QueryDict`: ordered key/value pairs where a key may repeat.
///
/// `__getitem__` and `get()` return the **last** value (`QueryDict.__getitem__`
/// delegates to `MultiValueDict.__getitem__`, which is `list[-1]`);
/// `getlist()` returns every value in insertion order. Both keys and values are
/// `unquote_plus`-decoded. `keep_blank_values=True` is what Django always
/// passes, so `a=` is present with an empty value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryDict {
    pairs: Vec<(String, String)>,
    /// `QueryDict._mutable`, as the object DRF hands a view as `request.data`
    /// sees it. A urlencoded body's `FormParser` result is immutable, and so is
    /// a multipart body's `MultiPartParser` result (`_parse` clears `_mutable`
    /// before returning, `django/http/multipartparser.py:364-365`) — but DRF
    /// copies it when `request.FILES` is non-empty
    /// (`rest_framework/request.py:289-291`) and `QueryDict.copy()` returns a
    /// **mutable** copy, so a multipart body with at least one file part reaches
    /// the view mutable. A handler that reproduces a serializer's in-place write
    /// (`data["url"] = …`) needs the difference: the write is
    /// `AttributeError: This QueryDict instance is immutable` on a urlencoded
    /// body or a file-less multipart one, and succeeds with a file part.
    mutable: bool,
    /// Whether a multipart body carried at least one file part. Django keeps
    /// the parsed files in `request.FILES` and merges them into `request.data`
    /// (`Request._load_data_and_files`), so `request.data` is a `QueryDict`
    /// whose file entries are `[<InMemoryUploadedFile>]` — not JSON
    /// serialisable. This port skips file parts (see `parsers.rs`); the flag
    /// is what lets a handler reproduce the encoder failure.
    files: bool,
    /// The file parts' field names, in body order. `request.data` carries those
    /// keys too (`_full_data.update(self._files)`), with the file object in
    /// place of the field's values.
    file_keys: Vec<String>,
}

impl QueryDict {
    /// `QueryDict(query_string)` (`django/http/request.py:580-608`).
    ///
    /// `max_num_fields` reproduces CPython's `urllib.parse.parse_qsl` check:
    /// it counts separators (`1 + qs.count('&')`, or 0 for an empty string) and
    /// raises when `max < count`.
    pub fn parse(query: &str, max_num_fields: Option<usize>) -> Result<Self, TooManyFields> {
        if let Some(max) = max_num_fields {
            let fields = if query.is_empty() {
                0
            } else {
                1 + query.matches('&').count()
            };
            if max < fields {
                return Err(TooManyFields);
            }
        }
        let mut pairs = Vec::new();
        for part in query.split('&') {
            if part.is_empty() {
                continue;
            }
            let (key, value) = match part.split_once('=') {
                Some((key, value)) => (key, value),
                None => (part, ""),
            };
            pairs.push((unquote_plus(key), unquote_plus(value)));
        }
        Ok(Self {
            pairs,
            mutable: false,
            files: false,
            file_keys: Vec::new(),
        })
    }

    /// Append one pair (the multipart parser's equivalent of
    /// `QueryDict.appendlist`).
    pub fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.pairs.push((key.into(), value.into()));
    }

    /// `QueryDict.__setitem__` on a **mutable** dict: every value for `key` is
    /// replaced by the single `value`, at the key's original position (or
    /// appended when it was absent). Python's underlying `dict` keeps an
    /// existing key's insertion position when its value is replaced.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();
        match self.pairs.iter().position(|(name, _)| *name == key) {
            Some(index) => {
                self.pairs.retain(|(name, _)| *name != key);
                self.pairs.insert(index, (key, value));
            }
            None => self.pairs.push((key, value)),
        }
    }

    /// Whether this `request.data` `QueryDict` accepts `__setitem__`:
    /// `QueryDict._mutable`.
    #[must_use]
    pub fn is_mutable(&self) -> bool {
        self.mutable
    }

    /// Whether the multipart body had a file part
    /// (`request.FILES` non-empty, merged into `request.data` by DRF).
    #[must_use]
    pub fn has_files(&self) -> bool {
        self.files
    }

    /// Record a multipart file part and its field name.
    ///
    /// With `request.FILES` non-empty, DRF's `_load_data_and_files` replaces
    /// `request.data` with a mutable copy (`rest_framework/request.py:289-291`;
    /// Django's `QueryDict.copy()` returns a mutable dict), which is what lets a
    /// serializer rewrite the body in place.
    pub fn mark_file(&mut self, name: Option<&str>) {
        self.files = true;
        self.mutable = true;
        if let Some(name) = name {
            self.file_keys.push(name.to_owned());
        }
    }

    /// Whether `key` was a file part (`key in request.FILES`).
    #[must_use]
    pub fn is_file(&self, key: &str) -> bool {
        self.file_keys.iter().any(|name| name == key)
    }

    /// The file parts' field names, in body order.
    #[must_use]
    pub fn file_keys(&self) -> &[String] {
        &self.file_keys
    }

    /// `QueryDict.get(key)` / `__getitem__`: the last value for `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// `QueryDict.get(key, default)` with a default.
    pub fn get_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key).unwrap_or(default)
    }

    /// `QueryDict.getlist(key)`.
    pub fn getlist(&self, key: &str) -> Vec<&str> {
        self.pairs
            .iter()
            .filter(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.pairs.iter().any(|(name, _)| name == key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.pairs.iter().map(|(key, _)| key.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// `QueryDict.dict()`: the last value per key.
    pub fn dict(&self) -> BTreeMap<&str, &str> {
        let mut map = BTreeMap::new();
        for (key, value) in &self.pairs {
            map.insert(key.as_str(), value.as_str());
        }
        map
    }
}

/// Python's `unquote_plus`: `+` becomes a space, then percent-decoding with
/// `errors="replace"`; an invalid escape (`%zz`) is left as written.
fn unquote_plus(value: &str) -> String {
    if value.contains('+') {
        py::unquote(&value.replace('+', " "))
    } else {
        py::unquote(value)
    }
}

/// The request as the DRF dispatch loop sees it.
pub struct ViewRequest<'a> {
    pub ctx: &'a RequestContext,
    pub route: &'a RoutePolicy,
    pub method: Method,
    /// `view.kwargs`, in URL order.
    pub kwargs: Vec<(&'a str, &'a str)>,
    /// `request.query_params`.
    pub query: QueryDict,
    /// `request.data`, parsed on first access (`_load_data_and_files`).
    data: OnceLock<Result<Data, ViewError>>,
    /// `view.format_kwarg` (`kwargs["format"]` on a format-suffix route).
    pub format_suffix: Option<&'a str>,
}

impl<'a> ViewRequest<'a> {
    pub fn new(
        ctx: &'a RequestContext,
        route: &'a RoutePolicy,
        kwargs: Vec<(&'a str, &'a str)>,
    ) -> Result<Self, TooManyFields> {
        let query = QueryDict::parse(&ctx.query_string, Some(DATA_UPLOAD_MAX_NUMBER_FIELDS))?;
        let format_suffix = kwargs
            .iter()
            .find(|(name, _)| *name == "format")
            .map(|(_, value)| *value);
        Ok(Self {
            ctx,
            route,
            method: ctx.method.clone(),
            kwargs,
            query,
            data: OnceLock::new(),
            format_suffix,
        })
    }

    /// `request.query_params.get("format")`, or the format-suffix kwarg.
    ///
    /// `DefaultContentNegotiation.select_renderer`: `format_suffix or
    /// request.query_params.get(format_query_param)`. An empty value is falsy,
    /// so it is treated as absent.
    pub fn negotiation_format(&self) -> Option<&str> {
        self.format_suffix
            .filter(|value| !value.is_empty())
            .or_else(|| self.query.get("format").filter(|value| !value.is_empty()))
    }

    /// `view.kwargs.get(name)`.
    pub fn kwarg(&self, name: &str) -> Option<&'a str> {
        self.kwargs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
    }

    /// `view.fields`: `[f for f in request.GET.get("fields", "").split(",") if f]`.
    pub fn fields(&self) -> Option<Vec<&str>> {
        split_commas(self.query.get_or("fields", ""))
    }

    /// `view.expand`, same shape.
    pub fn expand(&self) -> Option<Vec<&str>> {
        split_commas(self.query.get_or("expand", ""))
    }

    /// The `MethodPolicy` for this request, if the method is registered.
    pub fn method_policy(&self) -> Option<&'a MethodPolicy> {
        self.route.method_policy(&self.method)
    }

    /// `request.data`, parsed on first access by the default parsers
    /// (`rest_framework/request.py:330-384`).
    pub fn data(&self) -> Result<&Data, ViewError> {
        match self
            .data
            .get_or_init(|| parsers::parse(self.ctx, &self.route.parser_classes))
        {
            Ok(data) => Ok(data),
            Err(error) => Err(error.clone()),
        }
    }
}

fn split_commas(value: &str) -> Option<Vec<&str>> {
    let fields: Vec<&str> = value.split(',').filter(|field| !field.is_empty()).collect();
    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn querydict_last_value_wins() {
        let query = QueryDict::parse("a=1&b=2&a=3&b=", None).expect("parse");
        assert_eq!(query.get("a"), Some("3"));
        assert_eq!(query.get("b"), Some(""));
        assert_eq!(query.getlist("a"), vec!["1", "3"]);
        assert_eq!(query.getlist("b"), vec!["2", ""]);
        assert_eq!(query.get("missing"), None);
        assert_eq!(query.get_or("missing", "d"), "d");
        assert_eq!(query.len(), 4);
        assert_eq!(query.dict().get("a"), Some(&"3"));
    }

    #[test]
    fn querydict_decoding_matches_python() {
        // `+` is a space, `%2B` is a plus, invalid escapes stay literal, and a
        // bare key has an empty value.
        let query = QueryDict::parse("a=x+y&b=%2B&c=%zz&d&e=%E2%9C%93", None).expect("parse");
        assert_eq!(query.get("a"), Some("x y"));
        assert_eq!(query.get("b"), Some("+"));
        assert_eq!(query.get("c"), Some("%zz"));
        assert_eq!(query.get("d"), Some(""));
        assert_eq!(query.get("e"), Some("✓"));
        assert_eq!(
            QueryDict::parse("&&a=1&&", None)
                .expect("parse")
                .getlist("a"),
            vec!["1"]
        );
    }

    #[test]
    fn querydict_field_limit_counts_separators() {
        // `parse_qsl` counts `1 + qs.count('&')` and raises only when the
        // limit is strictly below that count.
        let at_limit = "a=1&b=2";
        assert!(QueryDict::parse(at_limit, Some(2)).is_ok());
        assert_eq!(QueryDict::parse(at_limit, Some(1)), Err(TooManyFields));
        assert!(QueryDict::parse("", Some(0)).is_ok());
        assert_eq!(QueryDict::parse("a=1", Some(0)), Err(TooManyFields));
    }

    #[test]
    fn fields_and_expand_skip_empty_items() {
        let empty = QueryDict::default();
        assert_eq!(split_commas(empty.get_or("fields", "")), None);
        assert_eq!(split_commas("id,name,"), Some(vec!["id", "name"]));
        let query = QueryDict::parse("fields=id,name&expand=assignee", None).expect("parse");
        assert_eq!(query.get("fields"), Some("id,name"));
        assert_eq!(query.get("expand"), Some("assignee"));
    }
}
