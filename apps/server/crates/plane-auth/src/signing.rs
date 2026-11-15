// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `django.core.signing`, as the session backend uses it.
//!
//! `SessionBase.encode` is
//! `signing.dumps(data, salt=key_salt, serializer=JSONSerializer, compress=True)`
//! (`django/contrib/sessions/backends/base.py:122-129`), where `key_salt` is
//! `"django.contrib.sessions." + type(self).__qualname__` → **`django.contrib
//! .sessions.SessionStore`** (HTTP_CONTRACT §5.3, Appendix B.1).
//!
//! The layout (`django/core/signing.py:201-301`):
//!
//! ```text
//! data      = json.dumps(dict, separators=(",", ":")).encode("latin-1")
//! z         = zlib.compress(data); use it iff len(z) < len(data) - 1
//! b64       = urlsafe_b64encode(payload).rstrip(b"=")
//! b64       = "." + b64 if compressed else b64
//! timestamp = b62_encode(int(time.time()))
//! value     = b64 + ":" + timestamp
//! signature = urlsafe_b64encode(HMAC-SHA256(sha256(salt + "signer" + SECRET_KEY),
//!                                          value)).rstrip(b"=")
//! encoded   = value + ":" + signature
//! ```
//!
//! `salted_hmac` keys the HMAC with `sha256(key_salt_bytes + secret_bytes)`
//! (`django/utils/crypto.py:19-45`), which is why the document's literal
//! `"django.contrib.sessions.SessionStoresigner"` appears in the derivation.

use std::io::Write;

use aws_lc_rs::{digest, hmac};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::json::{self, Json};
use crate::random::constant_time_compare;

/// `SessionStore.key_salt` (`django/contrib/sessions/backends/base.py:67-69`).
pub const SESSION_KEY_SALT: &str = "django.contrib.sessions.SessionStore";

/// `Signer.salt` for a signer created with `salt=…`: the salt itself.
///
/// `salted_hmac` appends `"signer"` to it (`django/core/signing.py:225`).
fn signer_salt(salt: &str) -> String {
    format!("{salt}signer")
}

/// `salted_hmac(key_salt, value, secret, algorithm="sha256").digest()`.
pub fn salted_hmac_sha256(key_salt: &str, value: &[u8], secret: &str) -> Vec<u8> {
    let mut material = Vec::with_capacity(key_salt.len() + secret.len());
    material.extend_from_slice(key_salt.as_bytes());
    material.extend_from_slice(secret.as_bytes());
    let derived = digest::digest(&digest::SHA256, &material);
    let key = hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref());
    hmac::sign(&key, value).as_ref().to_vec()
}

/// `django.utils.crypto.salted_hmac(...).hexdigest()`.
pub fn salted_hmac_hexdigest(key_salt: &str, value: &[u8], secret: &str) -> String {
    crate::random::hex(&salted_hmac_sha256(key_salt, value, secret))
}

/// `signing.b64_encode`: URL-safe base64 without padding.
pub fn b64_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// `signing.b64_decode`: re-pad, then URL-safe base64 decode.
pub fn b64_decode(text: &str) -> Result<Vec<u8>, SigningError> {
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| SigningError::Invalid)
}

/// `signing.b62_encode`: base-62 with the alphabet `0-9A-Za-z`, no padding.
pub fn b62_encode(mut value: u64) -> String {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut encoded = Vec::new();
    while value > 0 {
        encoded.push(ALPHABET[(value % 62) as usize]);
        value /= 62;
    }
    encoded.reverse();
    String::from_utf8(encoded).unwrap_or_default()
}

/// `signing.b62_decode`. Django raises `ValueError` for an unknown character;
/// the session backend treats that as a corrupt payload.
pub fn b62_decode(text: &str) -> Result<i64, SigningError> {
    if text == "0" {
        return Ok(0);
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits.is_empty() {
        return Err(SigningError::Invalid);
    }
    let mut decoded: i64 = 0;
    for character in digits.chars() {
        let digit = match character {
            '0'..='9' => i64::from(character as u8 - b'0'),
            'A'..='Z' => i64::from(character as u8 - b'A') + 10,
            'a'..='z' => i64::from(character as u8 - b'a') + 36,
            _ => return Err(SigningError::Invalid),
        };
        decoded = decoded
            .checked_mul(62)
            .and_then(|value| value.checked_add(digit))
            .ok_or(SigningError::Invalid)?;
    }
    Ok(if negative { -decoded } else { decoded })
}

/// Why a signed value was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SigningError {
    /// The signature does not match (`signing.BadSignature`). The session
    /// backend logs `"Session data corrupted"` for exactly this case.
    #[error("signature does not match")]
    BadSignature,
    /// Structurally invalid (no separator, bad base64, bad JSON, …). Django's
    /// `SessionStore.decode` swallows these silently.
    #[error("invalid signed value")]
    Invalid,
}

/// `signing.dumps(obj, salt, serializer=JSONSerializer, compress=True)`.
///
/// `timestamp` is `int(time.time())`, passed in so tests can pin it
/// (Appendix B.1 uses 1777777777).
pub fn dumps(
    value: &Json,
    salt: &str,
    secret_key: &str,
    timestamp: i64,
) -> Result<String, SigningError> {
    let data = json::to_compact_string(value).into_bytes();
    let mut compressed = false;
    let mut payload = data.clone();
    if data.len() > 1 {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&data)
            .map_err(|_| SigningError::Invalid)?;
        let z = encoder.finish().map_err(|_| SigningError::Invalid)?;
        // `if len(compressed) < (len(data) - 1)`.
        if z.len() < data.len() - 1 {
            payload = z;
            compressed = true;
        }
    }
    let encoded = b64_encode(&payload);
    let encoded = if compressed {
        format!(".{encoded}")
    } else {
        encoded
    };
    let value = format!("{encoded}:{}", b62_encode(timestamp as u64));
    let signature = b64_encode(&salted_hmac_sha256(
        &signer_salt(salt),
        value.as_bytes(),
        secret_key,
    ));
    Ok(format!("{value}:{signature}"))
}

/// `signing.loads(s, salt, serializer=JSONSerializer)` with no `max_age` and
/// `fallback_keys` from the deployment.
///
/// A `SECRET_KEY_FALLBACKS` entry that verifies is accepted; Plane leaves the
/// list empty (`django/conf/global_settings.py`, not overridden in
/// `plane/settings`).
pub fn loads(
    signed: &str,
    salt: &str,
    secret_key: &str,
    fallback_keys: &[String],
) -> Result<Json, SigningError> {
    let (value, signature) = signed.rsplit_once(':').ok_or(SigningError::BadSignature)?;
    let expected = salted_hmac_sha256(&signer_salt(salt), value.as_bytes(), secret_key);
    let expected = b64_encode(&expected);
    let mut verified = constant_time_compare(signature.as_bytes(), expected.as_bytes());
    if !verified {
        for fallback in fallback_keys {
            let candidate = b64_encode(&salted_hmac_sha256(
                &signer_salt(salt),
                value.as_bytes(),
                fallback,
            ));
            if constant_time_compare(signature.as_bytes(), candidate.as_bytes()) {
                verified = true;
                break;
            }
        }
    }
    if !verified {
        return Err(SigningError::BadSignature);
    }

    // TimestampSigner.unsign: split the timestamp off and decode it. `max_age`
    // is not passed by the session backend, so the timestamp is not checked.
    let (payload, timestamp) = value.rsplit_once(':').ok_or(SigningError::Invalid)?;
    b62_decode(timestamp)?;

    let (compressed, encoded) = match payload.strip_prefix('.') {
        Some(rest) => (true, rest),
        None => (false, payload),
    };
    let mut data = b64_decode(encoded)?;
    if compressed {
        let mut decoder = flate2::read::ZlibDecoder::new(data.as_slice());
        let mut decompressed = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut decompressed)
            .map_err(|_| SigningError::Invalid)?;
        data = decompressed;
    }
    let text = std::str::from_utf8(&data).map_err(|_| SigningError::Invalid)?;
    json::parse(text).map_err(|_| SigningError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::Json;

    fn object(entries: &[(&str, Json)]) -> Json {
        Json::Object(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
        )
    }

    // HTTP_CONTRACT Appendix B.1, the exact vector.
    const SECRET: &str = "contract-test-secret-key-not-for-deployment";
    const ENCODED_FULL: &str = ".eJxNjstuwyAQRX8lYtVKNgbj97L7fIM1MENM64JlSNU2yr8XR1l0O_fec-bGZrimZb5G2meHbGLCSqpRmbLRLZTd2A_l0I9dCa1ujMKapBWsYBrMB_ljgO_gL4Gb4NPuND9w_JlGfg5I69uzW_yXLRCXvB5tbRstUZEwA_S6w5Yaq6DW0ggcabA9dLo1TW7UVoLQoxmwp8620GhlDmikGF3wM31vbv9hk-qEKBjSlzM0O28Dm27sIYUL-ZSt5_Dr1hWqlovTy-NzMOm07UHTaya6bQbEPWNztxaKCy6l4n2OMHyC8_m8pLRNVbUGA-sSYqrY_f4HL9RrZQ:1wJND7:ywTmmvOLHYvBCPzyrQ_HeauuOal3DUrNa43jfPUyJbQ";

    fn appendix_dict() -> Json {
        object(&[
            (
                "_auth_user_id",
                Json::Str("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0".into()),
            ),
            (
                "backend",
                Json::Str("django.contrib.auth.backends.ModelBackend".into()),
            ),
            (
                "_auth_user_hash",
                Json::Str(
                    "9f2f4b1d3e0c8a7b6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c".into(),
                ),
            ),
            ("_session_expiry", Json::Int(3600)),
            (
                "device_info",
                object(&[
                    (
                        "user_agent",
                        Json::Str("Mozilla/5.0 (contract probe)".into()),
                    ),
                    ("ip_address", Json::Str("203.0.113.7".into())),
                    ("domain", Json::Str("http://localhost/".into())),
                ]),
            ),
        ])
    }

    #[test]
    fn appendix_b1_full_vector() {
        assert_eq!(
            dumps(&appendix_dict(), SESSION_KEY_SALT, SECRET, 1_777_777_777).expect("dumps"),
            ENCODED_FULL
        );
        assert_eq!(
            loads(ENCODED_FULL, SESSION_KEY_SALT, SECRET, &[]).expect("loads"),
            appendix_dict()
        );
    }

    #[test]
    fn appendix_b1_small_and_non_ascii_vectors() {
        // The small session is not compressed: 21 raw bytes vs 29 zlib bytes.
        let small = object(&[("_auth_user_id", Json::Str("a".into()))]);
        assert_eq!(
            dumps(&small, SESSION_KEY_SALT, SECRET, 1_777_777_777).expect("dumps"),
            "eyJfYXV0aF91c2VyX2lkIjoiYSJ9:1wJND7:sfyltBiEsICVZ8MB0qA4-ui9wGNe7hnt_nVcJtpaYLQ"
        );

        let non_ascii = object(&[
            ("name", Json::Str("Zoë — 日本語".into())),
            ("emoji", Json::Str("🚀".into())),
            ("sep", Json::Str("\u{2028}".into())),
        ]);
        // Django compressed this payload from 83 to 81 bytes; flate2's
        // backends produce 83-85 for the same input, so the
        // `len(z) < len(data) - 1` decision flips and the bytes differ. Django
        // accepts either form (both are signed), so the contract is the decoded
        // value, not the compressed stream. The vector must decode.
        assert_eq!(
            loads(
                ".eJyrVspLzE1VslKKyo8pNTBITVKIKTUyMDQBUmamqaZA0twoOabUItEyVUlHKTU3PysTqDimNMXCOAVIploYAIWLUwvAgkYGRhZKtQAaHBiB:1wJND7:aTmnhQlrzgEghEFlplZGh3fJ6fM4HHcYhFOz-_GzFIM",
                SESSION_KEY_SALT,
                SECRET,
                &[]
            )
            .expect("loads"),
            non_ascii
        );
        // And our own encoding round-trips.
        let encoded = dumps(&non_ascii, SESSION_KEY_SALT, SECRET, 1_777_777_777).expect("dumps");
        assert_eq!(
            loads(&encoded, SESSION_KEY_SALT, SECRET, &[]).expect("loads"),
            non_ascii
        );
    }

    #[test]
    fn bad_signatures_and_garbage_are_distinguished() {
        let tampered = ENCODED_FULL.replacen("eJxN", "eJxO", 1);
        assert_eq!(
            loads(&tampered, SESSION_KEY_SALT, SECRET, &[]),
            Err(SigningError::BadSignature)
        );
        assert_eq!(
            loads("garbage", SESSION_KEY_SALT, SECRET, &[]),
            Err(SigningError::BadSignature)
        );
        // Correctly signed but not a valid payload: Django's ValueError path.
        let unsigned = "bm90LWpzb24:1wJND7";
        let signature = b64_encode(&salted_hmac_sha256(
            &signer_salt(SESSION_KEY_SALT),
            unsigned.as_bytes(),
            SECRET,
        ));
        assert_eq!(
            loads(
                &format!("{unsigned}:{signature}"),
                SESSION_KEY_SALT,
                SECRET,
                &[]
            ),
            Err(SigningError::Invalid)
        );
        // A different salt must not verify.
        assert_eq!(
            loads(ENCODED_FULL, "django.core.signing", SECRET, &[]),
            Err(SigningError::BadSignature)
        );
    }

    #[test]
    fn fallback_keys_verify() {
        let encoded = dumps(
            &appendix_dict(),
            SESSION_KEY_SALT,
            "old-secret",
            1_777_777_777,
        )
        .expect("dumps");
        assert_eq!(
            loads(&encoded, SESSION_KEY_SALT, SECRET, &[]),
            Err(SigningError::BadSignature)
        );
        assert_eq!(
            loads(
                &encoded,
                SESSION_KEY_SALT,
                SECRET,
                &["old-secret".to_string()]
            )
            .expect("loads"),
            appendix_dict()
        );
    }

    #[test]
    fn base62_round_trips() {
        for value in [
            0i64,
            1,
            61,
            62,
            1_777_777_777,
            i64::from(i32::MAX),
            9_999_999_999,
        ] {
            let encoded = b62_encode(value as u64);
            assert_eq!(b62_decode(&encoded), Ok(value), "{value}");
        }
        assert_eq!(b62_encode(1_777_777_777), "1wJND7");
        assert_eq!(b62_decode("1wJND7"), Ok(1_777_777_777));
        assert_eq!(b62_decode("abc!"), Err(SigningError::Invalid));
        assert_eq!(b62_decode(""), Err(SigningError::Invalid));
    }

    #[test]
    fn salted_hmac_matches_django() {
        // computed with Django 5.2.17:
        // salted_hmac("salt", b"value", secret="secret", algorithm="sha256").hexdigest()
        assert_eq!(
            salted_hmac_hexdigest("salt", b"value", "secret"),
            "debaddcd9ecbaab181289870baf8af49b0af0a7050d620d1a63f1e69bd87ceb8"
        );
    }
}
