// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! PyJWT-compatible HS256 encoding, for the one JWT Plane mints itself.
//!
//! `WorkspaceInvitationsViewset.create` stores
//! `jwt.encode({"email": email, "timestamp": datetime.now().timestamp()},
//! settings.SECRET_KEY, algorithm="HS256")` in
//! `workspace_member_invites.token` (`plane/app/views/workspace/invite.py:90-
//! 99`, PyJWT 2.15.1). The token is compared byte for byte nowhere, but the
//! bytes still decide what the invitation e-mail link carries and what
//! `WorkspaceJoinEndpoint.post` accepts, so the encoder must agree with PyJWT
//! for the same payload and key — pinned by the golden vectors below.
//!
//! What PyJWT 2.15.1 does (`jwt/api_jws.py:126-208`, `jwt/api_jwt.py:158-174`,
//! `jwt/utils.py:36-37`):
//!
//! 1. header: `{"typ": "JWT", "alg": "HS256"}` serialised with
//!    `json.dumps(..., separators=(",", ":"), sort_keys=True)` — the keys come
//!    out sorted, so the bytes are always
//!    `{"alg":"HS256","typ":"JWT"}`;
//! 2. payload: `json.dumps(payload, separators=(",", ":"))` — **not** sorted,
//!    `ensure_ascii=True`, CPython's `float.__repr__`. [`crate::json::
//!    to_compact_string`] is exactly that dialect;
//! 3. both segments base64url-encoded without `=` padding;
//! 4. `signing_input = header_b64 + "." + payload_b64`;
//! 5. `HMAC-SHA256(key_bytes, signing_input)`, base64url without padding, where
//!    `key_bytes` is `settings.SECRET_KEY` encoded as UTF-8 (`force_bytes`).
//!
//! The signing key is a secret: it is never logged, printed or recorded here.

use aws_lc_rs::hmac;
use base64::Engine as _;

use crate::json::Json;

/// The exact header bytes PyJWT writes for `algorithm="HS256"`.
const HEADER: &str = "{\"alg\":\"HS256\",\"typ\":\"JWT\"}";

/// `jwt.encode(payload, key, algorithm="HS256")` for a payload that is already
/// the bytes CPython's `json.dumps(payload, separators=(",", ":"))` produces.
///
/// `key` is Django's `SECRET_KEY`; it is encoded as UTF-8, exactly
/// `force_bytes(str)`.
#[must_use]
pub fn encode_hs256(payload_json: &str, key: &str) -> String {
    let header = base64_url(HEADER.as_bytes());
    let payload = base64_url(payload_json.as_bytes());
    let signing_input = format!("{header}.{payload}");
    let signature = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
        signing_input.as_bytes(),
    );
    format!("{signing_input}.{}", base64_url(signature.as_ref()))
}

/// [`encode_hs256`] for a payload value, rendered with
/// [`crate::json::to_compact_string`] — PyJWT's `json.dumps` dialect.
#[must_use]
pub fn encode_hs256_value(payload: &Json, key: &str) -> String {
    encode_hs256(&crate::json::to_compact_string(payload), key)
}

/// The invitation payload: `{"email": <raw body dict>, "timestamp": <ts>}`.
///
/// `email` is the **raw** element of the request's `emails` list, not the
/// validated address — `invite.py:95-99` embeds whatever the body carried
/// (a dict with the address, a role, and any extra key), so the token changes
/// with the body's key order and unknown members. `timestamp` is
/// `datetime.now().timestamp()`: local time as POSIX seconds, a float.
#[must_use]
pub fn invitation_payload(email: &Json, timestamp: f64) -> Json {
    Json::Object(vec![
        ("email".to_owned(), email.clone()),
        ("timestamp".to_owned(), Json::Float(timestamp)),
    ])
}

/// Base64url without padding (`base64.urlsafe_b64encode(...).replace(b"=",
/// b"")`, `jwt/utils.py:36-37`).
fn base64_url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fixed, obviously fake key: the dev `SECRET_KEY` never appears in a
    /// test or a tracked file.
    const KEY: &str = "plane-rs-test-jwt-key-not-a-secret";

    /// The vectors are the venv's `jwt.encode(payload, KEY, algorithm=
    /// "HS256")` (PyJWT 2.15.1, CPython 3.12.13) for the payload rendered by
    /// `json.dumps(payload, separators=(",", ":"))`.
    #[test]
    fn the_encoder_matches_pyjwt() {
        let cases = [
            (
                r#"{"email":{"email":"invitee@example.com","role":15},"timestamp":1763458123.123456}"#,
                "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJlbWFpbCI6eyJlbWFpbCI6Imludml0ZWVAZXhhbXBsZS5jb20iLCJyb2xlIjoxNX0sInRpbWVzdGFtcCI6MTc2MzQ1ODEyMy4xMjM0NTZ9.oHu6cOI89wWLA_KC-IcaOVLDlTWX9fibckMysHoIq4U",
            ),
            (
                r#"{"email":{"email":"caf\u00e9@ex\u00e4mple.com","role":"15"},"timestamp":1763458123.0}"#,
                "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJlbWFpbCI6eyJlbWFpbCI6ImNhZlx1MDBlOUBleFx1MDBlNG1wbGUuY29tIiwicm9sZSI6IjE1In0sInRpbWVzdGFtcCI6MTc2MzQ1ODEyMy4wfQ.xRdAbiAjeWdw9_Xsl-7X1BTiraKBYy0PGQzPq5MXMik",
            ),
            (
                r#"{"email":{"email":"\ud83d\ude80@example.com","role":5},"timestamp":1763458123}"#,
                "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJlbWFpbCI6eyJlbWFpbCI6Ilx1ZDgzZFx1ZGU4MEBleGFtcGxlLmNvbSIsInJvbGUiOjV9LCJ0aW1lc3RhbXAiOjE3NjM0NTgxMjN9.cMWsJSlceoCuZTOwQGEfl2QHgdCYM3vKVw78STo2G1Y",
            ),
            (
                r#"{"email":{"email":"x@example.com"},"timestamp":0.0001}"#,
                "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJlbWFpbCI6eyJlbWFpbCI6InhAZXhhbXBsZS5jb20ifSwidGltZXN0YW1wIjowLjAwMDF9.CGxUJFOwFaoCAS0VrIJWz8w3a7-0__PrMQctInK0oqU",
            ),
            (
                r#"{"email":{},"timestamp":1.0}"#,
                "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJlbWFpbCI6e30sInRpbWVzdGFtcCI6MS4wfQ.PDsrNRV72fMjWHLJwJF5PVD13i1TyqMDWF1UnpSJbhg",
            ),
        ];
        for (payload, expected) in cases {
            assert_eq!(encode_hs256(payload, KEY), expected, "payload {payload}");
        }
    }

    #[test]
    fn a_payload_value_renders_with_pyjwts_dialect() {
        // `json.dumps({"email": {"email": "a@b", "role": 5}, "timestamp":
        // 1.0}, separators=(",", ":"))`.
        let email = Json::Object(vec![
            ("email".to_owned(), Json::Str("a@b".to_owned())),
            ("role".to_owned(), Json::Int(5)),
        ]);
        let payload = invitation_payload(&email, 1.0);
        assert_eq!(
            crate::json::to_compact_string(&payload),
            r#"{"email":{"email":"a@b","role":5},"timestamp":1.0}"#
        );
        // The same token as the raw-string form.
        assert_eq!(
            encode_hs256_value(&payload, KEY),
            encode_hs256(r#"{"email":{"email":"a@b","role":5},"timestamp":1.0}"#, KEY)
        );
    }

    #[test]
    fn the_header_is_pyjwts_sorted_header() {
        assert_eq!(
            base64_url(HEADER.as_bytes()),
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"
        );
    }
}
