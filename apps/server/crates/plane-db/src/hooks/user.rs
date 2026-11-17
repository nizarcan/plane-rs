// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H30 — `User.save` (`plane/db/models/user.py:169-187`, MODEL_HOOKS.md:2160).
//!
//! ```python
//! def save(self, *args, **kwargs):
//!     self.email = self.email.lower().strip()
//!     self.mobile_number = self.mobile_number
//!
//!     if self.token_updated_at is not None:
//!         self.token = uuid.uuid4().hex + uuid.uuid4().hex
//!         self.token_updated_at = timezone.now()
//!
//!     if not self.display_name:
//!         self.display_name = (
//!             self.email.split("@")[0]
//!             if len(self.email.split("@"))
//!             else "".join(random.choice(string.ascii_letters) for _ in range(6))
//!         )
//!
//!     if self.is_superuser:
//!         self.is_staff = True
//!
//!     super(User, self).save(*args, **kwargs)
//! ```
//!
//! Four facts a write path must reproduce, because every one of them is
//! observable in the row and none of them is in the request body:
//!
//! * `email` is lower-cased and stripped on **every** full save, including one
//!   that only changed `avatar_asset_id`. `User.email` is `null=True`
//!   (`user.py:61`), so `self.email.lower()` on a NULL raises
//!   `AttributeError: 'NoneType' object has no attribute 'lower'` — a family
//!   500, reported here as [`NullEmail`] instead of a panic.
//! * a non-NULL `token_updated_at` rotates `token` to **two** `uuid4().hex`
//!   values (64 lower-case hex characters) and moves `token_updated_at` to
//!   `now`, even when the save does not touch either column. The fixture makes
//!   `token_updated_at` non-NULL for every user (`tools/fixtures/build.py`),
//!   so every asset/avatar write rotates `users.token`.
//! * an empty `display_name` is derived from the **normalised** email's local
//!   part. The `else` branch is dead: `self.email.split("@")` always has at
//!   least one element, so `len(...)` is never 0. `User.get_display_name`
//!   (`user.py:189-197`) has the intended `== 2` test, but `save()` does not.
//! * `is_superuser` forces `is_staff`; a `False` `is_superuser` leaves
//!   `is_staff` alone (it does not clear it).
//!
//! `update_fields` is **not** consulted by the hook: Django runs `save()`
//! before it decides which columns to write, so a partial save still rotates
//! the in-memory token (and then omits it from the UPDATE). A caller that
//! passes `update_fields` must therefore keep the rotated value out of the
//! statement, exactly as [`crate::hooks::audit`] documents for H01.

use std::fmt;

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// The five fields H30 rewrites, in `User` column order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// `self.email.lower().strip()`.
    pub email: String,
    /// `self.token`: a new value only when `token_updated_at` was non-NULL.
    pub token: String,
    /// `self.token_updated_at`: `now` on rotation, unchanged otherwise.
    pub token_updated_at: Option<DateTime<Utc>>,
    /// `self.display_name`, possibly derived from the email.
    pub display_name: String,
    /// `self.is_staff`, set to `True` when `is_superuser` is.
    pub is_staff: bool,
}

/// `User.email` is NULL, so `self.email.lower()` raises in Python.
///
/// The message is the `AttributeError` text; a handler maps this to the family
/// 500 the same way every other uncaught exception reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NullEmail;

impl fmt::Display for NullEmail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AttributeError: 'NoneType' object has no attribute 'lower'")
    }
}

impl std::error::Error for NullEmail {}

/// `uuid.uuid4().hex + uuid.uuid4().hex` (`user.py:174`).
///
/// Both halves are 32 lower-case hex characters, so the result is 64 — exactly
/// the `api_token` shape `scenarios/normalize.yaml` verifies. CPython's
/// `uuid.uuid4` reads 16 bytes from `os.urandom`; `Uuid::new_v4` reads from the
/// same OS CSPRNG through `getrandom`.
#[must_use]
pub fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// `User.save()`'s assignments, evaluated before the row is written.
///
/// `now` is `timezone.now()` (the caller's clock; the hook never reads it
/// twice). `token`/`token_updated_at` are the instance's **pre-save** values:
/// Django assigns over them in place, so a second `save()` on the same
/// instance rotates again only if `token_updated_at` was non-NULL to begin
/// with — which it always is after the first rotation.
///
/// # Errors
///
/// [`NullEmail`] when `email` is `None` (Python's `AttributeError`).
pub fn before_save(
    email: Option<&str>,
    display_name: &str,
    is_superuser: bool,
    is_staff: bool,
    token: &str,
    token_updated_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<Prepared, NullEmail> {
    let email = email.ok_or(NullEmail)?;
    // `self.email.lower().strip()`. CPython's `str.strip()` is
    // `str.isspace()`-based, which counts U+001C..U+001F as whitespace on top
    // of the Unicode White_Space set `char::is_whitespace` uses.
    let email = python_strip(&email.to_lowercase()).to_owned();

    let (token, token_updated_at) = if token_updated_at.is_some() {
        (new_token(), Some(now))
    } else {
        (token.to_owned(), None)
    };

    let display_name = if display_name.is_empty() {
        // `self.email.split("@")[0]`; the random branch is unreachable (see
        // the module docs).
        email.split('@').next().unwrap_or("").to_owned()
    } else {
        display_name.to_owned()
    };

    Ok(Prepared {
        email,
        token,
        token_updated_at,
        display_name,
        is_staff: is_staff || is_superuser,
    })
}

/// CPython's `str.strip()` for the character classes `str.isspace()` accepts.
///
/// `char::is_whitespace` is the Unicode `White_Space` property; CPython also
/// treats the four ASCII separator controls U+001C..U+001F as whitespace
/// (`Objects/unicodeobject.c: _PyUnicode_IsWhitespace`).
fn python_strip(value: &str) -> &str {
    value.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '\u{1c}'..='\u{1f}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 123_456).unwrap()
    }

    /// The expected values below are recorded from the real
    /// `plane.db.models.user.User.save` (CPython 3.12.13, the venv) with
    /// `django.db.models.Model.save` patched out, so no row is written.
    #[test]
    fn the_four_assignments_match_user_save() {
        let prepared = before_save(
            Some("  MiXeD@Example.COM "),
            "",
            false,
            false,
            "",
            None,
            at(1_700_000_000),
        )
        .expect("an email is present");
        assert_eq!(prepared.email, "mixed@example.com");
        assert_eq!(prepared.display_name, "mixed");
        assert!(!prepared.is_staff);
        // No rotation: the token columns keep their stored values.
        assert_eq!(prepared.token, "");
        assert_eq!(prepared.token_updated_at, None);
    }

    #[test]
    fn an_existing_display_name_is_kept() {
        let prepared =
            before_save(Some("a@b@c"), "X", false, false, "", None, at(0)).expect("an email");
        // The email is not truncated at the first "@" and `display_name` wins.
        assert_eq!(prepared.email, "a@b@c");
        assert_eq!(prepared.display_name, "X");
    }

    #[test]
    fn an_empty_email_local_part_leaves_display_name_empty() {
        // `"".split("@")[0]` is `""`; the `not self.display_name` branch still
        // runs and assigns the empty string.
        let prepared = before_save(Some(""), "", false, false, "", None, at(0)).expect("email");
        assert_eq!(prepared.email, "");
        assert_eq!(prepared.display_name, "");
        // A one-element split: `len(self.email.split("@"))` is 1, truthy.
        let prepared = before_save(Some("solo"), "", false, false, "", None, at(0)).expect("email");
        assert_eq!(prepared.display_name, "solo");
    }

    #[test]
    fn is_superuser_forces_is_staff_and_does_not_clear_it() {
        let promoted = before_save(Some("a@b"), "N", true, false, "", None, at(0)).expect("email");
        assert!(promoted.is_staff);
        let kept = before_save(Some("a@b"), "N", false, true, "", None, at(0)).expect("email");
        assert!(kept.is_staff);
        let neither = before_save(Some("a@b"), "N", false, false, "", None, at(0)).expect("email");
        assert!(!neither.is_staff);
    }

    #[test]
    fn a_null_email_is_the_python_attribute_error() {
        let error = before_save(None, "N", false, false, "", None, at(0)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "AttributeError: 'NoneType' object has no attribute 'lower'"
        );
    }

    #[test]
    fn a_token_updated_at_rotates_the_token_to_two_uuid4_hexes() {
        let now = at(1_700_000_123);
        let prepared = before_save(
            Some("a@b"),
            "N",
            false,
            false,
            "old-token",
            Some(at(1_600_000_000)),
            now,
        )
        .expect("email");
        assert_eq!(prepared.token.len(), 64);
        assert!(
            prepared
                .token
                .chars()
                .all(|character| matches!(character, '0'..='9' | 'a'..='f')),
            "not lower-case hex: {}",
            prepared.token
        );
        // `timezone.now()`, the caller's clock, not `token_updated_at + Δ`.
        assert_eq!(prepared.token_updated_at, Some(now));
    }

    #[test]
    fn each_rotation_is_independent() {
        // `uuid.uuid4().hex + uuid.uuid4().hex`: two draws, and two calls never
        // collide in practice. This pins that the generator is not a constant.
        let first = new_token();
        let second = new_token();
        assert_eq!(first.len(), 64);
        assert_eq!(second.len(), 64);
        assert_ne!(first, second);
    }

    #[test]
    fn strip_matches_pythons_isspace_classes() {
        // U+001C..U+001F are whitespace for CPython but not `char::is_whitespace`.
        assert_eq!(
            before_save(Some("\u{1c} a@b \u{1f}"), "", false, false, "", None, at(0))
                .expect("email")
                .email,
            "a@b"
        );
        // NBSP and ideographic space are in both tables.
        assert_eq!(
            before_save(Some("\u{a0}a@b\u{3000}"), "", false, false, "", None, at(0))
                .expect("email")
                .email,
            "a@b"
        );
        // A zero-width space is not whitespace in either.
        assert_eq!(
            before_save(Some("\u{200b}a@b"), "", false, false, "", None, at(0))
                .expect("email")
                .email,
            "\u{200b}a@b"
        );
    }

    #[test]
    fn lowercasing_is_python_unicode_lower() {
        // `str.lower()` is full Unicode lowercasing, not ASCII-only.
        assert_eq!(
            before_save(Some("CAFÉ@EXAMPLE.COM"), "", false, false, "", None, at(0))
                .expect("email")
                .email,
            "café@example.com"
        );
    }
}
