// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/utils/password.py` — `is_password_strong`.
//!
//! ```python
//! ZXCVBN_MAX_LENGTH = 128
//!
//! def is_password_strong(password):
//!     windows = {password[:ZXCVBN_MAX_LENGTH], password[-ZXCVBN_MAX_LENGTH:]}
//!     return any(zxcvbn(window)["score"] >= 3 for window in windows)
//! ```
//!
//! Python uses `zxcvbn==4.4.28` (`apps/api/pyproject.toml:67`), which scores
//! the whole string. The Rust `zxcvbn` crate is an independent port that
//! differed from Python in two ways:
//!
//! 1. `zxcvbn()` silently truncated input to its first **100 characters**
//!    (`zxcvbn-3.1.1/src/lib.rs:156-158`), while Plane's window is 128.
//! 2. its `l33t_match` kept one-character matches; Python drops them
//!    (`zxcvbn/matching.py:244`), which changed the base guesses of repeats of
//!    `"1"` (5 instead of 12) and so the score of `"1" * 128` (0 instead
//!    of 1).
//!
//! The crate is vendored at `apps/server/vendor/zxcvbn/` with exactly those
//! two changes (`VENDORED.md`). `tools/auth/zxcvbn_corpus.json` records 742
//! passwords — 1 484 first/last windows, 654 of them longer than 100
//! characters — from the real Python; `tests/zxcvbn_corpus.rs` requires every
//! window's **score** to match, not only the `>= 3` answer.
//!
//! zxcvbn's cost is not linear: a 128-character random-printable password
//! takes about 0.6 s in a release build (Python took 0.4 s to 2.8 s for
//! different 128-character random strings on the same machine), because every
//! substring is looked up in every dictionary. [`is_password_strong`] is
//! therefore also exposed as [`is_password_strong_async`], which runs the
//! scoring on `spawn_blocking`; request paths use the async form.

use tokio::task::JoinError;

/// `ZXCVBN_MAX_LENGTH` (`password.py:11`).
pub const ZXCVBN_MAX_LENGTH: usize = 128;
/// The minimum score `is_password_strong` accepts.
pub const MIN_SCORE: u8 = 3;

/// The first and last [`ZXCVBN_MAX_LENGTH`] code points of `password`.
///
/// Python slices the string twice; for input of at most 128 characters both
/// windows are the whole password (compared twice by Python's set, once here).
fn windows(password: &str) -> (String, String) {
    if password.chars().count() <= ZXCVBN_MAX_LENGTH {
        let whole = password.to_string();
        return (whole.clone(), whole);
    }
    let first: String = password.chars().take(ZXCVBN_MAX_LENGTH).collect();
    // Python's `password[-128:]` starts at most 128 code points from the end;
    // walk back only that far instead of collecting the whole password.
    let start = password
        .char_indices()
        .rev()
        .nth(ZXCVBN_MAX_LENGTH - 1)
        .map_or(0, |(index, _)| index);
    (first, password[start..].to_string())
}

/// `zxcvbn(window)["score"]` for one of Plane's windows.
fn score(window: &str) -> u8 {
    zxcvbn::zxcvbn(window, &[]).score() as u8
}

fn is_strong_windows(first: &str, last: &str) -> bool {
    // Python puts the two windows in a set, so for a password of at most 128
    // characters (`first == last`) it scores once, and `any()` short-circuits.
    score(first) >= MIN_SCORE || (first != last && score(last) >= MIN_SCORE)
}

/// `is_password_strong(password)` (`password.py:8-15`).
///
/// The password's first and last 128 characters are scored; when the password
/// is at most 128 characters long both windows are the whole string and it is
/// scored once, as Python's set does.
///
/// This blocks for the duration of zxcvbn (up to ~0.6 s for a
/// 128-character random-printable password); request paths should use
/// [`is_password_strong_async`].
pub fn is_password_strong(password: &str) -> bool {
    let (first, last) = windows(password);
    is_strong_windows(&first, &last)
}

/// [`is_password_strong`] on the blocking pool, for async request paths.
///
/// Only the two 128-character windows are moved to the worker, never the whole
/// password.
///
/// # Errors
///
/// The [`JoinError`] of the blocking task (a panic inside zxcvbn). Python would
/// let the panic escape as a 500; callers map it to their internal error.
pub async fn is_password_strong_async(password: &str) -> Result<bool, JoinError> {
    let (first, last) = windows(password);
    tokio::task::spawn_blocking(move || is_strong_windows(&first, &last)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strong_short_password_is_accepted() {
        assert!(is_password_strong("Tr0ub4dor&3xY9!qZ"));
    }

    #[test]
    fn weak_passwords_are_rejected() {
        for password in [
            "password",
            "Password1",
            "123456",
            "qwerty",
            "aaaaaaaa",
            "plane123",
        ] {
            assert!(!is_password_strong(password), "{password:?}");
        }
    }

    #[test]
    fn a_window_over_128_characters_is_scored_at_the_window_length() {
        // Exactly Plane's cap is scored whole; Python gives 128 ones score 1
        // (1 537 guesses), the one-character l33t bug used to make it 0.
        assert!(!is_password_strong(&"1".repeat(128)));
        assert!(!is_password_strong(&"a".repeat(128)));
        // 129 characters: both windows are 128, still answered.
        assert!(!is_password_strong(&"a".repeat(129)));
    }

    #[test]
    fn the_windows_are_the_first_and_last_128_code_points() {
        // Unicode: `len` is code points, not bytes.
        assert!(!is_password_strong(&"😀".repeat(128)));
        // Only the last 128 characters are strong: Python scores the first
        // window 1 and the last window 3 for this recorded password.
        let tail_strong = format!("{}xY9!xY9!xY9!xY9!xY9!xY9!xY9!xY9!", "a".repeat(200));
        assert!(is_password_strong(&tail_strong));
        // ... and the other way round.
        let head_strong = format!("xY9!xY9!xY9!xY9!xY9!xY9!xY9!xY9!{}", "a".repeat(200));
        assert!(is_password_strong(&head_strong));
    }

    #[tokio::test]
    async fn the_async_form_agrees_with_the_sync_form() {
        let passwords = [
            "Tr0ub4dor&3xY9!qZ".to_string(),
            "password".to_string(),
            "a".repeat(128),
            "1".repeat(128),
            format!("{}xY9!xY9!", "a".repeat(200)),
        ];
        for password in passwords {
            let actual = is_password_strong_async(&password)
                .await
                .expect("zxcvbn did not panic");
            assert_eq!(actual, is_password_strong(&password), "{password:?}");
        }
    }
}
