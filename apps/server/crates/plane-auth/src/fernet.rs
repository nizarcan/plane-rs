// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/utils/encryption.py` — the Fernet scheme Plane uses for the
//! secret values in `instance_configurations`, and nothing else.
//!
//! `get_configuration_value` decrypts an encrypted row on read
//! (`instance_value.py:26-27`), and every caller treats a failure as "not
//! configured" (`ENV.md` §3.5). The scheme, as `cryptography==50.0.0`
//! implements it:
//!
//! ```text
//! derive_key(SECRET_KEY) = PBKDF2-HMAC-SHA256(SECRET_KEY, b"salt", 100_000, 32)
//! token = urlsafe_base64(0x80 || timestamp_u64_be || IV(16) || ciphertext || HMAC(32))
//! ```
//!
//! AES-128-CBC with PKCS7 padding, HMAC-SHA256 over every byte before the MAC,
//! the first 16 key bytes as the MAC key and the last 16 as the AES key.
//! `decrypt()` is called without a TTL, so an old token is accepted.
//!
//! Every failure mode returns `""`: `InvalidToken`, bad base64, non-ASCII,
//! short token, wrong version byte, a padding error, non-UTF-8 plaintext, or a
//! `SECRET_KEY` that did not encrypt it. That is the Python's
//! `except Exception: return ""`, and it is why a deployment with a rotated key
//! reports "not configured" instead of erroring.
//!
//! The derivation is cached: 100 000 PBKDF2 iterations cost milliseconds and
//! the Python pays them on *every* call.
//!
//! `ENV.md` §3.5 (and `tools/inventory/encryption_vector.py`) pins the test
//! vector used by the tests below.
//!
//! Moved here from `plane-http-app::common::fernet` by auth P0c: the generic
//! `get_configuration_value` port ([`crate::instance_config`]) lives in this
//! crate, and it decrypts an encrypted row. `plane-http-app` re-exports this
//! module, so its call sites did not change.

use std::num::NonZeroU32;
use std::sync::OnceLock;

use aws_lc_rs::cipher::{AES_128, DecryptingKey, DecryptionContext, UnboundCipherKey};
use aws_lc_rs::iv::{FixedLength, IV_LEN_128_BIT};
use base64::Engine as _;

/// `decrypt_data(encrypted_data)` (`plane/license/utils/encryption.py:31-39`).
///
/// `None` and `""` are `""`, like every other failure.
#[must_use]
pub fn decrypt_data(secret_key: &str, token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    decrypt_token(secret_key, token).unwrap_or_default()
}

/// One Fernet token; `None` is the Python's `InvalidToken`.
fn decrypt_token(secret_key: &str, token: &str) -> Option<String> {
    // `token.encode("ascii")`, then `base64.urlsafe_b64decode`.
    if !token.is_ascii() {
        return None;
    }
    let raw = base64::engine::general_purpose::URL_SAFE
        .decode(token)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token))
        .ok()?;
    // version(1) + timestamp(8) + IV(16) + at least one block + HMAC(32).
    if raw.len() < 73 || raw[0] != 0x80 {
        return None;
    }
    let body_end = raw.len() - 32;
    let key = fernet_key(secret_key);

    let hmac_key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, &key[..16]);
    aws_lc_rs::hmac::verify(&hmac_key, &raw[..body_end], &raw[body_end..]).ok()?;

    let iv_bytes: [u8; IV_LEN_128_BIT] = raw.get(9..25)?.try_into().ok()?;
    let ciphertext = raw.get(25..body_end)?;
    let unbound = UnboundCipherKey::new(&AES_128, &key[16..]).ok()?;
    let decrypting = DecryptingKey::cbc(unbound).ok()?;
    let mut buffer = ciphertext.to_vec();
    let plain = decrypting
        .decrypt(
            &mut buffer,
            DecryptionContext::Iv128(FixedLength::from(iv_bytes)),
        )
        .ok()?;

    // PKCS7: every padding byte repeats the length, 1..=16.
    let padding = *plain.last()? as usize;
    if padding == 0 || padding > 16 || padding > plain.len() {
        return None;
    }
    if !plain[plain.len() - padding..]
        .iter()
        .all(|&byte| byte as usize == padding)
    {
        return None;
    }
    String::from_utf8(plain[..plain.len() - padding].to_vec()).ok()
}

/// `derive_key(SECRET_KEY)` (`plane/license/utils/encryption.py:14-17`), cached
/// per process.
fn fernet_key(secret_key: &str) -> [u8; 32] {
    static KEY: OnceLock<([u8; 32], String)> = OnceLock::new();
    let (key, stored) = KEY.get_or_init(|| (derive_key(secret_key), secret_key.to_string()));
    if stored == secret_key {
        *key
    } else {
        // A process has one SECRET_KEY; a different one means a test or a
        // misconfiguration, and its key must not be confused with the cached one.
        derive_key(secret_key)
    }
}

/// `PBKDF2-HMAC-SHA256(SECRET_KEY, b"salt", 100_000, 32)`
/// (`plane/license/utils/encryption.py:14-17`).
fn derive_key(secret_key: &str) -> [u8; 32] {
    const ITERATIONS: NonZeroU32 = match NonZeroU32::new(100_000) {
        Some(iterations) => iterations,
        None => panic!("100000 is not zero"),
    };
    let mut derived = [0u8; 32];
    aws_lc_rs::pbkdf2::derive(
        aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA256,
        ITERATIONS,
        b"salt",
        secret_key.as_bytes(),
        &mut derived,
    );
    derived
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The deterministic token from `ENV.md` §3.5, produced by
    /// `tools/inventory/encryption_vector.py` with `time.time` and
    /// `os.urandom` patched, and verified against the repository's own
    /// `decrypt_data` before it was pinned here:
    ///
    /// ```sh
    /// SECRET_KEY=plane-rs-fernet-vector-not-a-real-secret \
    ///   DJANGO_SETTINGS_MODULE=plane.settings.production \
    ///   apps/api/.venv/bin/python -c 'import django; django.setup();
    /// from plane.license.utils.encryption import decrypt_data;
    /// print(repr(decrypt_data(TOKEN)))'
    /// ```
    const VECTOR_SECRET: &str = "plane-rs-fernet-vector-not-a-real-secret";
    const VECTOR_PLAINTEXT: &str = "postgres://plane:plane@plane-db:5432/plane";
    const VECTOR_TOKEN: &str = "gAAAAABlU_EAAAECAwQFBgcICQoLDA0OD_SnVf1mrlHhHb4c5e-aA8GMdLq0PUT_Pk9CZUuJwCqdkAkvnySvjAzQmocDM0BPwT_NuJmtB5ybzGqAEwvR6aTjQMEBapMa1RrzA6bVtWdw";

    #[test]
    fn fernet_decrypts_the_recorded_vector() {
        assert_eq!(decrypt_data(VECTOR_SECRET, VECTOR_TOKEN), VECTOR_PLAINTEXT);
    }

    #[test]
    fn fernet_failures_degrade_to_an_empty_string() {
        // `except Exception: return ""` (`encryption.py:37-39`).
        assert_eq!(decrypt_data(VECTOR_SECRET, ""), "");
        assert_eq!(decrypt_data(VECTOR_SECRET, "not-a-token"), "");
        assert_eq!(
            decrypt_data(VECTOR_SECRET, &VECTOR_TOKEN[..VECTOR_TOKEN.len() - 4]),
            "",
            "a truncated token is an InvalidToken in Python"
        );
        assert_eq!(
            decrypt_data("another-secret", VECTOR_TOKEN),
            "",
            "a different SECRET_KEY must not decrypt"
        );
        assert_eq!(decrypt_data(VECTOR_SECRET, "gAAAAE=\u{e9}"), "");
        // A valid base64 string with the wrong version byte.
        assert_eq!(decrypt_data(VECTOR_SECRET, "gQAAAAA"), "");
    }
}
