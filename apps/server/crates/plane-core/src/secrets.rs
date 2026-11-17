// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated secrets and tokens.
//!
//! Each function reproduces one Python generator byte for byte, because the
//! value is stored and later compared as a string (an HMAC key) rather than
//! parsed.

use uuid::Uuid;

/// `plane.db.models.webhook.generate_token` (`plane/db/models/webhook.py:17-18`):
/// `"plane_wh_" + uuid4().hex`.
///
/// `uuid.UUID.hex` is 32 lowercase hexadecimal characters with no hyphens, so
/// the value is 41 characters long and carries the 122 random bits of a
/// version-4 UUID. It is the `Webhook.secret_key` default
/// (`models/webhook.py:38`) and what `WebhookSecretRegenerateEndpoint`
/// assigns (`views/webhook/base.py:118-119`).
///
/// `Uuid::new_v4` draws from the OS random source through `getrandom` —
/// CPython's `uuid4` uses `os.urandom(16)`, the same 16 bytes — and fixes the
/// version and variant bits identically.
#[must_use]
pub fn webhook_secret_key() -> String {
    format!("plane_wh_{}", Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::webhook_secret_key;

    #[test]
    fn webhook_secret_shape_matches_generate_token() {
        let secret = webhook_secret_key();
        assert_eq!(secret.len(), 41, "{secret:?}");
        let hex = secret
            .strip_prefix("plane_wh_")
            .unwrap_or_else(|| panic!("missing prefix in {secret:?}"));
        assert_eq!(hex.len(), 32);
        assert!(
            hex.bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "not lowercase hex: {hex:?}"
        );
        // `uuid4().hex`: version 4 and the RFC 4122 variant.
        let parsed = uuid::Uuid::parse_str(hex).expect("32 hex characters parse");
        assert_eq!(
            parsed,
            uuid::Uuid::from_slice(parsed.as_bytes()).expect("uuid")
        );
        assert_eq!(parsed.get_version_num(), 4, "{hex:?}");
        assert_eq!(parsed.as_bytes()[8] & 0xc0, 0x80, "RFC 4122 variant");
    }

    #[test]
    fn webhook_secrets_are_not_repeated() {
        let first = webhook_secret_key();
        let second = webhook_secret_key();
        assert_ne!(first, second);
    }
}
