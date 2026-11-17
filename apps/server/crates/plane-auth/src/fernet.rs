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
//! [`encrypt_data`] is the other half (`encryption.py:19-30`), used by
//! `PATCH /api/instances/configurations/`: it stamps `int(time.time())` and 16
//! random bytes of IV into the token, so its output differs on every call.
//! `""` and the empty input are `""` (`if data: … else: return ""`), and an
//! encryption failure (an RNG or cipher error) is also `""` because the Python
//! wraps the whole body in `except Exception`. [`encrypt_data_at`] is the same
//! encryption with the clock and the IV supplied, which is what pins the bytes
//! against `cryptography` in the tests.
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

use aws_lc_rs::cipher::{
    AES_128, DecryptingKey, DecryptionContext, EncryptingKey, EncryptionContext, UnboundCipherKey,
};
use aws_lc_rs::iv::{FixedLength, IV_LEN_128_BIT};
use base64::Engine as _;

/// One AES block, which is also the IV length and the PKCS7 block size.
const BLOCK_LEN: usize = 16;

/// `encrypt_data(data)` (`plane/license/utils/encryption.py:19-30`).
///
/// A fresh IV (`os.urandom(16)`) and `int(time.time())` go into the token, so
/// two calls never agree. `""` is `""`, and so is any failure: the Python's
/// `except Exception: return ""` covers the RNG and the cipher as well as the
/// encoding.
#[must_use]
pub fn encrypt_data(secret_key: &str, data: &str) -> String {
    if data.is_empty() {
        return String::new();
    }
    let mut iv = [0u8; BLOCK_LEN];
    if getrandom::fill(&mut iv).is_err() {
        return String::new();
    }
    let timestamp = chrono::Utc::now().timestamp();
    if timestamp < 0 {
        // `struct.pack(">Q", current_time)` raises for a pre-epoch clock, and
        // the Python's `except Exception` answers `""`.
        return String::new();
    }
    encrypt_token(secret_key, data, timestamp as u64, iv).unwrap_or_default()
}

/// [`encrypt_data`] with the token's timestamp and IV supplied.
///
/// Exists so a test can pin the exact bytes `cryptography.fernet` produces for
/// the same inputs; production code calls [`encrypt_data`]. `""` and every
/// failure are `""`, like the Python.
#[must_use]
pub fn encrypt_data_at(
    secret_key: &str,
    data: &str,
    timestamp: u64,
    iv: [u8; BLOCK_LEN],
) -> String {
    if data.is_empty() {
        return String::new();
    }
    encrypt_token(secret_key, data, timestamp, iv).unwrap_or_default()
}

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

/// `Fernet._encrypt_from_parts` (`cryptography/fernet.py`): PKCS7 pad, encrypt
/// with AES-128-CBC, then HMAC-SHA256 the version, timestamp, IV and ciphertext.
///
/// `None` is an `Unspecified` cipher or RNG error; the callers turn it into
/// `""` the way the Python's `except Exception` does.
fn encrypt_token(
    secret_key: &str,
    data: &str,
    timestamp: u64,
    iv: [u8; BLOCK_LEN],
) -> Option<String> {
    let key = fernet_key(secret_key);
    // `padding.PKCS7(128).padder()`: 1..=16 bytes, a whole block when the
    // plaintext is already aligned.
    let padding = BLOCK_LEN - data.len() % BLOCK_LEN;
    let mut buffer = Vec::with_capacity(data.len() + padding);
    buffer.extend_from_slice(data.as_bytes());
    buffer.extend(std::iter::repeat_n(padding as u8, padding));

    let unbound = UnboundCipherKey::new(&AES_128, &key[BLOCK_LEN..]).ok()?;
    let encrypting = EncryptingKey::cbc(unbound).ok()?;
    encrypting
        .less_safe_encrypt(&mut buffer, EncryptionContext::Iv128(FixedLength::from(iv)))
        .ok()?;

    // `0x80 || timestamp_u64_be || IV || ciphertext`, then the raw HMAC.
    let mut token = Vec::with_capacity(1 + 8 + BLOCK_LEN + buffer.len() + 32);
    token.push(0x80);
    token.extend_from_slice(&timestamp.to_be_bytes());
    token.extend_from_slice(&iv);
    token.extend_from_slice(&buffer);
    let hmac_key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, &key[..BLOCK_LEN]);
    token.extend_from_slice(aws_lc_rs::hmac::sign(&hmac_key, &token).as_ref());

    // `base64.urlsafe_b64encode` keeps the `=` padding.
    Some(base64::engine::general_purpose::URL_SAFE.encode(&token))
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

    /// `encrypt_data` with the vector's own timestamp and IV must reproduce the
    /// token `cryptography` produced — every byte of it, so the PKCS7 padding,
    /// the AES-128-CBC ciphertext, the HMAC and the base64 are all pinned at
    /// once. The timestamp and IV are read out of the recorded token.
    #[test]
    fn fernet_encrypts_the_recorded_vector_byte_for_byte() {
        let raw = base64::engine::general_purpose::URL_SAFE
            .decode(VECTOR_TOKEN)
            .expect("the vector is base64url");
        let timestamp = u64::from_be_bytes(raw[1..9].try_into().expect("eight bytes"));
        let iv: [u8; BLOCK_LEN] = raw[9..25].try_into().expect("sixteen bytes");
        assert_eq!(timestamp, 1_700_000_000, "the vector's timestamp");
        assert_eq!(iv, [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
        assert_eq!(
            encrypt_data_at(VECTOR_SECRET, VECTOR_PLAINTEXT, timestamp, iv),
            VECTOR_TOKEN
        );
    }

    /// Vectors generated with the repository's own `encrypt_data`, with
    /// `cryptography.fernet.time.time` and `os.urandom` patched to
    /// `1700000000` and `bytes(range(16))`:
    ///
    /// ```sh
    /// apps/api/.venv/bin/python - <<'PY'
    /// import sys; sys.path.insert(0, "apps/api")
    /// import django; from django.conf import settings
    /// settings.configure(SECRET_KEY="plane-rs-fernet-vector-not-a-real-secret")
    /// django.setup()
    /// from unittest.mock import patch
    /// from plane.license.utils.encryption import encrypt_data
    /// for text in ["0123456789abcdef", "0123456789abcde", "0123456789abcdefg"]:
    ///     with patch("cryptography.fernet.time.time", return_value=1700000000), \
    ///          patch("cryptography.fernet.os.urandom", return_value=bytes(range(16))):
    ///         print(repr(text), encrypt_data(text))
    /// PY
    /// ```
    ///
    /// They cover the three padding shapes a Fernet plaintext can have: an
    /// exact block (a whole block of padding), one byte short of a block, and
    /// one byte past it.
    #[test]
    fn fernet_padding_matches_cryptography() {
        let iv = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let cases = [
            (
                "0123456789abcdef",
                "gAAAAABlU_EAAAECAwQFBgcICQoLDA0OD0wXkYf8g338ymI4mswjgu0XYrg0JyG3AtU5UU2YuXI-uXhgUw9G-k_S5oGIjrWddq8QokuN9szwr6ScEX5xDAw=",
            ),
            (
                "0123456789abcde",
                "gAAAAABlU_EAAAECAwQFBgcICQoLDA0OD6rsEcRUd4OuV1M5BQr8g1JwxdZh9KDPi9zDwXNzaBZE6eui54GgWNl-w1fhPWfY-A==",
            ),
            (
                "0123456789abcdefg",
                "gAAAAABlU_EAAAECAwQFBgcICQoLDA0OD0wXkYf8g338ymI4mswjgu14_OmQWx8-mt6hDEFPhQ6CHOYwdFMhe1kkHe99ElGqc45VAHJSZq6-i6-6k-erUPg=",
            ),
            (
                "h\u{e9}llo \u{2192} \u{4e16}\u{754c}",
                "gAAAAABlU_EAAAECAwQFBgcICQoLDA0OD-2zUP7S3bnpDKj0-Wl-IHhPXzIM06NEppMKT_Ie_cEEaHsw9NqLOL3GC8PTyFVFfRJrPYr-17epp8fZoH6m5h4=",
            ),
            (
                "x",
                "gAAAAABlU_EAAAECAwQFBgcICQoLDA0OD4ruq2tvoOF7pC4OqC4W713klKY5fe1XeVXfoe0vjLeCbzx8MGhXHfl6w3PmiWnXzw==",
            ),
        ];
        for (plaintext, expected) in cases {
            assert_eq!(
                encrypt_data_at(VECTOR_SECRET, plaintext, 1_700_000_000, iv),
                expected,
                "plaintext {plaintext:?}"
            );
            // And the pairs decrypt back through the same code path.
            assert_eq!(decrypt_data(VECTOR_SECRET, expected), plaintext);
        }
    }

    #[test]
    fn fernet_round_trips_its_own_output() {
        for plaintext in [
            "",
            "a",
            "postgres://plane:plane@plane-db:5432/plane",
            "0123456789abcdef",
            "  padded  ",
            "\u{4e16}\u{754c}",
        ] {
            let token = encrypt_data(VECTOR_SECRET, plaintext);
            if plaintext.is_empty() {
                assert_eq!(token, "", "if data: … else: return \"\"");
                continue;
            }
            assert_eq!(decrypt_data(VECTOR_SECRET, &token), plaintext);
            assert_eq!(decrypt_data("another-secret", &token), "");
        }
    }

    /// Two calls must not agree: `encrypt_data` draws a fresh IV and stamps the
    /// clock, so the ciphertext differs even for the same plaintext and second.
    #[test]
    fn fernet_encryption_is_nondeterministic() {
        let first = encrypt_data(VECTOR_SECRET, VECTOR_PLAINTEXT);
        let second = encrypt_data(VECTOR_SECRET, VECTOR_PLAINTEXT);
        assert_ne!(first, second);
        assert_eq!(decrypt_data(VECTOR_SECRET, &first), VECTOR_PLAINTEXT);
        assert_eq!(decrypt_data(VECTOR_SECRET, &second), VECTOR_PLAINTEXT);
        // A Fernet token: 0x80, then the base64 of 57+ bytes plus a multiple of
        // 16 of ciphertext, with `=` padding.
        assert!(first.starts_with("gAAAAA"), "{first}");
        assert_eq!(first.len() % 4, 0, "{first}");
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
