// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/cache.py`: DRF response caching over the django-redis client.
//!
//! ```python
//! def generate_cache_key(custom_path, auth_header=None):
//!     if auth_header:
//!         key_data = f"{custom_path}:{auth_header}"
//!     else:
//!         key_data = custom_path
//!     return key_data
//!
//! def cache_response(timeout=60 * 60, path=None, user=True):
//!     auth_header = None if request.user.is_anonymous else str(request.user.id) if user else None
//!     custom_path = path if path is not None else request.get_full_path()
//!     key = generate_cache_key(custom_path, auth_header)
//!     cached_result = cache.get(key)
//!     if cached_result is not None:
//!         return Response(cached_result["data"], status=cached_result["status"])
//!     response = view_func(instance, request, *args, **kwargs)
//!     if response.status_code == 200 and not settings.DEBUG:
//!         cache.set(key, {"data": response.data, "status": response.status_code}, timeout)
//!     return response
//!
//! def invalidate_cache_directly(path=None, url_params=False, user=True, request=None, multiple=False):
//!     if url_params and path:
//!         path_with_values = path
//!         for key, value in request.resolver_match.kwargs.items():
//!             path_with_values = path_with_values.replace(f":{key}", str(value))
//!         custom_path = path_with_values
//!     else:
//!         custom_path = path if path is not None else request.get_full_path()
//!     auth_header = None if request and request.user.is_anonymous else str(request.user.id) if user else None
//!     key = generate_cache_key(custom_path, auth_header)
//!     if multiple:
//!         cache.delete_many(keys=cache.keys(f"*{key}*"))
//!     else:
//!         cache.delete(key)
//! ```
//!
//! (`plane/utils/cache.py:16-88`.) Both decorators are ported as plain async
//! functions so a handler can call them from wherever its pipeline puts them;
//! the decorator's "before the view body" ordering is the caller's job.
//!
//! # Rendering a hit
//!
//! On a hit Django renders the **unpickled** objects: `Response(data)` goes
//! through `JSONRenderer` → `json.dumps(..., cls=rest_framework.utils.encoders.JSONEncoder)`.
//! [`PyValue`]'s `Serialize` implementation reproduces that encoder, so
//! `plane_serde::to_vec(Dialect::Drf, &cached.data)` gives the bytes the hit
//! would have produced. The five cached views all store serializer `.data`, in
//! which every primary key is a `uuid.UUID` object (`PrimaryKeyRelatedField`
//! returns `value.pk`) and every datetime is already an ISO string, so the hit
//! and the fresh path render identically: `str(uuid)` on both sides.

use crate::client::{CachedValue, Client};
use crate::error::Error;
use crate::redis::Redis;
use crate::value::PyValue;
use plane_core::py;

/// `generate_cache_key(custom_path, auth_header)`.
#[must_use]
pub fn generate_cache_key(custom_path: &str, auth_header: Option<&str>) -> String {
    match auth_header {
        Some(auth_header) if !auth_header.is_empty() => {
            let mut key = String::with_capacity(custom_path.len() + auth_header.len() + 1);
            key.push_str(custom_path);
            key.push(':');
            key.push_str(auth_header);
            key
        }
        // `if auth_header:` — an empty string and `None` both take this branch.
        _ => custom_path.to_string(),
    }
}

/// `HttpRequest.get_full_path()`: the escaped path plus `?` and the raw query
/// string when there is one (`django/http/request.py:213-231`).
#[must_use]
pub fn full_path(path: &str, query: &str) -> String {
    let mut out = py::escape_uri_path(path);
    if !query.is_empty() {
        out.push('?');
        out.push_str(&py::iri_to_uri(query));
    }
    out
}

/// The parts of `request` the cache helpers read.
#[derive(Debug, Clone, Copy)]
pub struct RequestFacts<'a> {
    /// `request.get_full_path()`.
    pub full_path: &'a str,
    /// `request.resolver_match.kwargs`, in resolution order.
    pub resolver_kwargs: &'a [(&'a str, String)],
    /// `str(request.user.id)`, or `None` for an anonymous user.
    pub user_id: Option<&'a str>,
}

impl RequestFacts<'_> {
    /// `request.user.is_anonymous`.
    #[must_use]
    pub fn is_anonymous(&self) -> bool {
        self.user_id.is_none()
    }
}

/// The `cache_response` decorator's key.
///
/// `path` is the decorator's `path=` argument (`None` means
/// `request.get_full_path()`), and `user` its `user=` flag.
#[must_use]
pub fn response_cache_key(path: Option<&str>, user: bool, request: &RequestFacts<'_>) -> String {
    let auth_header = if request.is_anonymous() {
        None
    } else if user {
        request.user_id
    } else {
        None
    };
    let custom_path = path.unwrap_or(request.full_path);
    generate_cache_key(custom_path, auth_header)
}

/// The `invalidate_cache` decorator's arguments.
#[derive(Debug, Clone, Copy)]
pub struct Invalidation<'a> {
    /// `path=`.
    pub path: Option<&'a str>,
    /// `url_params=`: substitute `:name` placeholders from the resolver kwargs.
    pub url_params: bool,
    /// `user=`: append `str(request.user.id)` for an authenticated caller.
    pub user: bool,
    /// `multiple=`: delete every key matching `*<key>*` instead of just `<key>`.
    pub multiple: bool,
}

impl Default for Invalidation<'_> {
    fn default() -> Self {
        Self {
            path: None,
            url_params: false,
            user: true,
            multiple: false,
        }
    }
}

/// The cache key `invalidate_cache_directly` computes.
///
/// This is expressed separately from [`invalidate`] so the differential
/// harness can compare keys without touching Redis.
#[must_use]
pub fn invalidation_key(spec: &Invalidation<'_>, request: &RequestFacts<'_>) -> String {
    let custom_path = if spec.url_params && spec.path.is_some() {
        // `str.replace(":name", value)` replaces every occurrence, and the
        // loop runs over `resolver_match.kwargs` in resolution order.
        let mut path = spec.path.unwrap_or_default().to_string();
        for (name, value) in request.resolver_kwargs {
            path = path.replace(&format!(":{name}"), value);
        }
        path
    } else {
        spec.path.unwrap_or(request.full_path).to_string()
    };
    // `None if request and request.user.is_anonymous else str(request.user.id) if user else None`
    let auth_header = if request.is_anonymous() {
        None
    } else if spec.user {
        request.user_id
    } else {
        None
    };
    generate_cache_key(&custom_path, auth_header)
}

/// A cached `{"data": …, "status": …}` payload.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedResponse {
    /// `cached_result["data"]`.
    pub data: PyValue,
    /// `cached_result["status"]`.
    pub status: u16,
}

impl CachedResponse {
    /// Read `{"data": …, "status": …}` out of a decoded cache value.
    ///
    /// Django indexes the dict directly, so a missing key is a `KeyError` and
    /// a 500; a non-`dict` value raises `TypeError`. Both become
    /// [`Error::Shape`].
    pub fn from_value(value: CachedValue) -> Result<Self, Error> {
        let value = value.into_value();
        let PyValue::Dict(_) = &value else {
            return Err(Error::Shape(format!(
                "cached value is a {}, not a dict",
                value.type_name()
            )));
        };
        let data = value
            .get("data")
            .ok_or_else(|| Error::Shape("cached value has no 'data' key".to_string()))?;
        let status = value
            .get("status")
            .ok_or_else(|| Error::Shape("cached value has no 'status' key".to_string()))?;
        let status = status
            .as_i64()
            .and_then(|status| u16::try_from(status).ok())
            .ok_or_else(|| Error::Shape("cached 'status' is not a status code".to_string()))?;
        Ok(Self {
            // `cache.get` returns the unpickled object, in which DRF's
            // `ReturnList`/`ReturnDict` wrappers have become plain
            // `list`/`dict` (their `__reduce__` rebuilds the builtin). The
            // wrappers only exist on the way *in*, to reproduce the bytes
            // Django writes.
            data: data.decayed(),
            status,
        })
    }

    /// The `{"data": …, "status": …}` tree `cache.set` stores.
    ///
    /// `data` is written exactly as given: a handler that wants Django's bytes
    /// passes [`crate::PyValue::return_list`]/[`crate::PyValue::return_dict`]
    /// (a `many=True`/single-object `serializer.data`), and a handler that
    /// only needs a readable value passes the plain tree.
    #[must_use]
    pub fn to_value(&self) -> PyValue {
        PyValue::dict(vec![
            ("data", self.data.clone()),
            ("status", PyValue::int(i64::from(self.status))),
        ])
    }
}

/// `cache.get(key)`, then `cached_result["data"]`/`["status"]`.
pub async fn lookup(redis: &Redis, key: &str) -> Result<Option<CachedResponse>, Error> {
    let Some(value) = Client::new(redis).get(key).await? else {
        return Ok(None);
    };
    CachedResponse::from_value(value).map(Some)
}

/// The store half of `cache_response`: `if response.status_code == 200 and not
/// settings.DEBUG`.
///
/// Returns whether anything was written.
pub async fn store(
    redis: &Redis,
    key: &str,
    data: &PyValue,
    status: u16,
    timeout: i64,
    debug: bool,
) -> Result<bool, Error> {
    if status != 200 || debug {
        return Ok(false);
    }
    let value = CachedResponse {
        data: data.clone(),
        status,
    };
    Client::new(redis)
        .set(key, &value.to_value(), Some(timeout))
        .await
}

/// `invalidate_cache_directly`, returning how many keys were removed.
///
/// With `multiple`, the deletion is `delete_many(keys(f"*{key}*"))`: every key
/// whose name contains `<key>` anywhere, not just the ones for this path. With
/// no match, `keys()` returns an empty list and `delete_many([])` returns 0
/// **without issuing a Redis command** (`django_redis/client/default.py:527-548`).
pub async fn invalidate(
    redis: &Redis,
    spec: &Invalidation<'_>,
    request: &RequestFacts<'_>,
) -> Result<usize, Error> {
    let key = invalidation_key(spec, request);
    let client = Client::new(redis);
    if spec.multiple {
        let keys = client.keys(&format!("*{key}*")).await?;
        client.delete_many(&keys).await
    } else {
        client.delete(&key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(kwargs: &'a [(&'a str, String)], user: Option<&'a str>) -> RequestFacts<'a> {
        RequestFacts {
            full_path: "/api/workspaces/acme/labels/?limit=10",
            resolver_kwargs: kwargs,
            user_id: user,
        }
    }

    #[test]
    fn keys_follow_generate_cache_key() {
        assert_eq!(
            generate_cache_key("/api/instances/", None),
            "/api/instances/"
        );
        assert_eq!(
            generate_cache_key("/api/users/me/", Some("8f1c0d3e-4b5a-6978")),
            "/api/users/me/:8f1c0d3e-4b5a-6978"
        );
        // `if auth_header:` treats "" as absent.
        assert_eq!(generate_cache_key("/x/", Some("")), "/x/");
    }

    #[test]
    fn full_path_escapes_like_django() {
        assert_eq!(full_path("/api/instances/", ""), "/api/instances/");
        assert_eq!(
            full_path("/api/workspaces/acme/labels/", "limit=10&cursor=20"),
            "/api/workspaces/acme/labels/?limit=10&cursor=20"
        );
        // The path is re-quoted, the query string is run through `iri_to_uri`.
        assert_eq!(full_path("/a b/", "q=caf\u{e9}"), "/a%20b/?q=caf%C3%A9");
        assert_eq!(full_path("/a%20b/", "x=1"), "/a%2520b/?x=1");
    }

    #[test]
    fn response_keys_split_on_the_user_flag() {
        let facts = request(&[], Some("u1"));
        assert_eq!(
            response_cache_key(None, true, &facts),
            "/api/workspaces/acme/labels/?limit=10:u1"
        );
        assert_eq!(
            response_cache_key(None, false, &facts),
            "/api/workspaces/acme/labels/?limit=10"
        );
        // Anonymous: no suffix even with `user=True`.
        let anonymous = request(&[], None);
        assert_eq!(
            response_cache_key(None, true, &anonymous),
            "/api/workspaces/acme/labels/?limit=10"
        );
        // An explicit `path=` replaces the full path.
        assert_eq!(
            response_cache_key(Some("/api/instances/"), false, &facts),
            "/api/instances/"
        );
    }

    #[test]
    fn invalidation_substitutes_url_params() {
        let kwargs = vec![
            ("slug", "acme".to_string()),
            (
                "project_id",
                "11111111-1111-1111-1111-111111111111".to_string(),
            ),
        ];
        let facts = request(&kwargs, Some("u1"));
        let spec = Invalidation {
            path: Some("/api/workspaces/:slug/labels/"),
            url_params: true,
            user: false,
            multiple: true,
        };
        assert_eq!(
            invalidation_key(&spec, &facts),
            "/api/workspaces/acme/labels/"
        );
        // A placeholder with no matching kwarg stays literal.
        let spec = Invalidation {
            path: Some("/api/workspaces/:slug/estimates/:estimate_id"),
            url_params: true,
            user: false,
            multiple: false,
        };
        assert_eq!(
            invalidation_key(&spec, &facts),
            "/api/workspaces/acme/estimates/:estimate_id"
        );
        // Without `url_params`, placeholders stay literal.
        let spec = Invalidation {
            path: Some("/api/workspaces/:slug/labels/"),
            url_params: false,
            user: false,
            multiple: false,
        };
        assert_eq!(
            invalidation_key(&spec, &facts),
            "/api/workspaces/:slug/labels/"
        );
        // `user=True` appends the caller's id.
        let spec = Invalidation {
            path: Some("/api/users/me/settings/"),
            url_params: false,
            user: true,
            multiple: false,
        };
        assert_eq!(
            invalidation_key(&spec, &facts),
            "/api/users/me/settings/:u1"
        );
    }

    #[test]
    fn cached_shape_round_trips() {
        let response = CachedResponse {
            data: PyValue::list(vec![PyValue::string("x")]),
            status: 200,
        };
        let value = response.to_value();
        assert_eq!(
            value,
            PyValue::dict(vec![
                ("data", PyValue::list(vec![PyValue::string("x")])),
                ("status", PyValue::int(200)),
            ])
        );
        let back = CachedResponse::from_value(CachedValue::Pickled(value)).unwrap();
        assert_eq!(back, response);
        // A wrong shape is an error, not a silent miss.
        assert!(CachedResponse::from_value(CachedValue::Pickled(PyValue::int(1))).is_err());
        assert!(
            CachedResponse::from_value(CachedValue::Pickled(PyValue::dict(vec![(
                "data",
                PyValue::none()
            )])))
            .is_err()
        );
    }
}
