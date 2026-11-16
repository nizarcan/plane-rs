// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `difftest selftest`: exercises the normaliser and the comparator on
//! synthetic cases, with no network or container involved.
//!
//! Three things are proven here:
//! 1. every injected difference is detected, in the right part;
//! 2. every legitimately volatile variation is accepted; and
//! 3. every semantic check (session signature, cookie/payload consistency,
//!    CSRF unmasking, password verification, Redis side effects) detects a
//!    deliberately broken value.
//!
//! A harness that passes case 2 but fails case 1 would be vacuous; the
//! selftest fails unless all three hold.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Utc};
use serde_json::{Value, json};

use crate::celery::CeleryMessage;
use crate::compare::{
    CONTENT_LENGTH_BODYLESS_RULE, Difference, FormattingStatus, check_drf_formatting,
    compare_bodies, compare_celery, compare_db, compare_formatting, compare_headers, compare_redis,
    compare_response_headers, compare_status, drf_compact,
};
use crate::config::{
    DbRule, NamedRule, NormalizeRules, RuleCheck, RuleMode, ShapeRule, UnorderedMode,
    UnorderedPathRule,
};
use crate::db::{DbDiff, TableDiff, diff_snapshots};
use crate::normalize::{BodyValue, NormalizedBody, NormalizerConfig, TargetNormalizer, Violation};
use crate::password;
use crate::redis::{RedisDiff, RedisEntry};
use crate::session;
use crate::timeparse::parse_timestamp;

struct Checks {
    count: usize,
}

impl Checks {
    fn new() -> Self {
        Self { count: 0 }
    }

    fn ok(&mut self, label: &str, condition: bool) -> Result<()> {
        self.count += 1;
        if condition {
            Ok(())
        } else {
            bail!("selftest failed: {label}")
        }
    }

    fn accepted(&mut self, label: &str, differences: &[Difference]) -> Result<()> {
        self.count += 1;
        if differences.is_empty() {
            Ok(())
        } else {
            bail!(
                "selftest failed: {label} produced {} difference(s): {:#?}",
                differences.len(),
                differences
            );
        }
    }

    fn detected(&mut self, label: &str, part: &str, differences: &[Difference]) -> Result<()> {
        self.count += 1;
        if differences.is_empty() {
            return Err(anyhow::anyhow!(
                "selftest failed: {label} was not detected (vacuous comparison)"
            ));
        }
        if !differences.iter().any(|difference| difference.part == part) {
            bail!(
                "selftest failed: {label} was detected but not in part {part:?}: {:?}",
                differences
                    .iter()
                    .map(|difference| difference.part.clone())
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
    }
}

fn shape(pattern: &str) -> ShapeRule {
    ShapeRule {
        pattern: pattern.to_string(),
        why: "selftest shape".to_string(),
    }
}

fn named(name: &str, why: &str, mode: RuleMode) -> NamedRule {
    NamedRule {
        name: name.to_string(),
        why: why.to_string(),
        mode,
        placeholder: None,
        shape: None,
        check: None,
        request: None,
    }
}

fn placeholder(name: &str, kind: &str, shape: &str) -> NamedRule {
    NamedRule {
        name: name.to_string(),
        why: format!("{name} is random"),
        mode: RuleMode::Placeholder,
        placeholder: Some(kind.to_string()),
        shape: Some(shape.to_string()),
        check: None,
        request: None,
    }
}

fn db_placeholder(table: &str, column: &str, kind: &str, shape: &str) -> DbRule {
    DbRule {
        table: table.to_string(),
        column: column.to_string(),
        why: format!("{table}.{column} is random"),
        mode: RuleMode::Placeholder,
        placeholder: Some(kind.to_string()),
        shape: Some(shape.to_string()),
        check: None,
        request: None,
    }
}

fn rules() -> Arc<NormalizeRules> {
    let mut shapes = BTreeMap::new();
    shapes.insert(
        "session_key".to_string(),
        shape(session::SESSION_KEY_PATTERN),
    );
    shapes.insert("csrf_secret".to_string(), shape(r"^[A-Za-z0-9]{32}$"));
    shapes.insert("csrf_token".to_string(), shape(r"^[A-Za-z0-9]{64}$"));
    shapes.insert("uuid_hex".to_string(), shape(r"^[0-9a-f]{32}$"));
    shapes.insert("hex64".to_string(), shape(r"^[0-9a-f]{64}$"));
    shapes.insert("hex_colour".to_string(), shape(r"^#[0-9a-fA-F]{6}$"));
    shapes.insert(
        "django_password_hash".to_string(),
        shape(password::SHAPE_PATTERN),
    );
    shapes.insert(
        "signed_session".to_string(),
        shape(session::SESSION_DATA_PATTERN),
    );
    shapes.insert("auth_user_hash".to_string(), shape(r"^[0-9a-f]{64}$"));
    shapes.insert("anchor_hex".to_string(), shape(r"^[0-9a-f]{32}$"));

    Arc::new(NormalizeRules {
        ignore_headers: vec![
            "date".to_string(),
            "server".to_string(),
            "content-length".to_string(),
        ],
        shapes,
        volatile_cookies: vec![
            placeholder("session-id", "session", "session_key"),
            placeholder("admin-session-id", "session", "session_key"),
            placeholder("csrftoken", "csrf-secret", "csrf_secret"),
        ],
        volatile_json_keys: vec![
            NamedRule {
                name: "csrf_token".to_string(),
                why: "masked csrf token".to_string(),
                mode: RuleMode::Placeholder,
                placeholder: Some("csrf-token".to_string()),
                shape: Some("csrf_token".to_string()),
                check: Some(RuleCheck::CsrfMasked),
                request: None,
            },
            placeholder("username", "username", "uuid_hex"),
            NamedRule {
                name: "token".to_string(),
                why: "random api token, empty until generated".to_string(),
                mode: RuleMode::PlaceholderIfNonEmpty,
                placeholder: Some("token".to_string()),
                shape: Some("hex64".to_string()),
                check: None,
                request: None,
            },
            placeholder("background_color", "colour", "hex_colour"),
            NamedRule {
                name: "anchor".to_string(),
                why: "deploy board anchor: uuid4().hex on create".to_string(),
                mode: RuleMode::PlaceholderIfNew,
                placeholder: Some("anchor".to_string()),
                shape: Some("anchor_hex".to_string()),
                check: None,
                request: None,
            },
            named("epoch", "whole-second wall clock", RuleMode::UnixTime),
        ],
        volatile_headers: vec![
            named(
                "X-RateLimit-Reset",
                "now + the throttle window",
                RuleMode::UnixTime,
            ),
            named(
                "Expires",
                "cache_page writes http_date(now + 7200)",
                RuleMode::HttpDate,
            ),
        ],
        volatile_celery_headers: vec![
            named("id", "celery task id", RuleMode::Drop),
            named("root_id", "celery root id", RuleMode::Drop),
            named("parent_id", "celery parent id", RuleMode::Drop),
            named("origin", "hostname:pid", RuleMode::Drop),
        ],
        unordered_json_paths: Vec::new(),
        volatile_db_columns: vec![
            db_placeholder("sessions", "session_key", "session", "session_key"),
            DbRule {
                table: "sessions".to_string(),
                column: "session_data".to_string(),
                why: "signed payload".to_string(),
                mode: RuleMode::SessionPayload,
                placeholder: None,
                shape: Some("signed_session".to_string()),
                check: None,
                request: None,
            },
            DbRule {
                table: "sessions".to_string(),
                column: "expire_date".to_string(),
                why: "now + cookie age".to_string(),
                mode: RuleMode::RelativeTime,
                placeholder: None,
                shape: None,
                check: None,
                request: None,
            },
            DbRule {
                table: "deploy_boards".to_string(),
                column: "anchor".to_string(),
                why: "deploy board anchor: uuid4().hex on create".to_string(),
                mode: RuleMode::PlaceholderIfNew,
                placeholder: Some("anchor".to_string()),
                shape: Some("anchor_hex".to_string()),
                check: None,
                request: None,
            },
            db_placeholder("users", "username", "username", "uuid_hex"),
            DbRule {
                table: "users".to_string(),
                column: "password".to_string(),
                why: "salted hash".to_string(),
                mode: RuleMode::Password,
                placeholder: None,
                shape: Some("django_password_hash".to_string()),
                check: None,
                request: None,
            },
            DbRule {
                table: "users".to_string(),
                column: "token".to_string(),
                why: "random api token".to_string(),
                mode: RuleMode::PlaceholderIfNonEmpty,
                placeholder: Some("token".to_string()),
                shape: Some("hex64".to_string()),
                check: None,
                request: None,
            },
            db_placeholder("profiles", "background_color", "colour", "hex_colour"),
            db_placeholder("workspaces", "background_color", "colour", "hex_colour"),
        ],
        timestamp_slack_secs: 2,
        db_schemas: vec!["public".to_string()],
        db_exclude_tables: vec![],
    })
}

const SECRET_KEY: &[u8] = b"contract-test-secret-key-not-for-deployment";
/// HTTP_CONTRACT.md Appendix B.1.
const SESSION_VECTOR: &str = ".eJxNjstuwyAQRX8lYtVKNgbj97L7fIM1MENM64JlSNU2yr8XR1l0O_fec-bGZrimZb5G2meHbGLCSqpRmbLRLZTd2A_l0I9dCa1ujMKapBWsYBrMB_ljgO_gL4Gb4NPuND9w_JlGfg5I69uzW_yXLRCXvB5tbRstUZEwA_S6w5Yaq6DW0ggcabA9dLo1TW7UVoLQoxmwp8620GhlDmikGF3wM31vbv9hk-qEKBjSlzM0O28Dm27sIYUL-ZSt5_Dr1hWqlovTy-NzMOm07UHTaya6bQbEPWNztxaKCy6l4n2OMHyC8_m8pLRNVbUGA-sSYqrY_f4HL9RrZQ:1wJND7:ywTmmvOLHYvBCPzyrQ_HeauuOal3DUrNa43jfPUyJbQ";
const CSRF_SECRET: &str = "0123456789abcdefghijklmnopqrstuv";
const CSRF_MASK: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef";
const CSRF_MASKED: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefqsuwyACEGIKMOQSUWY02468aceqsuwyA";
const VECTOR_USER_ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const PASSWORD: &str = "Diff-Test-Passw0rd-2026!";
const PASSWORD_HASH: &str =
    "pbkdf2_sha256$1000$SALTfixedSALTfixedSALT$w/D65kZ20JoZcd3sU7BR8JQMIi9dafeN0wBzp5J1hUo=";

fn normalizer(base_url: &str) -> TargetNormalizer {
    normalizer_with(base_url, Some(SECRET_KEY), &[PASSWORD])
}

fn normalizer_with(base_url: &str, secret: Option<&[u8]>, passwords: &[&str]) -> TargetNormalizer {
    TargetNormalizer::new(NormalizerConfig {
        rules: rules(),
        base_url: base_url.to_string(),
        known_uuids: Arc::new(HashSet::new()),
        secret_key: secret.map(|secret| Arc::new(secret.to_vec())),
        passwords: Arc::new(passwords.iter().map(|value| value.to_string()).collect()),
        aws: None,
    })
}

fn time(text: &str) -> DateTime<Utc> {
    parse_timestamp(text).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

fn body(value: Value) -> NormalizedBody {
    NormalizedBody {
        value: BodyValue::Json(value),
        modified: false,
    }
}

fn celery_message(task: &str, args: Value) -> CeleryMessage {
    CeleryMessage {
        queue: "celery".to_string(),
        task: Some(task.to_string()),
        headers: BTreeMap::new(),
        properties: BTreeMap::new(),
        args,
        kwargs: json!({}),
        body_extra: None,
    }
}

fn db_diff(table: &str, inserted: Vec<Value>, deleted: Vec<Value>, updated: Vec<Value>) -> DbDiff {
    let mut diff = DbDiff::default();
    diff.tables.insert(
        table.to_string(),
        TableDiff {
            inserted,
            deleted,
            updated,
        },
    );
    diff
}

fn violations(normalizer: &mut TargetNormalizer) -> Vec<Violation> {
    normalizer.take_violations()
}

fn violation_differences(normalizer: &mut TargetNormalizer) -> Vec<Difference> {
    violations(normalizer)
        .into_iter()
        .map(|violation| Difference::new(violation.part, violation.path, violation.message))
        .collect()
}

fn session_key(seed: char) -> String {
    // 128 characters of the seed: 'a' is a valid Django key, 'A' exercises
    // the wrong-alphabet shape check.
    std::iter::repeat_n(seed, 128).collect()
}

fn session_row(key: &str, payload: &str, user_id: &str) -> Value {
    // Mirrors plane/db/models/session.py: the columns are derived from the
    // signed payload, so a row where they disagree is a semantic violation.
    json!({
        "session_key": key,
        "session_data": payload,
        "user_id": user_id,
        "device_info": {
            "user_agent": "Mozilla/5.0 (contract probe)",
            "ip_address": "203.0.113.7",
            "domain": "http://localhost/",
        },
        "expire_date": "2026-11-22T20:00:00.123+00:00",
    })
}

pub fn run() -> Result<()> {
    let mut checks = Checks::new();
    println!("selftest: normaliser");

    // -- UUID placeholding -------------------------------------------------
    let uuid_a = "11111111-1111-4111-8111-111111111111";
    let uuid_b = "22222222-2222-4222-8222-222222222222";
    let uuid_c = "33333333-3333-4333-8333-333333333333";
    let uuid_d = "44444444-4444-4444-8444-444444444444";
    let uuid_e = "55555555-5555-4555-8555-555555555555";

    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    let start = time("2026-11-15T20:00:00Z");
    a.start_step(start, start);
    b.start_step(start, start);

    let headers_a = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "Location".to_string(),
            format!("http://127.0.0.1:18000/items/{uuid_a}/"),
        ),
        ("X-Request-Id".to_string(), uuid_b.to_string()),
    ];
    let headers_b = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "Location".to_string(),
            format!("http://127.0.0.1:18001/items/{uuid_c}/"),
        ),
        ("X-Request-Id".to_string(), uuid_d.to_string()),
    ];
    let norm_a = a.normalize_headers(&headers_a);
    let norm_b = b.normalize_headers(&headers_b);
    checks.accepted("header UUID + base URL", &compare_headers(&norm_a, &norm_b))?;
    checks.ok(
        "Location placeholder is <id:1>",
        norm_a[1].1 == "<base>/items/<id:1>/" && norm_b[1].1 == "<base>/items/<id:1>/",
    )?;

    let body_a = a.normalize_body(
        format!(r#"{{"id":"{uuid_e}","url":"http://127.0.0.1:18000/x/{uuid_e}"}}"#).as_bytes(),
        true,
    );
    let body_b = b.normalize_body(
        format!(r#"{{"id":"{uuid_e}","url":"http://127.0.0.1:18001/x/{uuid_e}"}}"#).as_bytes(),
        true,
    );
    checks.accepted("body UUID + base URL", &compare_bodies(&body_a, &body_b))?;
    checks.ok(
        "body id placeholder follows headers",
        body_a.pretty().contains("<id:3>") && body_b.pretty().contains("<id:3>"),
    )?;

    // Known UUIDs (seeded rows) are left alone.
    let known = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".to_string();
    checks.ok("baseline UUIDs are not placeholdered", {
        let mut normalizer = TargetNormalizer::new(NormalizerConfig {
            rules: rules(),
            base_url: "http://x".to_string(),
            known_uuids: Arc::new(HashSet::from([known.clone()])),
            secret_key: Some(Arc::new(SECRET_KEY.to_vec())),
            passwords: Arc::new(Vec::new()),
            aws: None,
        });
        normalizer.start_step(start, start);
        normalizer.normalize_text(&known) == known
    })?;
    // DECISIONS.md D16 + the `anchor` rule in `scenarios/normalize.yaml`: a
    // deploy board created during a run gets `uuid4().hex`, which differs per
    // side, so both must normalise to the same `<anchor:1>`; a fixture anchor
    // is in the baseline set, stays literal, and a change is still a diff.
    let fixture_anchor = "0123456789abcdef0123456789abcdef".to_string();
    let anchor_rules = rules();
    checks.ok("a generated deploy-board anchor is one placeholder", {
        let mut a = TargetNormalizer::new(NormalizerConfig {
            rules: Arc::clone(&anchor_rules),
            base_url: "http://x".to_string(),
            known_uuids: Arc::new(HashSet::from([fixture_anchor.clone()])),
            secret_key: Some(Arc::new(SECRET_KEY.to_vec())),
            passwords: Arc::new(Vec::new()),
            aws: None,
        });
        let mut b = TargetNormalizer::new(NormalizerConfig {
            rules: Arc::clone(&anchor_rules),
            base_url: "http://x".to_string(),
            known_uuids: Arc::new(HashSet::from([fixture_anchor.clone()])),
            secret_key: Some(Arc::new(SECRET_KEY.to_vec())),
            passwords: Arc::new(Vec::new()),
            aws: None,
        });
        a.start_step(start, start);
        b.start_step(start, start);
        let body_a = a.normalize_body(
            br#"{"anchor":"4f3c2b1a09988776655443322110ffee","views":{}}"#,
            true,
        );
        let body_b = b.normalize_body(
            br#"{"anchor":"00ffeeddccbbaa998877665544332211","views":{}}"#,
            true,
        );
        compare_bodies(&body_a, &body_b).is_empty()
            && body_a.pretty().contains("<anchor:1>")
            && body_b.pretty().contains("<anchor:1>")
            && a.applied().iter().any(|rule| rule.rule.contains("anchor"))
    })?;
    checks.ok("a fixture anchor stays literal and a change is a diff", {
        let mut a = TargetNormalizer::new(NormalizerConfig {
            rules: Arc::clone(&anchor_rules),
            base_url: "http://x".to_string(),
            known_uuids: Arc::new(HashSet::from([fixture_anchor.clone()])),
            secret_key: Some(Arc::new(SECRET_KEY.to_vec())),
            passwords: Arc::new(Vec::new()),
            aws: None,
        });
        let mut b = TargetNormalizer::new(NormalizerConfig {
            rules: Arc::clone(&anchor_rules),
            base_url: "http://x".to_string(),
            known_uuids: Arc::new(HashSet::from([fixture_anchor.clone()])),
            secret_key: Some(Arc::new(SECRET_KEY.to_vec())),
            passwords: Arc::new(Vec::new()),
            aws: None,
        });
        a.start_step(start, start);
        b.start_step(start, start);
        let body_a = a.normalize_body(
            format!(r#"{{"anchor":"{fixture_anchor}"}}"#).as_bytes(),
            true,
        );
        let body_b = b.normalize_body(br#"{"anchor":"ffffffffffffffffffffffffffffffff"}"#, true);
        // A keeps the literal; B, seeing a value the fixture never had, makes
        // it a placeholder — so the two bodies differ and the step fails.
        // The fixture's own anchors are human-readable ("fixture-anchor-alpha"),
        // so a baseline literal must not be shape-checked either.
        body_a.pretty().contains(&fixture_anchor)
            && !body_a.pretty().contains("<anchor:1>")
            && !compare_bodies(&body_a, &body_b).is_empty()
    })?;

    // The kombu envelope is the same UUID in Celery's JSON encoding. It has to
    // follow the text rule, or a fixture id becomes `<id:1>` on one side only
    // (the side that spells the argument as a JSON string).
    checks.ok("a baseline UUID inside a kombu envelope keeps its value", {
        let mut normalizer = TargetNormalizer::new(NormalizerConfig {
            rules: rules(),
            base_url: "http://x".to_string(),
            known_uuids: Arc::new(HashSet::from([known.clone()])),
            secret_key: Some(Arc::new(SECRET_KEY.to_vec())),
            passwords: Arc::new(Vec::new()),
            aws: None,
        });
        normalizer.start_step(start, start);
        let hex = known.replace('-', "");
        let original = json!([{ "__type__": "uuid", "__value__": { "hex": hex } }]);
        let mut message = celery_message("plane.tasks.test", original.clone());
        normalizer.normalize_celery(&mut message);
        message.args == original
    })?;
    checks.ok(
        "a generated UUID inside a kombu envelope still gets a placeholder",
        {
            let mut normalizer = normalizer("http://x");
            normalizer.start_step(start, start);
            let generated = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".to_string();
            let mut message = celery_message(
                "plane.tasks.test",
                json!([{ "__type__": "uuid", "__value__": { "hex": generated.replace('-', "") } }]),
            );
            normalizer.normalize_celery(&mut message);
            message.args == json!([{ "__type__": "uuid", "__value__": { "hex": "<id:1>" } }])
        },
    )?;

    // -- timestamps --------------------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(time("2026-11-15T20:00:00Z"), time("2026-11-15T20:00:00Z"));
    b.start_step(time("2026-11-15T20:00:02Z"), time("2026-11-15T20:00:02Z"));
    let ts_a = a.normalize_body(br#"{"created_at":"2026-11-15T20:00:01.123456Z"}"#, true);
    let ts_b = b.normalize_body(br#"{"created_at":"2026-11-15T20:00:02.654321Z"}"#, true);
    checks.accepted("in-window timestamps", &compare_bodies(&ts_a, &ts_b))?;
    checks.ok(
        "timestamp rendered as <now>",
        ts_a.pretty().contains("<now>") && ts_b.pretty().contains("<now>"),
    )?;
    let older = "2024-01-01T00:00:00Z";
    checks.ok(
        "out-of-window timestamps are compared exactly",
        a.normalize_text(older) == older,
    )?;

    // -- typed placeholders and cross-references --------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let key_a = session_key('a');
    let key_b = session_key('b');
    let cookie_a = a.normalize_headers(&[(
        "Set-Cookie".to_string(),
        format!("session-id={key_a}; Path=/; HttpOnly; SameSite=Lax"),
    )]);
    let cookie_b = b.normalize_headers(&[(
        "Set-Cookie".to_string(),
        format!("session-id={key_b}; Path=/; HttpOnly; SameSite=Lax"),
    )]);
    checks.accepted(
        "session cookie values are placeholders",
        &compare_headers(&cookie_a, &cookie_b),
    )?;
    checks.ok(
        "session cookie placeholder is typed",
        cookie_a[0].1.starts_with("session-id=<session:1>;"),
    )?;

    // The same raw key in the database row gets the same placeholder: the
    // cookie and the row cross-reference each other.
    let mut db_a = db_diff(
        "public.sessions",
        vec![session_row(&key_a, SESSION_VECTOR, VECTOR_USER_ID)],
        vec![],
        vec![],
    );
    let mut db_b = db_diff(
        "public.sessions",
        vec![session_row(&key_b, SESSION_VECTOR, VECTOR_USER_ID)],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut db_a);
    b.normalize_db_diff(&mut db_b);
    checks.accepted(
        "session row decodes and compares",
        &compare_db(&db_a, &db_b),
    )?;
    checks.ok(
        "the cookie and the row share one placeholder",
        db_a.tables["public.sessions"].inserted[0]["session_key"] == "<session:1>",
    )?;
    checks.ok(
        "the decoded payload is compared as an object",
        db_a.tables["public.sessions"].inserted[0]["session_data"]["backend"]
            == json!("django.contrib.auth.backends.ModelBackend")
            && db_a.tables["public.sessions"].inserted[0]["session_data"]["$signed_at"]
                == json!("<epoch>"),
    )?;
    checks.ok(
        "the session row is checked against its payload",
        violations(&mut a).is_empty() && violations(&mut b).is_empty(),
    )?;

    // A row whose key does not match the cookie its response set is a
    // database difference: B's row gets a second placeholder.
    let mut b_broken = normalizer("http://127.0.0.1:18001");
    b_broken.start_step(start, start);
    b_broken.normalize_headers(&[(
        "Set-Cookie".to_string(),
        format!("session-id={key_b}; Path=/"),
    )]);
    let mut db_broken = db_diff(
        "public.sessions",
        vec![session_row(&session_key('c'), SESSION_VECTOR, uuid_b)],
        vec![],
        vec![],
    );
    b_broken.normalize_db_diff(&mut db_broken);
    checks.detected(
        "a session row that does not match its cookie",
        "db",
        &compare_db(&db_a, &db_broken),
    )?;

    // A row whose user_id column disagrees with the signed payload is a
    // semantic violation, even when both targets do the same thing.
    let mut broken_row = normalizer("http://127.0.0.1:18000");
    broken_row.start_step(start, start);
    let mut db_semantic = db_diff(
        "public.sessions",
        vec![session_row(&key_a, SESSION_VECTOR, VECTOR_USER_ID)],
        vec![],
        vec![],
    );
    broken_row.normalize_db_diff(&mut db_semantic);
    checks.ok(
        "the payload's _auth_user_id is normalised",
        db_semantic.tables["public.sessions"].inserted[0]["session_data"]["_auth_user_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("<id:")),
    )?;
    // A row whose column disagrees with the payload is a semantic violation,
    // even when both targets do the same thing.
    let mut wrong_normalizer = normalizer("http://127.0.0.1:18000");
    wrong_normalizer.start_step(start, start);
    let mut raw_wrong = db_diff(
        "public.sessions",
        vec![session_row(&key_a, SESSION_VECTOR, uuid_c)],
        vec![],
        vec![],
    );
    wrong_normalizer.normalize_db_diff(&mut raw_wrong);
    checks.detected(
        "a sessions.user_id column that disagrees with session_data",
        "session",
        &violation_differences(&mut wrong_normalizer),
    )?;

    // -- write-path DB diffs (Phase 5) -------------------------------------
    // The rows below model a `stickies` update. `updated_by` and the two ids
    // are baseline fixture UUIDs, so they are not placeholdered: a wrong actor
    // is a byte difference, exactly as it is against the real database.
    let actor = uuid_a;
    let other_actor = uuid_c;
    let row_id = uuid_d;
    let baseline = "2026-01-01T00:00:00Z";
    let in_window = start.to_rfc3339();
    let write_normalizer = |base_url: &str, known: &[&str]| {
        let mut normalizer = TargetNormalizer::new(NormalizerConfig {
            rules: rules(),
            base_url: base_url.to_string(),
            known_uuids: Arc::new(known.iter().map(|value| (*value).to_string()).collect()),
            secret_key: Some(Arc::new(SECRET_KEY.to_vec())),
            passwords: Arc::new(Vec::new()),
            aws: None,
        });
        normalizer.start_step(start, start);
        normalizer
    };
    let sticky_row =
        |updated_by: &str, updated_at: &str, deleted_at: Option<&str>, sequence: i64| {
            json!({
                "id": row_id,
                "name": "A sticky",
                "sequence_id": sequence,
                "created_at": baseline,
                "updated_at": updated_at,
                "deleted_at": deleted_at,
                "created_by_id": actor,
                "updated_by_id": updated_by,
            })
        };
    let known = [actor, other_actor, row_id];
    let mut wa = write_normalizer("http://127.0.0.1:18000", &known);
    let mut wb = write_normalizer("http://127.0.0.1:18001", &known);
    let mut base_diff = db_diff(
        "public.stickies",
        vec![],
        vec![],
        vec![json!({"pk": [row_id], "changed": {
            "name": {"a": "old", "b": "A sticky"},
            "updated_at": {"a": baseline, "b": in_window},
            "updated_by_id": {"a": actor, "b": actor},
        }})],
    );
    wa.normalize_db_diff(&mut base_diff);
    let mut same_diff = base_diff.clone();
    wb.normalize_db_diff(&mut same_diff);
    checks.accepted(
        "an identical write compares",
        &compare_db(&base_diff, &same_diff),
    )?;
    checks.ok(
        "the clock column is the window placeholder, the counters are exact",
        base_diff.tables["public.stickies"].updated[0]["changed"]["updated_at"]["b"]
            == json!("<now>")
            && base_diff.tables["public.stickies"].updated[0]["changed"]["name"]["b"]
                == json!("A sticky"),
    )?;

    // A wrong `updated_by` is a difference, not a placeholder coincidence: both
    // values are baseline ids, so neither is placeholdered.
    let mut wrong_actor = db_diff(
        "public.stickies",
        vec![],
        vec![],
        vec![json!({"pk": [row_id], "changed": {
            "updated_by_id": {"a": actor, "b": other_actor},
        }})],
    );
    wa.normalize_db_diff(&mut base_diff);
    wb.normalize_db_diff(&mut wrong_actor);
    checks.detected(
        "a wrong updated_by",
        "db",
        &compare_db(&base_diff, &wrong_actor),
    )?;

    // A write that forgot to bump `updated_at` keeps the baseline value, which
    // is outside the step window and therefore compared literally.
    let mut not_bumped = db_diff(
        "public.stickies",
        vec![],
        vec![],
        vec![json!({"pk": [row_id], "changed": {
            "name": {"a": "old", "b": "A sticky"},
            "updated_at": {"a": baseline, "b": baseline},
        }})],
    );
    let mut missing_bump = db_diff(
        "public.stickies",
        vec![],
        vec![],
        vec![json!({"pk": [row_id], "changed": {
            "name": {"a": "old", "b": "A sticky"},
        }})],
    );
    wb.normalize_db_diff(&mut not_bumped);
    wb.normalize_db_diff(&mut missing_bump);
    checks.detected(
        "a missing updated_at bump",
        "db",
        &compare_db(&base_diff, &not_bumped),
    )?;
    checks.detected(
        "a missing updated_at column",
        "db",
        &compare_db(&base_diff, &missing_bump),
    )?;

    // Soft delete: an update that sets `deleted_at`, not a `DELETE`.
    let mut soft_deleted = db_diff(
        "public.stickies",
        vec![],
        vec![],
        vec![json!({"pk": [row_id], "changed": {
            "deleted_at": {"a": null, "b": in_window},
        }})],
    );
    let mut hard_deleted = db_diff(
        "public.stickies",
        vec![],
        vec![sticky_row(actor, &in_window, None, 7)],
        vec![],
    );
    wb.normalize_db_diff(&mut soft_deleted);
    wb.normalize_db_diff(&mut hard_deleted);
    checks.detected(
        "a soft delete compared with a hard delete",
        "db",
        &compare_db(&soft_deleted, &hard_deleted),
    )?;

    // An extra row, and an off-by-one counter, are differences.
    let inserted = |sequence: i64| {
        vec![json!({
            "id": row_id,
            "name": "A sticky",
            "sequence_id": sequence,
            "created_at": in_window,
            "updated_at": in_window,
            "deleted_at": null,
            "created_by_id": actor,
            "updated_by_id": actor,
        })]
    };
    let mut one_insert = db_diff("public.stickies", inserted(7), vec![], vec![]);
    let mut two_inserts = db_diff(
        "public.stickies",
        [inserted(7), inserted(8)].concat(),
        vec![],
        vec![],
    );
    let mut wrong_sequence = db_diff("public.stickies", inserted(8), vec![], vec![]);
    wb.normalize_db_diff(&mut one_insert);
    wb.normalize_db_diff(&mut two_inserts);
    wb.normalize_db_diff(&mut wrong_sequence);
    checks.detected("an extra row", "db", &compare_db(&one_insert, &two_inserts))?;
    checks.detected(
        "a wrong sequence_id",
        "db",
        &compare_db(&one_insert, &wrong_sequence),
    )?;

    // -- shape checks ------------------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let short_cookie = a.normalize_headers(&[(
        "Set-Cookie".to_string(),
        "session-id=0123456789abcdef0123456789abcdef; Path=/".to_string(),
    )]);
    let _ = short_cookie;
    checks.detected(
        "a session cookie of the wrong length",
        "shape",
        &violation_differences(&mut a),
    )?;
    let mut upper = normalizer("http://127.0.0.1:18001");
    upper.start_step(start, start);
    upper.normalize_headers(&[(
        "Set-Cookie".to_string(),
        format!("session-id={}; Path=/", session_key('A')),
    )]);
    checks.detected(
        "a session cookie with the wrong alphabet",
        "shape",
        &violation_differences(&mut upper),
    )?;

    let mut colour = normalizer("http://127.0.0.1:18000");
    colour.start_step(start, start);
    // Django's random colour alphabet is string.hexdigits, so mixed case is
    // legitimate and must not be flagged.
    colour.normalize_db_diff(&mut db_diff(
        "public.workspaces",
        vec![json!({"background_color": "#FAC41C"})],
        vec![],
        vec![],
    ));
    checks.ok(
        "mixed-case hex colours pass the shape",
        violations(&mut colour).is_empty(),
    )?;
    let mut bad_colour = normalizer("http://127.0.0.1:18000");
    bad_colour.start_step(start, start);
    bad_colour.normalize_db_diff(&mut db_diff(
        "public.workspaces",
        vec![json!({"background_color": "red"})],
        vec![],
        vec![],
    ));
    checks.detected(
        "a colour that is not #rrggbb",
        "shape",
        &violation_differences(&mut bad_colour),
    )?;

    let mut username = normalizer("http://127.0.0.1:18000");
    username.start_step(start, start);
    username.normalize_body(br#"{"username":"c3496b5467ae40e593bac86a93804231"}"#, true);
    checks.ok(
        "a uuid4().hex username passes the shape",
        violations(&mut username).is_empty(),
    )?;
    username.normalize_body(br#"{"username":"not-a-username"}"#, true);
    checks.detected(
        "a username that is not uuid4().hex",
        "shape",
        &violation_differences(&mut username),
    )?;

    // -- CSRF --------------------------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    a.start_step(start, start);
    a.normalize_headers(&[(
        "Set-Cookie".to_string(),
        format!("csrftoken={CSRF_SECRET}; Path=/; HttpOnly"),
    )]);
    a.normalize_body(
        format!(r#"{{"csrf_token":"{CSRF_MASKED}"}}"#).as_bytes(),
        true,
    );
    checks.ok(
        "a masked token that unmasks to the cookie is accepted",
        violations(&mut a).is_empty(),
    )?;

    let mut a = normalizer("http://127.0.0.1:18000");
    a.start_step(start, start);
    a.normalize_headers(&[(
        "Set-Cookie".to_string(),
        format!("csrftoken={CSRF_SECRET}; Path=/; HttpOnly"),
    )]);
    let other_masked = crate::csrf::mask(CSRF_SECRET, CSRF_MASK)
        .expect("mask")
        .replace('q', "Q");
    a.normalize_body(
        format!(r#"{{"csrf_token":"{other_masked}"}}"#).as_bytes(),
        true,
    );
    checks.detected(
        "a masked token that does not unmask to the cookie",
        "csrf",
        &violation_differences(&mut a),
    )?;

    let mut no_cookie = normalizer("http://127.0.0.1:18000");
    no_cookie.start_step(start, start);
    no_cookie.normalize_body(
        format!(r#"{{"csrf_token":"{CSRF_MASKED}"}}"#).as_bytes(),
        true,
    );
    checks.detected(
        "a masked token without the csrftoken cookie",
        "csrf",
        &violation_differences(&mut no_cookie),
    )?;

    // -- session payload semantics ----------------------------------------
    let mut tampered = SESSION_VECTOR.to_string();
    tampered.pop();
    tampered.push('A');
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let mut db_a = db_diff(
        "public.sessions",
        vec![session_row(&key_a, SESSION_VECTOR, VECTOR_USER_ID)],
        vec![],
        vec![],
    );
    let mut db_b = db_diff(
        "public.sessions",
        vec![session_row(&key_b, &tampered, VECTOR_USER_ID)],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut db_a);
    b.normalize_db_diff(&mut db_b);
    checks.detected(
        "a badly signed session payload",
        "session",
        &violation_differences(&mut b),
    )?;
    checks.ok(
        "the valid side has no session violation",
        violations(&mut a).is_empty(),
    )?;

    let mut wrong_key = normalizer_with("http://127.0.0.1:18000", Some(b"wrong-secret"), &[]);
    wrong_key.start_step(start, start);
    let mut db_wrong = db_diff(
        "public.sessions",
        vec![session_row(&key_a, SESSION_VECTOR, VECTOR_USER_ID)],
        vec![],
        vec![],
    );
    wrong_key.normalize_db_diff(&mut db_wrong);
    checks.detected(
        "a session payload that does not verify with the target key",
        "session",
        &violation_differences(&mut wrong_key),
    )?;

    // The decoded payload is compared structurally, and `_auth_user_hash`
    // becomes a constant marker because it is an HMAC of the salted password
    // hash (so it legitimately differs between targets).
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let mut db_a = db_diff(
        "public.sessions",
        vec![session_row(&key_a, SESSION_VECTOR, VECTOR_USER_ID)],
        vec![],
        vec![],
    );
    let mut db_b = db_diff(
        "public.sessions",
        vec![session_row(&key_b, SESSION_VECTOR, VECTOR_USER_ID)],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut db_a);
    b.normalize_db_diff(&mut db_b);
    checks.ok(
        "_auth_user_hash becomes a constant marker",
        db_a.tables["public.sessions"].inserted[0]["session_data"]["_auth_user_hash"]
            == json!("<auth-hash>"),
    )?;

    // -- password verification --------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let mut hash_a = db_diff(
        "public.users",
        vec![json!({"password": PASSWORD_HASH})],
        vec![],
        vec![],
    );
    let mut hash_b = db_diff(
        "public.users",
        vec![json!({"password": PASSWORD_HASH})],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut hash_a);
    b.normalize_db_diff(&mut hash_b);
    checks.accepted("a hash that verifies", &compare_db(&hash_a, &hash_b))?;
    checks.ok(
        "verification leaves no violation",
        violations(&mut a).is_empty() && violations(&mut b).is_empty(),
    )?;

    let mut wrong_hash = normalizer("http://127.0.0.1:18001");
    wrong_hash.start_step(start, start);
    let mut db_wrong = db_diff(
        "public.users",
        vec![
            json!({"password": "pbkdf2_sha256$1000$SALTfixedSALTfixedSALT$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}),
        ],
        vec![],
        vec![],
    );
    wrong_hash.normalize_db_diff(&mut db_wrong);
    checks.detected(
        "a password hash that does not verify",
        "password",
        &violation_differences(&mut wrong_hash),
    )?;

    // A different, valid hash for the same plaintext still differs in its
    // algorithm/iteration prefix.
    let other_iterations = password::encode_sha256(PASSWORD, "SALTfixedSALTfixedSALT", 2000);
    let mut iterations = normalizer("http://127.0.0.1:18001");
    iterations.start_step(start, start);
    let mut db_iterations = db_diff(
        "public.users",
        vec![json!({"password": other_iterations})],
        vec![],
        vec![],
    );
    iterations.normalize_db_diff(&mut db_iterations);
    checks.detected(
        "a different iteration count",
        "db",
        &compare_db(&hash_a, &db_iterations),
    )?;

    // -- Redis -------------------------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    // Register the asset id as a placeholder first, as a response body would.
    a.normalize_body(format!(r#"{{"asset_id":"{uuid_a}"}}"#).as_bytes(), true);
    b.normalize_body(format!(r#"{{"asset_id":"{uuid_b}"}}"#).as_bytes(), true);
    let now = start.timestamp() as f64 + 0.25;
    let mut redis_a = RedisDiff::default();
    redis_a.added.insert(
        format!(":1:throttle_asset_{uuid_a}").into_bytes(),
        RedisEntry {
            kind: "string".to_string(),
            size: 30,
            ttl_ms: 60_000,
            value: Some(json!([now, now - 1.0])),
        },
    );
    let mut redis_b = RedisDiff::default();
    redis_b.added.insert(
        format!(":1:throttle_asset_{uuid_b}").into_bytes(),
        RedisEntry {
            kind: "string".to_string(),
            size: 30,
            ttl_ms: 58_900,
            value: Some(json!([now + 0.5, now - 0.9])),
        },
    );
    a.normalize_redis_diff(&mut redis_a);
    b.normalize_redis_diff(&mut redis_b);
    checks.ok(
        "a Redis key name gets the same placeholder as the body",
        redis_a
            .added
            .keys()
            .next()
            .map(|key| String::from_utf8_lossy(key).into_owned())
            == Some(":1:throttle_asset_<id:1>".to_string()),
    )?;
    checks.accepted(
        "throttle timestamps and TTLs within tolerance",
        &compare_redis(&redis_a, &redis_b),
    )?;
    checks.ok(
        "in-window throttle timestamps become <now>",
        redis_a
            .added
            .values()
            .next()
            .and_then(|entry| entry.value.as_ref())
            .and_then(Value::as_array)
            .map(|values| values[0] == json!("<now>") && values[1] == json!("<now>"))
            .unwrap_or(false),
    )?;

    let mut redis_extra = redis_b.clone();
    redis_extra.added.insert(
        b":1:throttle_anon_127.0.0.1".to_vec(),
        RedisEntry {
            kind: "string".to_string(),
            size: 10,
            ttl_ms: 60_000,
            value: Some(json!([])),
        },
    );
    checks.detected(
        "an extra Redis key",
        "redis",
        &compare_redis(&redis_a, &redis_extra),
    )?;

    let mut redis_far_ttl = redis_b.clone();
    for entry in redis_far_ttl.added.values_mut() {
        entry.ttl_ms = 30_000;
    }
    checks.detected(
        "a Redis TTL that differs by more than the tolerance",
        "redis",
        &compare_redis(&redis_a, &redis_far_ttl),
    )?;

    // A value seen in an earlier step keeps its `<now>` even outside the
    // current window.
    let mut later = normalizer("http://127.0.0.1:18000");
    later.start_step(start, start);
    let mut first = RedisDiff::default();
    first.added.insert(
        b":1:throttle_anon_127.0.0.1".to_vec(),
        RedisEntry {
            kind: "string".to_string(),
            size: 10,
            ttl_ms: 60_000,
            value: Some(json!([now])),
        },
    );
    later.normalize_redis_diff(&mut first);
    later.start_step(time("2026-11-15T20:10:00Z"), time("2026-11-15T20:10:00Z"));
    let mut second = RedisDiff::default();
    second.changed.insert(
        b":1:throttle_anon_127.0.0.1".to_vec(),
        RedisEntry {
            kind: "string".to_string(),
            size: 10,
            ttl_ms: 60_000,
            value: Some(json!([now, now + 600.0])),
        },
    );
    later.normalize_redis_diff(&mut second);
    checks.ok(
        "an earlier step's Redis timestamp stays <now>",
        second
            .changed
            .values()
            .next()
            .and_then(|entry| entry.value.as_ref())
            .and_then(Value::as_array)
            .map(|values| values[0] == json!("<now>") && values[1] == json!("<now>"))
            .unwrap_or(false),
    )?;

    // -- page cache (DECISIONS.md D5a) ------------------------------------
    // `Expires` is `http_date(now + 7200)`: the wall-clock value differs per
    // target, the offset from the step clock does not.
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let expires_a = crate::normalize::http_date_for_test(start.timestamp() + 7200);
    let expires_b = crate::normalize::http_date_for_test(start.timestamp() + 7202);
    let headers_a = a.normalize_headers(&[("Expires".to_string(), expires_a)]);
    let headers_b = b.normalize_headers(&[("Expires".to_string(), expires_b)]);
    checks.accepted(
        "page-cache Expires two seconds apart",
        &compare_headers(&headers_a, &headers_b),
    )?;
    checks.ok(
        "Expires becomes <expires+7200s>",
        headers_a[0].1 == "<expires+7200s>" && headers_b[0].1 == "<expires+7200s>",
    )?;
    let wrong_expires = b.normalize_headers(&[(
        "Expires".to_string(),
        crate::normalize::http_date_for_test(start.timestamp() + 60),
    )]);
    checks.detected(
        "a page-cache Expires with the wrong age",
        "headers",
        &compare_headers(&headers_a, &wrong_expires),
    )?;
    // A cached hit serves the stored `Expires`, which is still `now + 7200`
    // from *its* store time - but a hit that re-derived `Expires` instead of
    // serving the stored header would be thousands of seconds off.
    let stale_expires = b.normalize_headers(&[(
        "Expires".to_string(),
        crate::normalize::http_date_for_test(start.timestamp() - 3600),
    )]);
    checks.detected(
        "an Expires from before the step",
        "headers",
        &compare_headers(&headers_a, &stale_expires),
    )?;

    // `Age` is `int(now) - int(stored_at)`; the two targets observe the hit a
    // moment apart, so one second of skew is accepted and nothing more.
    let age_5 = vec![("Age".to_string(), "5".to_string())];
    for (label, other, expected) in [
        ("one second apart", "6", true),
        ("identical", "5", true),
        ("two seconds apart", "7", false),
        ("an hour apart", "3605", false),
    ] {
        let differences = compare_headers(&age_5, &[("Age".to_string(), other.to_string())]);
        checks.ok(
            &format!(
                "page-cache Age {label} is {}",
                if expected { "accepted" } else { "detected" }
            ),
            differences.is_empty() == expected,
        )?;
    }
    checks.detected(
        "an Age only one side sends",
        "headers",
        &compare_headers(&age_5, &[]),
    )?;
    checks.detected(
        "a non-numeric Age",
        "headers",
        &compare_headers(&age_5, &[("Age".to_string(), "old".to_string())]),
    )?;

    // Page-cache Redis keys: Django writes a page key plus a `cache_header..`
    // registry key, Rust writes one key in its own namespace. Presence and the
    // TTL bucket are compared, nothing else.
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let digest_a = "11111111111111111111111111111111";
    let digest_b = "22222222222222222222222222222222";
    let mut page_a = RedisDiff::default();
    page_a.added.insert(
        format!(":1:views.decorators.cache.cache_header..{digest_a}.en-us.UTC").into_bytes(),
        RedisEntry {
            kind: "string".to_string(),
            size: 4,
            ttl_ms: 7_200_000,
            value: Some(json!([])),
        },
    );
    page_a.added.insert(
        format!(":1:views.decorators.cache.cache_page..GET.{digest_a}.{digest_a}.en-us.UTC")
            .into_bytes(),
        RedisEntry {
            kind: "string".to_string(),
            size: 900,
            ttl_ms: 7_200_000,
            // Django pickles a `Response`; the harness cannot read it.
            value: None,
        },
    );
    let mut page_b = RedisDiff::default();
    page_b.added.insert(
        format!(":1:plane_rs.cache_page.GET.{digest_b}.{digest_b}.en-us.UTC").into_bytes(),
        RedisEntry {
            kind: "string".to_string(),
            size: 120,
            ttl_ms: 7_198_500,
            value: Some(json!({"$bytes": "50525331"})),
        },
    );
    a.normalize_redis_diff(&mut page_a);
    b.normalize_redis_diff(&mut page_b);
    checks.ok(
        "the cache_header registry key is dropped",
        page_a.added.len() == 1
            && page_a
                .added
                .keys()
                .all(|key| String::from_utf8_lossy(key).contains("page-cache")),
    )?;
    checks.accepted(
        "a Django page key and a Rust page key match",
        &compare_redis(&page_a, &page_b),
    )?;
    checks.ok(
        "page-cache keys are named by TTL bucket",
        page_a
            .added
            .keys()
            .next()
            .map(|key| String::from_utf8_lossy(key).into_owned())
            == Some(":1:<page-cache:720:1>".to_string()),
    )?;

    let mut page_missing = page_b.clone();
    page_missing.added.clear();
    checks.detected(
        "a page-cache entry only one side stored",
        "redis",
        &compare_redis(&page_a, &page_missing),
    )?;
    let mut page_wrong_ttl = page_b.clone();
    for entry in page_wrong_ttl.added.values_mut() {
        entry.ttl_ms = 3_600_000;
    }
    checks.detected(
        "a page-cache entry with a different timeout",
        "redis",
        &compare_redis(&page_a, &page_wrong_ttl),
    )?;

    // -- DB diffs ----------------------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let mut insert_a = db_diff(
        "public.issues",
        vec![json!({"id": uuid_a, "name": "Work item", "created_at": "2026-11-15T20:00:00.1Z"})],
        vec![],
        vec![],
    );
    let mut insert_b = db_diff(
        "public.issues",
        vec![json!({"id": uuid_b, "name": "Work item", "created_at": "2026-11-15T20:00:00.9Z"})],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut insert_a);
    b.normalize_db_diff(&mut insert_b);
    checks.accepted(
        "database insert with generated id and timestamp",
        &compare_db(&insert_a, &insert_b),
    )?;

    let mut insert_b_extra = db_diff(
        "public.issues",
        vec![
            json!({"id": uuid_b, "name": "Work item", "created_at": "2026-11-15T20:00:00.9Z"}),
            json!({"id": uuid_c, "name": "Unexpected", "created_at": "2026-11-15T20:00:00.9Z"}),
        ],
        vec![],
        vec![],
    );
    b.normalize_db_diff(&mut insert_b_extra);
    checks.detected(
        "extra database row",
        "db",
        &compare_db(&insert_a, &insert_b_extra),
    )?;

    let mut delete_a = db_diff(
        "public.issues",
        vec![],
        vec![json!({"id": uuid_a, "name": "Gone"})],
        vec![],
    );
    a.normalize_db_diff(&mut delete_a);
    checks.detected(
        "missing database row",
        "db",
        &compare_db(&delete_a, &db_diff("public.issues", vec![], vec![], vec![])),
    )?;

    let mut update_a = db_diff(
        "public.issues",
        vec![],
        vec![],
        vec![json!({"pk": [uuid_a], "changed": {"name": {"a": "old", "b": "new"}}})],
    );
    let mut update_b = db_diff(
        "public.issues",
        vec![],
        vec![],
        vec![json!({"pk": [uuid_b], "changed": {"name": {"a": "old", "b": "new"}}})],
    );
    a.normalize_db_diff(&mut update_a);
    b.normalize_db_diff(&mut update_b);
    checks.accepted(
        "database update with generated id",
        &compare_db(&update_a, &update_b),
    )?;

    let mut update_c = db_diff(
        "public.issues",
        vec![],
        vec![],
        vec![json!({"pk": [uuid_b], "changed": {"name": {"a": "old", "b": "different"}}})],
    );
    b.normalize_db_diff(&mut update_c);
    checks.detected(
        "database update with a different value",
        "db",
        &compare_db(&update_a, &update_c),
    )?;

    let mut expiry_a = db_diff(
        "public.sessions",
        vec![json!({"expire_date": "2026-11-22T20:00:00.101+00:00"})],
        vec![],
        vec![],
    );
    let mut expiry_b = db_diff(
        "public.sessions",
        vec![json!({"expire_date": "2026-11-22T20:00:00.987+00:00"})],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut expiry_a);
    b.normalize_db_diff(&mut expiry_b);
    checks.accepted(
        "session expiry compared as a relative age",
        &compare_db(&expiry_a, &expiry_b),
    )?;
    let mut expiry_bad = db_diff(
        "public.sessions",
        vec![json!({"expire_date": "2026-11-23T20:00:00.987+00:00"})],
        vec![],
        vec![],
    );
    b.normalize_db_diff(&mut expiry_bad);
    checks.detected(
        "a different session age is still detected",
        "db",
        &compare_db(&expiry_a, &expiry_bad),
    )?;

    let mut colour_a = db_diff(
        "public.workspaces",
        vec![json!({"background_color": "#a1b2c3"})],
        vec![],
        vec![],
    );
    let mut colour_b = db_diff(
        "public.workspaces",
        vec![json!({"background_color": "#D4E5f6"})],
        vec![],
        vec![],
    );
    a.normalize_db_diff(&mut colour_a);
    b.normalize_db_diff(&mut colour_b);
    checks.accepted("random default colour", &compare_db(&colour_a, &colour_b))?;

    // -- snapshot diff, end to end ----------------------------------------
    let before = crate::db::DbSnapshot::default();
    let after = before.clone();
    checks.ok(
        "identical snapshots have an empty diff",
        diff_snapshots(&before, &after).is_empty(),
    )?;

    // -- Celery ------------------------------------------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    let mut b = normalizer("http://127.0.0.1:18001");
    a.start_step(start, start);
    b.start_step(start, start);
    let mut message_a = celery_message(
        "plane.tasks.issue_activity",
        json!([uuid_a, {"id": uuid_b}]),
    );
    message_a.headers.insert("id".to_string(), json!(uuid_c));
    message_a.headers.insert(
        "argsrepr".to_string(),
        json!(format!("('{uuid_a}', {{'id': '{uuid_b}'}})")),
    );
    let mut message_b = celery_message(
        "plane.tasks.issue_activity",
        json!([uuid_c, {"id": uuid_d}]),
    );
    message_b.headers.insert("id".to_string(), json!(uuid_e));
    message_b.headers.insert(
        "argsrepr".to_string(),
        json!(format!("('{uuid_c}', {{'id': '{uuid_d}'}})")),
    );
    a.normalize_celery(&mut message_a);
    b.normalize_celery(&mut message_b);
    checks.accepted(
        "celery ids, args and argsrepr",
        &compare_celery(&[message_a.clone()], &[message_b.clone()]),
    )?;
    checks.ok(
        "celery transport headers are dropped",
        !message_a.headers.contains_key("id"),
    )?;

    // `argsrepr`/`kwargsrepr` are `saferepr(kwargs, 1024)`. An untruncated pair
    // is exact; a truncated pair is compared by the prefix before Celery's
    // `, ...` marker, because the harness's placeholders shift the tail.
    let mut repr_exact_a = celery_message("plane.tasks.issue_activity", json!([]));
    repr_exact_a
        .headers
        .insert("argsrepr".to_string(), json!("('alpha', 1)"));
    let mut repr_exact_b = celery_message("plane.tasks.issue_activity", json!([]));
    repr_exact_b
        .headers
        .insert("argsrepr".to_string(), json!("('alpha', 2)"));
    checks.detected(
        "celery untruncated argsrepr mismatch",
        "celery",
        &compare_celery(&[repr_exact_a.clone()], &[repr_exact_b.clone()]),
    )?;

    let mut repr_tail_a = celery_message("plane.tasks.issue_activity", json!([]));
    repr_tail_a.headers.insert(
        "kwargsrepr".to_string(),
        json!("{'x': [1, 2, <id:10>], ...}"),
    );
    let mut repr_tail_b = celery_message("plane.tasks.issue_activity", json!([]));
    repr_tail_b.headers.insert(
        "kwargsrepr".to_string(),
        json!("{'x': [1, 2, <id:4>], ...}}"),
    );
    checks.accepted(
        "celery truncated kwargsrepr shifted tail",
        &compare_celery(&[repr_tail_a.clone()], &[repr_tail_b.clone()]),
    )?;

    let mut repr_early_a = celery_message("plane.tasks.issue_activity", json!([]));
    repr_early_a
        .headers
        .insert("kwargsrepr".to_string(), json!("{'x': [1, 2, ...}"));
    let mut repr_early_b = celery_message("plane.tasks.issue_activity", json!([]));
    repr_early_b
        .headers
        .insert("kwargsrepr".to_string(), json!("{'y': [1, 2, ...}"));
    checks.detected(
        "celery truncated kwargsrepr early mismatch",
        "celery",
        &compare_celery(&[repr_early_a.clone()], &[repr_early_b.clone()]),
    )?;

    let mut message_wrong_task = message_b.clone();
    message_wrong_task.task = Some("plane.tasks.something_else".to_string());
    checks.detected(
        "celery task name",
        "celery",
        &compare_celery(&[message_a.clone()], &[message_wrong_task]),
    )?;
    let mut message_wrong_args = message_b.clone();
    message_wrong_args.args = json!(["different"]);
    checks.detected(
        "celery args",
        "celery",
        &compare_celery(&[message_a.clone()], &[message_wrong_args]),
    )?;
    let start_seconds = start.timestamp();
    let mut epoch_a = celery_message("plane.tasks.issue_activity", json!([]));
    epoch_a.kwargs = json!({"epoch": start_seconds});
    let mut epoch_b = celery_message("plane.tasks.issue_activity", json!([]));
    epoch_b.kwargs = json!({"epoch": start_seconds + 1});
    a.normalize_celery(&mut epoch_a);
    b.normalize_celery(&mut epoch_b);
    checks.accepted(
        "celery epoch one second apart",
        &compare_celery(&[epoch_a.clone()], &[epoch_b.clone()]),
    )?;
    let mut epoch_old = celery_message("plane.tasks.issue_activity", json!([]));
    epoch_old.kwargs = json!({"epoch": 1600000000});
    a.normalize_celery(&mut epoch_old);
    checks.detected(
        "celery epoch from another period",
        "celery",
        &compare_celery(&[epoch_a], &[epoch_old]),
    )?;

    let mut message_wrong_header = message_b.clone();
    message_wrong_header
        .headers
        .insert("retries".to_string(), json!(1));
    checks.detected(
        "celery header",
        "celery",
        &compare_celery(&[message_a.clone()], &[message_wrong_header]),
    )?;
    checks.detected(
        "missing celery message",
        "celery",
        &compare_celery(&[message_a], &[]),
    )?;

    println!("selftest: comparator");
    // -- status and headers ------------------------------------------------
    checks.detected("status code", "status", &compare_status(200, 422))?;
    checks.accepted("same status", &compare_status(201, 201))?;

    let header_a = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Vary".to_string(), "Cookie".to_string()),
    ];
    let header_b = header_a.clone();
    checks.accepted("identical headers", &compare_headers(&header_a, &header_b))?;
    checks.detected(
        "header value",
        "headers",
        &compare_headers(
            &header_a,
            &[
                ("content-type".to_string(), "application/json".to_string()),
                ("vary".to_string(), "Origin".to_string()),
            ],
        ),
    )?;
    checks.detected(
        "missing header",
        "headers",
        &compare_headers(&header_a, &header_a[..1]),
    )?;

    // -- bodyless Content-Length (DECISIONS.md D18) -------------------------
    let with_length = |value: Option<&str>| -> Vec<(String, String)> {
        value
            .map(|value| vec![("Content-Length".to_string(), value.to_string())])
            .unwrap_or_default()
    };
    let (differences, rule) =
        compare_response_headers(204, 204, &with_length(Some("0")), &with_length(None));
    checks.accepted(
        "204 with Content-Length: 0 against 204 without it",
        &differences,
    )?;
    checks.ok(
        "the bodyless-content-length rule is reported",
        rule == Some(CONTENT_LENGTH_BODYLESS_RULE),
    )?;
    let (differences, rule) =
        compare_response_headers(304, 304, &with_length(Some("512")), &with_length(None));
    checks.accepted(
        "304 with Content-Length against 304 without it",
        &differences,
    )?;
    checks.ok(
        "the 304 form reports the same rule",
        rule == Some(CONTENT_LENGTH_BODYLESS_RULE),
    )?;
    let (differences, _) =
        compare_response_headers(204, 204, &with_length(Some("0")), &with_length(Some("5")));
    checks.detected(
        "204 Content-Length: 0 against 204 Content-Length: 5",
        "headers",
        &differences,
    )?;
    let (differences, _) =
        compare_response_headers(204, 204, &with_length(Some("5")), &with_length(None));
    checks.detected(
        "204 Content-Length: 5 against 204 without it",
        "headers",
        &differences,
    )?;
    let (differences, _) =
        compare_response_headers(200, 200, &with_length(Some("0")), &with_length(None));
    checks.detected(
        "200 Content-Length: 0 against 200 without it",
        "headers",
        &differences,
    )?;
    let (differences, rule) =
        compare_response_headers(204, 200, &with_length(Some("0")), &with_length(None));
    checks.detected("204 against 200", "headers", &differences)?;
    checks.ok("the rule does not fire across statuses", rule.is_none())?;

    // -- JSON bodies -------------------------------------------------------
    let identical = body(serde_json::from_str(r#"{"a":1,"b":[true,null,"x"]}"#).expect("json"));
    checks.accepted(
        "identical JSON",
        &compare_bodies(&identical, &identical.clone()),
    )?;
    checks.detected(
        "JSON value",
        "body",
        &compare_bodies(&body(json!({"a": 1})), &body(json!({"a": 2}))),
    )?;
    checks.detected(
        "JSON key order",
        "body",
        &compare_bodies(
            &body(serde_json::from_str(r#"{"a":1,"b":2}"#).expect("json")),
            &body(serde_json::from_str(r#"{"b":2,"a":1}"#).expect("json")),
        ),
    )?;
    checks.detected(
        "JSON number lexeme",
        "body",
        &compare_bodies(
            &body(serde_json::from_str(r#"{"n":100.0}"#).expect("json")),
            &body(serde_json::from_str(r#"{"n":100}"#).expect("json")),
        ),
    )?;
    checks.detected(
        "JSON missing key",
        "body",
        &compare_bodies(&body(json!({"a": 1})), &body(json!({}))),
    )?;
    checks.detected(
        "JSON array order",
        "body",
        &compare_bodies(&body(json!([1, 2])), &body(json!([2, 1]))),
    )?;

    // -- D15 byte rule -----------------------------------------------------
    checks.ok(
        "DRF-compact body passes",
        check_drf_formatting(br#"{"a":1,"b":"x"}"#).is_ok(),
    )?;
    checks.ok(
        "spaced body fails",
        check_drf_formatting(br#"{"a": 1, "b": "x"}"#).is_err(),
    )?;
    checks.detected(
        "formatting mismatch",
        "body",
        &compare_formatting(
            &FormattingStatus::Pass,
            &FormattingStatus::Fail("spaced".to_string()),
        ),
    )?;
    checks.accepted(
        "both sides non-compact (JsonResponse routes)",
        &compare_formatting(
            &FormattingStatus::Fail("spaced".to_string()),
            &FormattingStatus::Fail("spaced".to_string()),
        ),
    )?;

    let line_separator = "{\"a\":\"\u{2028}\"}";
    let escaped = "{\"a\":\"\\u2028\"}";
    checks.ok(
        "U+2028 must be escaped as six ASCII characters",
        check_drf_formatting(escaped.as_bytes()).is_ok()
            && check_drf_formatting(line_separator.as_bytes()).is_err(),
    )?;
    checks.ok(
        "drf_compact keeps key order and number lexemes",
        drf_compact(&serde_json::from_str(r#"{"b":1.0,"a":2e3}"#).expect("json")).expect("compact")
            == r#"{"b":1.0,"a":2e+3}"#,
    )?;

    // -- text and binary bodies -------------------------------------------
    let text_a = NormalizedBody {
        value: BodyValue::Text("hello".to_string()),
        modified: false,
    };
    let text_c = NormalizedBody {
        value: BodyValue::Text("hello!".to_string()),
        modified: false,
    };
    checks.accepted("identical text bodies", &compare_bodies(&text_a, &text_a))?;
    checks.detected("text body", "body", &compare_bodies(&text_a, &text_c))?;
    let binary_a = NormalizedBody {
        value: BodyValue::Binary("00ff".to_string()),
        modified: false,
    };
    let binary_b = NormalizedBody {
        value: BodyValue::Binary("00fe".to_string()),
        modified: false,
    };
    checks.detected("binary body", "body", &compare_bodies(&binary_a, &binary_b))?;

    // -- D16: path-scoped unordered comparisons ---------------------------
    // DECISIONS.md D16: an array Django builds with `list(set(...))` is a
    // multiset; an object keyed by such a set compares its key set, not its
    // key order. The rule is scoped to a JSON path *and* to the request that
    // makes Django build a set: a page grouped by an ordinary column carries
    // `ARRAY_AGG(DISTINCT …)` output, which must stay an exact comparison.
    let mut unordered = (*rules()).clone();
    unordered.unordered_json_paths = vec![
        UnorderedPathRule {
            path: "$.results.*.results.*.label_ids".to_string(),
            mode: UnorderedMode::Array,
            request: Some("*group_by=labels__id*".to_string()),
            why: "selftest: the multi-grouper builds the id array from a set".to_string(),
        },
        UnorderedPathRule {
            path: "$.results.*.results.*.results.*.assignee_ids".to_string(),
            mode: UnorderedMode::Array,
            request: Some("*group_by=assignees__id*".to_string()),
            why: "selftest: the sub-grouped multi-grouper builds the id array from a set"
                .to_string(),
        },
        UnorderedPathRule {
            path: "$.results".to_string(),
            mode: UnorderedMode::ObjectKeys,
            request: Some("*group_by=labels__id*".to_string()),
            why: "selftest: the m2m grouper's group keys come from a set".to_string(),
        },
        UnorderedPathRule {
            path: "$.results.*.results.*.assignees__id".to_string(),
            mode: UnorderedMode::ParentKey,
            request: Some("*group_by=assignees__id*".to_string()),
            why: "selftest: a row shared across groups keeps a database-order group column"
                .to_string(),
        },
    ];
    let unordered_rules = Arc::new(unordered);
    let unordered_normalizer = |request: &str| {
        let mut normalizer = TargetNormalizer::new(NormalizerConfig {
            rules: unordered_rules.clone(),
            base_url: "http://x".to_string(),
            known_uuids: Arc::new(HashSet::new()),
            secret_key: None,
            passwords: Arc::new(Vec::new()),
            aws: None,
        });
        normalizer.start_step(start, start);
        normalizer.set_request(request.to_string());
        normalizer
    };
    let normalise = |request: &str, raw: &str| {
        let mut normalizer = unordered_normalizer(request);
        normalizer.normalize_body(raw.as_bytes(), true)
    };
    let request = "GET /api/workspaces/w/projects/p/issues/?group_by=labels__id";

    // The same elements in a different order are equal at the rule's path ...
    let forward = normalise(
        request,
        r#"{"results":{"g":{"results":[{"label_ids":["b","a"]}]}}}"#,
    );
    let backward = normalise(
        request,
        r#"{"results":{"g":{"results":[{"label_ids":["a","b"]}]}}}"#,
    );
    checks.accepted(
        "a set-derived array is a multiset",
        &compare_bodies(&forward, &backward),
    )?;

    // ... but a missing, extra or duplicated element is not.
    for (label, raw) in [
        (
            "missing",
            r#"{"results":{"g":{"results":[{"label_ids":["a"]}]}}}"#,
        ),
        (
            "extra",
            r#"{"results":{"g":{"results":[{"label_ids":["a","b","c"]}]}}}"#,
        ),
        (
            "duplicate",
            r#"{"results":{"g":{"results":[{"label_ids":["a","a","b"]}]}}}"#,
        ),
    ] {
        let other = normalise(request, raw);
        checks.detected(
            &format!("a set-derived array with a {label} element"),
            "body",
            &compare_bodies(&forward, &other),
        )?;
    }

    // `parent_key` canonicalises the raw group column to the group the row is
    // filed under, so the two database orders compare equal.
    let assignee_request = "GET /api/workspaces/w/projects/p/issues/?group_by=assignees__id";
    let parent_a = normalise(
        assignee_request,
        r#"{"results":{"g1":{"results":[{"assignees__id":"g2"}]}}}"#,
    );
    let parent_b = normalise(
        assignee_request,
        r#"{"results":{"g1":{"results":[{"assignees__id":"g1"}]}}}"#,
    );
    checks.accepted(
        "a shared row's raw group column is canonicalised to its group",
        &compare_bodies(&parent_a, &parent_b),
    )?;

    // The rule is scoped to the m2m request: a page grouped by an ordinary
    // column carries `ARRAY_AGG(DISTINCT …)` output, whose order is part of the
    // body. The same two arrays must differ there.
    let state_request = "GET /api/workspaces/w/projects/p/issues/?group_by=state_id";
    let state_forward = normalise(
        state_request,
        r#"{"results":{"g":{"results":[{"label_ids":["b","a"]}]}}}"#,
    );
    let state_backward = normalise(
        state_request,
        r#"{"results":{"g":{"results":[{"label_ids":["a","b"]}]}}}"#,
    );
    checks.detected(
        "an array on a non-m2m grouped page compares in order",
        "body",
        &compare_bodies(&state_forward, &state_backward),
    )?;
    // `parent_key` is scoped the same way: on a `group_by=state_id` page its
    // path cannot match, and the value it would have rewritten is a real
    // column.
    let state_parent = unordered_normalizer(state_request).normalize_body(
        br#"{"results":{"g":{"results":[{"assignees__id":"g2"}]}}}"#,
        true,
    );
    checks.ok(
        "parent_key does not rewrite a non-m2m grouped page",
        state_parent.pretty().contains("\"assignees__id\": \"g2\""),
    )?;

    // The rule is path-scoped: the same key outside it keeps its order.
    let outside = normalise(request, r#"{"items":[{"label_ids":["b","a"]}]}"#);
    let outside_reordered = normalise(request, r#"{"items":[{"label_ids":["a","b"]}]}"#);
    checks.detected(
        "an array outside the rule's path compares in order",
        "body",
        &compare_bodies(&outside, &outside_reordered),
    )?;

    // The m2m grouper's key order is unordered on its endpoint ...
    let keys_forward = normalise(
        request,
        r#"{"results":{"a":{"results":[],"total_results":0},"b":{"results":[],"total_results":0}}}"#,
    );
    let keys_backward = normalise(
        request,
        r#"{"results":{"b":{"results":[],"total_results":0},"a":{"results":[],"total_results":0}}}"#,
    );
    checks.accepted(
        "m2m group keys compare as a set",
        &compare_bodies(&keys_forward, &keys_backward),
    )?;
    // ... and exact on a step whose request does not match the rule.
    let other_request = "GET /api/workspaces/w/projects/p/issues/?group_by=state_id";
    let keys_ordered = normalise(
        other_request,
        r#"{"results":{"a":{"results":[],"total_results":0},"b":{"results":[],"total_results":0}}}"#,
    );
    let keys_reordered = normalise(
        other_request,
        r#"{"results":{"b":{"results":[],"total_results":0},"a":{"results":[],"total_results":0}}}"#,
    );
    checks.detected(
        "group key order stays exact on a non-m2m step",
        "body",
        &compare_bodies(&keys_ordered, &keys_reordered),
    )?;

    // -- D17: presigned S3 URLs -------------------------------------------
    // Each side's `Location` is verified with its own credentials and
    // `X-Amz-Date`; everything except the date and the signature compares
    // byte for byte, and the dates may be at most two seconds apart.
    let aws = crate::presign::Credentials {
        access_key_id: "AKIDEXAMPLE".to_string(),
        secret_access_key: "SECRETEXAMPLE".to_string(),
    };
    let presigned_url = |date: &str, path: &str, expires: i64| {
        crate::presign::sign_presigned_get(
            &aws,
            "http",
            "plane-fixture.test",
            path,
            &[(
                "response-content-disposition",
                "attachment; filename=\"spec.pdf\"",
            )],
            date,
            "us-east-1",
            expires,
        )
    };
    let presigned_normalizer = |started: DateTime<Utc>, ended: DateTime<Utc>| {
        let mut base = (*rules()).clone();
        base.volatile_headers.push(NamedRule {
            name: "Location".to_string(),
            why: "selftest: presigned S3 URL".to_string(),
            mode: RuleMode::PresignedUrl,
            placeholder: None,
            shape: None,
            check: None,
            request: Some("*api/assets/*".to_string()),
        });
        base.unordered_json_paths = Vec::new();
        let mut normalizer = TargetNormalizer::new(NormalizerConfig {
            rules: Arc::new(base),
            base_url: "http://x".to_string(),
            known_uuids: Arc::new(HashSet::new()),
            secret_key: None,
            passwords: Arc::new(Vec::new()),
            aws: Some(aws.clone()),
        });
        normalizer.start_step(started, ended);
        normalizer.set_request("GET /api/assets/v2/workspaces/w/download/asset/");
        normalizer
    };
    // SigV4's compact date as the instant the target signed at.
    let at = |text: &str| -> DateTime<Utc> {
        chrono::NaiveDateTime::parse_from_str(text, "%Y%m%dT%H%M%SZ")
            .map(|naive| naive.and_utc())
            .unwrap_or(start)
    };
    let signed = |date: &str, path: &str, expires: i64| {
        let mut normalizer = presigned_normalizer(at(date), at(date) + Duration::seconds(1));
        let headers = vec![("Location".to_string(), presigned_url(date, path, expires))];
        (normalizer.normalize_headers(&headers), normalizer)
    };

    let (one_second_a, mut na) = signed("20261116T120000Z", "/uploads/a/1.pdf", 60);
    let (one_second_b, _nb) = signed("20261116T120001Z", "/uploads/a/1.pdf", 60);
    checks.accepted(
        "presigned URLs one second apart",
        &crate::compare::compare_headers(&one_second_a, &one_second_b),
    )?;
    checks.ok(
        "presigned URLs are presented as verified",
        one_second_a[0].1.contains("<presigned-date:") && na.take_violations().is_empty(),
    )?;

    // A tampered signature is a violation on that side, never a pass.
    let (mut tampered, _) = signed("20261116T120000Z", "/uploads/a/1.pdf", 60);
    tampered[0].1 = tampered[0]
        .1
        .replacen("X-Amz-Signature=", "X-Amz-Signature=0", 1);
    let mut tampered_normalizer = presigned_normalizer(start, start + Duration::seconds(1));
    let tampered_out = tampered_normalizer.normalize_headers(&tampered);
    checks.ok(
        "a tampered presigned signature is a violation",
        !tampered_normalizer.take_violations().is_empty() && tampered_out[0].1 == tampered[0].1,
    )?;

    // A valid signature for a changed `X-Amz-Expires`, or for a changed path,
    // is still a different URL: the bytes are compared after masking the date
    // and the signature.
    let (other_expiry, _) = signed("20261116T120000Z", "/uploads/a/1.pdf", 61);
    checks.detected(
        "a changed X-Amz-Expires",
        "headers",
        &crate::compare::compare_headers(&one_second_a, &other_expiry),
    )?;
    let (other_path, _) = signed("20261116T120000Z", "/uploads/a/2.pdf", 60);
    checks.detected(
        "a changed presigned path",
        "headers",
        &crate::compare::compare_headers(&one_second_a, &other_path),
    )?;

    // Five seconds apart: each date is inside its own window, but the pair is
    // outside D17's two-second tolerance.
    let (five_apart, _) = signed("20261116T120005Z", "/uploads/a/1.pdf", 60);
    checks.detected(
        "presigned URLs five seconds apart",
        "headers",
        &crate::compare::compare_headers(&one_second_a, &five_apart),
    )?;

    // A date outside the request window is a violation even when the signature
    // is valid for it.
    let mut late = presigned_normalizer(start, start + Duration::seconds(1));
    let late_headers = vec![(
        "Location".to_string(),
        presigned_url("20261116T130000Z", "/uploads/a/1.pdf", 60),
    )];
    let _ = late.normalize_headers(&late_headers);
    checks.ok(
        "a presigned date outside the request window is a violation",
        !late.take_violations().is_empty(),
    )?;
    // A `Location` on a step the rule does not name keeps the exact comparison.
    let mut elsewhere = presigned_normalizer(start, start + Duration::seconds(1));
    elsewhere.set_request("GET /api/workspaces/w/projects/p/");
    let elsewhere_headers = vec![("Location".to_string(), "/some/redirect/".to_string())];
    let elsewhere_out = elsewhere.normalize_headers(&elsewhere_headers);
    checks.ok(
        "a Location outside the rule's request glob stays exact",
        elsewhere_out[0].1 == "/some/redirect/" && elsewhere.take_violations().is_empty(),
    )?;

    // -- ordering of placeholders across parts ----------------------------
    let mut a = normalizer("http://127.0.0.1:18000");
    a.start_step(start, start);
    let headers = a.normalize_headers(&[
        ("X-Second".to_string(), uuid_b.to_string()),
        ("Location".to_string(), format!("/{uuid_a}")),
    ]);
    checks.ok(
        "Location is assigned before other headers",
        headers[1].1 == "/<id:1>" && headers[0].1 == "<id:2>",
    )?;
    let body_after = a.normalize_body(format!(r#"["{uuid_c}"]"#).as_bytes(), true);
    checks.ok(
        "body continues the numbering after headers",
        body_after.pretty().contains("<id:3>"),
    )?;

    println!("selftest: {} checks passed", checks.count);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn selftest_passes() {
        super::run().expect("selftest must pass");
    }
}
