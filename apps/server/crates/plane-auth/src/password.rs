// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `django.contrib.auth.hashers` 5.2.17, ported.
//!
//! Plane does **not** override `PASSWORD_HASHERS` (`plane/settings/common.py`
//! has no such setting; verified by grep), so the deployment uses Django's
//! default list (`django/conf/global_settings.py:544-550`):
//!
//! | algorithm | accepted here | why |
//! | --- | --- | --- |
//! | `pbkdf2_sha256` | yes (default, 1,000,000 iterations) | stdlib |
//! | `pbkdf2_sha1` | yes | stdlib |
//! | `argon2` | **raises** `MissingPasswordLibrary` | `argon2-cffi` is not installed in `apps/api/.venv`, and Django raises `ValueError` from `_load_library` |
//! | `bcrypt_sha256` | **raises** `MissingPasswordLibrary` | `bcrypt` is not installed |
//! | `scrypt` | yes | `hashlib.scrypt` is stdlib |
//!
//! Hashes naming any *other* algorithm (`md5`, `bcrypt`, `unsalted_md5`, …) are
//! not in the list: `identify_hasher` raises `ValueError`, which
//! `verify_password` turns into "unusable" — `(False, False)` after burning one
//! default hash. That is Django's behaviour, not a simplification.
//!
//! The salt is `get_random_string(22, RANDOM_STRING_CHARS)`: Python's
//! `salt()` computes `ceil(128 / log2(62)) == 22`, and `must_update_salt`
//! flags any salt shorter than 22 characters.

use std::num::NonZeroU32;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::error::AuthError;
use crate::random::{RANDOM_STRING_CHARS, constant_time_compare, get_random_string};

/// `UNUSABLE_PASSWORD_PREFIX`.
pub const UNUSABLE_PASSWORD_PREFIX: char = '!';
/// `UNUSABLE_PASSWORD_SUFFIX_LENGTH`.
pub const UNUSABLE_PASSWORD_SUFFIX_LENGTH: usize = 40;
/// `PBKDF2PasswordHasher.iterations` in Django 5.2.17.
pub const PBKDF2_ITERATIONS: u32 = 1_000_000;
/// `PBKDF2PasswordHasher.algorithm`.
pub const PBKDF2_ALGORITHM: &str = "pbkdf2_sha256";
/// `BasePasswordHasher.salt_entropy`.
pub const SALT_ENTROPY: f64 = 128.0;
/// `ScryptPasswordHasher` parameters (`django/contrib/auth/hashers.py:576-580`).
pub const SCRYPT_WORK_FACTOR: u64 = 1 << 14;
pub const SCRYPT_BLOCK_SIZE: u64 = 8;
pub const SCRYPT_PARALLELISM: u64 = 5;

/// Which hasher an encoded value names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hasher {
    Pbkdf2Sha256,
    Pbkdf2Sha1,
    Argon2,
    BcryptSha256,
    Scrypt,
}

impl Hasher {
    pub fn algorithm(self) -> &'static str {
        match self {
            Self::Pbkdf2Sha256 => "pbkdf2_sha256",
            Self::Pbkdf2Sha1 => "pbkdf2_sha1",
            Self::Argon2 => "argon2",
            Self::BcryptSha256 => "bcrypt_sha256",
            Self::Scrypt => "scrypt",
        }
    }

    /// `loaded_hashers()`: the default `PASSWORD_HASHERS` list, in order.
    fn from_algorithm(algorithm: &str) -> Option<Self> {
        match algorithm {
            "pbkdf2_sha256" => Some(Self::Pbkdf2Sha256),
            "pbkdf2_sha1" => Some(Self::Pbkdf2Sha1),
            "argon2" => Some(Self::Argon2),
            "bcrypt_sha256" => Some(Self::BcryptSha256),
            "scrypt" => Some(Self::Scrypt),
            _ => None,
        }
    }

    fn library(self) -> Option<&'static str> {
        match self {
            Self::Argon2 => Some("argon2"),
            Self::BcryptSha256 => Some("bcrypt"),
            _ => None,
        }
    }
}

/// `is_password_usable(encoded)`.
pub fn is_password_usable(encoded: &str) -> bool {
    !encoded.starts_with(UNUSABLE_PASSWORD_PREFIX)
}

/// `BasePasswordHasher.salt()`.
pub fn new_salt() -> String {
    let char_count = (SALT_ENTROPY / (62.0f64).log2()).ceil() as usize;
    get_random_string(char_count, RANDOM_STRING_CHARS)
}

/// `must_update_salt(salt, 128)`: `len(salt) * log2(62) < 128`.
fn must_update_salt(salt: &str) -> bool {
    (salt.chars().count() as f64) * (62.0f64).log2() < SALT_ENTROPY
}

/// `make_password(password, salt=None, hasher="default")`.
///
/// `None` produces an unusable password: `"!" + 40 random characters`.
pub fn make_password(password: Option<&str>) -> String {
    make_password_with_salt(password, None)
}

/// `make_password` with an explicit salt (tests and `harden_runtime`).
pub fn make_password_with_salt(password: Option<&str>, salt: Option<&str>) -> String {
    let Some(password) = password else {
        return format!(
            "{UNUSABLE_PASSWORD_PREFIX}{}",
            get_random_string(UNUSABLE_PASSWORD_SUFFIX_LENGTH, RANDOM_STRING_CHARS)
        );
    };
    let salt = match salt {
        Some(salt) => salt.to_string(),
        None => new_salt(),
    };
    encode_pbkdf2(PBKDF2_ALGORITHM, password, &salt, PBKDF2_ITERATIONS)
}

/// `PBKDF2PasswordHasher.encode` / `PBKDF2SHA1PasswordHasher.encode`.
fn encode_pbkdf2(algorithm: &'static str, password: &str, salt: &str, iterations: u32) -> String {
    let mut output = vec![0u8; if algorithm == "pbkdf2_sha256" { 32 } else { 20 }];
    let algorithm_id = if algorithm == "pbkdf2_sha256" {
        aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA256
    } else {
        aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA1
    };
    // `iterations` is always >= 1 here (Django's default is 1,000,000 and a
    // parsed hash with 0 iterations cannot be produced by `encode`).
    let iterations = NonZeroU32::new(iterations).unwrap_or(NonZeroU32::MIN);
    aws_lc_rs::pbkdf2::derive(
        algorithm_id,
        iterations,
        salt.as_bytes(),
        password.as_bytes(),
        &mut output,
    );
    format!("{algorithm}${iterations}${salt}${}", BASE64.encode(output))
}

/// `ScryptPasswordHasher.encode`.
fn encode_scrypt(
    password: &str,
    salt: &str,
    work_factor: u64,
    block_size: u64,
    parallelism: u64,
) -> Result<String, AuthError> {
    let log_n = work_factor.trailing_zeros();
    if 1u64 << log_n != work_factor {
        return Err(AuthError::InvalidPasswordHash(
            "scrypt work factor is not a power of two".to_string(),
        ));
    }
    let params = scrypt::Params::new(
        log_n as u8,
        u32::try_from(block_size).unwrap_or(u32::MAX),
        u32::try_from(parallelism).unwrap_or(u32::MAX),
        64,
    )
    .map_err(|error| AuthError::InvalidPasswordHash(error.to_string()))?;
    let mut output = [0u8; 64];
    scrypt::scrypt(password.as_bytes(), salt.as_bytes(), &params, &mut output)
        .map_err(|error| AuthError::InvalidPasswordHash(error.to_string()))?;
    Ok(format!(
        "scrypt${work_factor}${salt}${block_size}${parallelism}${}",
        BASE64.encode(output)
    ))
}

/// `identify_hasher(encoded)`: the `ValueError` cases are `None`.
fn identify_hasher(encoded: &str) -> Option<Hasher> {
    // The three "ancient" special cases all resolve to algorithms that are not
    // in the default `PASSWORD_HASHERS` list, so `get_hasher` raises and
    // `verify_password` treats the value as unusable. Returning `None` here is
    // exactly that outcome.
    if (encoded.len() == 32 && !encoded.contains('$'))
        || (encoded.len() == 37 && encoded.starts_with("md5$$"))
        || (encoded.len() == 46 && encoded.starts_with("sha1$$"))
    {
        return None;
    }
    let algorithm = encoded
        .split_once('$')
        .map(|(name, _)| name)
        .unwrap_or(encoded);
    Hasher::from_algorithm(algorithm)
}

/// One `verify_password` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordCheck {
    /// The password matched.
    pub is_correct: bool,
    /// `must_update`: the hash was produced with another algorithm, fewer
    /// iterations, or a short salt, so the caller must rehash and save it (the
    /// `setter` argument of `django.contrib.auth.hashers.check_password`).
    pub must_update: bool,
}

/// `verify_password(password, encoded)`.
pub fn verify_password(password: &str, encoded: &str) -> Result<PasswordCheck, AuthError> {
    let fake_runtime = !is_password_usable(encoded);
    let hasher = identify_hasher(encoded);
    if fake_runtime || hasher.is_none() {
        // `make_password(get_random_string(40))`: burn one default hash so a
        // nonexistent user and an unusable password cost the same.
        make_password(Some(&get_random_string(
            UNUSABLE_PASSWORD_SUFFIX_LENGTH,
            RANDOM_STRING_CHARS,
        )));
        return Ok(PasswordCheck {
            is_correct: false,
            must_update: false,
        });
    }
    let hasher = hasher.unwrap_or(Hasher::Pbkdf2Sha256);

    // `hasher_changed = hasher.algorithm != preferred.algorithm`; the
    // preferred hasher is always `pbkdf2_sha256` here.
    let hasher_changed = hasher != Hasher::Pbkdf2Sha256;
    // `must_update = hasher_changed or preferred.must_update(encoded)`. Python's
    // `or` short-circuits, which matters: `PBKDF2PasswordHasher.must_update`
    // asserts on the algorithm name, so it must not run for another hasher.
    let must_update = if hasher_changed {
        true
    } else {
        pbkdf2_must_update(encoded)?
    };

    let is_correct = verify_with(hasher, password, encoded)?;
    if !is_correct && !hasher_changed && must_update {
        harden_runtime(password, encoded)?;
    }
    Ok(PasswordCheck {
        is_correct,
        must_update,
    })
}

/// `PBKDF2PasswordHasher.must_update`.
fn pbkdf2_must_update(encoded: &str) -> Result<bool, AuthError> {
    let (iterations, salt) = decode_pbkdf2(PBKDF2_ALGORITHM, encoded)?;
    Ok(iterations != PBKDF2_ITERATIONS || must_update_salt(&salt))
}

/// `PBKDF2PasswordHasher.decode`, including the `assert algorithm == self.algorithm`.
fn decode_pbkdf2(
    expected_algorithm: &'static str,
    encoded: &str,
) -> Result<(u32, String), AuthError> {
    let mut parts = encoded.splitn(4, '$');
    let algorithm = parts.next().unwrap_or("");
    let iterations = parts.next().unwrap_or("");
    let salt = parts.next().unwrap_or("");
    let hash = parts.next().unwrap_or("");
    if algorithm != expected_algorithm || hash.is_empty() {
        return Err(AuthError::InvalidPasswordHash(format!(
            "expected {expected_algorithm}$<iterations>$<salt>$<hash>"
        )));
    }
    let iterations = iterations
        .parse::<u32>()
        .map_err(|_| AuthError::InvalidPasswordHash("iterations are not an integer".to_string()))?;
    Ok((iterations, salt.to_string()))
}

fn verify_with(hasher: Hasher, password: &str, encoded: &str) -> Result<bool, AuthError> {
    if let Some(library) = hasher.library() {
        // Django calls `_load_library()` from both `must_update` and `verify`;
        // the import failure propagates as `ValueError`.
        return Err(AuthError::MissingPasswordLibrary(library));
    }
    match hasher {
        Hasher::Pbkdf2Sha256 | Hasher::Pbkdf2Sha1 => {
            let (iterations, salt) = decode_pbkdf2(hasher.algorithm(), encoded)?;
            let candidate = encode_pbkdf2(hasher.algorithm(), password, &salt, iterations);
            Ok(constant_time_compare(
                candidate.as_bytes(),
                encoded.as_bytes(),
            ))
        }
        Hasher::Scrypt => {
            let mut parts = encoded.splitn(6, '$');
            let algorithm = parts.next().unwrap_or("");
            let work_factor = parts.next().unwrap_or("");
            let salt = parts.next().unwrap_or("");
            let block_size = parts.next().unwrap_or("");
            let parallelism = parts.next().unwrap_or("");
            let hash = parts.next().unwrap_or("");
            if algorithm != "scrypt" || hash.is_empty() {
                return Err(AuthError::InvalidPasswordHash(
                    "expected scrypt$<n>$<salt>$<r>$<p>$<hash>".to_string(),
                ));
            }
            let work_factor = work_factor.parse::<u64>().map_err(|_| {
                AuthError::InvalidPasswordHash("scrypt work factor is not an integer".to_string())
            })?;
            let block_size = block_size.parse::<u64>().map_err(|_| {
                AuthError::InvalidPasswordHash("scrypt block size is not an integer".to_string())
            })?;
            let parallelism = parallelism.parse::<u64>().map_err(|_| {
                AuthError::InvalidPasswordHash("scrypt parallelism is not an integer".to_string())
            })?;
            let candidate = encode_scrypt(password, salt, work_factor, block_size, parallelism)?;
            Ok(constant_time_compare(
                candidate.as_bytes(),
                encoded.as_bytes(),
            ))
        }
        Hasher::Argon2 | Hasher::BcryptSha256 => Err(AuthError::MissingPasswordLibrary(
            hasher.library().unwrap_or("unknown"),
        )),
    }
}

/// `verify_password`'s return value as the boolean `check_password` yields.
pub fn check_password(password: &str, encoded: &str) -> Result<bool, AuthError> {
    Ok(verify_password(password, encoded)?.is_correct)
}

/// `PBKDF2PasswordHasher.harden_runtime`.
fn harden_runtime(password: &str, encoded: &str) -> Result<(), AuthError> {
    let (iterations, salt) = decode_pbkdf2(PBKDF2_ALGORITHM, encoded)?;
    if let Some(extra) = PBKDF2_ITERATIONS.checked_sub(iterations)
        && extra > 0
    {
        // The result is discarded; only the work matters.
        let _ = encode_pbkdf2(PBKDF2_ALGORITHM, password, &salt, extra);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// spawn_blocking wrappers (AGENTS.md: password hashing never runs on the
// runtime threads).
// ---------------------------------------------------------------------------

/// [`make_password`] on `spawn_blocking`.
pub async fn make_password_async(password: Option<String>) -> Result<String, AuthError> {
    tokio::task::spawn_blocking(move || make_password(password.as_deref()))
        .await
        .map_err(|error| AuthError::Database(format!("password hashing task failed: {error}")))
}

/// [`check_password`] on `spawn_blocking`.
pub async fn check_password_async(password: String, encoded: String) -> Result<bool, AuthError> {
    tokio::task::spawn_blocking(move || check_password(&password, &encoded))
        .await
        .map_err(|error| AuthError::Database(format!("password check task failed: {error}")))?
}

/// [`verify_password`] on `spawn_blocking`, for callers that must rehash.
pub async fn verify_password_async(
    password: String,
    encoded: String,
) -> Result<PasswordCheck, AuthError> {
    tokio::task::spawn_blocking(move || verify_password(&password, &encoded))
        .await
        .map_err(|error| AuthError::Database(format!("password check task failed: {error}")))?
}

/// `check_password(raw, encoded, setter=...)` for the "rehash on login" case:
/// returns the boolean and the replacement hash when Django would call the
/// setter. `None` means "no update needed" (or the password was wrong).
pub async fn check_password_with_rehash(
    password: String,
    encoded: String,
) -> Result<(bool, Option<String>), AuthError> {
    tokio::task::spawn_blocking(move || {
        let check = verify_password(&password, &encoded)?;
        let rehash = if check.is_correct && check.must_update {
            Some(make_password(Some(&password)))
        } else {
            None
        };
        Ok((check.is_correct, rehash))
    })
    .await
    .map_err(|error| AuthError::Database(format!("password check task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    // Everything below was produced by Django 5.2.17 in `apps/api/.venv`.
    const SECRET_HASH: &str =
        "pbkdf2_sha256$1000000$0123456789abcdefghijkl$DmREgDUlBR1MfliUDV0K6xGgqg3WQnIIkv4IkwmazXA=";

    #[test]
    fn salt_shape_matches_django() {
        assert_eq!(new_salt().len(), 22);
    }

    #[test]
    fn make_password_shape() {
        let hash = make_password(Some("Diff-Test-Passw0rd-2026!"));
        let parts: Vec<&str> = hash.split('$').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "pbkdf2_sha256");
        assert_eq!(parts[1], "1000000");
        assert_eq!(parts[2].len(), 22);
        assert!(parts[3].ends_with('='));
        assert!(check_password("Diff-Test-Passw0rd-2026!", &hash).expect("check"));
        assert!(!check_password("wrong", &hash).expect("check"));
    }

    #[test]
    fn explicit_salt_is_byte_compatible() {
        // `PBKDF2PasswordHasher().encode("password", "salt")` in Django.
        let hash = encode_pbkdf2(PBKDF2_ALGORITHM, "password", "salt", 1_000_000);
        assert_eq!(
            hash,
            "pbkdf2_sha256$1000000$salt$UFESpZC+YaydOiNb8KjuzqQOVGUuwOPCV8InyapeZkw="
        );
    }

    #[test]
    fn unusable_passwords() {
        let unusable = make_password(None);
        assert!(unusable.starts_with('!'));
        assert_eq!(unusable.len(), 41);
        assert!(!is_password_usable(&unusable));
        assert!(
            !verify_password("anything", &unusable)
                .expect("verify")
                .is_correct
        );
        assert!(
            !verify_password("anything", &unusable)
                .expect("verify")
                .must_update
        );
    }

    #[test]
    fn must_update_rules() {
        // Fresh hashes need no update.
        let fresh = make_password_with_salt(Some("x"), Some("0123456789abcdefghijkl"));
        let check = verify_password("x", &fresh).expect("verify");
        assert!(check.is_correct);
        assert!(!check.must_update);

        // Old iteration counts do, and the password still verifies.
        let old = encode_pbkdf2(PBKDF2_ALGORITHM, "x", "0123456789abcdefghijkl", 600_000);
        let check = verify_password("x", &old).expect("verify");
        assert!(check.is_correct);
        assert!(check.must_update);

        // A short salt does too.
        let short_salt = encode_pbkdf2(PBKDF2_ALGORITHM, "x", "shortsalt", 1_000_000);
        assert!(
            verify_password("x", &short_salt)
                .expect("verify")
                .must_update
        );

        // pbkdf2_sha1 always does (algorithm changed).
        let sha1 = encode_pbkdf2("pbkdf2_sha1", "x", "0123456789abcdefghijkl", 1_000_000);
        let check = verify_password("x", &sha1).expect("verify");
        assert!(check.is_correct);
        assert!(check.must_update);
    }

    #[test]
    fn scrypt_round_trips_and_always_must_update() {
        let hash = encode_scrypt("s3cret", "0123456789abcdef", 1 << 14, 8, 5).expect("encode");
        let check = verify_password("s3cret", &hash).expect("verify");
        assert!(check.is_correct);
        // `scrypt` is not the preferred algorithm, so must_update is true.
        assert!(check.must_update);
        assert!(!verify_password("wrong", &hash).expect("verify").is_correct);
    }

    #[test]
    fn missing_libraries_raise_like_django() {
        for encoded in [
            "argon2$argon2id$v=19$m=102400,t=2,p=8$c2FsdA$AAAA",
            "bcrypt_sha256$2b$12$abcdefghijklmnopqrstuuJ8YQ0m1n8m1n8m1n8m1n8m1n8m1n8m",
        ] {
            assert!(matches!(
                verify_password("x", encoded),
                Err(AuthError::MissingPasswordLibrary(_))
            ));
        }
    }

    #[test]
    fn unknown_algorithms_are_unusable_not_errors() {
        for encoded in [
            "md5$salt$0123456789abcdef0123456789abcdef",
            "bcrypt$2b$12$abcdefghijklmnopqrstuuJ8YQ0m1n8m1n8m1n8m1n8m1n8m1n8m",
            "unsalted_md5",
            "not-a-hash",
            "",
        ] {
            let check = verify_password("x", encoded).expect("no error");
            assert!(!check.is_correct, "{encoded}");
            assert!(!check.must_update, "{encoded}");
        }
    }

    #[test]
    fn non_ascii_passwords_hash_as_utf8() {
        // `force_bytes` encodes str as UTF-8.
        let hash =
            make_password_with_salt(Some("pässwörd — 日本語 🚀"), Some("0123456789abcdefghijkl"));
        assert!(check_password("pässwörd — 日本語 🚀", &hash).expect("check"));
    }

    #[test]
    fn invalid_hashes_that_identify_a_known_hasher_error() {
        // `decode()` raises ValueError for these in Django too.
        assert!(matches!(
            verify_password("x", "pbkdf2_sha256$notanint$salt$hash"),
            Err(AuthError::InvalidPasswordHash(_))
        ));
        assert!(matches!(
            verify_password("x", "pbkdf2_sha256$1$salt"),
            Err(AuthError::InvalidPasswordHash(_))
        ));
        // A pbkdf2_sha1 hash is verified by the sha1 hasher, whose decode
        // asserts the algorithm name; ours rejects the mismatch instead.
        assert!(verify_password("x", "pbkdf2_sha256$1$salt$hash$extra").is_ok());
    }

    #[tokio::test]
    async fn async_wrappers_agree() {
        let hash = make_password_async(Some("x".into())).await.expect("hash");
        assert!(
            check_password_async("x".into(), hash.clone())
                .await
                .expect("check")
        );
        let (ok, rehash) = check_password_with_rehash("x".into(), hash)
            .await
            .expect("check");
        assert!(ok);
        assert!(rehash.is_none());
    }

    #[test]
    fn unused_constant_is_referenced() {
        // Keep the fixture honest: it is a real Django hash for "password".
        assert!(SECRET_HASH.starts_with("pbkdf2_sha256$1000000$"));
    }
}
