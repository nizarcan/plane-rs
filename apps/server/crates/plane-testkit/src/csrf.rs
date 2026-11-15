// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's CSRF masked-token scheme.
//!
//! The cookie stores a 32-character secret; the value handed to the client (in
//! the JSON body of `/auth/get-csrf-token/` or in a form field) is a 64-character
//! *masked* token: a random 32-character mask followed by the secret shifted by
//! the mask, character by character, over
//! `abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789`
//! (`py:django/middleware/csrf.py:44-81`).
//!
//! The harness needs both directions: unmask to check that a target's token
//! belongs to that target's cookie, and mask to reproduce Django's own test
//! vector.

/// `CSRF_SECRET_LENGTH`.
pub const SECRET_LENGTH: usize = 32;
/// `CSRF_TOKEN_LENGTH`.
pub const TOKEN_LENGTH: usize = 2 * SECRET_LENGTH;
/// `CSRF_ALLOWED_CHARS`.
pub const ALLOWED_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn index_of(byte: u8) -> Option<usize> {
    ALLOWED_CHARS.bytes().position(|allowed| allowed == byte)
}

/// True when `value` has exactly `length` characters and all of them are in
/// Django's CSRF alphabet.
pub fn is_token_shaped(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| index_of(byte).is_some())
}

/// Django's `_unmask_cipher_token`: `mask + cipher` becomes `secret`.
///
/// Returns `None` for a token that is not exactly 64 allowed characters, which
/// is what `_check_token_format` rejects before unmasking.
pub fn unmask(token: &str) -> Option<String> {
    if !is_token_shaped(token, TOKEN_LENGTH) {
        return None;
    }
    let bytes = token.as_bytes();
    let (mask, cipher) = bytes.split_at(SECRET_LENGTH);
    let mut secret = String::with_capacity(SECRET_LENGTH);
    for (mask_byte, cipher_byte) in mask.iter().zip(cipher.iter()) {
        let mask_index = index_of(*mask_byte)?;
        let cipher_index = index_of(*cipher_byte)?;
        // Python's `CHARS[index]` with a possibly negative index wraps around,
        // which is what `(cipher - mask).rem_euclid(len)` implements.
        let secret_index = (cipher_index + ALLOWED_CHARS.len() - mask_index) % ALLOWED_CHARS.len();
        secret.push(ALLOWED_CHARS.as_bytes()[secret_index] as char);
    }
    Some(secret)
}

/// Mask `secret` with `mask`, Django's `_mask_cipher_secret`.
///
/// Returns `None` unless both inputs are 32 allowed characters.
pub fn mask(secret: &str, mask: &str) -> Option<String> {
    if !is_token_shaped(secret, SECRET_LENGTH) || !is_token_shaped(mask, SECRET_LENGTH) {
        return None;
    }
    let mut token = String::with_capacity(TOKEN_LENGTH);
    token.push_str(mask);
    for (secret_byte, mask_byte) in secret.bytes().zip(mask.bytes()) {
        let secret_index = index_of(secret_byte)?;
        let mask_index = index_of(mask_byte)?;
        let cipher_index = (secret_index + mask_index) % ALLOWED_CHARS.len();
        token.push(ALLOWED_CHARS.as_bytes()[cipher_index] as char);
    }
    Some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HTTP_CONTRACT.md Appendix B.2, vector 1 (deterministic).
    const SECRET: &str = "0123456789abcdefghijklmnopqrstuv";
    const MASK: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef";
    const MASKED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA";

    #[test]
    fn appendix_b2_vector_1_round_trips() {
        assert_eq!(mask(SECRET, MASK).as_deref(), Some(MASKED));
        assert_eq!(unmask(MASKED).as_deref(), Some(SECRET));
    }

    #[test]
    fn appendix_b2_vector_2_round_trips() {
        let secret = "CTIGYblqvShV8RkLZ7bt4ZaWAiFZYPoF";
        let masked = "auJMQXMEZbVPi6NmJTFdzXZ62SxwRmpZCdhiEYXUkT2AgNXXyQGwtMZSs02lF1Du";
        assert_eq!(unmask(masked).as_deref(), Some(secret));
    }

    #[test]
    fn an_unmasked_secret_is_not_a_masked_token() {
        assert_eq!(unmask(SECRET), None, "32 characters is too short");
        // A 32-character value is accepted by Django as a token directly, but
        // it is a secret, not a masked token, and must not unmask.
        assert_eq!(unmask("CTIGYblqvShV8RkLZ7bt4ZaWAiFZYPoF"), None);
    }

    #[test]
    fn bad_lengths_and_characters_are_rejected() {
        assert_eq!(unmask(""), None);
        assert_eq!(unmask(&MASKED[..63]), None);
        assert_eq!(unmask(&format!("{MASKED}_")), None);
        assert_eq!(unmask(&MASKED.replace('A', "-")), None);
        assert_eq!(mask(SECRET, "short"), None);
        assert_eq!(mask("short", MASK), None);
    }

    #[test]
    fn masking_is_not_identity() {
        let other = mask(SECRET, MASK).expect("token");
        assert_ne!(other, SECRET);
        assert_eq!(unmask(&other).as_deref(), Some(SECRET));
    }
}
