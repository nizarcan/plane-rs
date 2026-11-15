// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's session payload codec.
//!
//! The `sessions.session_data` column is `signing.dumps(session_dict,
//! salt="django.contrib.sessions.SessionStore", compress=True)`:
//! `TimestampSigner` over URL-safe base64 of (optionally zlib-compressed) JSON,
//! HMAC-SHA256 keyed with `sha256(salt + "signer" + SECRET_KEY)`
//! (`HTTP_CONTRACT.md` §5.3 and Appendix B.1).
//!
//! The harness verifies the signature with the target's own `SECRET_KEY` before
//! comparing decoded payloads: a payload Django would reject must not be
//! normalised away. `decode` is deliberately strict (no `max_age`, matching the
//! session backend, which enforces expiry through `expire_date`).

use serde_json::Value;

use crate::b64;
use crate::password::{derive_key, hmac_sha256}; // `password`'s crypto helpers live in one place

/// `SessionBase.key_salt` for Plane's `SessionStore`.
pub const SESSION_KEY_SALT: &str = "django.contrib.sessions.SessionStore";
/// The salt Django's `Signer` derives the HMAC key from: `salt + "signer"`.
pub const SESSION_SIGNER_SALT: &str = "django.contrib.sessions.SessionStoresigner";
/// `plane/db/models/session.py:14`: `VALID_KEY_CHARS = ascii_lowercase + digits`.
pub const SESSION_KEY_PATTERN: &str = r"^[a-z0-9]{128}$";
/// `value:timestamp:signature`, with an optional `.` compression marker.
pub const SESSION_DATA_PATTERN: &str = r"^\.?[A-Za-z0-9_-]+:[0-9A-Za-z]+:[A-Za-z0-9_-]+$";
/// Django's base62 alphabet, used for the signing timestamp.
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedSession {
    /// The decoded session dictionary, in wire key order.
    pub payload: Value,
    /// The signing timestamp, decoded from base62 (Unix seconds).
    pub signed_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("expected value:timestamp:signature")]
    Layout,
    #[error("signature does not verify with this target's SECRET_KEY")]
    BadSignature,
    #[error("signing timestamp is not base62")]
    BadTimestamp,
    #[error("payload is not URL-safe base64")]
    BadBase64,
    #[error("payload is not valid zlib")]
    BadCompression,
    #[error("payload is not valid JSON")]
    BadJson,
}

/// Verify and decode one `sessions.session_data` value.
pub fn decode(session_data: &str, secret_key: &[u8]) -> Result<DecodedSession, SessionError> {
    // Signer.unsign: split the signature off, verify over everything before it.
    let (payload_and_timestamp, signature) =
        session_data.rsplit_once(':').ok_or(SessionError::Layout)?;

    let key = derive_key(SESSION_SIGNER_SALT, secret_key);
    let expected =
        b64::encode_urlsafe_unpadded(&hmac_sha256(&key, payload_and_timestamp.as_bytes()));
    if !constant_time_eq(expected.as_bytes(), signature.as_bytes()) {
        return Err(SessionError::BadSignature);
    }

    // TimestampSigner.unsign: split the timestamp off the verified value.
    let (payload, timestamp) = payload_and_timestamp
        .rsplit_once(':')
        .ok_or(SessionError::Layout)?;
    let signed_at = b62_decode(timestamp).ok_or(SessionError::BadTimestamp)?;

    let compressed = payload.starts_with('.');
    let encoded = if compressed { &payload[1..] } else { payload };
    let raw = b64::decode_urlsafe_unpadded(encoded).ok_or(SessionError::BadBase64)?;
    let raw = if compressed { decompress(&raw)? } else { raw };

    let text = match std::str::from_utf8(&raw) {
        Ok(text) => text.to_string(),
        // The JSON serializer encodes as latin-1 after `ensure_ascii`, so this
        // is only reachable for a payload written by another serializer.
        Err(_) => raw.iter().map(|byte| *byte as char).collect(),
    };
    let payload: Value = serde_json::from_str(&text).map_err(|_| SessionError::BadJson)?;
    Ok(DecodedSession { payload, signed_at })
}

fn decompress(raw: &[u8]) -> Result<Vec<u8>, SessionError> {
    use std::io::Read;
    let mut decoder = flate2::read::ZlibDecoder::new(raw);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|_| SessionError::BadCompression)?;
    Ok(out)
}

fn b62_decode(text: &str) -> Option<i64> {
    if text.is_empty() {
        return None;
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    for byte in digits.bytes() {
        let digit = BASE62.iter().position(|allowed| *allowed == byte)? as i64;
        value = value.checked_mul(62)?.checked_add(digit)?;
    }
    Some(if negative { -value } else { value })
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in a.iter().zip(b.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// HTTP_CONTRACT.md Appendix B.1.
    const SECRET_KEY: &[u8] = b"contract-test-secret-key-not-for-deployment";
    const VECTOR: &str = ".eJxNjstuwyAQRX8lYtVKNgbj97L7fIM1MENM64JlSNU2yr8XR1l0O_fec-bGZrimZb5G2meHbGLCSqpRmbLRLZTd2A_l0I9dCa1ujMKapBWsYBrMB_ljgO_gL4Gb4NPuND9w_JlGfg5I69uzW_yXLRCXvB5tbRstUZEwA_S6w5Yaq6DW0ggcabA9dLo1TW7UVoLQoxmwp8620GhlDmikGF3wM31vbv9hk-qEKBjSlzM0O28Dm27sIYUL-ZSt5_Dr1hWqlovTy-NzMOm07UHTaya6bQbEPWNztxaKCy6l4n2OMHyC8_m8pLRNVbUGA-sSYqrY_f4HL9RrZQ:1wJND7:ywTmmvOLHYvBCPzyrQ_HeauuOal3DUrNa43jfPUyJbQ";
    const VECTOR_EXPIRY: i64 = 1_777_777_777;

    #[test]
    fn appendix_b1_vector_decodes() {
        let decoded = decode(VECTOR, SECRET_KEY).expect("vector must verify");
        assert_eq!(decoded.signed_at, VECTOR_EXPIRY);
        assert_eq!(
            decoded.payload,
            json!({
                "_auth_user_id": "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
                "backend": "django.contrib.auth.backends.ModelBackend",
                "_auth_user_hash": "9f2f4b1d3e0c8a7b6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c",
                "_session_expiry": 3600,
                "device_info": {
                    "user_agent": "Mozilla/5.0 (contract probe)",
                    "ip_address": "203.0.113.7",
                    "domain": "http://localhost/"
                }
            })
        );
    }

    #[test]
    fn the_uncompressed_variant_decodes() {
        let encoded =
            "eyJfYXV0aF91c2VyX2lkIjoiYSJ9:1wJND7:sfyltBiEsICVZ8MB0qA4-ui9wGNe7hnt_nVcJtpaYLQ";
        let decoded = decode(encoded, SECRET_KEY).expect("vector must verify");
        assert_eq!(decoded.payload, json!({"_auth_user_id": "a"}));
        assert_eq!(decoded.signed_at, VECTOR_EXPIRY);
    }

    #[test]
    fn a_wrong_secret_does_not_verify() {
        let error = decode(VECTOR, b"another-secret").expect_err("must fail");
        assert!(matches!(error, SessionError::BadSignature), "{error}");
    }

    #[test]
    fn a_tampered_payload_does_not_verify() {
        let tampered = VECTOR.replacen("eJxNjstuwyAQRX8l", "eJxNjstuwyAQRX8lA", 1);
        let error = decode(&tampered, SECRET_KEY).expect_err("must fail");
        assert!(matches!(error, SessionError::BadSignature), "{error}");
    }

    #[test]
    fn a_tampered_signature_does_not_verify() {
        let mut tampered = VECTOR.to_string();
        tampered.pop();
        tampered.push('A');
        let error = decode(&tampered, SECRET_KEY).expect_err("must fail");
        assert!(matches!(error, SessionError::BadSignature), "{error}");
    }

    #[test]
    fn garbage_is_rejected() {
        for value in ["", "abc", "a:b", "a:b:c", "!!!:1:???", ".not-zlib:1:abc"] {
            assert!(decode(value, SECRET_KEY).is_err(), "{value:?} must fail");
        }
    }

    #[test]
    fn the_shapes_match_real_values() {
        let key_shape = regex::Regex::new(SESSION_KEY_PATTERN).expect("pattern");
        let data_shape = regex::Regex::new(SESSION_DATA_PATTERN).expect("pattern");
        assert!(key_shape.is_match(
            "5m4mrmocoxbkaq2fxmtg0opz600fzwelcsfkubfgy7o5atzcl2ugr2a4qynz04axaqkeoen26haygq7lemmqrtpmgbaeh4hl0qae9iaiurlf590r2yi2663rvnlw4cdp"
        ));
        assert!(!key_shape.is_match("3f2b8f6e1d4a4b7c8e2f0a1b2c3d4e5f"));
        assert!(!key_shape.is_match(
            "5M4MRMocoxbkaq2fxmtg0opz600fzwelcsfkubfgy7o5atzcl2ugr2a4qynz04axaqkeoen26haygq7lemmqrtpmgbaeh4hl0qae9iaiurlf590r2yi2663rvnlw4cdp"
        ));
        assert!(data_shape.is_match(VECTOR));
        assert!(data_shape.is_match(
            "eyJfYXV0aF91c2VyX2lkIjoiYSJ9:1wJND7:sfyltBiEsICVZ8MB0qA4-ui9wGNe7hnt_nVcJtpaYLQ"
        ));
        assert!(!data_shape.is_match("not a payload"));
    }
}
