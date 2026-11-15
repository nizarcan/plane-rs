// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Helper for the password-hash interop proof (`apps/server/tools/auth/hash_roundtrip.py`).
//!
//! It is an example, not a binary: `cargo build --release --workspace` does not
//! build it, and it is never linked into `plane-api`.
//!
//! ```sh
//! cargo run -p plane-auth --release --example hash_roundtrip -- gen --count 100 --seed 7
//! cargo run -p plane-auth --release --example hash_roundtrip -- verify < file.jsonl
//! cargo run -p plane-auth --release --example hash_roundtrip -- fixtures
//! cargo run -p plane-auth --release --example hash_roundtrip -- bench --iterations 10
//! ```
//!
//! `gen` writes one `{"password": …, "hash": …}` object per line; `verify` reads
//! the same shape and writes a one-line JSON summary. `fixtures` emits hashes
//! whose `must_update` answer must agree with Django's:
//!
//! * a fresh 1,000,000-iteration `pbkdf2_sha256` → `false`;
//! * a 600,000-iteration one → `true`;
//! * a `pbkdf2_sha1` one → `true`;
//! * an unusable password → `false` (and never correct);
//! * an unknown algorithm → `false` (and never correct).

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::time::Instant;

use plane_auth::password::{
    PBKDF2_ALGORITHM, check_password, is_password_usable, make_password_with_salt, verify_password,
};

/// A tiny deterministic PRNG (xorshift64*). Determinism is the point: a rerun
/// of the proof with the same seed hashes the same passwords.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn pick(&mut self, alphabet: &[char]) -> char {
        alphabet[self.below(alphabet.len())]
    }
}

/// Passwords with ASCII, Latin-1, CJK, astral characters, spaces, quotes and
/// symbols: every class that changes the JSON or UTF-8 encoding.
const ALPHABET: &[char] = &[
    'a', 'b', 'c', 'Z', '0', '9', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '-', '_', '=',
    '+', '[', ']', '{', '}', ';', ':', ',', '.', '<', '>', '/', '?', '|', '~', '`', '\'', '"',
    '\\', ' ', 'é', 'ü', 'ñ', 'ß', 'ø', 'å', 'ç', '—', '€', '日', '本', '語', 'あ', 'é', '🚀',
    '🎉', '𝄞', '👩', '‍', '🔐',
];

fn random_password(rng: &mut Rng) -> String {
    let length = 1 + rng.below(40);
    (0..length).map(|_| rng.pick(ALPHABET)).collect()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    match command {
        "gen" => {
            let count = flag(&args, "--count").unwrap_or(100);
            let seed = flag(&args, "--seed").unwrap_or(1);
            let mut rng = Rng(seed as u64 | 1);
            let stdout = std::io::stdout();
            let mut out = std::io::BufWriter::new(stdout.lock());
            for _ in 0..count {
                let password = random_password(&mut rng);
                let hash = make_password_with_salt(Some(&password), None);
                let line = serde_json::json!({"password": password, "hash": hash});
                if writeln!(out, "{line}").is_err() {
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        "verify" => verify(flag_str(&args, "--input")),
        "fixtures" => {
            if write_fixtures().is_err() {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        "bench" => {
            let iterations = flag(&args, "--iterations").unwrap_or(10);
            bench(iterations)
        }
        other => {
            eprintln!("unknown command {other:?}");
            ExitCode::FAILURE
        }
    }
}

fn flag_str(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn flag(args: &[String], name: &str) -> Option<u64> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse().ok())
}

fn verify(input: Option<String>) -> ExitCode {
    let lines: Box<dyn Iterator<Item = Result<String, std::io::Error>>> = match input {
        Some(path) => match std::fs::File::open(&path) {
            Ok(file) => Box::new(std::io::BufReader::new(file).lines()),
            Err(error) => {
                eprintln!("cannot open {path}: {error}");
                return ExitCode::FAILURE;
            }
        },
        None => Box::new(std::io::stdin().lock().lines()),
    };
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let mut checked = 0u64;
    let mut correct = 0u64;
    let mut must_update = 0u64;
    let mut unusable = 0u64;
    let mut errors: Vec<String> = Vec::new();
    for line in lines {
        let Ok(line) = line else {
            return ExitCode::FAILURE;
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            eprintln!("unparsable line: {line}");
            return ExitCode::FAILURE;
        };
        let Some(password) = value.get("password").and_then(|value| value.as_str()) else {
            eprintln!("line without password: {line}");
            return ExitCode::FAILURE;
        };
        let Some(hash) = value.get("hash").and_then(|value| value.as_str()) else {
            eprintln!("line without hash: {line}");
            return ExitCode::FAILURE;
        };
        match check_password(password, hash) {
            Ok(is_correct) => {
                checked += 1;
                if is_correct {
                    correct += 1;
                }
                if !is_password_usable(hash) {
                    unusable += 1;
                }
                // A second call is cheap for correctness but not for runtime;
                // only ask for `must_update` when the fixture says it matters.
                if value.get("check_must_update").and_then(|v| v.as_bool()) == Some(true) {
                    match verify_password(password, hash) {
                        Ok(check) => {
                            let expected = value
                                .get("expect_must_update")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if check.must_update != expected {
                                errors.push(format!(
                                    "must_update was {}, expected {expected}",
                                    check.must_update
                                ));
                            } else if check.must_update {
                                must_update += 1;
                            }
                        }
                        Err(error) => errors.push(error.to_string()),
                    }
                }
            }
            Err(error) => errors.push(format!("check_password failed: {error}")),
        }
    }
    let summary = serde_json::json!({
        "checked": checked,
        "correct": correct,
        "must_update_true": must_update,
        "unusable": unusable,
        "errors": errors,
    });
    if writeln!(out, "{summary}").is_err() {
        return ExitCode::FAILURE;
    }
    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn write_fixtures() -> std::io::Result<()> {
    let salt = "0123456789abcdefghijkl";
    let password = "Fixture-Passw0rd-2026!";
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    let fresh = make_password_with_salt(Some(password), Some(salt));
    let old = encode("pbkdf2_sha256", password, salt, 600_000, false);
    let sha1 = encode("pbkdf2_sha1", password, salt, 1_000_000, true);
    let unusable = plane_auth::password::make_password(None);
    for (kind, hash, expect_correct, expect_must_update, check_must_update) in [
        ("fresh_pbkdf2_sha256", fresh, true, false, true),
        ("old_iterations", old, true, true, true),
        ("pbkdf2_sha1", sha1, true, true, true),
        ("unusable", unusable, false, false, true),
        (
            "unknown_algorithm",
            format!("md5${salt}$0123456789abcdef0123456789abcdef"),
            false,
            false,
            true,
        ),
    ] {
        let line = serde_json::json!({
            "kind": kind,
            "password": password,
            "hash": hash,
            "expect_correct": expect_correct,
            "expect_must_update": expect_must_update,
            "check_must_update": check_must_update,
        });
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// Encode a fixture with a specific algorithm/iteration count. `sha1` selects
/// PBKDF2-HMAC-SHA1 with a 20-byte output, exactly like
/// `PBKDF2SHA1PasswordHasher`.
fn encode(algorithm: &str, password: &str, salt: &str, iterations: u32, sha1: bool) -> String {
    use base64::Engine;
    let mut output = vec![0u8; if sha1 { 20 } else { 32 }];
    let algorithm_id = if sha1 {
        aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA1
    } else {
        aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA256
    };
    aws_lc_rs::pbkdf2::derive(
        algorithm_id,
        std::num::NonZeroU32::new(iterations).expect("iterations"),
        salt.as_bytes(),
        password.as_bytes(),
        &mut output,
    );
    format!(
        "{algorithm}${iterations}${salt}${}",
        base64::engine::general_purpose::STANDARD.encode(output)
    )
}

fn bench(iterations: u64) -> ExitCode {
    let password = "Bench-Passw0rd-2026!";
    let mut total = 0f64;
    let mut hash = String::new();
    for _ in 0..iterations {
        let started = Instant::now();
        hash = make_password_with_salt(Some(password), None);
        total += started.elapsed().as_secs_f64() * 1000.0;
    }
    let mean = total / iterations.max(1) as f64;
    let started = Instant::now();
    let verified = check_password(password, &hash).unwrap_or(false);
    let verify_ms = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{}",
        serde_json::json!({
            "algorithm": PBKDF2_ALGORITHM,
            "iterations": plane_auth::password::PBKDF2_ITERATIONS,
            "hashes": iterations,
            "mean_hash_ms": mean,
            "verify_ms": verify_ms,
            "verified": verified,
        })
    );
    ExitCode::SUCCESS
}
