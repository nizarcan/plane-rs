// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django password hashes.
//!
//! Plane stores Django's default `pbkdf2_sha256$<iterations>$<salt>$<digest>`
//! (`py:django/contrib/auth/hashers.py:313-350`), with a 22-character salt over
//! `[A-Za-z0-9]` and a standard-base64 SHA-256 digest. A hash is only useful as
//! a differential signal if it is *checked*, not masked: a Rust implementation
//! that writes a hash Django cannot verify must be a DIFF, so the harness
//! re-derives the hash from the declared plaintext and the hash's own salt and
//! iteration count.
//!
//! `verify` uses the `pbkdf2` crate (HMAC-SHA256) rather than reimplementing
//! PBKDF2; the salt and iteration count come from the stored value, so a
//! different iteration count does not make verification fail silently — it is
//! part of the input, exactly as `PBKDF2PasswordHasher.verify` does it.

use hmac::Hmac;
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

use crate::b64;

/// Django's `RANDOM_STRING_CHARS`, the salt alphabet.
pub const SALT_ALPHABET: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
/// `BasePasswordHasher.salt_entropy` (128 bits) at log2(62) bits per character.
pub const SALT_LENGTH: usize = 22;
/// A `pbkdf2_sha256` digest is a 32-byte SHA-256 in standard base64.
pub const DIGEST_LENGTH: usize = 44;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DjangoPasswordHash {
    pub algorithm: String,
    pub iterations: u32,
    pub salt: String,
    pub digest: String,
}

/// The regular expression shape of a Django password column value.
///
/// Covers the two values a `users.password` column can hold: an encoded
/// `pbkdf2_sha256` hash, and Django's unusable-password marker (`"!"` followed
/// by 40 random characters, `py:django/contrib/auth/hashers.py:63-67`).
pub const SHAPE_PATTERN: &str =
    r"^(?:pbkdf2_sha256\$[0-9]{1,9}\$[A-Za-z0-9]{1,64}\$[A-Za-z0-9+/]{43}=|![A-Za-z0-9]{40})$";

/// Parse `algorithm$iterations$salt$digest`. Returns `None` for the unusable
/// marker and for anything that is not a Django hash at all.
pub fn parse(encoded: &str) -> Option<DjangoPasswordHash> {
    let mut parts = encoded.splitn(4, '$');
    let algorithm = parts.next()?;
    let iterations = parts.next()?;
    let salt = parts.next()?;
    let digest = parts.next()?;
    if algorithm.is_empty() || salt.is_empty() || digest.is_empty() {
        return None;
    }
    Some(DjangoPasswordHash {
        algorithm: algorithm.to_string(),
        iterations: iterations.parse().ok()?,
        salt: salt.to_string(),
        digest: digest.to_string(),
    })
}

/// `PBKDF2PasswordHasher.encode`: `pbkdf2_hmac("sha256", password, salt,
/// iterations)` in standard base64.
pub fn encode_sha256(password: &str, salt: &str, iterations: u32) -> String {
    let mut digest = [0_u8; 32];
    derive_sha256(password, salt, iterations, &mut digest);
    format!(
        "pbkdf2_sha256${iterations}${salt}${}",
        b64::encode_standard(&digest)
    )
}

/// PBKDF2-HMAC-SHA256 into a 32-byte buffer.
///
/// Verifying Django hashes means 1,000,000 iterations per hash, which takes
/// several seconds with the portable Rust implementation in a debug build.
/// `aws-lc-rs` is already in the dependency tree (through rustls and lapin) and
/// its C implementation is fast, so it is used whenever the inputs satisfy its
/// documented minimums (salt >= 16 bytes, password >= 14 bytes, iterations >=
/// 1000). Shorter inputs fall back to the portable crate, which is correct but
/// slower; a Django hash always has a 22-byte salt and 1,000,000 iterations.
fn derive_sha256(password: &str, salt: &str, iterations: u32, out: &mut [u8; 32]) {
    let aws_supported = salt.len() >= 16
        && password.len() >= 14
        && iterations >= 1_000
        && std::num::NonZeroU32::new(iterations).is_some();
    if aws_supported {
        aws_lc_rs::pbkdf2::derive(
            aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA256,
            std::num::NonZeroU32::new(iterations).expect("iterations is non-zero"),
            salt.as_bytes(),
            password.as_bytes(),
            out,
        );
    } else {
        pbkdf2_hmac::<Sha256>(password.as_bytes(), salt.as_bytes(), iterations, out);
    }
}

/// `PBKDF2PasswordHasher.verify`: re-encode with the hash's own salt and
/// iteration count and compare the whole string in constant time.
///
/// `None` means "not a hash this harness knows how to verify" (another
/// algorithm, or the unusable marker); `Some(false)` means the plaintext is
/// wrong, which is a real difference.
pub fn verify(encoded: &str, password: &str) -> Option<bool> {
    let parsed = parse(encoded)?;
    if parsed.algorithm != "pbkdf2_sha256" {
        return None;
    }
    let recomputed = encode_sha256(password, &parsed.salt, parsed.iterations);
    Some(constant_time_eq(recomputed.as_bytes(), encoded.as_bytes()))
}

/// True for the `!`-prefixed unusable-password marker.
pub fn is_unusable(encoded: &str) -> bool {
    encoded.starts_with('!')
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

/// HMAC-SHA256 in lower-case hex, as Django's `salted_hmac(...).hexdigest()`.
pub fn salted_hmac_hex(salt: &str, value: &[u8], secret: &[u8]) -> String {
    use hmac::Mac;
    let mut mac = Hmac::<Sha256>::new_from_slice(&derive_key(salt, secret))
        .expect("HMAC accepts keys of any length");
    mac.update(value);
    hex(&mac.finalize().into_bytes())
}

/// `salted_hmac`'s key derivation: `sha256(salt + secret).digest()`
/// (`py:django/utils/crypto.py:19-45`).
pub fn derive_key(salt: &str, secret: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(secret);
    hasher.finalize().to_vec()
}

/// HMAC-SHA256 digest bytes.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    use hmac::Mac;
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

/// HMAC-SHA256 digest in lower-case hex.
pub fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    hex(&hmac_sha256(key, message))
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The password that `scenarios/basic-session.yaml` sends.
    const PASSWORD: &str = "Diff-Test-Passw0rd-2026!";

    /// Produced by Django 5.2.17's `PBKDF2PasswordHasher.encode` with an
    /// explicit salt and 1,000 iterations, so the test stays fast.
    const FAST_VECTOR: &str =
        "pbkdf2_sha256$1000$SALTfixedSALTfixedSALT$w/D65kZ20JoZcd3sU7BR8JQMIi9dafeN0wBzp5J1hUo=";

    /// The full-strength hash Django wrote for the same password in a real
    /// run (`iterations = 1_000_000`, Django 5.2's default). This exact value
    /// was read from a `users` row created by the recorded session.
    const REAL_VECTOR: &str =
        "pbkdf2_sha256$1000000$101NS5m3LpWZGBnqhqD6tG$S3lZ1U1xaD5c8mQZ+mWuYb5+xYRXTO6ehG+wXbr7BUU=";

    #[test]
    fn verifies_django_hashes() {
        assert_eq!(verify(FAST_VECTOR, PASSWORD), Some(true));
        assert_eq!(verify(FAST_VECTOR, "wrong"), Some(false));
        assert_eq!(verify(REAL_VECTOR, PASSWORD), Some(true));
        assert_eq!(verify(REAL_VECTOR, "Diff-Test-Passw0rd-2025!"), Some(false));
    }

    #[test]
    fn the_iteration_count_is_part_of_the_hash() {
        let tampered = REAL_VECTOR.replace("$1000000$", "$500000$");
        assert_eq!(verify(&tampered, PASSWORD), Some(false));
    }

    #[test]
    fn parse_exposes_algorithm_and_iterations() {
        let parsed = parse(REAL_VECTOR).expect("parse");
        assert_eq!(parsed.algorithm, "pbkdf2_sha256");
        assert_eq!(parsed.iterations, 1_000_000);
        assert_eq!(parsed.salt, "101NS5m3LpWZGBnqhqD6tG");
        assert_eq!(parsed.digest.len(), DIGEST_LENGTH);
        assert_eq!(
            encode_sha256(PASSWORD, &parsed.salt, parsed.iterations),
            REAL_VECTOR
        );
    }

    #[test]
    fn unknown_algorithms_and_markers_are_not_verified() {
        assert_eq!(verify("!", PASSWORD), None);
        assert_eq!(verify("pbkdf2_sha1$1000$salt$digest", PASSWORD), None);
        assert_eq!(verify("", PASSWORD), None);
        assert!(is_unusable("!abc"));
    }

    #[test]
    fn the_shape_pattern_covers_the_two_column_values() {
        let shape = regex::Regex::new(SHAPE_PATTERN).expect("valid pattern");
        assert!(shape.is_match(REAL_VECTOR));
        assert!(shape.is_match(FAST_VECTOR));
        assert!(shape.is_match(&format!("!{}", "a".repeat(40))));
        assert!(!shape.is_match("plaintext"));
        assert!(!shape.is_match("pbkdf2_sha256$1000$salt"));
        assert!(!shape.is_match("!short"));
    }

    #[test]
    fn salted_hmac_derives_the_key_from_salt_and_secret() {
        // Independently computed with Python:
        //   django.utils.crypto.salted_hmac("salt", b"value", b"secret",
        //                                   algorithm="sha256").hexdigest()
        assert_eq!(
            salted_hmac_hex("salt", b"value", b"secret"),
            "debaddcd9ecbaab181289870baf8af49b0af0a7050d620d1a63f1e69bd87ceb8"
        );
        // Same key derivation, plain HMAC-SHA256 helper.
        assert_eq!(
            hmac_sha256_hex(&derive_key("salt", b"secret"), b"value"),
            "debaddcd9ecbaab181289870baf8af49b0af0a7050d620d1a63f1e69bd87ceb8"
        );
    }
}
