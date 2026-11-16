// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's page cache (`cache_page` / `CacheMiddleware`), in its own key
//! namespace.
//!
//! Exactly one Plane view uses it: `TimezoneEndpoint.get`
//! (`plane/app/views/timezone/base.py:23`, `@method_decorator(cache_page(60 *
//! 60 * 2))`), which is delegated to `CacheMiddleware` with
//! `page_timeout=7200`
//! (`django/views/decorators/cache.py:cache_page`).
//!
//! # D5a: same behaviour, separate namespace and format
//!
//! During coexistence the endpoint must behave identically whichever backend
//! answers it, but the two must not read each other's cache entries: Django
//! stores a **pickled `rest_framework.response.Response` object**
//! (`UpdateCacheMiddleware.process_response` → `cache.set(cache_key, r,
//! timeout)`), which only Django can rebuild, and a Rust-written value under a
//! Django key would make a later Django request fail with an unpickling error.
//! So Rust writes `:1:plane_rs.cache_page.<method>.<md5(uri)>.<md5(headers)>.
//! <language>.<timezone>` in a private binary format (see [`encode`]) and
//! never touches `views.decorators.cache.*`. What it does reproduce exactly is
//! everything the client can observe:
//!
//! * the page is stored for the decorator's timeout (7200 s), keyed by the
//!   request URI and the response's `Vary` headers, and served *unchanged*
//!   until it expires;
//! * on a miss the response carries `Expires` (RFC 5322, added only when the
//!   view did not set one) and `Cache-Control: max-age=7200`
//!   (`django/utils/cache.py:patch_response_headers`);
//! * on a hit it carries the stored headers plus `Age`, computed as
//!   `max(0, max_age - (expires - now))` — Django's formula in
//!   `FetchFromCacheMiddleware.process_request` — which for a
//!   `patch_response_headers` `Expires` is the number of whole seconds since
//!   the entry was stored;
//! * only `GET`/`HEAD` are served from it, only `200` responses are stored,
//!   and `private`/`no-cache`/`no-store`/`Vary: *`/a cookie with `Vary: Cookie`
//!   suppress the store.
//!
//! The harness rule that goes with this is in `plane-testkit`
//! (`normalize::RedisRule::PageCache`): Django's `cache_header..`/`cache_page..`
//! keys on one side are matched to `plane_rs.cache_page.*` keys on the other by
//! presence and TTL bucket only.
//!
//! # Intended call
//!
//! ```no_run
//! # use plane_cache::page::{self, PageResponse};
//! # use plane_cache::Redis;
//! # async fn handler(redis: &Redis, uri: &str, now: i64) -> Result<(), Box<dyn std::error::Error>> {
//! let key = page::page_key("GET", uri, &[], "en-us", "UTC");
//! if let Some(hit) = page::lookup(redis, &key).await? {
//!     // Serve `hit.headers` + `Age` + `hit.body` unchanged.
//!     let _ = (hit.status, hit.age(now), hit.body);
//!     return Ok(());
//! }
//! let mut response = PageResponse { status: 200, headers: Vec::new(), body: Vec::new() };
//! // ... render the view's response ...
//! page::patch_response_headers(&mut response, 60 * 60 * 2, now);
//! if page::is_cacheable(&response, false) {
//!     page::store(redis, &key, &response, 60 * 60 * 2, now).await?;
//! }
//! # Ok(())
//! # }
//! ```

use md5::{Digest, Md5};

use crate::client::{self, make_key};
use crate::error::Error;
use crate::redis::Redis;

/// The prefix of every Rust page-cache key, before django-redis adds `:1:`.
///
/// Deliberately different from Django's `views.decorators.cache.` so that a
/// Django process never unpickles a Rust value (D5a).
pub const KEY_PREFIX: &str = "plane_rs.cache_page.";

/// The magic bytes at the start of every stored page, then a format version.
const MAGIC: &[u8; 4] = b"PRS1";
/// The value format's version. Bumped when [`encode`]'s layout changes.
const FORMAT: u8 = 1;

/// The parts of a response the page cache stores.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageResponse {
    /// HTTP status.
    pub status: u16,
    /// Headers in the order they would be sent.
    pub headers: Vec<(String, String)>,
    /// The rendered body.
    pub body: Vec<u8>,
}

impl PageResponse {
    /// The value of a header, matched case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// Set a header, replacing an existing one case-insensitively.
    pub fn set_header(&mut self, name: &str, value: String) {
        if let Some(entry) = self
            .headers
            .iter_mut()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
        {
            entry.1 = value;
            return;
        }
        self.headers.push((name.to_string(), value));
    }
}

/// A page read back from the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedPage {
    /// The stored status.
    pub status: u16,
    /// The stored headers, in the order they were written.
    pub headers: Vec<(String, String)>,
    /// The stored body, served unchanged until expiry.
    pub body: Vec<u8>,
    /// `int(time.time())` when the entry was written.
    pub stored_at: i64,
    /// The `max-age` the entry was written with.
    pub max_age: i64,
}

impl CachedPage {
    /// The value of a header, matched case-insensitively.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// `FetchFromCacheMiddleware.process_request`'s `Age`:
    ///
    /// ```python
    /// if get_max_age(response) is not None and parse_http_date_safe(response["Expires"]) is not None:
    ///     remaining_seconds = expires_timestamp - int(time.time())
    ///     response["Age"] = max(0, max_age_seconds - remaining_seconds)
    /// ```
    ///
    /// With the `Expires` `patch_response_headers` wrote
    /// (`http_date(stored_at + max_age)`, whole seconds) this is exactly
    /// `now - stored_at`, clamped at zero for a clock that went backwards.
    /// Django sets no `Age` when either header is missing or unparsable; this
    /// returns `None` in that case.
    #[must_use]
    pub fn age(&self, now: i64) -> Option<u64> {
        self.header("Expires")?;
        self.header("Cache-Control")?;
        Some(u64::try_from(now - self.stored_at).unwrap_or(0))
    }
}

/// The page-cache key for one request.
///
/// Mirrors Django's `_generate_cache_key` shape —
/// `views.decorators.cache.cache_page.<prefix>.<method>.<md5(uri)>.<md5(headers)>.<language>.<timezone>`
/// — under [`KEY_PREFIX`], so a reader can tell the two apart at a glance and
/// the length stays bounded.
///
/// `vary_values` is the concatenation of the request's values for the headers
/// the response's `Vary` lists (in the order `learn_cache_key` stored them,
/// sorted, with `-` replaced by `_` — see [`vary_header_names`]);
/// `language` is `request.LANGUAGE_CODE` (Django's `_i18n_cache_key_suffix`)
/// and `timezone` the active timezone name.
#[must_use]
pub fn page_key(
    method: &str,
    absolute_uri: &str,
    vary_values: &[&str],
    language: &str,
    timezone: &str,
) -> String {
    let uri = hex(&Md5::digest(absolute_uri.as_bytes()));
    let mut headers = Md5::new();
    for value in vary_values {
        headers.update(value.as_bytes());
    }
    let headers = hex(&headers.finalize());
    format!("{KEY_PREFIX}{method}.{uri}.{headers}.{language}.{timezone}")
}

/// The header names a response's `Vary` contributes to the key, in Django's
/// form: upper-cased, `-` → `_`, prefixed `HTTP_`, with `Accept-Language`
/// dropped while i18n is on, sorted.
#[must_use]
pub fn vary_header_names(vary: &str, use_i18n: bool) -> Vec<String> {
    let mut names: Vec<String> = vary
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| format!("HTTP_{}", name.to_uppercase().replace('-', "_")))
        .filter(|name| !(use_i18n && name == "HTTP_ACCEPT_LANGUAGE"))
        .collect();
    names.sort();
    names
}

/// `django.utils.http.http_date`: RFC 5322 in UTC with a literal `GMT`.
///
/// The same function `plane_auth::cookies::http_date` implements for cookie
/// expiry; duplicated rather than making this crate depend on `plane-auth`.
#[must_use]
pub fn http_date(epoch_seconds: i64) -> String {
    use chrono::{DateTime, Utc};
    let Some(moment) = DateTime::<Utc>::from_timestamp(epoch_seconds, 0) else {
        return String::new();
    };
    moment.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// `django.utils.cache.patch_response_headers`: add `Expires` (only when the
/// view did not set one) and `Cache-Control: max-age=<timeout>`.
///
/// A negative timeout is clamped to zero; an existing `max-age` keeps the
/// smaller of the two, as `patch_cache_control` does.
pub fn patch_response_headers(response: &mut PageResponse, timeout_seconds: i64, now: i64) {
    let timeout = timeout_seconds.max(0);
    if response.header("Expires").is_none() {
        response.set_header("Expires", http_date(now + timeout));
    }
    let existing = response.header("Cache-Control").map(str::to_string);
    response.set_header(
        "Cache-Control",
        cache_control_with_max_age(existing.as_deref(), timeout),
    );
}

/// `patch_cache_control(response, max_age=timeout)` for the directives Plane's
/// one page-cached view can produce (it sets no `Cache-Control` of its own).
fn cache_control_with_max_age(existing: Option<&str>, timeout: i64) -> String {
    let Some(existing) = existing else {
        return format!("max-age={timeout}");
    };
    let mut directives: Vec<String> = Vec::new();
    let mut seen_max_age = false;
    for directive in existing.split(',') {
        let directive = directive.trim();
        if directive.is_empty() {
            continue;
        }
        if let Some(value) = directive.strip_prefix("max-age=") {
            seen_max_age = true;
            // `patch_cache_control` keeps the minimum of the two ages.
            let value = value.trim().parse::<i64>().unwrap_or(timeout);
            directives.push(format!("max-age={}", value.min(timeout)));
        } else {
            directives.push(directive.to_string());
        }
    }
    if !seen_max_age {
        directives.push(format!("max-age={timeout}"));
    }
    directives.join(", ")
}

/// Whether `UpdateCacheMiddleware` would store this response.
///
/// Mirrors `process_response`'s guards: a streaming response, a status outside
/// `(200, 304)`, a cookie together with `Vary: Cookie`, a `Cache-Control` with
/// `private`/`no-cache`/`no-store`, or `Vary: *` all suppress the store.
/// `store` additionally requires `200`, because Django only writes when
/// `response.status_code == 200`.
#[must_use]
pub fn is_cacheable(response: &PageResponse, sets_cookies: bool) -> bool {
    if !matches!(response.status, 200 | 304) {
        return false;
    }
    let vary = response.header("Vary").unwrap_or_default();
    if sets_cookies && header_listed(vary, "cookie") {
        return false;
    }
    let cache_control = response
        .header("Cache-Control")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !cache_control.is_empty()
        && ["private", "no-cache", "no-store"]
            .iter()
            .any(|directive| cache_control.contains(directive))
    {
        return false;
    }
    !header_listed(vary, "*")
}

/// `has_vary_header`: whether `Vary` lists `header`.
fn header_listed(vary: &str, header: &str) -> bool {
    vary.split(',')
        .any(|name| name.trim().eq_ignore_ascii_case(header))
}

/// The stored bytes for one page.
///
/// Deliberately not a pickle: Django must never be able to unpickle a
/// Rust-written value (D5a). Layout:
///
/// ```text
/// "PRS1" | u8 format | u16 status | i64 stored_at | i64 max_age
///        | u32 header count | (u32 name length, name, u32 value length, value)*
///        | body
/// ```
#[must_use]
pub fn encode(page: &CachedPage) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + page.body.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT);
    out.extend_from_slice(&page.status.to_le_bytes());
    out.extend_from_slice(&page.stored_at.to_le_bytes());
    out.extend_from_slice(&page.max_age.to_le_bytes());
    out.extend_from_slice(&(page.headers.len() as u32).to_le_bytes());
    for (name, value) in &page.headers {
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    out.extend_from_slice(&page.body);
    out
}

/// The inverse of [`encode`].
pub fn decode(bytes: &[u8]) -> Result<CachedPage, Error> {
    let mut reader = Reader { bytes, position: 0 };
    if reader.take(4, "magic")? != MAGIC {
        return Err(Error::Decode(
            "page cache value is not a plane_rs entry".to_string(),
        ));
    }
    let format = reader.byte("format")?;
    if format != FORMAT {
        return Err(Error::Decode(format!(
            "page cache format {format} is not {FORMAT}"
        )));
    }
    let status = u16::from_le_bytes(reader.array::<2>("status")?);
    let stored_at = i64::from_le_bytes(reader.array::<8>("stored_at")?);
    let max_age = i64::from_le_bytes(reader.array::<8>("max_age")?);
    let count = u32::from_le_bytes(reader.array::<4>("header count")?) as usize;
    let mut headers = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        let name_length = u32::from_le_bytes(reader.array::<4>("name length")?) as usize;
        let name = std::str::from_utf8(reader.take(name_length, "name")?)
            .map_err(|_| Error::Decode("header name is not UTF-8".to_string()))?
            .to_string();
        let value_length = u32::from_le_bytes(reader.array::<4>("value length")?) as usize;
        let value = std::str::from_utf8(reader.take(value_length, "value")?)
            .map_err(|_| Error::Decode("header value is not UTF-8".to_string()))?
            .to_string();
        headers.push((name, value));
    }
    let body = reader.take(reader.remaining(), "body")?.to_vec();
    Ok(CachedPage {
        status,
        headers,
        body,
        stored_at,
        max_age,
    })
}

/// `cache.get(cache_key)` for a page key.
///
/// A miss is `None`; a value that is not a page (or is corrupt) is an error,
/// which the request pipeline renders as a 500 — the same outcome as Django
/// raising out of `pickle.loads`.
pub async fn lookup(redis: &Redis, key: &str) -> Result<Option<CachedPage>, Error> {
    let Some(bytes) = redis.get_bytes(&make_key(key)).await? else {
        return Ok(None);
    };
    decode(&bytes).map(Some)
}

/// `cache.set(cache_key, response, timeout)`.
///
/// The timeout is `cache_page`'s `page_timeout` (7200 for the timezones view);
/// a non-positive timeout deletes the key, as django-redis' `set` does.
/// Returns whether anything was written.
pub async fn store(
    redis: &Redis,
    key: &str,
    response: &PageResponse,
    timeout_seconds: i64,
    now: i64,
) -> Result<bool, Error> {
    if response.status != 200 {
        return Ok(false);
    }
    let page = CachedPage {
        status: response.status,
        headers: response.headers.clone(),
        body: response.body.clone(),
        stored_at: now,
        max_age: timeout_seconds.max(0),
    };
    let full = make_key(key);
    let bytes = encode(&page);
    let milliseconds = timeout_seconds.saturating_mul(1000);
    if milliseconds <= 0 {
        redis.del(&full).await?;
        return Ok(true);
    }
    redis.set_bytes_px(&full, &bytes, milliseconds).await?;
    Ok(true)
}

/// The key django-redis writes for a page, for tests and the harness.
#[must_use]
pub fn full_key(key: &str) -> String {
    make_key(key)
}

/// `md5(...).hexdigest()`.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Reader<'_> {
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, count: usize, what: &'static str) -> Result<&[u8], Error> {
        let end = self.position.saturating_add(count);
        let Some(slice) = self.bytes.get(self.position..end) else {
            return Err(Error::Decode(format!(
                "page cache value truncated at {what}"
            )));
        };
        self.position = end;
        Ok(slice)
    }

    fn byte(&mut self, what: &'static str) -> Result<u8, Error> {
        Ok(self.take(1, what)?[0])
    }

    fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N], Error> {
        let mut out = [0_u8; N];
        out.copy_from_slice(self.take(N, what)?);
        Ok(out)
    }
}

/// The throttle-style `:1:` prefix the page cache shares with the rest of the
/// django-redis client, for the harness rule.
pub const VERSION: i64 = client::VERSION;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_namespaced_and_shaped_like_djangos() {
        let key = page_key(
            "GET",
            "http://localhost:8000/api/timezones/",
            &[],
            "en-us",
            "UTC",
        );
        assert!(key.starts_with("plane_rs.cache_page.GET."), "{key}");
        assert!(key.ends_with(".en-us.UTC"), "{key}");
        // Two md5 digests between the dots.
        let parts: Vec<&str> = key.split('.').collect();
        assert_eq!(parts.len(), 7, "{key}");
        assert_eq!(parts[3].len(), 32);
        assert_eq!(parts[4], "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            full_key(&key),
            format!(":1:{key}"),
            "django-redis adds the version prefix"
        );
        // The signature django-redis writes must not collide with Django's.
        assert!(!key.starts_with("views.decorators.cache."));
    }

    #[test]
    fn vary_names_match_learn_cache_key() {
        assert_eq!(
            vary_header_names("Accept, Accept-Language, Cookie", true),
            vec!["HTTP_ACCEPT".to_string(), "HTTP_COOKIE".to_string()]
        );
        assert_eq!(
            vary_header_names("accept-language", false),
            vec!["HTTP_ACCEPT_LANGUAGE".to_string()]
        );
        assert_eq!(vary_header_names("", true), Vec::<String>::new());
        // A different vary list changes the key.
        let one = page_key("GET", "http://x/", &["a"], "en-us", "UTC");
        let two = page_key("GET", "http://x/", &["b"], "en-us", "UTC");
        assert_ne!(one, two);
        let same = page_key("GET", "http://x/", &["a"], "en-us", "UTC");
        assert_eq!(one, same);
    }

    #[test]
    fn expires_is_rfc_5322_gmt() {
        // Python: `django.utils.http.http_date(1789000000)`.
        assert_eq!(http_date(1_789_000_000), "Thu, 10 Sep 2026 00:26:40 GMT");
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
    }

    #[test]
    fn patched_headers_match_patch_response_headers() {
        let mut response = PageResponse {
            status: 200,
            headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            body: b"[]".to_vec(),
        };
        patch_response_headers(&mut response, 7200, 1_789_000_000);
        assert_eq!(
            response.header("Expires"),
            Some("Thu, 10 Sep 2026 02:26:40 GMT")
        );
        assert_eq!(response.header("Cache-Control"), Some("max-age=7200"));
        // A view-set `Expires` is kept.
        let mut custom = PageResponse {
            status: 200,
            headers: vec![(
                "Expires".to_string(),
                "Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
            )],
            body: Vec::new(),
        };
        patch_response_headers(&mut custom, 7200, 1_789_000_000);
        assert_eq!(
            custom.header("Expires"),
            Some("Thu, 01 Jan 1970 00:00:00 GMT")
        );
        // An existing max-age keeps the smaller value.
        let mut small = PageResponse {
            status: 200,
            headers: vec![("Cache-Control".to_string(), "max-age=60".to_string())],
            body: Vec::new(),
        };
        patch_response_headers(&mut small, 7200, 0);
        assert_eq!(small.header("Cache-Control"), Some("max-age=60"));
        // A negative timeout is clamped.
        let mut negative = PageResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        };
        patch_response_headers(&mut negative, -5, 0);
        assert_eq!(negative.header("Cache-Control"), Some("max-age=0"));
    }

    #[test]
    fn age_is_seconds_since_the_store() {
        let page = CachedPage {
            status: 200,
            headers: vec![
                ("Expires".to_string(), http_date(1_789_000_000 + 7200)),
                ("Cache-Control".to_string(), "max-age=7200".to_string()),
            ],
            body: Vec::new(),
            stored_at: 1_789_000_000,
            max_age: 7200,
        };
        assert_eq!(page.age(1_789_000_000), Some(0));
        assert_eq!(page.age(1_789_000_001), Some(1));
        assert_eq!(page.age(1_789_001_800), Some(1800));
        // A clock that went backwards clamps at zero, like `max(0, …)`.
        assert_eq!(page.age(1_788_999_990), Some(0));
        // No `Expires` or no `Cache-Control` means no `Age`, as in Django.
        let mut bare = page.clone();
        bare.headers.retain(|(name, _)| name != "Expires");
        assert_eq!(bare.age(1_789_000_100), None);
        let mut no_cc = page.clone();
        no_cc.headers.retain(|(name, _)| name != "Cache-Control");
        assert_eq!(no_cc.age(1_789_000_100), None);
    }

    #[test]
    fn cacheable_matches_the_middleware_guards() {
        let ok = PageResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert!(is_cacheable(&ok, false));
        assert!(is_cacheable(&ok, true));
        // 304 is patched but not stored; `store` rejects it.
        let mut not_modified = ok.clone();
        not_modified.status = 304;
        assert!(is_cacheable(&not_modified, false));
        for status in [201, 204, 400, 404, 500] {
            let mut response = ok.clone();
            response.status = status;
            assert!(!is_cacheable(&response, false), "status {status}");
        }
        let mut cookie = ok.clone();
        cookie.set_header("Vary", "Cookie".to_string());
        assert!(!is_cacheable(&cookie, true));
        assert!(is_cacheable(&cookie, false));
        for directive in ["private", "no-cache", "no-store"] {
            let mut response = ok.clone();
            response.set_header("Cache-Control", directive.to_string());
            assert!(!is_cacheable(&response, false), "{directive}");
        }
        let mut star = ok.clone();
        star.set_header("Vary", "*".to_string());
        assert!(!is_cacheable(&star, false));
    }

    #[test]
    fn values_round_trip_and_are_not_pickles() {
        let page = CachedPage {
            status: 200,
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Cache-Control".to_string(), "max-age=7200".to_string()),
            ],
            body: b"[[\"Midway Island\",\"Pacific/Midway\"]]".to_vec(),
            stored_at: 1_789_000_000,
            max_age: 7200,
        };
        let bytes = encode(&page);
        assert_eq!(&bytes[..4], b"PRS1");
        assert_ne!(bytes[0], 0x80, "a pickle would start with PROTO");
        assert_eq!(decode(&bytes).expect("decodes"), page);
        // Truncation and foreign values are errors, not silent misses.
        assert!(decode(&bytes[..10]).is_err());
        assert!(decode(b"\x80\x04}\x94.").is_err());
    }
}
