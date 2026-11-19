// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Differential test: the magic-code state machine against
//! `plane/authentication/provider/credentials/magic_code.py`.
//!
//! `tools/auth/magic_interop.py` runs the real `MagicCodeProvider` against the
//! same Redis database this test uses. Both directions are exercised:
//!
//! 1. **Python issues, Rust verifies.** The record Python writes is read by
//!    Rust (as bytes), the code is compared by the Rust provider, and the three
//!    keys are deleted.
//! 2. **Rust issues, Python verifies.** Python reads the record Rust wrote and
//!    logs in with it, which proves the record's bytes are what
//!    `json.loads(...)["token"]` expects.
//! 3. **The caps agree.** Four issued codes and five verify attempts, then the
//!    exhaustion error and the key deletions at each step.
//! 4. **The counter's window** is the first increment's, and the verify counter
//!    inherits the token's remaining TTL.
//!
//! The code itself is never printed: it is read inside the process that needs
//! it and only lengths, error codes and key state cross the boundary.
//!
//! Requires:
//!
//! * `PLANE_RS_TEST_REDIS_URL` — an isolated dev database (9, 13 or 14;
//!   `tools/auth/magic_interop.py` refuses anything else);
//! * `PLANE_RS_TEST_DATABASE_URL` — the Rust provider's `users` lookup is a
//!   real query (`magic_code.py:126`), so the test needs the schema database
//!   (`dev/mkdb.sh`, see `tests/schema_check.rs`).
//!
//! Without either variable the test is skipped with a notice unless
//! `PLANE_RS_REQUIRE_DB_TESTS=1`.

use std::process::Command;
use std::str::FromStr;

use deadpool_postgres::Client;
use plane_auth::auth_flow::{
    AdapterError, AuthErrorCode, AuthProvider, MagicCodeFlags, MagicCodeProvider, issued_key,
    magic_key, verify_attempts_key,
};
use plane_cache::{EvalArg, ParsedUrl, Redis};
use serde_json::{Value as Json, json};
use uuid::Uuid;

/// `apps/server/crates/plane-auth` → `apps/api/.venv/bin/python`.
fn django_python() -> std::path::PathBuf {
    plane_oracle::python()
}
/// `apps/server/tools/auth/magic_interop.py`.
const SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/auth/magic_interop.py"
);
/// The Django project root.
fn django_root() -> std::path::PathBuf {
    plane_oracle::require_api()
}

static INTEROP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn test_redis_url() -> Option<String> {
    match std::env::var("PLANE_RS_TEST_REDIS_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => skip("PLANE_RS_TEST_REDIS_URL"),
    }
}

fn test_database_url() -> Option<String> {
    match plane_db::schema_check::test_database_url() {
        Ok(Some(url)) => Some(url),
        Ok(None) => skip("PLANE_RS_TEST_DATABASE_URL"),
        Err(message) => panic!("{message}"),
    }
}

fn skip(variable: &str) -> Option<String> {
    let message =
        format!("magic interop: {variable} is not set, skipping the Django/Rust comparison");
    if std::env::var("PLANE_RS_REQUIRE_DB_TESTS").as_deref() == Ok("1") {
        panic!("{message} (PLANE_RS_REQUIRE_DB_TESTS=1)");
    }
    eprintln!("{message}");
    None
}

fn checked_redis(url: &str) -> Redis {
    let parsed = ParsedUrl::parse(url).expect("PLANE_RS_TEST_REDIS_URL must parse");
    assert!(
        matches!(parsed.db, 9 | 13 | 14),
        "the interop test only runs against Redis databases 9, 13 and 14 (got {}): \
         it deletes keys",
        parsed.db
    );
    Redis::from_url(url).expect("redis client")
}

async fn database(url: &str) -> deadpool_postgres::Pool {
    let config = tokio_postgres::Config::from_str(url).expect("PLANE_RS_TEST_DATABASE_URL");
    let manager = deadpool_postgres::Manager::new(config, tokio_postgres::NoTls);
    deadpool_postgres::Pool::builder(manager)
        .max_size(2)
        .build()
        .expect("pool")
}

/// Run `tools/auth/magic_interop.py` and return its JSON result.
fn python(url: &str, job: Json, name: &str) -> Json {
    let directory = std::env::temp_dir();
    let job_path = directory.join(format!(
        "plane-magic-interop-{name}-{}-job.json",
        std::process::id()
    ));
    let out_path = directory.join(format!(
        "plane-magic-interop-{name}-{}-out.json",
        std::process::id()
    ));
    std::fs::write(&job_path, serde_json::to_vec(&job).expect("job")).expect("write interop job");

    let output = Command::new(django_python())
        .arg(SCRIPT)
        .arg("--job")
        .arg(&job_path)
        .arg("--out")
        .arg(&out_path)
        .current_dir(django_root())
        .env("REDIS_URL", url)
        .env("PYTHONPATH", django_root())
        .env("SKIP_ENV_VAR", "0")
        .env("EMAIL_HOST", "127.0.0.1")
        .output()
        .expect("run the interop tool");
    let _ = std::fs::remove_file(&job_path);
    assert!(
        output.status.success(),
        "tools/auth/magic_interop.py {name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&out_path).expect("read interop output");
    let _ = std::fs::remove_file(&out_path);
    serde_json::from_str(&text).expect("interop output is JSON")
}

fn flags() -> MagicCodeFlags {
    MagicCodeFlags {
        smtp_configured: true,
        magic_link_login_enabled: true,
    }
}

/// `token_key`, `:issued` and `:verify_attempts` are all gone.
async fn assert_keys_gone(redis: &Redis, key: &str) {
    for name in [key.to_string(), issued_key(key), verify_attempts_key(key)] {
        assert!(
            !redis.exists(&name).await.expect("exists"),
            "{name} survived"
        );
    }
}

/// Replace the code inside a record with `######`, so two records written by
/// different implementations can be compared byte for byte without printing a
/// login credential.
fn mask_token(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw).into_owned();
    let marker = "\"token\": \"";
    let Some(start) = text.find(marker) else {
        return text;
    };
    let digits_start = start + marker.len();
    let digits_end = text[digits_start..]
        .find('"')
        .map(|offset| digits_start + offset)
        .unwrap_or(digits_start);
    format!("{}######{}", &text[..digits_start], &text[digits_end..])
}

/// The error message the Rust provider raised, or `"ok"`.
fn outcome(error: Result<(), AdapterError>) -> String {
    match error {
        Ok(()) => "ok".to_string(),
        Err(AdapterError::Auth(error)) => error.message().to_string(),
        Err(AdapterError::Internal(message)) => panic!("unexpected 500: {message}"),
    }
}

/// `data["token"]` of a record Rust wrote, for a Rust-side verify.
fn code_of(raw: &[u8]) -> String {
    let data: Json = serde_json::from_slice(raw).expect("record is JSON");
    data["token"]
        .as_str()
        .expect("token is a string")
        .to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn the_magic_state_machine_matches_python() {
    let (Some(url), Some(database_url)) = (test_redis_url(), test_database_url()) else {
        return;
    };
    let _guard = INTEROP_LOCK.lock().await;
    let redis = checked_redis(&url);
    let pool = database(&database_url).await;
    let client: Client = pool.get().await.expect("schema database");
    let suffix = Uuid::new_v4().simple().to_string();

    // ---- 1. Python issues, Rust verifies (record bytes included) --------
    let py_email = format!("ap0b-py-{suffix}@example.com");
    let out = python(
        &url,
        json!({"op": "initiate", "email": py_email, "user_exists": false}),
        "initiate-py",
    );
    assert_eq!(out["outcome"], "ok", "{out}");
    assert_eq!(out["token_len"], 6);
    assert_eq!(out["token_is_six_digits"], true);
    // The record: `{"current_attempt": 0, "email": …, "token": "……"}`.
    assert_eq!(
        out["record"]["keys"],
        json!(["current_attempt", "email", "token"])
    );
    assert_eq!(out["record"]["current_attempt"], 0);
    assert_eq!(out["state"]["token"]["ttl"], 600);
    assert_eq!(out["state"]["issued"]["value_hex"], "31"); // "1"
    assert!(out["state"]["issued"]["ttl"].as_i64().unwrap() <= 600);
    assert_eq!(out["state"]["verify"]["exists"], false);

    let key = magic_key(&py_email);
    let python_record = redis
        .get_bytes(&key)
        .await
        .expect("get")
        .expect("Python wrote the record");
    let python_masked = mask_token(&python_record);
    assert_eq!(
        python_masked,
        format!("{{\"current_attempt\": 0, \"email\": \"{py_email}\", \"token\": \"######\"}}")
    );

    // Rust compares the Python-issued code, read from Redis.
    let mut provider = MagicCodeProvider::new(key.clone(), Some(code_of(&python_record)), flags())
        .expect("provider");
    provider
        .set_user_data(&client, &redis)
        .await
        .expect("Rust verifies the Python-issued code");
    assert_eq!(provider.user_data()["email"], json!(py_email));
    assert_eq!(
        provider.user_data()["user"],
        json!({
            "avatar": "",
            "first_name": "",
            "last_name": "",
            "provider_id": "",
            "is_password_autoset": true,
        })
    );
    assert_keys_gone(&redis, &key).await;

    // ---- 2. Rust issues, Python verifies (record bytes included) --------
    let rs_email = format!("ap0b-rs-{suffix}@example.com");
    let rs_key = magic_key(&rs_email);
    let issued = MagicCodeProvider::new(&rs_email, None, flags())
        .expect("provider")
        .initiate(&client, &redis)
        .await
        .expect("Rust issues a code");
    assert_eq!(issued.key, rs_key);
    assert_eq!(issued.token.len(), 6);
    assert!(
        issued
            .token
            .chars()
            .all(|character| character.is_ascii_digit())
    );
    let rust_record = redis
        .get_bytes(&rs_key)
        .await
        .expect("get")
        .expect("record");
    assert_eq!(
        mask_token(&rust_record),
        format!("{{\"current_attempt\": 0, \"email\": \"{rs_email}\", \"token\": \"######\"}}"),
        "Rust writes the bytes json.dumps writes"
    );
    // `debug` never prints the code.
    assert!(!format!("{issued:?}").contains(&issued.token));

    let out = python(
        &url,
        json!({"op": "verify", "key": rs_key, "code_from_redis": true, "user_exists": false}),
        "verify-rs",
    );
    assert_eq!(out["outcome"], "ok", "{out}");
    assert_eq!(out["user_data_keys"], json!(["email", "user"]));
    assert_eq!(out["user_data_email_matches"], true);
    assert_eq!(out["state"]["token"]["exists"], false);
    assert_eq!(out["state"]["issued"]["exists"], false);
    assert_eq!(out["state"]["verify"]["exists"], false);

    // ---- 3. The issued cap (4) and the verify cap (5) -------------------
    let py_cap_email = format!("ap0b-py-cap-{suffix}@example.com");
    let mut python_cap = Vec::new();
    for _ in 0..5 {
        let out = python(
            &url,
            json!({"op": "initiate", "email": py_cap_email, "user_exists": false}),
            "cap-py",
        );
        python_cap.push(out["outcome"].as_str().unwrap_or_default().to_string());
    }
    assert_eq!(python_cap, vec!["ok", "ok", "ok", "ok", "error"]);

    let rs_cap_email = format!("ap0b-rs-cap-{suffix}@example.com");
    let mut rust_cap = Vec::new();
    for _ in 0..5 {
        let provider = MagicCodeProvider::new(&rs_cap_email, None, flags()).expect("provider");
        rust_cap.push(match provider.initiate(&client, &redis).await {
            Ok(_) => "ok".to_string(),
            Err(AdapterError::Auth(error)) => {
                assert_eq!(error.code, AuthErrorCode::EmailCodeAttemptExhaustedSignUp);
                assert_eq!(
                    error.payload,
                    vec![("email".to_string(), json!(rs_cap_email.clone()))]
                );
                "error".to_string()
            }
            Err(AdapterError::Internal(message)) => panic!("unexpected 500: {message}"),
        });
    }
    assert_eq!(rust_cap, python_cap);

    let py_verify_email = format!("ap0b-py-verify-{suffix}@example.com");
    let out = python(
        &url,
        json!({"op": "initiate", "email": py_verify_email, "user_exists": false}),
        "verify-cap-py-issue",
    );
    assert_eq!(out["outcome"], "ok", "{out}");
    let mut python_attempts = Vec::new();
    for _ in 0..6 {
        let out = python(
            &url,
            json!({
                "op": "verify",
                "key": magic_key(&py_verify_email),
                "code": "0000000",
                "user_exists": false,
            }),
            "verify-cap-py",
        );
        python_attempts.push((
            out["error_message"].as_str().unwrap_or("ok").to_string(),
            out["state"]["token"]["exists"].clone(),
            out["state"]["verify"]["exists"].clone(),
        ));
    }
    assert_eq!(
        python_attempts
            .iter()
            .map(|(message, _, _)| message.as_str())
            .collect::<Vec<_>>(),
        vec![
            "INVALID_MAGIC_CODE_SIGN_UP",
            "INVALID_MAGIC_CODE_SIGN_UP",
            "INVALID_MAGIC_CODE_SIGN_UP",
            "INVALID_MAGIC_CODE_SIGN_UP",
            "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP",
            "EXPIRED_MAGIC_CODE_SIGN_UP",
        ]
    );
    // The fifth attempt deletes the token and the verify counter; the issued
    // counter stays.
    assert_eq!(python_attempts[4].1, json!(false));
    assert_eq!(python_attempts[4].2, json!(false));
    assert_eq!(
        python_attempts[5],
        (
            "EXPIRED_MAGIC_CODE_SIGN_UP".to_string(),
            json!(false),
            json!(false)
        )
    );

    let rs_verify_email = format!("ap0b-rs-verify-{suffix}@example.com");
    let rs_verify_key = magic_key(&rs_verify_email);
    MagicCodeProvider::new(&rs_verify_email, None, flags())
        .expect("provider")
        .initiate(&client, &redis)
        .await
        .expect("issue");
    let mut rust_attempts = Vec::new();
    for _ in 0..6 {
        let mut provider =
            MagicCodeProvider::new(rs_verify_key.clone(), Some("0000000".to_string()), flags())
                .expect("provider");
        let message = outcome(provider.set_user_data(&client, &redis).await);
        rust_attempts.push((
            message,
            redis.exists(&rs_verify_key).await.expect("exists"),
            redis
                .exists(&verify_attempts_key(&rs_verify_key))
                .await
                .expect("exists"),
        ));
    }
    assert_eq!(
        rust_attempts,
        vec![
            ("INVALID_MAGIC_CODE_SIGN_UP".to_string(), true, true),
            ("INVALID_MAGIC_CODE_SIGN_UP".to_string(), true, true),
            ("INVALID_MAGIC_CODE_SIGN_UP".to_string(), true, true),
            ("INVALID_MAGIC_CODE_SIGN_UP".to_string(), true, true),
            (
                "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP".to_string(),
                false,
                false
            ),
            ("EXPIRED_MAGIC_CODE_SIGN_UP".to_string(), false, false),
        ]
    );
    assert_eq!(
        python_attempts
            .iter()
            .map(|(message, token, verify)| (
                message.clone(),
                token.as_bool().unwrap_or(false),
                verify.as_bool().unwrap_or(false)
            ))
            .collect::<Vec<_>>(),
        rust_attempts
    );

    // ---- 4. The counters' windows ---------------------------------------
    // The verify counter gets the token's remaining TTL, not a fresh 600: a
    // token window shortened to 30 seconds counts down with the token.
    let window_email = format!("ap0b-window-{suffix}@example.com");
    let window_key = magic_key(&window_email);
    let issued = MagicCodeProvider::new(&window_email, None, flags())
        .expect("provider")
        .initiate(&client, &redis)
        .await
        .expect("issue");
    let shortened: i64 = redis
        .eval(
            "return redis.call('EXPIRE', KEYS[1], tonumber(ARGV[1]))",
            &[&window_key],
            &[EvalArg::Int(30)],
        )
        .await
        .expect("expire");
    assert_eq!(shortened, 1);
    let mut provider =
        MagicCodeProvider::new(window_key.clone(), Some("0000000".to_string()), flags())
            .expect("provider");
    assert_eq!(
        outcome(provider.set_user_data(&client, &redis).await),
        "INVALID_MAGIC_CODE_SIGN_UP"
    );
    let token_ttl = redis.ttl_seconds(&window_key).await.expect("ttl");
    let verify_ttl = redis
        .ttl_seconds(&verify_attempts_key(&window_key))
        .await
        .expect("ttl");
    assert!((25..=30).contains(&token_ttl), "token ttl {token_ttl}");
    assert_eq!(verify_ttl, token_ttl, "the counter expires with the token");

    let py_window_email = format!("ap0b-window-py-{suffix}@example.com");
    let py_window_key = magic_key(&py_window_email);
    let out = python(
        &url,
        json!({"op": "initiate", "email": py_window_email, "user_exists": false}),
        "window-py-issue",
    );
    assert_eq!(out["outcome"], "ok", "{out}");
    let out = python(
        &url,
        json!({
            "op": "verify",
            "key": py_window_key,
            "code": "0000000",
            "user_exists": false,
        }),
        "window-py-verify",
    );
    assert_eq!(out["error_message"], "INVALID_MAGIC_CODE_SIGN_UP");
    // The same rule on the Python side: the verify counter's TTL is the
    // token's remaining TTL, not a fresh window.
    let py_token_ttl = out["state"]["token"]["ttl"].as_i64().expect("ttl");
    let py_verify_ttl = out["state"]["verify"]["ttl"].as_i64().expect("ttl");
    assert!(
        (py_token_ttl - py_verify_ttl).abs() <= 1,
        "{py_token_ttl} vs {py_verify_ttl}"
    );
    let _ = issued;

    // ---- cleanup ---------------------------------------------------------
    let mut keys = vec![
        rs_verify_key.clone(),
        window_key.clone(),
        py_window_key.clone(),
        magic_key(&py_cap_email),
        magic_key(&rs_cap_email),
        magic_key(&py_verify_email),
    ];
    for key in [rs_verify_key, window_key, py_window_key] {
        keys.push(issued_key(&key));
        keys.push(verify_attempts_key(&key));
    }
    redis.del_many(&keys).await.expect("cleanup");
}
