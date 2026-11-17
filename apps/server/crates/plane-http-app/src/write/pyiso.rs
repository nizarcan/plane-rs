// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! CPython's `date.fromisoformat` / `datetime.fromisoformat` and Django's
//! regex fallback, the two halves of `django.utils.dateparse`.
//!
//! `django.utils.dateparse.parse_datetime` (`django/utils/dateparse.py:104-124`,
//! Django 5.2) first calls `datetime.datetime.fromisoformat`, and only on
//! `ValueError` falls back to `datetime_re`. `parse_date` (`:80-92`) is the same
//! shape with `date.fromisoformat` and `date_re`.
//!
//! The interpreter runs CPython's C implementation (`Modules/_datetimemodule.c`),
//! which is *not* the same as `Lib/_pydatetime.py`: it accepts times without
//! separators (`123456`), ignores one character between the time and the zone,
//! treats `+00:00:00.5` as UTC, and lets a week date carry a time. Every branch
//! below cites the C function it mirrors. The corpus at
//! `tests/golden/datetime-parsing.json` pins every one of them against the real
//! thing; regenerate it with `tools/inventory/datetime_corpus.py`.
//!
//! The regex fallback is hand-written rather than pulled from a crate because
//! Python's `re` is not the usual regex dialect: `\d` is Unicode `Nd` (and
//! `int()` accepts those digits), `\s` is `str.isspace()`, and `$` also matches
//! before a single trailing `\n`.

use chrono::{
    Datelike, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone, Weekday,
};

/// A parsed Python `datetime.datetime`.
///
/// `offset_us` is `Some` for an aware value. Python's `fromisoformat` and
/// Django's `datetime_re` only ever produce a fixed `datetime.timezone`, so an
/// offset in microseconds is a complete description of the `tzinfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PyDateTime {
    pub naive: NaiveDateTime,
    pub offset_us: Option<i64>,
}

impl PyDateTime {
    const fn naive(naive: NaiveDateTime) -> Self {
        Self {
            naive,
            offset_us: None,
        }
    }
}

/// `datetime.datetime.fromisoformat` (`_datetimemodule.c:datetime_fromisoformat`,
/// CPython 3.12.13). `None` mirrors the `ValueError`/`TypeError` it raises.
#[must_use]
pub fn fromisoformat_datetime(text: &str) -> Option<PyDateTime> {
    let bytes = text.as_bytes();
    // `_sanitize_isoformat_str` rejects anything shorter than "all valid ISO
    // strings are at least 7 characters long".
    if bytes.len() < 7 {
        return None;
    }
    let separator = find_datetime_separator(bytes)?;
    // `parse_isoformat_date(p, separator_location)`: the length argument only
    // decides whether a week date has a day component.
    let (year, month, day) = parse_isoformat_date(bytes, separator)?;
    let date = date_from_ymd(year, month, day)?;
    if bytes.len() == separator {
        // A bare date: `len > separator_location` is false in the C caller.
        return Some(PyDateTime::naive(date.and_time(NaiveTime::MIN)));
    }
    // The C code skips the separator by its UTF-8 width, so any character
    // (including a multi-byte one) is accepted there.
    let rest = text.get(separator..)?;
    let skip = rest.chars().next()?.len_utf8();
    let time = parse_isoformat_time(bytes.get(separator + skip..)?)?;
    Some(PyDateTime {
        naive: date.and_time(time.0),
        offset_us: time.1,
    })
}

/// `date.fromisoformat` (`_datetimemodule.c:date_fromisoformat`): the whole
/// string must be 7, 8 or 10 characters long, and a basic-format date ignores
/// whatever follows the last component within that length.
#[must_use]
pub fn fromisoformat_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    if !matches!(bytes.len(), 7 | 8 | 10) {
        return None;
    }
    let (year, month, day) = parse_isoformat_date(bytes, bytes.len())?;
    date_from_ymd(year, month, day)
}

/// Django `parse_date` (`django/utils/dateparse.py:80-92`).
#[must_use]
pub fn parse_date(text: &str) -> Option<NaiveDate> {
    fromisoformat_date(text).or_else(|| parse_date_regex(text))
}

/// Django `parse_datetime` (`django/utils/dateparse.py:104-124`).
#[must_use]
pub fn parse_datetime(text: &str) -> Option<PyDateTime> {
    fromisoformat_datetime(text).or_else(|| parse_datetime_regex(text))
}

/// `_find_isoformat_datetime_separator` (`_datetimemodule.c`): where the date
/// part ends. `None` mirrors its `-1` return, which the caller turns into an
/// invalid string.
fn find_datetime_separator(bytes: &[u8]) -> Option<usize> {
    let len = bytes.len();
    if len == 7 {
        return Some(7);
    }
    if bytes[4] == b'-' {
        if bytes[5] == b'W' {
            // YYYY-W??
            if len < 8 {
                return None;
            }
            if len > 8 && bytes[8] == b'-' {
                // YYYY-Www-D (10) or YYYY-Www-HH (8)
                if len == 9 {
                    return None;
                }
                if len > 10 && bytes[10].is_ascii_digit() {
                    // YYYY-Www-## is ambiguous; a hyphen at 8 is assumed.
                    return Some(8);
                }
                return Some(10);
            }
            // YYYY-Www (8)
            return Some(8);
        }
        // YYYY-MM-DD (10)
        return Some(10);
    }
    if bytes[4] == b'W' {
        // YYYYWww (7) or YYYYWwwd (8): the first non-digit decides.
        let mut index = 7;
        while index < len {
            if !bytes[index].is_ascii_digit() {
                break;
            }
            index += 1;
        }
        if index < 9 {
            return Some(index);
        }
        return Some(if index % 2 == 0 { 7 } else { 8 });
    }
    // YYYYMMDD (8)
    Some(8)
}

/// `parse_isoformat_date` (`_datetimemodule.c`).
///
/// `len` is the caller's limit: the week-date branch looks at it to decide
/// whether a day component follows, and everything else is read straight out
/// of `bytes` (the C code reads the NUL-terminated buffer, so a read at the end
/// just fails).
fn parse_isoformat_date(bytes: &[u8], len: usize) -> Option<(u32, u32, u32)> {
    let mut position = 0usize;
    let year = parse_digits(bytes, &mut position, 4)?;
    let uses_separator = bytes.get(position) == Some(&b'-');
    if uses_separator {
        position += 1;
    }
    if bytes.get(position) == Some(&b'W') {
        position += 1;
        let week = parse_digits(bytes, &mut position, 2)?;
        let day = if position < len {
            if uses_separator {
                if bytes.get(position) != Some(&b'-') {
                    return None;
                }
                position += 1;
            }
            parse_digits(bytes, &mut position, 1)?
        } else {
            1
        };
        let date = isoweek_to_gregorian(year, week, day)?;
        return Some((date.year() as u32, date.month(), date.day()));
    }
    let month = parse_digits(bytes, &mut position, 2)?;
    if uses_separator {
        if bytes.get(position) != Some(&b'-') {
            return None;
        }
        position += 1;
    }
    let day = parse_digits(bytes, &mut position, 2)?;
    Some((year, month, day))
}

/// `parse_digits` (`_datetimemodule.c`), ASCII digits only.
fn parse_digits(bytes: &[u8], position: &mut usize, count: usize) -> Option<u32> {
    let mut value = 0u32;
    for _ in 0..count {
        let digit = *bytes.get(*position)?;
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(digit - b'0');
        *position += 1;
    }
    Some(value)
}

/// `iso_to_ymd` (`_datetimemodule.c`): ISO week date to a calendar date, with
/// Python's bounds (year 1..=9999).
fn isoweek_to_gregorian(year: u32, week: u32, day: u32) -> Option<NaiveDate> {
    let weekday = match day {
        1 => Weekday::Mon,
        2 => Weekday::Tue,
        3 => Weekday::Wed,
        4 => Weekday::Thu,
        5 => Weekday::Fri,
        6 => Weekday::Sat,
        7 => Weekday::Sun,
        _ => return None,
    };
    let date = NaiveDate::from_isoywd_opt(i32::try_from(year).ok()?, week, weekday)?;
    (1..=9999).contains(&date.year()).then_some(date)
}

fn date_from_ymd(year: u32, month: u32, day: u32) -> Option<NaiveDate> {
    if !(1..=9999).contains(&year) {
        return None;
    }
    NaiveDate::from_ymd_opt(i32::try_from(year).ok()?, month, day)
}

/// `parse_isoformat_time` (`_datetimemodule.c`): `HH[:MM[:SS[.fff[fff]]]]`
/// followed by `Z`, `±HH[:MM[:SS[.ffffff]]]`, or nothing.
fn parse_isoformat_time(bytes: &[u8]) -> Option<(NaiveTime, Option<i64>)> {
    // The C code scans for the first 'Z', '+' or '-' in the remaining string.
    let zone = bytes
        .iter()
        .position(|byte| matches!(byte, b'Z' | b'+' | b'-'))
        .unwrap_or(bytes.len());
    let scanned = scan_hh_mm_ss(bytes, zone)?;
    let time = NaiveTime::from_hms_micro_opt(
        scanned.hour,
        scanned.minute,
        scanned.second,
        scanned.microsecond,
    )?;
    if zone == bytes.len() {
        // No timezone: anything left over at the end of the string is an error.
        return scanned.at_end.then_some((time, None));
    }
    if bytes[zone] == b'Z' {
        // `*(tzinfo_pos + 1) != '\0'`: anything after the `Z` fails, but an
        // embedded NUL is the C string's end.
        return (bytes.get(zone + 1).copied().unwrap_or(0) == 0).then_some((time, Some(0)));
    }
    let sign = if bytes[zone] == b'-' { -1i64 } else { 1 };
    let offset = scan_hh_mm_ss(bytes.get(zone + 1..)?, bytes.len() - zone - 1)?;
    if !offset.at_end {
        return None;
    }
    let seconds =
        i64::from(offset.hour) * 3600 + i64::from(offset.minute) * 60 + i64::from(offset.second);
    // `tzinfo_from_isoformat_results` returns UTC whenever the *seconds* part
    // of the offset is zero, dropping a `+00:00:00.5` microsecond component.
    let micros = if seconds == 0 {
        0
    } else {
        sign * i64::from(offset.microsecond)
    };
    let total = sign * seconds * 1_000_000 + micros;
    // `new_timezone`: the offset is strictly between -24h and +24h.
    if total <= -86_400_000_000 || total >= 86_400_000_000 {
        return None;
    }
    Some((time, Some(total)))
}

struct Scan {
    hour: u32,
    minute: u32,
    second: u32,
    microsecond: u32,
    at_end: bool,
}

/// `parse_hh_mm_ss_ff` (`_datetimemodule.c`), including its `p_end` limit: the
/// components are read from `bytes` (the rest of the original string) but
/// `end` is where the time zone designator was found.
fn scan_hh_mm_ss(bytes: &[u8], end: usize) -> Option<Scan> {
    let mut components = [0u32; 4];
    let mut position = 0usize;
    let mut has_separator = true;
    let mut index = 0usize;
    while index < 3 {
        components[index] = parse_digits(bytes, &mut position, 2)?;
        let character = bytes.get(position).copied().unwrap_or(0);
        position += 1;
        if index == 0 {
            has_separator = character == b':';
        }
        if position >= end {
            return Some(Scan {
                hour: components[0],
                minute: components[1],
                second: components[2],
                microsecond: 0,
                at_end: character == 0,
            });
        }
        if has_separator && character == b':' {
            index += 1;
            continue;
        }
        if character == b'.' || character == b',' {
            return scan_fraction(bytes, position, end, components);
        }
        if has_separator {
            return None;
        }
        position -= 1;
        index += 1;
    }
    scan_fraction(bytes, position, end, components)
}

fn scan_fraction(
    bytes: &[u8],
    mut position: usize,
    end: usize,
    mut components: [u32; 4],
) -> Option<Scan> {
    let remaining = end.checked_sub(position)?;
    if remaining == 0 {
        return None;
    }
    let to_parse = remaining.min(6);
    let digits = parse_digits(bytes, &mut position, to_parse)?;
    const CORRECTION: [u32; 5] = [100_000, 10_000, 1_000, 100, 10];
    components[3] = if to_parse < 6 {
        digits * CORRECTION[to_parse - 1]
    } else {
        digits
    };
    while bytes.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
    }
    Some(Scan {
        hour: components[0],
        minute: components[1],
        second: components[2],
        microsecond: components[3],
        at_end: bytes.get(position).copied().unwrap_or(0) == 0,
    })
}

/// Django `datetime_re` (`django/utils/dateparse.py:18-23`):
/// `YYYY-MM-DD[T ]H:MM[:SS[.,]ffffff]\s*(Z|±HH[:MM])?$`, with Python's
/// backtracking over the `\d{1,2}` groups.
fn parse_datetime_regex(text: &str) -> Option<PyDateTime> {
    let chars: Vec<char> = text.chars().collect();
    let year = py_uint(&chars, 0, 4)?;
    if chars.get(4) != Some(&'-') {
        return None;
    }
    for month_len in [2usize, 1] {
        let Some(month) = py_uint(&chars, 5, month_len) else {
            continue;
        };
        let month_separator = 5 + month_len;
        if chars.get(month_separator) != Some(&'-') {
            continue;
        }
        for day_len in [2usize, 1] {
            let Some(day) = py_uint(&chars, month_separator + 1, day_len) else {
                continue;
            };
            let day_end = month_separator + 1 + day_len;
            let Some(&clock_separator) = chars.get(day_end) else {
                continue;
            };
            if clock_separator != 'T' && clock_separator != ' ' {
                continue;
            }
            for hour_len in [2usize, 1] {
                let Some(hour) = py_uint(&chars, day_end + 1, hour_len) else {
                    continue;
                };
                let hour_end = day_end + 1 + hour_len;
                if chars.get(hour_end) != Some(&':') {
                    continue;
                }
                for minute_len in [2usize, 1] {
                    let Some(minute) = py_uint(&chars, hour_end + 1, minute_len) else {
                        continue;
                    };
                    let tail = hour_end + 1 + minute_len;
                    if let Some(value) =
                        finish_datetime_regex(&chars, tail, year, month, day, hour, minute)
                    {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn finish_datetime_regex(
    chars: &[char],
    tail: usize,
    year: u32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
) -> Option<PyDateTime> {
    let mut second = 0u32;
    let mut microsecond = 0u32;
    let zone;
    if chars.get(tail) == Some(&':') {
        let mut matched = None;
        for second_len in [2usize, 1] {
            let Some(value) = py_uint(chars, tail + 1, second_len) else {
                continue;
            };
            let after = tail + 1 + second_len;
            let (end, micro) = match chars.get(after) {
                Some('.') | Some(',') => {
                    let run = digit_run(chars, after + 1);
                    // `(?:[.,](?P<microsecond>\d{1,6})\d{0,6})?`
                    if run == 0 || run > 12 {
                        continue;
                    }
                    let captured = run.min(6);
                    let Some(digits) = py_uint(chars, after + 1, captured) else {
                        continue;
                    };
                    const CORRECTION: [u32; 5] = [100_000, 10_000, 1_000, 100, 10];
                    let scaled = if captured < 6 {
                        digits * CORRECTION[captured - 1]
                    } else {
                        digits
                    };
                    (after + 1 + run, scaled)
                }
                _ => (after, 0),
            };
            let Some(zone) = match_zone(chars, skip_python_whitespace(chars, end)) else {
                continue;
            };
            matched = Some((value, micro, zone));
            break;
        }
        let (value, micro, matched_zone) = matched?;
        second = value;
        microsecond = micro;
        zone = matched_zone;
    } else {
        zone = match_zone(chars, skip_python_whitespace(chars, tail))?;
    }
    let date = date_from_ymd(year, month, day)?;
    let time = NaiveTime::from_hms_micro_opt(hour, minute, second, microsecond)?;
    Some(PyDateTime {
        naive: date.and_time(time),
        offset_us: zone.offset(),
    })
}

/// The `(?P<tzinfo>…)` group: absent (a naive value) or a fixed offset.
enum Zone {
    Absent,
    Offset(i64),
}

impl Zone {
    fn offset(self) -> Option<i64> {
        match self {
            Self::Absent => None,
            Self::Offset(value) => Some(value),
        }
    }
}

/// `(?P<tzinfo>Z|[+-]\d{2}(?::?\d{2})?)?$`, plus the `$` anchor.
fn match_zone(chars: &[char], position: usize) -> Option<Zone> {
    if chars.get(position) == Some(&'Z') && is_regex_end(chars, position + 1) {
        return Some(Zone::Offset(0));
    }
    if let Some(&sign) = chars.get(position)
        && (sign == '+' || sign == '-')
        && let Some(hours) = py_uint(chars, position + 1, 2)
    {
        if chars.get(position + 3) == Some(&':')
            && let Some(minutes) = py_uint(chars, position + 4, 2)
            && is_regex_end(chars, position + 6)
        {
            return regex_offset(sign, hours, minutes).map(Zone::Offset);
        }
        if let Some(minutes) = py_uint(chars, position + 3, 2)
            && is_regex_end(chars, position + 5)
        {
            return regex_offset(sign, hours, minutes).map(Zone::Offset);
        }
        if is_regex_end(chars, position + 3) {
            return regex_offset(sign, hours, 0).map(Zone::Offset);
        }
    }
    is_regex_end(chars, position).then_some(Zone::Absent)
}

/// `get_fixed_timezone(offset)` (`django/utils/timezone.py:33-40`): the offset
/// is in minutes, and `datetime.timezone` requires it to be under 24 hours.
/// `None` mirrors the `ValueError` that `parse_datetime` lets escape.
fn regex_offset(sign: char, hours: u32, minutes: u32) -> Option<i64> {
    let total = i64::from(hours) * 60 + i64::from(minutes);
    if total >= 1440 {
        return None;
    }
    let signed = if sign == '-' { -total } else { total };
    Some(signed * 60 * 1_000_000)
}

/// Python's `$` without `MULTILINE`: the end of the string, or just before a
/// final `\n`.
fn is_regex_end(chars: &[char], position: usize) -> bool {
    position == chars.len() || (position + 1 == chars.len() && chars[position] == '\n')
}

/// Django `date_re` (`django/utils/dateparse.py:16`):
/// `(?P<year>\d{4})-(?P<month>\d{1,2})-(?P<day>\d{1,2})$`.
fn parse_date_regex(text: &str) -> Option<NaiveDate> {
    let chars: Vec<char> = text.chars().collect();
    let year = py_uint(&chars, 0, 4)?;
    if chars.get(4) != Some(&'-') {
        return None;
    }
    for month_len in [2usize, 1] {
        let Some(month) = py_uint(&chars, 5, month_len) else {
            continue;
        };
        let month_separator = 5 + month_len;
        if chars.get(month_separator) != Some(&'-') {
            continue;
        }
        for day_len in [2usize, 1] {
            let Some(day) = py_uint(&chars, month_separator + 1, day_len) else {
                continue;
            };
            if is_regex_end(&chars, month_separator + 1 + day_len)
                && let Some(date) = date_from_ymd(year, month, day)
            {
                return Some(date);
            }
        }
    }
    None
}

/// `int()` over a `\d` capture: Unicode decimal digits, ASCII or not.
fn py_uint(chars: &[char], start: usize, count: usize) -> Option<u32> {
    let mut value = 0u32;
    for index in start..start + count {
        value = value
            .checked_mul(10)?
            .checked_add(py_digit_value(*chars.get(index)?)?)?;
    }
    Some(value)
}

fn digit_run(chars: &[char], start: usize) -> usize {
    let mut count = 0;
    while chars
        .get(start + count)
        .is_some_and(|character| py_digit_value(*character).is_some())
    {
        count += 1;
    }
    count
}

/// `str.isspace()`: what Python's `\s` matches in a `str` pattern.
fn is_python_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{9}'..='\u{d}'
            | '\u{1c}'..='\u{20}'
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'..='\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

fn skip_python_whitespace(chars: &[char], mut position: usize) -> usize {
    while chars
        .get(position)
        .is_some_and(|character| is_python_whitespace(*character))
    {
        position += 1;
    }
    position
}

/// Unicode category `Nd` (Python's `\d` and `int()`), Unicode 15.0.0 as shipped
/// with CPython 3.12. Every block is ten consecutive code points.
const DECIMAL_DIGITS: &[u32] = &[
    0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
    0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
    0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0, 0xff10,
    0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0, 0x11650,
    0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50, 0x11da0, 0x11f50, 0x16a60, 0x16ac0,
    0x16b50, 0x1d7ce, 0x1e140, 0x1e2f0, 0x1e4f0, 0x1e950, 0x1fbf0,
];

fn py_digit_value(character: char) -> Option<u32> {
    let code = character as u32;
    // `0x1d7ce..=0x1d7ff` is five ten-digit blocks; every other entry is one.
    if (0x1d7ce..=0x1d7ff).contains(&code) {
        return Some((code - 0x1d7ce) % 10);
    }
    DECIMAL_DIGITS
        .iter()
        .find(|start| code >= **start && code < **start + 10)
        .map(|start| code - *start)
}

/// `timezone.make_aware(value, tz)` plus DRF's `valid_datetime`
/// (`rest_framework/utils/timezone.py:5-25`).
///
/// Python keeps `fold=0` for an ambiguous wall time and for an imaginary one
/// (a wall time inside a DST gap), and `valid_datetime` never rejects a
/// `zoneinfo.ZoneInfo` value: for an ambiguous time `datetime_exists` compares
/// it against its UTC equivalent with `==`, which is `False` whenever the
/// wall-clock fields differ, so the ambiguity check short-circuits. Both cases
/// are therefore accepted with the offset in effect *before* the transition.
/// Returns the UTC wall time.
#[must_use]
pub fn localize(naive: NaiveDateTime, timezone: chrono_tz::Tz) -> Option<NaiveDateTime> {
    match timezone.from_local_datetime(&naive) {
        LocalResult::Single(value) => Some(value.naive_utc()),
        LocalResult::Ambiguous(earliest, _) => Some(earliest.naive_utc()),
        LocalResult::None => utc_before_gap(timezone, naive),
    }
}

/// The wall time `timezone` shows for an instant: what
/// `value.astimezone(field_timezone)` produces (before Django converts it back
/// to UTC for the database).
#[must_use]
pub fn local_wall_time(utc: NaiveDateTime, timezone: chrono_tz::Tz) -> NaiveDateTime {
    timezone.from_utc_datetime(&utc).naive_local()
}

/// The `fold=0` interpretation of a wall time inside a DST gap: the offset of
/// the last minute before the gap, i.e. the offset in effect before the
/// transition (PEP 495).
fn utc_before_gap(timezone: chrono_tz::Tz, naive: NaiveDateTime) -> Option<NaiveDateTime> {
    let mut probe = naive;
    // A gap is at most a skipped day (Pacific/Apia, 2011), so 26 hours of
    // minute steps always leave it.
    for _ in 0..1_560 {
        probe = probe.checked_sub_signed(TimeDelta::minutes(1))?;
        if let LocalResult::Single(value) | LocalResult::Ambiguous(value, _) =
            timezone.from_local_datetime(&probe)
        {
            let offset = value.naive_local() - value.naive_utc();
            return naive.checked_sub_signed(offset);
        }
    }
    None
}
