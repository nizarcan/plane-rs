// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Differential test: the `INCR` + first-`EXPIRE` Lua script against
//! `redis-py`.
//!
//! `tools/auth/magic_interop.py` runs `MagicCodeProvider._INCREMENT_COUNTER_SCRIPT`
//! through the real `redis-py` client against the same Redis database this
//! test uses. Three properties are proven:
//!
//! 1. **Python increments, Rust continues.** The counter value is shared and
//!    the TTL set by the first increment survives every later one.
//! 2. **Rust increments, Python continues.** Same, in the other direction, so
//!    the `EVAL` command this crate builds is the one `redis-py` builds.
//! 3. **`SET … EX` agrees on the value bytes and the TTL.**
//!
//! Requires `PLANE_RS_TEST_REDIS_URL` pointing at an isolated dev database
//! (9, 13 or 14 — the tool refuses anything else). Without the variable the
//! test is skipped with a notice unless `PLANE_RS_REQUIRE_DB_TESTS=1`.

use std::process::Command;

use plane_cache::{EvalArg, ParsedUrl, Redis};
use serde_json::{Value as Json, json};

/// `apps/server/crates/plane-cache` → `apps/api/.venv/bin/python`.
const PYTHON: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../api/.venv/bin/python");
/// `apps/server/tools/auth/magic_interop.py`.
const SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/auth/magic_interop.py"
);
/// The Django project root, used as the child's working directory and its
/// `sys.path` entry.
const DJANGO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../api");

/// Both interop tests share database 9/13/14 and delete keys, so they must not
/// run concurrently even though `cargo test` uses threads.
static INTEROP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn test_redis_url() -> Option<String> {
    match std::env::var("PLANE_RS_TEST_REDIS_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            let message = "eval interop: PLANE_RS_TEST_REDIS_URL is not set, \
                           skipping the redis-py comparison";
            if std::env::var("PLANE_RS_REQUIRE_DB_TESTS").as_deref() == Ok("1") {
                panic!("{message} (PLANE_RS_REQUIRE_DB_TESTS=1)");
            }
            eprintln!("{message}");
            None
        }
    }
}

/// Refuse to touch anything but the databases this task owns.
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

/// Run `tools/auth/magic_interop.py` and return its JSON result.
fn python(url: &str, job: Json, name: &str) -> Json {
    let directory = std::env::temp_dir();
    let job_path = directory.join(format!(
        "plane-eval-interop-{name}-{}-job.json",
        std::process::id()
    ));
    let out_path = directory.join(format!(
        "plane-eval-interop-{name}-{}-out.json",
        std::process::id()
    ));
    std::fs::write(&job_path, serde_json::to_vec(&job).expect("job")).expect("write interop job");

    let output = Command::new(PYTHON)
        .arg(SCRIPT)
        .arg("--job")
        .arg(&job_path)
        .arg("--out")
        .arg(&out_path)
        .current_dir(DJANGO_ROOT)
        .env("REDIS_URL", url)
        .env("PYTHONPATH", DJANGO_ROOT)
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

#[tokio::test(flavor = "current_thread")]
async fn the_counter_script_matches_redis_py() {
    let Some(url) = test_redis_url() else {
        return;
    };
    let _guard = INTEROP_LOCK.lock().await;
    let redis = checked_redis(&url);
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let python_first = format!("interop:eval:python-first:{suffix}");
    let rust_first = format!("interop:eval:rust-first:{suffix}");

    // ---- 1. Python increments, Rust continues ---------------------------
    let out = python(
        &url,
        json!({
            "op": "eval",
            "steps": [
                {"key": python_first, "ttl": 600},
                {"key": python_first, "ttl": 600},
            ],
        }),
        "incr-python",
    );
    assert_eq!(out["steps"][0]["result"], 1);
    assert_eq!(out["steps"][1]["result"], 2);
    // `INCR` kept the first increment's window.
    assert!(out["steps"][1]["ttl"].as_i64().unwrap() <= 600);
    assert!(out["steps"][1]["ttl"].as_i64().unwrap() >= 590);
    assert_eq!(
        redis
            .incr_expire_on_first(&python_first, 600)
            .await
            .expect("rust increments a Python-written key"),
        3
    );
    let ttl = redis.ttl_seconds(&python_first).await.expect("ttl");
    assert!((590..=600).contains(&ttl), "ttl {ttl}");

    // ---- 2. Rust increments, Python continues ---------------------------
    assert_eq!(
        redis
            .incr_expire_on_first(&rust_first, 600)
            .await
            .expect("rust creates the counter"),
        1
    );
    assert_eq!(
        redis
            .incr_expire_on_first(&rust_first, 600)
            .await
            .expect("rust increments again"),
        2
    );
    let out = python(
        &url,
        json!({
            "op": "eval",
            "steps": [{"key": rust_first, "ttl": 600}],
        }),
        "incr-rust",
    );
    assert_eq!(
        out["steps"][0]["result"], 3,
        "redis-py continues Rust's key"
    );
    assert!(out["steps"][0]["ttl"].as_i64().unwrap() <= 600);

    // ---- 3. `SET … EX` agrees on bytes and TTL --------------------------
    let value_key = format!("interop:eval:set:{suffix}");
    redis
        .set_bytes_ex(&value_key, b"{\"a\": 1}", 600)
        .await
        .expect("rust set ex");
    let out = python(
        &url,
        json!({"op": "state", "keys": [value_key], "secret_keys": false}),
        "state",
    );
    let state = &out["keys"][&value_key];
    assert_eq!(state["exists"], true);
    assert_eq!(state["type"], "string");
    assert_eq!(state["value_hex"], "7b2261223a20317d");
    assert!(state["ttl"].as_i64().unwrap() <= 600);

    // A key that expired and is re-created restarts at 1 with a new window:
    // `DEL` then Rust's first increment is the same code path.
    redis.del(&rust_first).await.expect("del");
    assert_eq!(
        redis
            .incr_expire_on_first(&rust_first, 42)
            .await
            .expect("restart"),
        1
    );
    assert_eq!(redis.ttl_seconds(&rust_first).await.expect("ttl"), 42);

    // ---- 4. The general `EVAL` helper round-trips arguments ------------
    // `return {KEYS[1], ARGV[1], ARGV[2]}` — a multi-bulk reply proves the key
    // and argument encoding, including a negative int.
    let reply: Vec<String> = redis
        .eval(
            "return {KEYS[1], ARGV[1], ARGV[2]}",
            &["interop:eval:args"],
            &[EvalArg::Str("a b"), EvalArg::Int(-7)],
        )
        .await
        .expect("eval");
    assert_eq!(reply, vec!["interop:eval:args", "a b", "-7"]);

    // ---- cleanup --------------------------------------------------------
    redis
        .del_many(&[python_first, rust_first, value_key])
        .await
        .expect("cleanup");
}
