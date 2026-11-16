// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/timezones/` — the reference port of `apps/server/HANDLERS.md`.
//!
//! Python: `plane/app/views/timezone/base.py:20-210`
//! (`plane.app.views.timezone.base.TimezoneEndpoint`, a plain DRF `APIView`
//! with `AllowAny`). Registered as `plane.app.views.timezone.base.TimezoneEndpoint`
//! `GET`, which is what `routes.json` records for `/api/timezones/`; the runtime
//! takes the path, the methods, `SessionAuthentication`, `AllowAny`,
//! `AnonRateThrottle`, the `Allow` header and `OPTIONS` from that entry.
//!
//! ## What the view does
//!
//! It renders a fixed table of 120 `(label, IANA zone)` pairs, each with the
//! zone's **current** UTC offset, sorted by `(offset, label)`.
//!
//! The offset formatting is not the obvious one and is ported as it is:
//!
//! * `offset` is `int(now.astimezone(tz).strftime("%z"))` — `"-0930"` becomes
//!   `-930`, so the sort key is *not* minutes and not even monotonic in the
//!   offset's sign convention. Sorting is `(that integer, label)`.
//! * `hours_offset = total_seconds // 3600` is Python's **floor** division and
//!   `minutes_offset = abs(total_seconds % 3600) // 60` uses Python's
//!   non-negative `%`, so a negative non-whole-hour zone renders the hour one
//!   step further from zero: `Pacific/Marquesas` (-09:30) is `UTC-10:30`
//!   (`base.py:186-189`). That is a long-standing upstream quirk; the
//!   differential test pins it (`-10:30`, not `-09:30`).
//! * `now` is `datetime.now()` — a naive local timestamp — and
//!   `astimezone(tz)` interprets it as local time, so the result is the offset
//!   **at the current instant**, which is what this port computes from
//!   `clock::now()`.
//!
//! ## The page cache (`cache_page(60 * 60 * 2)`)
//!
//! `get` carries `@method_decorator(cache_page(60 * 60 * 2))` (`base.py:23`).
//! Through `decorator_from_middleware_with_args` that is Django's
//! `CacheMiddleware` around **the method**, i.e. inside `APIView.dispatch`, and
//! this module reproduces what a client can observe with `plane_cache::page`
//! (DECISIONS.md D5a):
//!
//! 1. on a miss: `patch_response_headers(response, 7200)` adds `Expires`
//!    (`http_date(now + 7200)`) and `Cache-Control: max-age=7200`; the entry is
//!    stored for 7200 s with the same key the next request will look up;
//! 2. on a hit: the stored status, headers and body are served unchanged plus
//!    `Age = max(0, max_age - (expires - now))`
//!    (`django/middleware/cache.py:196-203`);
//! 3. only `GET`/`HEAD` read it, only a `200` is stored, and the
//!    `private`/`no-cache`/`no-store`/`Vary: *`/cookie guards suppress a store.
//!
//! The key is `_generate_cache_key`'s (`django/utils/cache.py:350-364`):
//! `plane_rs.cache_page.<method>.<md5(absolute_uri)>.<md5(vary values)>.
//! <language>.<timezone>` in `plane-cache`'s private namespace, so a Django
//! process never unpickles a Rust value and vice versa. Two facts make the
//! `Vary` list empty for this view, as it is for Django:
//!
//! * `learn_cache_key` runs on the response **before** `finalize_response`
//!   (`rest_framework/views.py:437-446` copies `default_response_headers`,
//!   which is where `Vary: Accept` comes from, only after the handler
//!   returns), so `response.has_header("Vary")` is false and the header list is
//!   `[]`;
//! * the language is `settings.LANGUAGE_CODE` (`en-us`) and the timezone
//!   `settings.TIME_ZONE` (`UTC`) because this route has no `TimezoneMixin`
//!   and Plane does not install `LocaleMiddleware`
//!   (`plane/settings/common.py:314,319`).
//!
//! Nothing is enqueued: the view touches no model and publishes no task.

use chrono::{DateTime, Offset, Utc};
use serde::Serialize;

use chrono_tz::Tz;
use http::Method;
use plane_cache::page::{self, CachedPage, PageResponse};
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;

use crate::common::request::absolute_uri;

/// `cache_page(60 * 60 * 2)` (`base.py:23`).
const CACHE_SECONDS: i64 = 2 * 60 * 60;

/// `settings.LANGUAGE_CODE` (`plane/settings/common.py:314`).
const LANGUAGE: &str = "en-us";
/// `settings.TIME_ZONE` (`plane/settings/common.py:319`).
const TIMEZONE: &str = "UTC";
/// The response's `Vary` list at `learn_cache_key` time — empty, because
/// `finalize_response` adds `Vary: Accept` only after the decorated handler
/// returned (`rest_framework/views.py:437-446`). See the module docs.
const VARY: &[&str] = &[];

/// Register `TimezoneEndpoint`'s `GET`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    let mut view = registry.api_view("plane.app.views.timezone.base.TimezoneEndpoint")?;
    view.method(Method::GET, get)?;
    Ok(())
}

/// `TimezoneEndpoint.get`, behind Django's `cache_page`.
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let redis = ctx.redis.ok_or_else(|| {
            // `FetchFromCacheMiddleware.process_request` calls `cache.get`
            // first; a process without a cache would raise there.
            ViewError::Unknown(
                "the process has no Redis cache handle; cache_page would fail the request"
                    .to_string(),
            )
        })?;
        let now = plane_core::clock::now();
        let stored_at = now.timestamp();
        let uri = absolute_uri(ctx);

        // `FetchFromCacheMiddleware.process_request`: a `HEAD` first looks for
        // the GET entry, then for its own (`django/middleware/cache.py:186-199`).
        if *ctx.method() == Method::HEAD {
            let key = page::page_key("GET", &uri, VARY, LANGUAGE, TIMEZONE);
            if let Some(hit) = page::lookup(redis, &key).await? {
                return serve(hit, stored_at);
            }
        }
        let key = page::page_key(ctx.method().as_str(), &uri, VARY, LANGUAGE, TIMEZONE);
        if let Some(hit) = page::lookup(redis, &key).await? {
            return serve(hit, stored_at);
        }

        // A miss: build the view's response, then `patch_response_headers`
        // (`django/middleware/cache.py:108-131`). `learn_cache_key` hashes the
        // response's `Vary` list, which is empty here (see the module docs).
        let mut response = PageResponse {
            status: 200,
            headers: Vec::new(),
            body: render_body(now),
        };
        page::patch_response_headers(&mut response, CACHE_SECONDS, stored_at);
        if page::is_cacheable(&response, false) {
            page::store(redis, &key, &response, CACHE_SECONDS, stored_at).await?;
        }
        Ok(
            ViewResponse::raw(response.status, APPLICATION_JSON, response.body)
                .with_headers(response.headers),
        )
    })
}

/// `FetchFromCacheMiddleware.process_request`'s hit: the stored response plus
/// `Age` (`django/middleware/cache.py:196-203`).
fn serve(hit: CachedPage, now: i64) -> Result<ViewResponse, ViewError> {
    // `Age` is only set when both `Cache-Control: max-age` and `Expires` are
    // present and parsable; our own entries always carry both.
    let age = hit.age(now);
    let mut response =
        ViewResponse::raw(hit.status, APPLICATION_JSON, hit.body).with_headers(hit.headers);
    if let Some(age) = age {
        response = response.with_header("Age", age.to_string());
    }
    Ok(response)
}

/// One rendered entry, in the key order Django's dict ends up with.
///
/// `sort_key` is `int(strftime("%z"))` from the Python and is skipped in the
/// body; `del tz["offset"]` removes it before rendering (`base.py:206-208`).
#[derive(Debug, Serialize)]
struct Timezone<'a> {
    #[serde(skip)]
    sort_key: i32,
    utc_offset: String,
    gmt_offset: String,
    value: &'a str,
    label: &'a str,
}

#[derive(Debug, Serialize)]
struct Envelope<'a> {
    timezones: &'a [Timezone<'a>],
}

/// `TimezoneEndpoint.get`'s body, for a given instant.
fn render_body(now: DateTime<Utc>) -> Vec<u8> {
    let mut timezones: Vec<Timezone<'_>> = LOCATIONS
        .iter()
        .filter_map(|(label, value)| entry(now, label, value))
        .collect();
    // `timezone_list.sort(key=lambda x: (x["offset"], x["label"]))` — a stable
    // sort on a tuple, so ties keep the table's order.
    timezones.sort_by(|a, b| (a.sort_key, a.label).cmp(&(b.sort_key, b.label)));

    // DRF's JSONRenderer, not serde_json's default: the envelope has one key,
    // but the workspace's `serde_json` is built without `preserve_order`, so a
    // `json!` object would sort keys alphabetically. `Dialect::Drf` is the same
    // formatter the runtime uses for `ViewResponse::Json`.
    plane_serde::to_vec(
        Dialect::Drf,
        &Envelope {
            timezones: &timezones,
        },
    )
    .expect("the timezone table is valid UTF-8 with no non-finite floats")
}

/// One `(label, zone)` pair, or `None` when pytz would have raised
/// `UnknownTimeZoneError` and the Python skips the entry (`base.py:200-201`).
fn entry<'a>(now: DateTime<Utc>, label: &'a str, value: &'a str) -> Option<Timezone<'a>> {
    let tz: Tz = value.parse().ok()?;
    // `now.astimezone(tz)`: the offset in effect at this instant.
    let local = now.with_timezone(&tz);
    let total = local.offset().fix().local_minus_utc();
    // Python: `total_seconds // 3600` (floor) and `abs(total_seconds % 3600) // 60`
    // (non-negative modulo). `div_euclid`/`rem_euclid` are the same two rules.
    let hours = total.div_euclid(3600);
    let minutes = total.rem_euclid(3600) / 60;
    let signal = if hours >= 0 { '+' } else { '-' };
    let offset = format!("{signal}{:02}:{minutes:02}", hours.abs());
    // The sort key comes from `strftime("%z")` — the *true* offset, not the
    // floored one above: `int("-0930") == -930` for Marquesas, whose display
    // offset is `UTC-10:30`.
    let unsigned = total.abs();
    let sort_key =
        if total >= 0 { 1 } else { -1 } * ((unsigned / 3600) * 100 + (unsigned % 3600) / 60);
    Some(Timezone {
        sort_key,
        utc_offset: format!("UTC{offset}"),
        gmt_offset: format!("GMT{offset}"),
        value,
        label,
    })
}

/// `timezone_locations`, verbatim from `base.py:25-173`.
///
/// Extracted from the Python source's own list literal (an `ast.literal_eval`
/// of the assignment), not retyped, so labels and identifiers cannot drift:
///
/// ```sh
/// apps/api/.venv/bin/python - <<'PY'
/// import ast, pathlib
/// src = pathlib.Path("apps/api/plane/app/views/timezone/base.py").read_text()
/// for node in ast.walk(ast.parse(src)):
///     if isinstance(node, ast.Assign) and any(
///         getattr(t, "id", None) == "timezone_locations" for t in node.targets
///     ):
///         print(ast.literal_eval(node.value))
/// PY
/// ```
static LOCATIONS: &[(&str, &str)] = &[
    ("Midway Island", "Pacific/Midway"),
    ("American Samoa", "Pacific/Pago_Pago"),
    ("Hawaii", "Pacific/Honolulu"),
    ("Aleutian Islands", "America/Adak"),
    ("Marquesas Islands", "Pacific/Marquesas"),
    ("Alaska", "America/Anchorage"),
    ("Gambier Islands", "Pacific/Gambier"),
    ("Pacific Time (US and Canada)", "America/Los_Angeles"),
    ("Baja California", "America/Tijuana"),
    ("Mountain Time (US and Canada)", "America/Denver"),
    ("Arizona", "America/Phoenix"),
    ("Chihuahua, Mazatlan", "America/Chihuahua"),
    ("Central Time (US and Canada)", "America/Chicago"),
    ("Saskatchewan", "America/Regina"),
    ("Guadalajara, Mexico City, Monterrey", "America/Mexico_City"),
    ("Tegucigalpa, Honduras", "America/Tegucigalpa"),
    ("Costa Rica", "America/Costa_Rica"),
    ("Eastern Time (US and Canada)", "America/New_York"),
    ("Lima", "America/Lima"),
    ("Bogota", "America/Bogota"),
    ("Quito", "America/Guayaquil"),
    ("Chetumal", "America/Cancun"),
    ("Caracas (Old Venezuela Time)", "America/Caracas"),
    ("Atlantic Time (Canada)", "America/Halifax"),
    ("Caracas", "America/Caracas"),
    ("Santiago", "America/Santiago"),
    ("La Paz", "America/La_Paz"),
    ("Manaus", "America/Manaus"),
    ("Georgetown", "America/Guyana"),
    ("Bermuda", "Atlantic/Bermuda"),
    ("Newfoundland Time (Canada)", "America/St_Johns"),
    ("Buenos Aires", "America/Argentina/Buenos_Aires"),
    ("Brasilia", "America/Sao_Paulo"),
    ("Greenland", "America/Godthab"),
    ("Montevideo", "America/Montevideo"),
    ("Falkland Islands", "Atlantic/Stanley"),
    (
        "South Georgia and the South Sandwich Islands",
        "Atlantic/South_Georgia",
    ),
    ("Azores", "Atlantic/Azores"),
    ("Cape Verde Islands", "Atlantic/Cape_Verde"),
    ("Dublin", "Europe/Dublin"),
    ("Reykjavik", "Atlantic/Reykjavik"),
    ("Lisbon", "Europe/Lisbon"),
    ("Monrovia", "Africa/Monrovia"),
    ("Casablanca", "Africa/Casablanca"),
    (
        "Central European Time (Berlin, Rome, Paris)",
        "Europe/Paris",
    ),
    ("West Central Africa", "Africa/Lagos"),
    ("Algiers", "Africa/Algiers"),
    ("Lagos", "Africa/Lagos"),
    ("Tunis", "Africa/Tunis"),
    (
        "Eastern European Time (Cairo, Helsinki, Kyiv)",
        "Europe/Kyiv",
    ),
    ("Athens", "Europe/Athens"),
    ("Jerusalem", "Asia/Jerusalem"),
    ("Johannesburg", "Africa/Johannesburg"),
    ("Harare, Pretoria", "Africa/Harare"),
    ("Moscow Time", "Europe/Moscow"),
    ("Baghdad", "Asia/Baghdad"),
    ("Nairobi", "Africa/Nairobi"),
    ("Kuwait, Riyadh", "Asia/Riyadh"),
    ("Tehran", "Asia/Tehran"),
    ("Abu Dhabi", "Asia/Dubai"),
    ("Baku", "Asia/Baku"),
    ("Yerevan", "Asia/Yerevan"),
    ("Astrakhan", "Europe/Astrakhan"),
    ("Tbilisi", "Asia/Tbilisi"),
    ("Mauritius", "Indian/Mauritius"),
    ("Kabul", "Asia/Kabul"),
    ("Islamabad", "Asia/Karachi"),
    ("Karachi", "Asia/Karachi"),
    ("Tashkent", "Asia/Tashkent"),
    ("Yekaterinburg", "Asia/Yekaterinburg"),
    ("Maldives", "Indian/Maldives"),
    ("Chagos", "Indian/Chagos"),
    ("Chennai", "Asia/Kolkata"),
    ("Kolkata", "Asia/Kolkata"),
    ("Mumbai", "Asia/Kolkata"),
    ("New Delhi", "Asia/Kolkata"),
    ("Sri Jayawardenepura", "Asia/Colombo"),
    ("Kathmandu", "Asia/Kathmandu"),
    ("Dhaka", "Asia/Dhaka"),
    ("Almaty", "Asia/Almaty"),
    ("Bishkek", "Asia/Bishkek"),
    ("Thimphu", "Asia/Thimphu"),
    ("Yangon (Rangoon)", "Asia/Yangon"),
    ("Cocos Islands", "Indian/Cocos"),
    ("Bangkok", "Asia/Bangkok"),
    ("Hanoi", "Asia/Ho_Chi_Minh"),
    ("Jakarta", "Asia/Jakarta"),
    ("Novosibirsk", "Asia/Novosibirsk"),
    ("Krasnoyarsk", "Asia/Krasnoyarsk"),
    ("Beijing", "Asia/Shanghai"),
    ("Singapore", "Asia/Singapore"),
    ("Perth", "Australia/Perth"),
    ("Hong Kong", "Asia/Hong_Kong"),
    ("Ulaanbaatar", "Asia/Ulaanbaatar"),
    ("Palau", "Pacific/Palau"),
    ("Eucla", "Australia/Eucla"),
    ("Tokyo", "Asia/Tokyo"),
    ("Seoul", "Asia/Seoul"),
    ("Yakutsk", "Asia/Yakutsk"),
    ("Adelaide", "Australia/Adelaide"),
    ("Darwin", "Australia/Darwin"),
    ("Sydney", "Australia/Sydney"),
    ("Brisbane", "Australia/Brisbane"),
    ("Guam", "Pacific/Guam"),
    ("Vladivostok", "Asia/Vladivostok"),
    ("Tahiti", "Pacific/Tahiti"),
    ("Lord Howe Island", "Australia/Lord_Howe"),
    ("Solomon Islands", "Pacific/Guadalcanal"),
    ("Magadan", "Asia/Magadan"),
    ("Norfolk Island", "Pacific/Norfolk"),
    ("Bougainville Island", "Pacific/Bougainville"),
    ("Chokurdakh", "Asia/Srednekolymsk"),
    ("Auckland", "Pacific/Auckland"),
    ("Wellington", "Pacific/Auckland"),
    ("Fiji Islands", "Pacific/Fiji"),
    ("Anadyr", "Asia/Anadyr"),
    ("Chatham Islands", "Pacific/Chatham"),
    ("Nuku'alofa", "Pacific/Tongatapu"),
    ("Samoa", "Pacific/Apia"),
    ("Kiritimati Island", "Pacific/Kiritimati"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The golden body was recorded from the real Python algorithm for the same
    /// instant (`datetime(2026, 11, 16, 0, 0, tzinfo=utc)` through the view's own
    /// `timezone_locations`, pytz and `JSONRenderer`), so it also proves that
    /// chrono-tz and pytz agree on every one of the 120 zones at that instant:
    ///
    /// ```sh
    /// apps/api/.venv/bin/python - <<'PY'
    /// import ast, datetime, json, pathlib, pytz
    /// src = pathlib.Path("apps/api/plane/app/views/timezone/base.py").read_text()
    /// for node in ast.walk(ast.parse(src)):
    ///     if isinstance(node, ast.Assign) and any(
    ///         getattr(t, "id", None) == "timezone_locations" for t in node.targets
    ///     ):
    ///         pairs = ast.literal_eval(node.value)
    /// now = datetime.datetime(2026, 11, 16, tzinfo=datetime.timezone.utc)
    /// out = []
    /// for label, identifier in pairs:
    ///     tz = pytz.timezone(identifier)
    ///     aware = now.astimezone(tz)
    ///     total = int(aware.utcoffset().total_seconds())
    ///     hours, minutes = total // 3600, abs(total % 3600) // 60
    ///     offset = f"{'+' if hours >= 0 else '-'}{abs(hours):02}:{minutes:02}"
    ///     out.append({
    ///         "offset": int(aware.strftime("%z")),
    ///         "utc_offset": f"UTC{offset}",
    ///         "gmt_offset": f"GMT{offset}",
    ///         "value": identifier,
    ///         "label": label,
    ///     })
    /// out.sort(key=lambda x: (x["offset"], x["label"]))
    /// for tz in out:
    ///     del tz["offset"]
    /// print(json.dumps({"timezones": out}, separators=(",", ":"), ensure_ascii=False))
    /// PY
    /// ```
    const GOLDEN: &str = include_str!("../../tests/golden/timezones-2026-11-16.json");

    fn instant() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-16T00:00:00Z")
            .expect("fixed instant")
            .with_timezone(&Utc)
    }

    #[test]
    fn the_body_matches_django_byte_for_byte() {
        let body = render_body(instant());
        let text = String::from_utf8(body).expect("utf-8");
        assert_eq!(text, GOLDEN);
    }

    #[test]
    fn the_negative_half_hour_quirk_is_ported() {
        // `total_seconds // 3600` floors, so -09:30 renders as -10:30. Bugs are
        // ported, not fixed (RUST_REWRITE.md, "When behaviour looks like a bug").
        let marquesas = entry(instant(), "Marquesas Islands", "Pacific/Marquesas").expect("zone");
        assert_eq!(marquesas.utc_offset, "UTC-10:30");
        assert_eq!(marquesas.gmt_offset, "GMT-10:30");
        assert_eq!(marquesas.sort_key, -930);
    }

    #[test]
    fn positive_offsets_use_the_same_flooring() {
        let kathmandu = entry(instant(), "Kathmandu", "Asia/Kathmandu").expect("zone");
        assert_eq!(kathmandu.utc_offset, "UTC+05:45");
        let kolkata = entry(instant(), "Kolkata", "Asia/Kolkata").expect("zone");
        assert_eq!(kolkata.utc_offset, "UTC+05:30");
        assert_eq!(kolkata.sort_key, 530);
    }

    #[test]
    fn every_zone_in_the_table_resolves() {
        // A zone chrono-tz does not know would be silently dropped the way
        // pytz raises `UnknownTimeZoneError`; the golden test would then differ,
        // but this says which one.
        for (label, value) in LOCATIONS {
            assert!(
                value.parse::<Tz>().is_ok(),
                "{value} ({label}) is not in chrono-tz"
            );
        }
        assert_eq!(LOCATIONS.len(), 120, "base.py:25-173 lists 120 entries");
    }

    #[test]
    fn the_cache_headers_are_the_ones_django_patches_on() {
        // `patch_response_headers(response, 7200)` for the fixed instant:
        // `Expires` is now + 7200 s in HTTP-date form, `Cache-Control` is
        // `max-age=7200`.
        let mut response = PageResponse {
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        };
        page::patch_response_headers(&mut response, CACHE_SECONDS, instant().timestamp());
        assert_eq!(
            response.header("Expires"),
            Some("Mon, 16 Nov 2026 02:00:00 GMT")
        );
        assert_eq!(response.header("Cache-Control"), Some("max-age=7200"));
        assert!(page::is_cacheable(&response, false));
    }

    #[test]
    fn the_page_key_is_the_rust_namespace_and_names_the_method() {
        let uri = "http://127.0.0.1:18100/api/timezones/";
        let key = page::page_key("GET", uri, VARY, LANGUAGE, TIMEZONE);
        assert!(key.starts_with("plane_rs.cache_page.GET."), "{key}");
        assert!(key.ends_with(".en-us.UTC"), "{key}");
        assert_ne!(
            key,
            page::page_key("HEAD", uri, VARY, LANGUAGE, TIMEZONE),
            "HEAD stores under its own method"
        );
        // The `Vary` list is part of the key; this view's is empty.
        assert_eq!(
            page::page_key("GET", uri, &[], LANGUAGE, TIMEZONE),
            page::page_key("GET", uri, VARY, LANGUAGE, TIMEZONE)
        );
    }
}
