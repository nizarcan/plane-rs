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
//! Python uses `zxcvbn==4.4.28` (`apps/api/pyproject.toml:67`). The Rust
//! `zxcvbn` 3.1.1 crate is an independent port whose `zxcvbn()` silently
//! truncates the password to its first **100 characters**
//! (`zxcvbn-3.1.1/src/lib.rs:156-158`, "This prevents potential DoS attacks").
//! Plane's window is 128, so the crate cannot answer for a window longer than
//! 100.
//!
//! `tools/auth/zxcvbn_corpus.json` records 546 passwords (1 092 windows) from
//! the real Python:
//!
//! | window length | windows | score differences |
//! | --- | --- | --- |
//! | ≤ 100 | 830 | **0** |
//! | > 100 | 262 | **33** |
//!
//! Every difference is a window longer than the crate's cap; at least one
//! password crosses the `score >= 3` boundary (3 of 546), so the crate is not
//! a drop-in replacement for the policy. `tests/zxcvbn_corpus.rs` pins both
//! counts.
//!
//! Until the matcher set is ported, [`is_password_strong`] answers with the
//! crate **only where it is proven identical** (both windows ≤ 100 characters)
//! and returns [`PasswordStrengthError::NotPorted`] otherwise, so a long
//! password is a 500 rather than a weaker policy. The orchestrator decides
//! whether to port `zxcvbn/matching.py` + `scoring.py` (~1 000 lines plus the
//! frequency lists) or to accept a different policy for long passwords.

/// `ZXCVBN_MAX_LENGTH` (`password.py:11`).
pub const ZXCVBN_MAX_LENGTH: usize = 128;
/// The length the Rust crate scores (`zxcvbn-3.1.1/src/lib.rs:156-158`).
pub const CRATE_MAX_LENGTH: usize = 100;
/// The minimum score `is_password_strong` accepts.
pub const MIN_SCORE: u8 = 3;

/// `is_password_strong` could not be answered faithfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PasswordStrengthError {
    /// A window is longer than the crate's 100-character cap. This is the
    /// deliberate not-implemented path: the Python matcher set has not been
    /// ported, and the crate's answer is known to differ for these windows
    /// (33 of 262 in the recorded corpus).
    #[error(
        "password strength is not ported for a {length}-character window: the Rust zxcvbn crate \
         scores only the first 100 characters and differs from Python zxcvbn 4.4.28 beyond that"
    )]
    NotPorted {
        /// The longer window's length, in code points.
        length: usize,
    },
}

/// `is_password_strong(password)` (`password.py:8-15`).
///
/// The password is scored twice: its first 128 characters and its last 128.
/// For a string at most 100 characters long both windows are the whole string
/// and the crate matches Python on every recorded case; longer input needs the
/// unported matchers.
///
/// # Errors
///
/// [`PasswordStrengthError::NotPorted`] when either window exceeds 100
/// characters. A caller maps it to the family's 500, exactly as an unexpected
/// exception in Python's `zxcvbn` would.
pub fn is_password_strong(password: &str) -> Result<bool, PasswordStrengthError> {
    let characters: Vec<char> = password.chars().collect();
    let first: String = characters.iter().take(ZXCVBN_MAX_LENGTH).collect();
    let last: String = characters
        .iter()
        .skip(characters.len().saturating_sub(ZXCVBN_MAX_LENGTH))
        .collect();
    let longest = first.chars().count().max(last.chars().count());
    if longest > CRATE_MAX_LENGTH {
        return Err(PasswordStrengthError::NotPorted { length: longest });
    }
    let strong = |window: &str| zxcvbn::zxcvbn(window, &[]).score() as u8 >= MIN_SCORE;
    Ok(strong(&first) || strong(&last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strong_short_password_is_accepted() {
        assert_eq!(is_password_strong("Tr0ub4dor&3xY9!qZ"), Ok(true));
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
            assert_eq!(is_password_strong(password), Ok(false), "{password:?}");
        }
    }

    #[test]
    fn a_window_over_the_crates_cap_is_not_ported() {
        let error = is_password_strong(&"a".repeat(101)).expect_err("over 100");
        assert_eq!(error, PasswordStrengthError::NotPorted { length: 101 });
        // 128 characters is Plane's own cap, still over the crate's.
        assert!(is_password_strong(&"a".repeat(128)).is_err());
        // Exactly 100 is inside the proven region.
        assert!(is_password_strong(&"a".repeat(100)).is_ok());
    }

    #[test]
    fn the_windows_are_the_first_and_last_128_code_points() {
        // A 200-character password: both windows are 128, so the answer is the
        // not-implemented path regardless of the characters.
        assert_eq!(
            is_password_strong(&format!("{}x", "a".repeat(199))),
            Err(PasswordStrengthError::NotPorted { length: 128 })
        );
        // Unicode: `len` is code points, not bytes.
        assert!(is_password_strong(&"😀".repeat(100)).is_ok());
        assert!(is_password_strong(&"😀".repeat(101)).is_err());
    }

    #[test]
    fn the_error_mentions_the_crate_cap_without_a_placeholder() {
        let message = PasswordStrengthError::NotPorted { length: 128 }.to_string();
        assert!(message.contains("100 characters"));
        assert!(!message.to_lowercase().contains("todo"));
    }
}
