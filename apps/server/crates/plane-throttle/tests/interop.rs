// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Differential test: `plane-throttle` against the real Django throttle
//! classes, on the same Redis database.
//!
//! `tools/throttle/interop.py` runs `rest_framework.throttling.*` and Plane's
//! subclasses through `RequestFactory` requests with a frozen `timer`, and
//! records for every step the decision, the cache key, the raw Redis value
//! bytes, the TTL and any `X-RateLimit-*` values. This test runs the same
//! scripted steps through [`plane_throttle::check_throttles`] and compares.
//!
//! Three passes prove the properties the rewrite needs:
//!
//! 1. **oracle** — Django alone runs every step from an empty database;
//! 2. **handoff** — from an empty database, Django runs the first third, Rust
//!    the second third, Django the last third. Every record must match the
//!    oracle, which is simultaneously (a) Rust reading Django's buckets, (b)
//!    Rust writing bytes Django then reads without error, and (c) Django
//!    reading Rust's buckets and reaching the same decision;
//! 3. TTLs are compared with a tolerance because wall-clock time passes
//!    between the two runs.
//!
//! Requires `PLANE_RS_TEST_REDIS_URL` pointing at an isolated dev database
//! (9-11, 13 or 14 — the test refuses anything else because it calls
//! `FLUSHDB`).
//! Without the variable the test is skipped with a notice unless
//! `PLANE_RS_REQUIRE_DB_TESTS=1`.

use std::collections::BTreeMap;
use std::process::Command;

use http::{HeaderMap, HeaderName, HeaderValue};
use plane_core::config::ThrottlingSettings;
use plane_throttle::cache::{make_key, make_key_opt};
use plane_throttle::{
    Cache, ParsedUrl, Redis, StaticUser, ThrottleClass, ThrottleOutcome, ThrottleRequest,
    check_throttles,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// `apps/server/crates/plane-throttle` → `apps/api/.venv/bin/python`.
fn django_python() -> std::path::PathBuf {
    plane_oracle::python()
}
/// `apps/server/tools/throttle/interop.py`.
const SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/throttle/interop.py"
);
/// The Django project root, used as the child's working directory and its
/// `sys.path` entry.
fn django_root() -> std::path::PathBuf {
    plane_oracle::require_api()
}

/// A TTL may drift by this much between the Python run and the Rust run; the
/// write itself is `PX <duration*1000>` on both sides.
const TTL_TOLERANCE_MS: i64 = 3_000;

/// The scenarios share one Redis database and each `FLUSHDB`s it, so they must
/// not run concurrently even though `cargo test` runs test functions in
/// parallel threads. A `tokio::sync::Mutex` is runtime-agnostic here (there is
/// no timer involved), so it serialises across the per-test runtimes.
static INTEROP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// ---------------------------------------------------------------------------
// Scenario description
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Step {
    Request(RequestStep),
    SetHistory {
        key: String,
        history: Vec<f64>,
        timeout: i64,
    },
    Read {
        key: String,
    },
}

#[derive(Serialize)]
struct RequestStep {
    /// Serialised as `"class"`, but a list: a view runs every throttle in
    /// `get_throttles()`, e.g. `UserEndpoint.generate_email_verification_code`
    /// runs `[EmailVerificationThrottle, AuthenticationThrottle]`.
    #[serde(rename = "class")]
    classes: Vec<&'static str>,
    now: f64,
    remote: Option<&'static str>,
    xff: Option<&'static str>,
    authenticated: bool,
    user_pk: Option<String>,
    api_key: Option<&'static str>,
    asset_id: Option<&'static str>,
}

#[derive(Serialize)]
struct Scenario {
    num_proxies: Option<u16>,
    steps: Vec<Step>,
}

impl Scenario {
    fn new(num_proxies: Option<u16>) -> Self {
        Self {
            num_proxies,
            steps: Vec::new(),
        }
    }

    fn request(
        mut self,
        class: ThrottleClass,
        now: f64,
        remote: &'static str,
        xff: Option<&'static str>,
    ) -> Self {
        self.steps.push(Step::Request(RequestStep {
            classes: vec![class.dotted()],
            now,
            remote: Some(remote),
            xff,
            authenticated: false,
            user_pk: None,
            api_key: None,
            asset_id: None,
        }));
        self
    }

    fn request_as_user(
        mut self,
        class: ThrottleClass,
        now: f64,
        remote: &'static str,
        user_pk: Uuid,
    ) -> Self {
        self.steps.push(Step::Request(RequestStep {
            classes: vec![class.dotted()],
            now,
            remote: Some(remote),
            xff: None,
            authenticated: true,
            user_pk: Some(user_pk.to_string()),
            api_key: None,
            asset_id: None,
        }));
        self
    }

    fn request_with_api_key(
        mut self,
        class: ThrottleClass,
        now: f64,
        remote: &'static str,
        api_key: Option<&'static str>,
    ) -> Self {
        self.steps.push(Step::Request(RequestStep {
            classes: vec![class.dotted()],
            now,
            remote: Some(remote),
            xff: None,
            authenticated: false,
            user_pk: None,
            api_key,
            asset_id: None,
        }));
        self
    }

    fn request_with_asset(
        mut self,
        class: ThrottleClass,
        now: f64,
        remote: &'static str,
        asset_id: Option<&'static str>,
    ) -> Self {
        self.steps.push(Step::Request(RequestStep {
            classes: vec![class.dotted()],
            now,
            remote: Some(remote),
            xff: None,
            authenticated: false,
            user_pk: None,
            api_key: None,
            asset_id,
        }));
        self
    }

    fn request_with_classes(
        mut self,
        classes: &[ThrottleClass],
        now: f64,
        remote: &'static str,
        user_pk: Uuid,
    ) -> Self {
        self.steps.push(Step::Request(RequestStep {
            classes: classes.iter().map(|class| class.dotted()).collect(),
            now,
            remote: Some(remote),
            xff: None,
            authenticated: true,
            user_pk: Some(user_pk.to_string()),
            api_key: None,
            asset_id: None,
        }));
        self
    }

    fn set_history(mut self, key: &str, history: &[f64], timeout: i64) -> Self {
        self.steps.push(Step::SetHistory {
            key: key.to_string(),
            history: history.to_vec(),
            timeout,
        });
        self
    }

    fn read(mut self, key: &str) -> Self {
        self.steps.push(Step::Read {
            key: key.to_string(),
        });
        self
    }
}

// ---------------------------------------------------------------------------
// Trace records
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
struct Record {
    op: String,
    allowed: Option<bool>,
    key: Option<String>,
    full_key: Option<String>,
    value_hex: Option<String>,
    ttl: Option<i64>,
    wait: Option<f64>,
    meta: BTreeMap<String, Value>,
    rate: Option<Vec<i64>>,
    none_key_value_hex: Option<String>,
    /// One entry per throttle class that produced a cache key, in
    /// `get_throttles()` order.
    buckets: Vec<Bucket>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
struct Bucket {
    key: Option<String>,
    full_key: Option<String>,
    value_hex: Option<String>,
    ttl: Option<i64>,
}

impl Record {
    /// Everything that must match exactly: the decision, the key, the bytes
    /// written, the header side values and the reported rate.
    fn assert_matches(&self, expected: &Record, index: usize, scenario: &str) {
        let context = || {
            format!(
                "{scenario} step {index} ({}):\nexpected {expected:#?}\nactual   {self:#?}",
                expected.op
            )
        };
        assert_eq!(self.allowed, expected.allowed, "decision — {}", context());
        assert_eq!(self.key, expected.key, "cache key — {}", context());
        assert_eq!(
            self.full_key,
            expected.full_key,
            "redis key — {}",
            context()
        );
        assert_eq!(
            self.value_hex,
            expected.value_hex,
            "raw value bytes — {}",
            context()
        );
        assert_eq!(
            self.none_key_value_hex,
            expected.none_key_value_hex,
            ":1:None bucket — {}",
            context()
        );
        assert_eq!(self.meta, expected.meta, "X-RateLimit-* — {}", context());
        assert_eq!(self.rate, expected.rate, "rate — {}", context());
        assert_eq!(
            self.buckets.len(),
            expected.buckets.len(),
            "bucket count — {}",
            context()
        );
        for (actual_bucket, expected_bucket) in self.buckets.iter().zip(expected.buckets.iter()) {
            assert_eq!(
                actual_bucket.key,
                expected_bucket.key,
                "bucket key — {}",
                context()
            );
            assert_eq!(
                actual_bucket.full_key,
                expected_bucket.full_key,
                "bucket redis key — {}",
                context()
            );
            assert_eq!(
                actual_bucket.value_hex,
                expected_bucket.value_hex,
                "bucket bytes — {}",
                context()
            );
            match (actual_bucket.ttl, expected_bucket.ttl) {
                (None, None) => {}
                (Some(actual), Some(expected)) => assert!(
                    (actual - expected).abs() <= TTL_TOLERANCE_MS,
                    "bucket ttl {actual} vs {expected} ms — {}",
                    context()
                ),
                other => panic!("bucket ttl mismatch {other:?} — {}", context()),
            }
        }

        // DRF's `Throttled(wait)` stores `math.ceil(wait)`; that integer is
        // what the header carries.
        let expected_wait = expected.wait.map(|wait| wait.ceil() as u64);
        // `self.wait` is already through `Throttled.__init__`'s `math.ceil`;
        // the ceiling is idempotent.
        let actual_wait = self.wait.map(|wait| wait.ceil() as u64);
        assert_eq!(actual_wait, expected_wait, "wait — {}", context());

        match (self.ttl, expected.ttl) {
            (None, None) => {}
            (Some(actual), Some(expected)) => {
                assert!(
                    (actual - expected).abs() <= TTL_TOLERANCE_MS,
                    "ttl {actual} vs {expected} ms — {}",
                    context()
                );
            }
            other => panic!("ttl mismatch {other:?} — {}", context()),
        }
    }
}

// ---------------------------------------------------------------------------
// The two runners
// ---------------------------------------------------------------------------

fn test_redis_url() -> Option<String> {
    match std::env::var("PLANE_RS_TEST_REDIS_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            let message = "plane-throttle interop: PLANE_RS_TEST_REDIS_URL is not set, \
                           skipping the Django/Rust Redis comparison";
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
        matches!(parsed.db, 9..=11 | 13 | 14),
        "the interop test only runs against Redis databases 9-11 or 13-14 (got {}): it FLUSHDBs them",
        parsed.db
    );
    Redis::from_url(url).expect("redis client")
}

fn scenario_path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("plane-throttle-{name}-{}.json", std::process::id()))
}

/// Run the Python tool over `steps[from..to]` and return its records.
fn python_trace(
    url: &str,
    scenario: &Scenario,
    name: &str,
    from: usize,
    to: usize,
    flush: bool,
) -> Vec<Record> {
    let path = scenario_path(name);
    std::fs::write(
        &path,
        serde_json::to_vec(scenario).expect("serialise scenario"),
    )
    .expect("write scenario");

    let mut command = Command::new(django_python());
    command
        .current_dir(django_root())
        .arg(SCRIPT)
        .arg(&path)
        .arg("--from-index")
        .arg(from.to_string())
        .arg("--to-index")
        .arg(to.to_string());
    if flush {
        command.arg("--flush");
    }
    command
        .env("DJANGO_SETTINGS_MODULE", "plane.settings.production")
        .env("REDIS_URL", url)
        // Django needs a database URL to import the settings; nothing in this
        // test touches Postgres, so any valid URL will do.
        .env(
            "DATABASE_URL",
            "postgres://plane:plane@127.0.0.1:55432/plane_tpl",
        )
        .env("SECRET_KEY", "throttle-interop-not-for-deployment")
        .env("API_KEY_RATE_LIMIT", "60/minute")
        .env("AUTHENTICATION_RATE_LIMIT", "10/minute")
        .env_remove("NUM_PROXIES");

    let output = command.output().expect("run the Python interop tool");
    assert!(
        output.status.success(),
        "interop.py failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let trace: Value = serde_json::from_slice(&output.stdout).expect("parse interop.py output");
    let steps = trace
        .get("steps")
        .and_then(Value::as_array)
        .expect("trace.steps");

    // The settings Django imported must be the ones the Rust side is about to
    // use, otherwise a "match" would be meaningless.
    assert_eq!(
        trace.get("api_key_rate_limit").and_then(Value::as_str),
        Some("60/minute")
    );
    assert_eq!(
        trace
            .get("authentication_rate_limit")
            .and_then(Value::as_str),
        Some("10/minute")
    );
    assert_eq!(
        trace
            .get("default_throttle_rates")
            .and_then(|rates| rates.get("anon"))
            .and_then(Value::as_str),
        Some("30/minute")
    );
    assert_eq!(
        trace
            .get("default_throttle_rates")
            .and_then(|rates| rates.get("asset_id"))
            .and_then(Value::as_str),
        Some("5/minute")
    );
    assert_eq!(
        trace
            .get("num_proxies")
            .and_then(Value::as_u64)
            .map(|value| value as u16),
        scenario.num_proxies
    );

    steps
        .iter()
        .cloned()
        .map(|step| serde_json::from_value(step).expect("record shape"))
        .collect()
}

/// Run the same steps through `plane-throttle`, producing the same records.
async fn rust_trace(
    redis: &Redis,
    settings: &ThrottlingSettings,
    scenario: &Scenario,
    from: usize,
    to: usize,
) -> Vec<Record> {
    let cache = Cache::new(redis);
    let mut records = Vec::with_capacity(to - from);

    for step in &scenario.steps[from..to] {
        match step {
            Step::Request(request) => {
                let classes: Vec<ThrottleClass> = request
                    .classes
                    .iter()
                    .map(|path| {
                        ThrottleClass::from_dotted(path)
                            .unwrap_or_else(|| panic!("unmapped throttle class {path}"))
                    })
                    .collect();
                let mut headers = HeaderMap::new();
                if let Some(xff) = request.xff {
                    insert_header(&mut headers, "x-forwarded-for", xff);
                }
                if let Some(api_key) = request.api_key {
                    insert_header(&mut headers, "x-api-key", api_key);
                }
                let user = match (request.authenticated, &request.user_pk) {
                    (true, Some(pk)) => {
                        StaticUser::authenticated(Uuid::parse_str(pk).expect("uuid"))
                    }
                    _ => StaticUser::anonymous(),
                };
                let throttle_request = ThrottleRequest {
                    settings,
                    user: &user,
                    headers: &headers,
                    remote_addr: request.remote,
                    asset_id: request.asset_id,
                    scope_override: None,
                };

                let outcome =
                    check_throttles(redis, &classes, &throttle_request, request.now).await;
                let (allowed, wait, rate_limit) = match outcome.expect("throttle evaluation") {
                    ThrottleOutcome::Allowed { rate_limit } => (true, None, rate_limit),
                    ThrottleOutcome::Throttled(rejection) => {
                        (false, rejection.wait.map(|wait| wait as f64), None)
                    }
                };

                // The bucket each throttle class used, in `get_throttles()`
                // order; classes whose `get_cache_key` returned `None` have no
                // bucket.
                let mut buckets = Vec::with_capacity(classes.len());
                for class in &classes {
                    let Some(key) = class.cache_key(&throttle_request) else {
                        continue;
                    };
                    let full = make_key(&key);
                    let value = cache.get_raw(&full).await.expect("get raw");
                    let ttl = cache.pttl(&full).await.expect("pttl");
                    buckets.push(Bucket {
                        key: Some(key),
                        full_key: Some(full),
                        value_hex: value.map(|bytes| plane_cache::hex(bytes.as_slice())),
                        ttl: Some(ttl),
                    });
                }
                let none_key_value_hex = if buckets.is_empty() {
                    cache
                        .get_raw(&make_key_opt(None))
                        .await
                        .expect("get :1:None")
                        .map(|bytes| plane_cache::hex(bytes.as_slice()))
                } else {
                    None
                };
                let first = buckets.first().cloned().unwrap_or(Bucket {
                    key: None,
                    full_key: None,
                    value_hex: None,
                    ttl: None,
                });
                let rate = if classes.len() == 1 {
                    let rate = classes[0].rate(settings).expect("rate");
                    Some(vec![rate.num_requests, rate.duration])
                } else {
                    None
                };

                // The `X-RateLimit-*` values `ApiKeyRateThrottle` writes into
                // `request.META` and `finalize_response` copies to the response.
                let mut meta = BTreeMap::new();
                if let Some(headers) = rate_limit {
                    for (name, value) in headers.as_headers() {
                        meta.insert(
                            name.to_string(),
                            Value::from(value.parse::<i64>().unwrap_or_default()),
                        );
                    }
                }

                records.push(Record {
                    op: "request".to_string(),
                    allowed: Some(allowed),
                    key: first.key,
                    full_key: first.full_key,
                    value_hex: first.value_hex,
                    ttl: first.ttl,
                    wait,
                    meta,
                    rate,
                    none_key_value_hex,
                    buckets,
                });
            }
            Step::SetHistory {
                key,
                history,
                timeout,
            } => {
                cache
                    .set_history(key, history, *timeout)
                    .await
                    .expect("set history");
                let full = make_key(key);
                let value = cache.get_raw(&full).await.expect("get raw");
                let ttl = cache.pttl(&full).await.expect("pttl");
                records.push(Record {
                    op: "set_history".to_string(),
                    allowed: None,
                    key: Some(key.clone()),
                    full_key: Some(full),
                    value_hex: value.map(|bytes| plane_cache::hex(bytes.as_slice())),
                    ttl: Some(ttl),
                    wait: None,
                    meta: BTreeMap::new(),
                    rate: None,
                    none_key_value_hex: None,
                    buckets: Vec::new(),
                });
            }
            Step::Read { key } => {
                let full = make_key(key);
                let value = cache.get_raw(&full).await.expect("get raw");
                let ttl = cache.pttl(&full).await.expect("pttl");
                records.push(Record {
                    op: "read".to_string(),
                    allowed: None,
                    key: Some(key.clone()),
                    full_key: Some(full),
                    value_hex: value.map(|bytes| plane_cache::hex(bytes.as_slice())),
                    ttl: Some(ttl),
                    wait: None,
                    meta: BTreeMap::new(),
                    rate: None,
                    none_key_value_hex: None,
                    buckets: Vec::new(),
                });
            }
        }
    }
    records
}

fn insert_header(headers: &mut HeaderMap, name: &'static str, value: &str) {
    headers.insert(
        HeaderName::from_static(name),
        HeaderValue::from_str(value).expect("header value"),
    );
}

// ---------------------------------------------------------------------------
// The comparison
// ---------------------------------------------------------------------------

async fn run_interop(name: &str, scenario: Scenario) {
    let Some(url) = test_redis_url() else {
        return;
    };
    let _guard = INTEROP_LOCK.lock().await;
    let redis = checked_redis(&url);
    let total = scenario.steps.len();
    let first_split = total / 3;
    let second_split = (total * 2) / 3;

    // 1. Oracle: Django for every step, from an empty database.
    redis.flush_db().await.expect("flush");
    let oracle = python_trace(&url, &scenario, name, 0, total, false);

    // 2. Handoff: Django, then Rust, then Django again, from an empty database.
    redis.flush_db().await.expect("flush");
    let mut handoff = python_trace(&url, &scenario, name, 0, first_split, false);
    let settings = ThrottlingSettings {
        api_key_rate_limit: "60/minute".to_string(),
        authentication_rate_limit: "10/minute".to_string(),
        num_proxies: scenario.num_proxies,
        anon_rate: "30/minute",
        asset_rate: "5/minute",
        email_verification_rate: "3/hour",
    };
    handoff.extend(rust_trace(&redis, &settings, &scenario, first_split, second_split).await);
    handoff.extend(python_trace(
        &url,
        &scenario,
        name,
        second_split,
        total,
        false,
    ));

    assert_eq!(handoff.len(), oracle.len(), "{name}: trace length");
    for (index, (actual, expected)) in handoff.iter().zip(oracle.iter()).enumerate() {
        actual.assert_matches(expected, index, name);
    }

    let (allowed, denied) =
        oracle
            .iter()
            .fold((0, 0), |(allowed, denied), record| match record.allowed {
                Some(true) => (allowed + 1, denied),
                Some(false) => (allowed, denied + 1),
                None => (allowed, denied),
            });
    eprintln!(
        "{name}: {total} steps matched ({allowed} allowed, {denied} denied; \
         Django 0..{first_split}, Rust {first_split}..{second_split}, Django \
         {second_split}..{total})"
    );
}

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

const NOW: f64 = 1_789_000_000.123_456;
const IP: &str = "203.0.113.7";

/// `AnonRateThrottle`: 30/minute, then the 31st request exactly at the limit.
#[tokio::test]
async fn anon_throttle_matches_django() {
    let mut scenario = Scenario::new(None);
    for index in 0..30 {
        scenario = scenario.request(ThrottleClass::Anon, NOW + index as f64, IP, None);
    }
    // The boundary request: 30 already in the window, so this one is refused.
    scenario = scenario.request(ThrottleClass::Anon, NOW + 30.0, IP, None);
    // Exactly one window later every entry is `<= now - duration` and is
    // dropped, so this one is allowed again.
    scenario = scenario.request(ThrottleClass::Anon, NOW + 60.0, IP, None);
    // An authenticated caller is not throttled at all (`get_cache_key` → None).
    scenario = scenario.request_as_user(
        ThrottleClass::Anon,
        NOW + 61.0,
        IP,
        Uuid::parse_str("8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0").unwrap(),
    );
    run_interop("anon", scenario).await;
}

/// `NUM_PROXIES=1`: the rightmost `X-Forwarded-For` entry is the client.
#[tokio::test]
async fn anon_with_one_proxy_matches_django() {
    let scenario = Scenario::new(Some(1))
        .request(
            ThrottleClass::Anon,
            NOW,
            "10.0.0.1",
            Some("1.1.1.1, 2.2.2.2"),
        )
        .request(
            ThrottleClass::Anon,
            NOW + 1.0,
            "10.0.0.1",
            Some("1.1.1.1, 2.2.2.2"),
        )
        .request(
            ThrottleClass::Anon,
            NOW + 2.0,
            "10.0.0.1",
            Some("9.9.9.9, 2.2.2.2"),
        )
        .request(
            ThrottleClass::Anon,
            NOW + 3.0,
            "10.0.0.1",
            Some(" 1.1.1.1 ,  2.2.2.2 "),
        );
    run_interop("anon-num-proxies-1", scenario).await;
}

/// `NUM_PROXIES=0` and `NUM_PROXIES=None`: the two cases the contract calls out.
#[tokio::test]
async fn anon_proxy_edge_cases_match_django() {
    let zero = Scenario::new(Some(0))
        .request(
            ThrottleClass::Anon,
            NOW,
            "10.0.0.1",
            Some("1.1.1.1, 2.2.2.2"),
        )
        .request(
            ThrottleClass::Anon,
            NOW + 1.0,
            "10.0.0.1",
            Some("3.3.3.3, 4.4.4.4"),
        )
        .request(
            ThrottleClass::Anon,
            NOW + 2.0,
            "10.0.0.2",
            Some("1.1.1.1, 2.2.2.2"),
        );
    run_interop("anon-num-proxies-0", zero).await;

    // With NUM_PROXIES unset, DRF joins every XFF entry with the whitespace
    // removed: all clients behind the proxy share one bucket.
    let none = Scenario::new(None)
        .request(
            ThrottleClass::Anon,
            NOW,
            "10.0.0.1",
            Some("1.1.1.1, 2.2.2.2"),
        )
        .request(
            ThrottleClass::Anon,
            NOW + 1.0,
            "10.0.0.2",
            Some("3.3.3.3, 4.4.4.4"),
        )
        .request(ThrottleClass::Anon, NOW + 2.0, "10.0.0.3", None);
    run_interop("anon-num-proxies-none", none).await;
}

/// `AuthenticationThrottle`: 10/minute, keyed on the IP for every caller.
#[tokio::test]
async fn authentication_throttle_matches_django() {
    let mut scenario = Scenario::new(None);
    let user = Uuid::parse_str("8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0").unwrap();
    for index in 0..10 {
        scenario = scenario.request(ThrottleClass::Authentication, NOW + index as f64, IP, None);
    }
    // 11th: refused. Signed in or not does not matter; the ident is the IP.
    scenario = scenario.request_as_user(ThrottleClass::Authentication, NOW + 10.0, IP, user);
    scenario = scenario.request(
        ThrottleClass::Authentication,
        NOW + 11.0,
        "198.51.100.9",
        None,
    );
    run_interop("authentication", scenario).await;
}

/// `EmailVerificationThrottle`: 3/hour per authenticated user, then per IP.
#[tokio::test]
async fn email_verification_throttle_matches_django() {
    let user = Uuid::parse_str("8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0").unwrap();
    let other = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
    let mut scenario = Scenario::new(None);
    for index in 0..3 {
        scenario = scenario.request_as_user(
            ThrottleClass::EmailVerification,
            NOW + index as f64,
            IP,
            user,
        );
    }
    scenario = scenario.request_as_user(ThrottleClass::EmailVerification, NOW + 3.0, IP, user);
    // A different user has their own bucket.
    scenario = scenario.request_as_user(ThrottleClass::EmailVerification, NOW + 4.0, IP, other);
    // Anonymous callers fall back to the IP bucket.
    for index in 0..3 {
        scenario = scenario.request(
            ThrottleClass::EmailVerification,
            NOW + 5.0 + index as f64,
            IP,
            None,
        );
    }
    scenario = scenario.request(ThrottleClass::EmailVerification, NOW + 8.0, IP, None);
    run_interop("email-verification", scenario).await;
}

/// `ApiKeyRateThrottle`: 60/minute per token, plus the keyless `:1:None` read.
#[tokio::test]
async fn api_key_throttle_matches_django() {
    let token = "plane_api_abc123";
    let mut scenario = Scenario::new(None);
    for index in 0..60 {
        scenario = scenario.request_with_api_key(
            ThrottleClass::ApiKey,
            NOW + index as f64,
            IP,
            Some(token),
        );
    }
    // 61st request with the token, half a second before the oldest entry is a
    // full window old: refused, and no X-RateLimit-* headers.
    scenario = scenario.request_with_api_key(ThrottleClass::ApiKey, NOW + 59.5, IP, Some(token));
    // At exactly one window the oldest entry is `<= now - duration` and drops,
    // so this one is allowed again and reports 59 remaining.
    scenario = scenario.request_with_api_key(ThrottleClass::ApiKey, NOW + 60.0, IP, Some(token));
    // No X-Api-Key: allowed, never written, headers computed from `:1:None`.
    scenario = scenario.request_with_api_key(ThrottleClass::ApiKey, NOW + 61.0, IP, None);
    // A different token has its own bucket.
    scenario = scenario.request_with_api_key(
        ThrottleClass::ApiKey,
        NOW + 62.0,
        IP,
        Some("plane_api_def456"),
    );
    run_interop("api-key", scenario).await;
}

/// `AssetRateThrottle`: 5/minute per asset id.
#[tokio::test]
async fn asset_throttle_matches_django() {
    let mut scenario = Scenario::new(None);
    for index in 0..5 {
        scenario =
            scenario.request_with_asset(ThrottleClass::Asset, NOW + index as f64, IP, Some("a1b2"));
    }
    scenario = scenario.request_with_asset(ThrottleClass::Asset, NOW + 5.0, IP, Some("a1b2"));
    // Another asset id, and a request with no asset id (not throttled).
    scenario = scenario.request_with_asset(ThrottleClass::Asset, NOW + 6.0, IP, Some("c3d4"));
    scenario = scenario.request_with_asset(ThrottleClass::Asset, NOW + 7.0, IP, None);
    run_interop("asset", scenario).await;
}

/// `UserEndpoint.generate_email_verification_code` runs two throttles
/// (`plane/app/views/user/base.py:72-80`): `EmailVerificationThrottle` and
/// `AuthenticationThrottle`. Both are evaluated even after one fails, each
/// writes its own bucket, and the refusal carries the larger of the two waits.
#[tokio::test]
async fn multi_throttle_view_matches_django() {
    let user = Uuid::parse_str("8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0").unwrap();
    let classes = [
        ThrottleClass::EmailVerification,
        ThrottleClass::Authentication,
    ];
    let mut scenario = Scenario::new(None);
    // 3/hour runs out at the fourth request while the 10/minute budget is
    // still open: only the email throttle fails, but both buckets grow.
    for index in 0..10 {
        scenario = scenario.request_with_classes(&classes, NOW + index as f64, "203.0.113.7", user);
    }
    // Both exhausted now; the wait is the email one (about an hour, versus
    // under a minute for the IP budget).
    scenario = scenario.request_with_classes(&classes, NOW + 10.0, "203.0.113.7", user);
    scenario = scenario.request_with_classes(&classes, NOW + 11.0, "203.0.113.7", user);
    // The email bucket is per user, the authentication one per IP: another
    // user on the same address still fails on the email scope only.
    let other = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
    scenario = scenario.request_with_classes(&classes, NOW + 12.0, "203.0.113.7", other);
    run_interop("multi-throttle", scenario).await;
}

/// A bucket longer than the current limit, i.e. a rate lowered while a longer
/// history is still cached. DRF refuses with `Throttled(None)` because
/// `available_requests` is non-positive (issue #1438), so there is no
/// `Retry-After` and no "Expected available in" suffix.
#[tokio::test]
async fn overlong_history_matches_django() {
    let ip = "203.0.113.7";
    let key = format!("throttle_anon_{ip}");
    let history: Vec<f64> = (0..40).map(|index| NOW - index as f64).collect();
    let scenario = Scenario::new(None)
        .set_history(&key, &history, 60)
        .request(ThrottleClass::Anon, NOW + 1.0, ip, None)
        // The same bucket one window later has expired and is usable again.
        .request(ThrottleClass::Anon, NOW + 61.0, ip, None);
    run_interop("overlong-history", scenario).await;
}

/// The pickle writer and TTLs, without going through a throttle.
#[tokio::test]
async fn stored_bytes_match_django() {
    let long: Vec<f64> = (0..60).map(|index| NOW + index as f64).collect();
    let scenario = Scenario::new(None)
        .set_history("throttle_anon_empty", &[], 60)
        .set_history("throttle_anon_one", &[NOW], 60)
        .set_history("throttle_anon_two", &[NOW, NOW - 0.5], 60)
        .set_history(
            "throttle_anon_extremes",
            &[1e-5, 1e16, 5e-324, f64::MAX, -1.5e-9, 100.0],
            60,
        )
        .set_history("throttle_anon_long", &long, 60)
        .set_history("throttle_anon_zero_ttl", &[NOW], 1)
        .read("throttle_anon_one")
        .read("throttle_anon_missing");
    run_interop("stored-bytes", scenario).await;
}
