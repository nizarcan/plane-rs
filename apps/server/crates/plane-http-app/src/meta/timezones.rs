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
//! ## The cache decorator, and what is not ported yet
//!
//! `get` carries `@method_decorator(cache_page(60 * 60 * 2))`
//! (`base.py:23`). `cache_page` is Django's `CacheMiddleware`, which for a
//! response that can be rendered:
//!
//! 1. patches `Expires` and `Cache-Control: max-age=7200` onto the response
//!    (`django/utils/cache.py:patch_response_headers`);
//! 2. writes two django-redis keys — `views.decorators.cache.cache_header…`
//!    (the `Vary` header list, here `[]`) and
//!    `views.decorators.cache.cache_page..GET.<md5(url)>.<md5(headers)>` with
//!    the suffix `.<language>.<timezone>` — each with a 7200 s TTL, through a
//!    post-render callback (`django/middleware/cache.py:108-131`);
//! 3. serves the second request for the same URL from that cache without
//!    running the view.
//!
//! This port reproduces (1), because those two headers are visible to the
//! client, and does **not** reproduce (2) or (3): the cached value is a Python
//! pickle of the `rest_framework.response.Response` object, which needs the
//! django-redis page cache from `plane-cache`. Consequences to know about:
//!
//! * a second request for the same URL re-renders the table instead of reading
//!   it back (a few microseconds) and does **not** send the `Age` header
//!   `FetchFromCacheMiddleware` sets on a hit (`django/middleware/cache.py:196-203`);
//! * a differential run compares Redis, so this route reports one extra
//!   key pair on the Django side, and the second step of a scenario also
//!   differs by that `Age` header, until `plane-cache` lands. `HANDLERS.md`
//!   ("side effects") has the rule: a route is not marked `verified` in
//!   `ROUTES.md` while a cache it writes is unported.
//!
//! Nothing is enqueued: the view touches no model and publishes no task.

use chrono::{DateTime, Offset, Utc};
use serde::Serialize;

use chrono_tz::Tz;
use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;

/// `cache_page(60 * 60 * 2)` (`base.py:23`).
const CACHE_SECONDS: u64 = 2 * 60 * 60;

/// Register `TimezoneEndpoint`'s `GET`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    let mut view = registry.api_view("plane.app.views.timezone.base.TimezoneEndpoint")?;
    view.method(Method::GET, get)?;
    Ok(())
}

/// `TimezoneEndpoint.get`.
fn get<'a>(_ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let now = plane_core::clock::now();
        Ok(ViewResponse::raw(200, APPLICATION_JSON, render_body(now))
            .with_header("Expires", http_date(now))
            .with_header("Cache-Control", format!("max-age={CACHE_SECONDS}")))
    })
}

/// `http_date(time.time() + timeout)`, the `Expires` value `cache_page` patches
/// on (`django/utils/cache.py:patch_response_headers` →
/// `django/utils/http.py:http_date` → `email.utils.formatdate(usegmt=True)`).
///
/// Second resolution, UTC, English abbreviations; chrono's `%a`/`%b` are the
/// fixed C-locale names Django's `formatdate` uses.
fn http_date(now: DateTime<Utc>) -> String {
    let at = now + chrono::Duration::seconds(CACHE_SECONDS as i64);
    at.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
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
        assert_eq!(http_date(instant()), "Mon, 16 Nov 2026 02:00:00 GMT");
    }
}
