// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python's random-string and comparison helpers, ported.
//!
//! `django.utils.crypto.get_random_string` is
//! `"".join(secrets.choice(allowed_chars) for _ in range(length))`
//! (`django/utils/crypto.py:47-58`). `secrets.choice` uses
//! `_randbelow(len(alphabet))`, so every character is uniform; the rejection
//! sampling below has the same distribution (a naive `byte % 62` would not).

/// `RANDOM_STRING_CHARS` (`django/utils/crypto.py:45`).
pub const RANDOM_STRING_CHARS: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// `VALID_KEY_CHARS` (`django/contrib/sessions/backends/base.py:15` and
/// `plane/db/models/session.py:13`).
pub const VALID_KEY_CHARS: &str = "abcdefghijklmnopqrstuvwxyz0123456789";
/// `CSRF_ALLOWED_CHARS` (`django/middleware/csrf.py:47`).
pub const CSRF_ALLOWED_CHARS: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// `get_random_string(length, allowed_chars)`: uniformly random characters.
///
/// Panics only on an impossible OS entropy failure or an empty alphabet — both
/// are programming errors, not request-path conditions.
pub fn get_random_string(length: usize, allowed_chars: &str) -> String {
    let alphabet: Vec<char> = allowed_chars.chars().collect();
    assert!(
        !alphabet.is_empty(),
        "get_random_string needs a non-empty alphabet"
    );
    let count = alphabet.len();
    // The largest multiple of `count` that fits in a byte. Bytes at or above it
    // are discarded, which removes the modulo bias.
    let limit = 256 - (256 % count);

    let mut out = String::with_capacity(length);
    let mut buffer = [0u8; 64];
    while out.len() < length {
        if let Err(error) = getrandom::fill(&mut buffer) {
            panic!("the OS random number generator failed: {error}");
        }
        for byte in buffer {
            if usize::from(byte) >= limit {
                continue;
            }
            out.push(alphabet[usize::from(byte) % count]);
            if out.len() == length {
                break;
            }
        }
    }
    out
}

/// `django.utils.crypto.constant_time_compare`: `secrets.compare_digest`.
pub fn constant_time_compare(left: &[u8], right: &[u8]) -> bool {
    aws_lc_rs::constant_time::verify_slices_are_equal(left, right).is_ok()
}

/// `bytes.hex()`.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn random_strings_have_the_right_shape() {
        let key = get_random_string(128, VALID_KEY_CHARS);
        assert_eq!(key.len(), 128);
        assert!(
            key.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );

        let secret = get_random_string(32, CSRF_ALLOWED_CHARS);
        assert_eq!(secret.len(), 32);
        assert!(secret.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn random_strings_are_uniform_enough() {
        // 62 characters: every one must appear in 40 keys of 128 characters.
        let mut seen = BTreeSet::new();
        for _ in 0..40 {
            seen.extend(get_random_string(128, CSRF_ALLOWED_CHARS).chars());
        }
        assert_eq!(seen.len(), 62);
    }

    #[test]
    fn comparisons_and_hex() {
        assert!(constant_time_compare(b"abc", b"abc"));
        assert!(!constant_time_compare(b"abc", b"abd"));
        assert!(!constant_time_compare(b"abc", b"ab"));
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }
}
