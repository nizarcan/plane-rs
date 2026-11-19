// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Differential test: `plane-cache` against the real Django cache.
//!
//! `tools/cache/interop.py` runs `django.core.cache.cache` with Plane's
//! `CACHES` settings against the same Redis database. Three properties are
//! proven:
//!
//! 1. **Python writes, Rust reads.** Every corpus value Python stores decodes
//!    in Rust to the same tree.
//! 2. **Rust writes, Python reads.** Every value Rust stores is readable by
//!    `cache.get` and compares equal to what Python itself would have stored;
//!    the raw bytes are identical too, which is what the differential
//!    harness's Redis comparison sees.
//! 3. **Invalidation parity.** Given the same seeded keyspace and the same
//!    `invalidate_cache_directly` arguments, both sides delete exactly the
//!    same keys (including `multiple=True`'s `KEYS *…*` → `delete_many`).
//!
//! Requires `PLANE_RS_TEST_REDIS_URL` pointing at an isolated dev database
//! (13 or 14 — the tool refuses anything else because it calls `FLUSHDB`).
//! Without the variable the test is skipped with a notice unless
//! `PLANE_RS_REQUIRE_DB_TESTS=1`.

mod common;

use std::collections::BTreeSet;
use std::process::Command;

use common::{build, same};
use plane_cache::drf::{CachedResponse, Invalidation, RequestFacts, invalidate, lookup};
use plane_cache::{Client, ParsedUrl, PyValue, Redis, make_key};
use serde_json::{Value as Json, json};

/// `apps/server/crates/plane-cache` → `apps/api/.venv/bin/python`.
fn django_python() -> std::path::PathBuf {
    plane_oracle::python()
}
/// `apps/server/tools/cache/interop.py`.
const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/cache/interop.py");
/// The Django project root, used as the child's working directory and its
/// `sys.path` entry.
fn django_root() -> std::path::PathBuf {
    plane_oracle::require_api()
}
/// The golden corpus: the values both sides store.
const CORPUS: &str = include_str!("golden/pickle-corpus.json");

/// The scenarios share one Redis database and each `FLUSHDB`s it, so they must
/// not run concurrently even though `cargo test` runs test functions in
/// parallel threads.
static INTEROP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn test_redis_url() -> Option<String> {
    match std::env::var("PLANE_RS_TEST_REDIS_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            let message = "plane-cache interop: PLANE_RS_TEST_REDIS_URL is not set, \
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
        matches!(parsed.db, 13 | 14),
        "the interop test only runs against Redis databases 13-14 (got {}): it FLUSHDBs them",
        parsed.db
    );
    Redis::from_url(url).expect("redis client")
}

/// One corpus case reduced to what the interop needs.
struct Case {
    name: String,
    /// The value as the annotated tree: what Django actually pickles (with
    /// the `ReturnList`/`ReturnDict` wrappers and object sharing).
    tree: Json,
    /// The same value as `PyValue`, for the Rust writes.
    written: PyValue,
    /// What `pickle.loads` gives back, i.e. what `cache.get` returns.
    value: PyValue,
}

/// A representative slice of the corpus: every group, and only values a cache
/// entry can hold.
fn cases() -> Vec<Case> {
    let document: Json = serde_json::from_str(CORPUS).expect("corpus is valid JSON");
    let mut seen_groups = BTreeSet::new();
    let mut out = Vec::new();
    for case in document["cases"].as_array().expect("cases") {
        let group = case["group"].as_str().unwrap_or("misc").to_string();
        // One case per group keeps the run short; the golden test already
        // covers every case's bytes.
        if !seen_groups.insert(group.clone()) {
            continue;
        }
        out.push(Case {
            name: case["name"].as_str().expect("name").to_string(),
            tree: case["tree"].clone(),
            written: build(case["tree"].clone(), &mut Vec::new()),
            value: build(case["value"].clone(), &mut Vec::new()),
        });
    }
    // Plus the shapes the five cached views actually store, at several sizes.
    for case in document["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().unwrap_or_default();
        if name.starts_with("labels-rows-")
            || name.starts_with("estimates-points-")
            || name.starts_with("configurations-rows-")
            || name.starts_with("admins-rows-")
            || name.starts_with("instances-")
            || name.starts_with("history-")
        {
            out.push(Case {
                name: name.to_string(),
                tree: case["tree"].clone(),
                written: build(case["tree"].clone(), &mut Vec::new()),
                value: build(case["value"].clone(), &mut Vec::new()),
            });
        }
    }
    out
}

fn cache_key(case: &Case) -> String {
    format!("interop/{}", case.name)
}

/// Run `tools/cache/interop.py` and return its JSON result.
fn python(url: &str, job: Json, name: &str) -> Json {
    let directory = std::env::temp_dir();
    let job_path = directory.join(format!(
        "plane-cache-interop-{name}-{}-job.json",
        std::process::id()
    ));
    let out_path = directory.join(format!(
        "plane-cache-interop-{name}-{}-out.json",
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
        .env("DJANGO_SETTINGS_MODULE", "plane.settings.production")
        .output()
        .expect("run the interop tool");
    let _ = std::fs::remove_file(&job_path);
    assert!(
        output.status.success(),
        "tools/cache/interop.py {name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&out_path).expect("read interop output");
    let _ = std::fs::remove_file(&out_path);
    serde_json::from_str(&text).expect("interop output is JSON")
}

#[tokio::test(flavor = "current_thread")]
async fn django_and_rust_share_the_cache() {
    let Some(url) = test_redis_url() else {
        return;
    };
    let _guard = INTEROP_LOCK.lock().await;
    let redis = checked_redis(&url);
    let cases = cases();
    assert!(
        cases.len() >= 30,
        "expected a broad interop slice, got {}",
        cases.len()
    );

    // ---- 1. Python writes, Rust reads -----------------------------------
    redis.flush_db().await.expect("flushdb");
    let job = json!({
        "op": "write",
        "cases": cases
            .iter()
            .map(|case| json!({"key": cache_key(case), "tree": case.tree, "timeout": 300}))
            .collect::<Vec<_>>(),
    });
    let written = python(&url, job, "write-by-python");
    let written_hex: Vec<(String, String)> = written["written"]
        .as_array()
        .expect("written")
        .iter()
        .map(|entry| {
            (
                entry["key"].as_str().expect("key").to_string(),
                entry["hex"].as_str().expect("hex").to_string(),
            )
        })
        .collect();
    assert_eq!(written_hex.len(), cases.len());

    let client = Client::new(&redis);
    for (case, (key, expected_hex)) in cases.iter().zip(&written_hex) {
        let value = client
            .get(key)
            .await
            .expect("reads a Django-written value")
            .unwrap_or_else(|| panic!("{key} disappeared"))
            .into_value();
        assert!(
            same(&value, &case.value),
            "{}: Rust read {value:?}, Django stored {:?}",
            case.name,
            case.value
        );
        let raw = client
            .get_raw(&make_key(key))
            .await
            .expect("raw get")
            .expect("raw value");
        assert_eq!(
            plane_cache::hex(&raw),
            *expected_hex,
            "{}: raw bytes",
            case.name
        );
    }

    // ---- 2. Rust writes, Python reads -----------------------------------
    redis.flush_db().await.expect("flushdb");
    for case in &cases {
        client
            .set(&cache_key(case), &case.written, Some(300))
            .await
            .expect("Rust writes");
    }
    let mut rust_hex = Vec::with_capacity(cases.len());
    for case in &cases {
        let raw = client
            .get_raw(&make_key(&cache_key(case)))
            .await
            .expect("raw get")
            .expect("raw value");
        rust_hex.push(plane_cache::hex(&raw));
    }
    // Every byte must equal what Django wrote for the same object: the
    // differential harness compares raw Redis values.
    for ((case, rust), (_, django)) in cases.iter().zip(&rust_hex).zip(&written_hex) {
        assert_eq!(
            rust, django,
            "{}: Rust and Django wrote different bytes for the same object",
            case.name
        );
    }

    let job = json!({
        "op": "read",
        "keys": cases.iter().map(cache_key).collect::<Vec<_>>(),
    });
    let read = python(&url, job, "read-by-python");
    for (case, entry) in cases.iter().zip(read["values"].as_array().expect("values")) {
        // `cache.get` returns its `None` default for a stored `None`, so the
        // `none` case comes back as "not found". The raw value above proved
        // the key is there.
        let value = if entry["found"].as_bool() == Some(true) {
            build(entry["tree"].clone(), &mut Vec::new())
        } else {
            PyValue::None
        };
        assert!(
            same(&value, &case.value),
            "{}: Django read {value:?}, expected {:?}",
            case.name,
            case.value
        );
        // And DRF renders it identically to the Rust renderer. The tool
        // records `binascii.hexlify(JSONRenderer().render(value))`.
        let Some(rendered) = entry["rendered"].as_str() else {
            continue;
        };
        let ours =
            plane_serde::to_vec(plane_serde::Dialect::Drf, &case.value.decayed()).expect("render");
        assert_eq!(
            plane_cache::hex(&ours),
            rendered,
            "{}: Rust and DRF render differently",
            case.name
        );
    }

    // ---- 3. The `{"data": …, "status": …}` envelope ---------------------
    let envelope = CachedResponse {
        data: PyValue::return_list(vec![PyValue::dict(vec![
            ("id", PyValue::uuid(uuid::Uuid::nil())),
            ("name", PyValue::string("bug")),
            ("sort_order", PyValue::float(65535.0)),
        ])]),
        status: 200,
    };
    client
        .set("interop/envelope", &envelope.to_value(), Some(300))
        .await
        .expect("write envelope");
    let hit = lookup(&redis, "interop/envelope")
        .await
        .expect("lookup runs")
        .expect("hit");
    // A hit's `data` is the *unpickled* value: DRF's `ReturnList` came back as
    // a plain list.
    assert_eq!(hit.data, envelope.data.decayed());
    assert_eq!(hit.status, envelope.status);

    // ---- 4. Invalidation parity -----------------------------------------
    let seeds: Vec<Json> = vec![
        json!({"key": "/api/instances/", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "/api/workspaces/acme/labels/", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "/api/workspaces/acme/labels/?limit=10", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "/api/workspaces/acme/labels/:8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "/api/workspaces/beta/labels/", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "/api/users/me/", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "/api/users/me/:8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "/api/users/me/settings/:8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "workspaces:acme:states/", "tree": {"s": "x"}, "timeout": 300}),
        json!({"key": "/unrelated/", "tree": {"s": "x"}, "timeout": 300}),
    ];
    let specs = [
        (
            "labels-multiple-url-params",
            json!({"path": "/api/workspaces/:slug/labels/", "url_params": true, "user": false,
                   "multiple": true,
                   "request": {"full_path": "/api/workspaces/acme/labels/?limit=10",
                               "kwargs": {"slug": "acme"},
                               "user_id": "8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0"}}),
        ),
        (
            "labels-single-user-false",
            json!({"path": "/api/workspaces/:slug/labels/", "url_params": true, "user": false,
                   "multiple": false,
                   "request": {"full_path": "/api/workspaces/acme/labels/",
                               "kwargs": {"slug": "acme"},
                               "user_id": "8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0"}}),
        ),
        (
            "users-me-settings-user-true",
            json!({"path": "/api/users/me/settings/", "url_params": false, "user": true,
                   "multiple": false,
                   "request": {"full_path": "/api/users/me/settings/",
                               "kwargs": {},
                               "user_id": "8f1c0d3e-4b5a-6978-8796-a5b4c3d2e1f0"}}),
        ),
        (
            "instances-single",
            json!({"path": "/api/instances/", "url_params": false, "user": false,
                   "multiple": false,
                   "request": {"full_path": "/api/instances/", "kwargs": {},
                               "user_id": null}}),
        ),
        (
            "states-no-request",
            json!({"path": "workspaces/:slug/states/", "url_params": false, "user": false,
                   "multiple": true, "request": null}),
        ),
        (
            "no-match-multiple",
            json!({"path": "/api/does-not-exist/", "url_params": false, "user": false,
                   "multiple": true,
                   "request": {"full_path": "/api/does-not-exist/", "kwargs": {},
                               "user_id": null}}),
        ),
    ];

    let mut total_deleted = 0_usize;
    let mut multiple_deleted = 0_usize;
    for (name, spec) in &specs {
        let job = json!({"op": "invalidate", "seeds": seeds, "spec": spec});
        let expected = python(&url, job, name);
        let expected_after: BTreeSet<String> = expected["after"]
            .as_array()
            .expect("after")
            .iter()
            .map(|key| key.as_str().expect("key").to_string())
            .collect();

        // Same seed set, then the Rust invalidation.
        redis.flush_db().await.expect("flushdb");
        for seed in &seeds {
            let key = seed["key"].as_str().expect("key");
            let value = build(seed["tree"].clone(), &mut Vec::new());
            client.set(key, &value, Some(300)).await.expect("seed");
        }

        let full_path = spec["request"]["full_path"]
            .as_str()
            .unwrap_or("/")
            .to_string();
        let user_id = spec["request"]["user_id"].as_str().map(str::to_string);
        let kwargs_owned: Vec<(String, String)> = spec["request"]["kwargs"]
            .as_object()
            .map(|map| {
                map.iter()
                    .map(|(key, value)| (key.clone(), value.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let resolver_kwargs: Vec<(&str, String)> = kwargs_owned
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect();
        let request = RequestFacts {
            full_path: &full_path,
            resolver_kwargs: &resolver_kwargs,
            user_id: user_id.as_deref(),
        };
        let invalidation = Invalidation {
            path: spec["path"].as_str(),
            url_params: spec["url_params"].as_bool().unwrap_or(false),
            user: spec["user"].as_bool().unwrap_or(true),
            multiple: spec["multiple"].as_bool().unwrap_or(false),
        };
        let deleted = invalidate(&redis, &invalidation, &request)
            .await
            .expect("invalidation runs");
        total_deleted += deleted;
        if invalidation.multiple {
            multiple_deleted += deleted;
        }

        let mut after: Vec<String> = redis.keys(":1:*").await.expect("list keys");
        after.sort();
        let after: BTreeSet<String> = after.into_iter().collect();
        assert_eq!(
            after, expected_after,
            "{name}: surviving keys differ (Rust deleted {deleted})"
        );
        let expected_deleted: usize = seeds
            .iter()
            .filter(|seed| !after.contains(&format!(":1:{}", seed["key"].as_str().unwrap_or(""))))
            .count();
        assert_eq!(
            deleted, expected_deleted,
            "{name}: Rust reported a different number of deleted keys"
        );
    }
    // The specs must actually delete things, or the comparison is vacuous.
    assert!(total_deleted > 0, "no spec deleted anything");
    assert!(
        multiple_deleted >= 2,
        "the `multiple=True` specs must delete more than one key, deleted {multiple_deleted}"
    );
}

/// `cache.add` and `cache.incr` against the real Django cache: the same
/// boolean/int result, the same raw bytes, and a TTL that is written by `add`
/// and **not** reset by `incr`.
#[tokio::test(flavor = "current_thread")]
async fn django_and_rust_agree_on_add_and_incr() {
    let Some(url) = test_redis_url() else {
        return;
    };
    let _guard = INTEROP_LOCK.lock().await;
    let redis = checked_redis(&url);
    let client = Client::new(&redis);

    // Seeded before each side's run, so both start from identical bytes.
    let seeds = [
        ("interop-add-incr/add-existing", json!({"i": "0"}), 300),
        ("interop-add-incr/add-zero-existing", json!({"i": "0"}), 300),
        ("interop-add-incr/incr", json!({"i": "7"}), 600),
        ("interop-add-incr/incr-twice", json!({"i": "7"}), 600),
        ("interop-add-incr/incr-pickled", json!(true), 600),
    ];
    // `op`, key, value, timeout. `timeout: null` is Django's `timeout=None`
    // (no expiry); `0` is the `nx=True` non-positive branch.
    let cases = [
        (
            "add",
            "interop-add-incr/add-missing",
            json!({"i": "0"}),
            Some(3600),
        ),
        (
            "add",
            "interop-add-incr/add-existing",
            json!({"i": "5"}),
            Some(3600),
        ),
        (
            "add",
            "interop-add-incr/add-zero-missing",
            json!({"i": "0"}),
            Some(0),
        ),
        (
            "add",
            "interop-add-incr/add-zero-existing",
            json!({"i": "5"}),
            Some(0),
        ),
        (
            "add",
            "interop-add-incr/add-no-timeout",
            json!({"i": "0"}),
            None,
        ),
        ("incr", "interop-add-incr/incr", json!({"i": "0"}), None),
        (
            "incr",
            "interop-add-incr/incr-twice",
            json!({"i": "0"}),
            None,
        ),
        (
            "incr",
            "interop-add-incr/incr-twice",
            json!({"i": "0"}),
            None,
        ),
        (
            "incr",
            "interop-add-incr/incr-pickled",
            json!({"i": "0"}),
            None,
        ),
        (
            "incr",
            "interop-add-incr/incr-missing",
            json!({"i": "0"}),
            None,
        ),
    ];

    // Python first: seed, then run every case in order.
    redis.flush_db().await.expect("flushdb");
    for (key, tree, timeout) in &seeds {
        let value = build(tree.clone(), &mut Vec::new());
        client.set(key, &value, Some(*timeout)).await.expect("seed");
    }
    let job = json!({
        "op": "add_incr",
        "cases": cases
            .iter()
            .map(|(op, key, tree, timeout)| json!({
                "op": op,
                "key": key,
                "tree": tree,
                "timeout": timeout,
            }))
            .collect::<Vec<_>>(),
    });
    let expected = python(&url, job, "add-incr");
    let expected = expected["results"].as_array().expect("results").clone();
    assert_eq!(expected.len(), cases.len());

    // Rust: the same seed and the same calls.
    redis.flush_db().await.expect("flushdb");
    for (key, tree, timeout) in &seeds {
        let value = build(tree.clone(), &mut Vec::new());
        client.set(key, &value, Some(*timeout)).await.expect("seed");
    }
    for (index, (op, key, tree, timeout)) in cases.iter().enumerate() {
        let want = &expected[index];
        assert_eq!(want["key"].as_str(), Some(*key), "python case order");
        match *op {
            "add" => {
                let value = build(tree.clone(), &mut Vec::new());
                let added = client.add(key, &value, *timeout).await.expect("add");
                assert_eq!(Some(added), want["result"].as_bool(), "{key}: add result");
            }
            "incr" => {
                let result = client.incr(key, 1).await;
                match want.get("error").and_then(|error| error.as_str()) {
                    Some(message) => {
                        let error = result.expect_err("missing key must be an error");
                        assert_eq!(error.to_string(), message, "{key}: incr error");
                    }
                    None => {
                        let value = result.expect("incr");
                        assert_eq!(Some(value), want["value"].as_i64(), "{key}: incr value");
                    }
                }
            }
            other => panic!("unknown op {other}"),
        }
        let full = plane_cache::make_key(key);
        let raw = client.get_raw(&full).await.expect("raw");
        let hex = raw.map(|bytes| {
            use std::fmt::Write as _;
            let mut out = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                write!(out, "{byte:02x}").expect("hex");
            }
            out
        });
        assert_eq!(
            hex.as_deref(),
            want["hex"].as_str(),
            "{key}: raw bytes after {}",
            op
        );
        let pttl = client.pttl(&full).await.expect("pttl");
        let want_pttl = want["pttl_ms"].as_i64().expect("pttl");
        assert!(
            (pttl - want_pttl).abs() <= 3_000,
            "{key}: pttl {pttl} vs python {want_pttl}"
        );
    }
}
