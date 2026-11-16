// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Table-driven tests for [`super::Settings`], one test per ENV.md rule.
//!
//! Each test cites the ENV.md section it reproduces; when a case is surprising
//! the comment quotes the Python expression from `apps/api`.

use super::*;

/// Base environment: only what the `api` role truly needs (Redis and nothing
/// else is mandatory at import).
fn base() -> EnvMap {
    EnvMap::new().set("REDIS_URL", "redis://127.0.0.1:6379/0")
}

fn load(env: EnvMap) -> Settings {
    Settings::from_source(&env, Role::Api).expect("settings should load")
}

fn load_err(env: EnvMap) -> ConfigError {
    Settings::from_source(&env, Role::Api).expect_err("settings should fail")
}

// ENV.md §1.1: SECRET_KEY unset/empty falls back to a random 50-character key
// (owner decision in PROGRESS.md: keep the fallback and log CRITICAL for the
// two known-bad values).
#[test]
fn secret_key() {
    let unset = load(base());
    assert!(unset.secret_key.is_random());
    assert_eq!(unset.secret_key.value().len(), 50);
    assert!(
        unset
            .secret_key
            .value()
            .chars()
            .all(|c| "abcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*(-_=+)".contains(c))
    );

    let empty = load(base().set("SECRET_KEY", ""));
    assert!(empty.secret_key.is_random());
    assert_ne!(empty.secret_key.value(), unset.secret_key.value());

    let explicit = load(base().set("SECRET_KEY", "not-a-secret"));
    assert!(!explicit.secret_key.is_random());
    assert_eq!(explicit.secret_key.value(), "not-a-secret");
    assert!(!format!("{explicit:?}").contains("not-a-secret"));

    // The two placeholder values are accepted but logged at CRITICAL.
    for value in [
        "60gp0byfz2dvffa45cxl20p1scy9xbpf6d8c5y0geejgkyp1b5",
        "change-this-key-on-deployment",
    ] {
        assert_eq!(
            load(base().set("SECRET_KEY", value)).secret_key.value(),
            value
        );
    }
}

// ENV.md §1.1: `DEBUG = int(os.environ.get("DEBUG", 0)) == 1`.
#[test]
fn debug_parsing() {
    assert!(!load(base()).debug);
    assert!(!load(base().set("DEBUG", "0")).debug);
    assert!(load(base().set("DEBUG", "1")).debug);
    assert!(load(base().set("DEBUG", " 1 ")).debug);
    assert!(load(base().set("DEBUG", "+1")).debug);
    assert!(!load(base().set("DEBUG", "2")).debug);
    for value in ["true", "false", "", "yes", "1.0"] {
        let error = load_err(base().set("DEBUG", value));
        assert_eq!(error.variable.as_deref(), Some("DEBUG"), "DEBUG={value:?}");
        assert!(error.message.contains("int(DEBUG)"), "{error}");
    }
}

// ENV.md §1.1/§2.2: `ALLOWED_HOSTS = env("ALLOWED_HOSTS", "*").split(",")`, no
// strip, no filtering; set-but-empty gives `[""]`.
#[test]
fn allowed_hosts() {
    assert_eq!(load(base()).allowed_hosts, vec!["*".to_string()]);
    assert_eq!(
        load(base().set("ALLOWED_HOSTS", "")).allowed_hosts,
        vec![String::new()]
    );
    assert_eq!(
        load(base().set("ALLOWED_HOSTS", "a.example.com,.b.example.com")).allowed_hosts,
        vec!["a.example.com".to_string(), ".b.example.com".to_string()]
    );
    assert_eq!(
        load(base().set("ALLOWED_HOSTS", " a, b")).allowed_hosts,
        vec![" a".to_string(), " b".to_string()]
    );
}

// ENV.md §2.3: a non-empty stripped list selects allow-list mode; anything
// else sets CORS_ALLOW_ALL_ORIGINS.
#[test]
fn cors_modes() {
    let unset = load(base());
    assert!(unset.cors.allow_all_origins);
    assert!(unset.cors.allowed_origins.is_empty());
    assert!(unset.cors.allow_credentials);
    assert_eq!(
        unset.cors.allow_headers,
        vec![
            "accept",
            "authorization",
            "content-type",
            "user-agent",
            "x-csrftoken",
            "x-requested-with",
            "X-API-Key"
        ]
    );
    assert_eq!(
        unset.cors.allow_methods,
        vec!["DELETE", "GET", "OPTIONS", "PATCH", "POST", "PUT"]
    );
    assert_eq!(unset.cors.preflight_max_age, 86400);

    let list = load(base().set("CORS_ALLOWED_ORIGINS", " a ,, b ,"));
    assert!(!list.cors.allow_all_origins);
    assert_eq!(
        list.cors.allowed_origins,
        vec!["a".to_string(), "b".to_string()]
    );

    let blank = load(base().set("CORS_ALLOWED_ORIGINS", " , "));
    assert!(blank.cors.allow_all_origins);
    assert!(blank.cors.allowed_origins.is_empty());
}

// ENV.md §2.1: `_cookies_secure(web_url, cors_allowed_origins, app_base_url)`
// uses the *raw* environment values and a substring test for "http:".
#[test]
fn cookie_secure_rule() {
    assert!(cookies_secure(Some("https://x"), &[], None));
    assert!(cookies_secure(Some(" HTTPS://x "), &[], None));
    assert!(cookies_secure(None, &[], Some("https://app")));
    assert!(cookies_secure(Some(""), &[], Some("https://app")));
    assert!(!cookies_secure(Some("http://x"), &[], None));
    assert!(!cookies_secure(None, &[], None));
    assert!(!cookies_secure(Some(""), &[], Some("localhost:3000")));

    let https_origins = vec!["https://a".to_string(), "https://b".to_string()];
    assert!(cookies_secure(None, &https_origins, None));
    let mixed = vec!["https://a".to_string(), "http://b".to_string()];
    assert!(!cookies_secure(None, &mixed, None));
    // The "http:" test is a substring test, so this flips to insecure.
    let sneaky = vec!["https://x/p?u=http:".to_string()];
    assert!(!cookies_secure(None, &sneaky, None));
}

// ENV.md §1.8: APP_BASE_URL/ADMIN_BASE_URL/SPACE_BASE_URL/LIVE_BASE_URL are
// validated with `is_valid_url` and silently nulled when invalid, but the raw
// value still decides cookie security.
#[test]
fn base_url_validation() {
    let invalid = load(base().set("APP_BASE_URL", "localhost:3000"));
    assert_eq!(invalid.urls.app_base_url, None);
    assert!(!invalid.cookies.session_cookie_secure);

    let valid = load(base().set("APP_BASE_URL", " http://x"));
    assert_eq!(valid.urls.app_base_url.as_deref(), Some(" http://x"));

    let https_raw = load(
        base()
            .set("APP_BASE_URL", "https://x")
            .set("CORS_ALLOWED_ORIGINS", "http://y"),
    );
    assert_eq!(https_raw.urls.app_base_url.as_deref(), Some("https://x"));
    assert!(https_raw.cookies.session_cookie_secure);

    let with_paths = load(
        base()
            .set("ADMIN_BASE_URL", "https://admin.example.com")
            .set("SPACE_BASE_URL", "https://space.example.com")
            .set("LIVE_BASE_URL", "https://live.example.com/base")
            .set("LIVE_BASE_PATH", "/live/"),
    );
    assert_eq!(with_paths.urls.admin_base_path, "/god-mode/");
    assert_eq!(with_paths.urls.space_base_path, "/spaces/");
    assert_eq!(
        with_paths.urls.live_url.as_deref(),
        Some("https://live.example.com/live/")
    );
    assert_eq!(with_paths.urls.app_base_path, "/");

    // `urljoin` replaces the last segment when the path has no leading slash.
    let relative = load(
        base()
            .set("LIVE_BASE_URL", "https://live.example.com/base")
            .set("LIVE_BASE_PATH", "live/"),
    );
    assert_eq!(
        relative.urls.live_url.as_deref(),
        Some("https://live.example.com/live/")
    );

    // No base URL means no LIVE_URL at all.
    assert_eq!(load(base()).urls.live_url, None);
}

// ENV.md §1.1: `COOKIE_DOMAIN` passes through unchanged, then `or None`; an
// empty value emits no Domain attribute.
#[test]
fn cookie_domain() {
    assert_eq!(load(base()).cookies.session_cookie_domain, None);
    assert_eq!(
        load(base().set("COOKIE_DOMAIN", ""))
            .cookies
            .session_cookie_domain,
        None
    );
    let domain = load(base().set("COOKIE_DOMAIN", ".plane.so"));
    assert_eq!(
        domain.cookies.session_cookie_domain.as_deref(),
        Some(".plane.so")
    );
    assert_eq!(
        domain.cookies.csrf_cookie_domain.as_deref(),
        Some(".plane.so")
    );
}

// ENV.md §1.8/§2: cookie ages, names and CSRF constants.
#[test]
fn cookie_constants() {
    let settings = load(base());
    assert_eq!(settings.cookies.session_cookie_name, "session-id");
    assert_eq!(settings.cookies.session_cookie_age, 604800);
    assert_eq!(
        settings.cookies.admin_session_cookie_name,
        "admin-session-id"
    );
    assert_eq!(settings.cookies.admin_session_cookie_age, 3600);
    assert!(settings.cookies.session_cookie_httponly);
    assert_eq!(settings.cookies.session_cookie_samesite, "Lax");
    assert_eq!(settings.cookies.session_cookie_path, "/");
    assert!(!settings.cookies.session_save_every_request);
    assert_eq!(settings.cookies.csrf_cookie_name, "csrftoken");
    assert_eq!(settings.cookies.csrf_cookie_age, 31449600);
    assert_eq!(settings.cookies.csrf_header_name, "X-CSRFToken");

    let custom = load(
        base()
            .set("SESSION_COOKIE_NAME", "plane-session")
            .set("SESSION_COOKIE_AGE", "10")
            .set("ADMIN_SESSION_COOKIE_AGE", "20")
            .set("SESSION_SAVE_EVERY_REQUEST", "1"),
    );
    assert_eq!(custom.cookies.session_cookie_name, "plane-session");
    assert_eq!(custom.cookies.session_cookie_age, 10);
    assert_eq!(custom.cookies.admin_session_cookie_age, 20);
    assert!(custom.cookies.session_save_every_request);

    // CSRF_COOKIE_SECURE = SESSION_COOKIE_SECURE.
    let secure = load(base().set("WEB_URL", "https://plane.example.com"));
    assert!(secure.cookies.csrf_cookie_secure);
    assert_eq!(secure.cookies.csrf_trusted_origins, Vec::<String>::new());
    let trusted = load(base().set("CORS_ALLOWED_ORIGINS", "https://a.example.com"));
    assert_eq!(
        trusted.cookies.csrf_trusted_origins,
        vec!["https://a.example.com".to_string()]
    );
}

// ENV.md §1.12/§2.8: `_retention_days` accepts 0, rejects negatives and
// non-integers; the three variables have different defaults.
#[test]
fn retention_days() {
    let defaults = load(base());
    assert_eq!(defaults.limits.api_activity_log_retention_days, 14);
    assert_eq!(defaults.limits.webhook_log_retention_days, 14);
    assert_eq!(defaults.limits.email_log_retention_days, 7);

    let values = load(
        base()
            .set("API_ACTIVITY_LOG_RETENTION_DAYS", "0")
            .set("WEBHOOK_LOG_RETENTION_DAYS", "abc")
            .set("EMAIL_LOG_RETENTION_DAYS", "-3"),
    );
    assert_eq!(values.limits.api_activity_log_retention_days, 0);
    assert_eq!(values.limits.webhook_log_retention_days, 14);
    assert_eq!(values.limits.email_log_retention_days, 7);

    let spaced = load(base().set("API_ACTIVITY_LOG_RETENTION_DAYS", " 30 "));
    assert_eq!(spaced.limits.api_activity_log_retention_days, 30);
}

// ENV.md §1.12: `FILE_SIZE_LIMIT` is int()-parsed with no validation and is
// also DATA_UPLOAD_MAX_MEMORY_SIZE.
#[test]
fn file_size_limit() {
    let defaults = load(base());
    assert_eq!(defaults.limits.file_size_limit, 5_242_880);
    assert_eq!(defaults.limits.data_upload_max_memory_size, 5_242_880);

    for value in ["", "5MB", "5242880.0"] {
        let error = load_err(base().set("FILE_SIZE_LIMIT", value));
        assert_eq!(
            error.variable.as_deref(),
            Some("FILE_SIZE_LIMIT"),
            "{value:?}"
        );
    }

    let custom = load(base().set("FILE_SIZE_LIMIT", "4096"));
    assert_eq!(custom.limits.file_size_limit, 4096);
    assert_eq!(custom.limits.data_upload_max_memory_size, 4096);
}

// ENV.md §1.12: the three plain `int()` variables crash at import.
#[test]
fn plain_int_variables() {
    let defaults = load(base());
    assert_eq!(defaults.limits.hard_delete_after_days, 60);
    assert_eq!(defaults.limits.unuploaded_asset_delete_days, 7);

    let negative = load(
        base()
            .set("HARD_DELETE_AFTER_DAYS", "-1")
            .set("UNUPLOADED_ASSET_DELETE_DAYS", "-1"),
    );
    assert_eq!(negative.limits.hard_delete_after_days, -1);
    assert_eq!(negative.limits.unuploaded_asset_delete_days, -1);

    for variable in ["HARD_DELETE_AFTER_DAYS", "UNUPLOADED_ASSET_DELETE_DAYS"] {
        let error = load_err(base().set(variable, ""));
        assert_eq!(error.variable.as_deref(), Some(variable));
    }
}

// ENV.md §1.10: NUM_PROXIES accepts 0-3 *decimal* characters (Unicode digits
// included) and warns for anything else, leaving the setting unset.
#[test]
fn num_proxies() {
    let unset = load(base());
    assert_eq!(unset.throttling.num_proxies, None);
    assert!(unset.warnings.is_empty());

    assert_eq!(
        load(base().set("NUM_PROXIES", "")).throttling.num_proxies,
        None
    );
    assert_eq!(
        load(base().set("NUM_PROXIES", "0")).throttling.num_proxies,
        Some(0)
    );
    assert_eq!(
        load(base().set("NUM_PROXIES", "1")).throttling.num_proxies,
        Some(1)
    );
    assert_eq!(
        load(base().set("NUM_PROXIES", "123"))
            .throttling
            .num_proxies,
        Some(123)
    );
    assert_eq!(
        load(base().set("NUM_PROXIES", " 1 "))
            .throttling
            .num_proxies,
        Some(1)
    );
    assert_eq!(
        load(base().set("NUM_PROXIES", "٣")).throttling.num_proxies,
        Some(3)
    );
    assert_eq!(
        load(base().set("NUM_PROXIES", "１２"))
            .throttling
            .num_proxies,
        Some(12)
    );

    for value in ["1234", "abc", "1.5", "-1", "１２３４"] {
        let settings = load(base().set("NUM_PROXIES", value));
        assert_eq!(settings.throttling.num_proxies, None, "{value:?}");
        assert_eq!(settings.warnings.len(), 1, "{value:?}");
        assert_eq!(settings.warnings[0].variable, "NUM_PROXIES");
    }

    let defaults = load(base());
    assert_eq!(defaults.throttling.api_key_rate_limit, "60/minute");
    assert_eq!(defaults.throttling.authentication_rate_limit, "10/minute");
    assert_eq!(defaults.throttling.anon_rate, "30/minute");
    assert_eq!(defaults.throttling.asset_rate, "5/minute");
    assert_eq!(defaults.throttling.email_verification_rate, "3/hour");
}

// ENV.md §0/§1.4: REDIS_URL is required by api/worker/beat but not migrator;
// REDIS_SSL is a substring test; REDIS_SSL_CERT_REQS comes from the URL query
// first, then the environment, then "required".
#[test]
fn redis_rules() {
    let error = load_err(EnvMap::new());
    assert_eq!(error.variable.as_deref(), Some("REDIS_URL"));

    let empty = load_err(EnvMap::new().set("REDIS_URL", ""));
    assert_eq!(empty.variable.as_deref(), Some("REDIS_URL"));

    // The migrator does not need Redis (deliberate departure from Django's
    // import-time coupling; ENV.md §0).
    let migrator = Settings::from_source(&EnvMap::new(), Role::Migrator).expect("migrator");
    assert_eq!(migrator.redis.url, None);
    assert_eq!(migrator.redis.ssl, None);

    let plain = load(base());
    assert_eq!(plain.redis.url.as_deref(), Some("redis://127.0.0.1:6379/0"));
    assert_eq!(plain.redis.ssl, Some(false));
    assert_eq!(plain.redis.ssl_cert_reqs, "required");
    assert_eq!(plain.redis.ssl_ca_certs, None);

    let tls = load(base().set("REDIS_URL", "rediss://h:6379/2"));
    assert_eq!(tls.redis.ssl, Some(true));
    // A password containing "rediss" turns the *cache* SSL on.
    let in_password = load(base().set("REDIS_URL", "redis://:rediss-secret@h/0"));
    assert_eq!(in_password.redis.ssl, Some(true));

    let env_value = load(base().set("REDIS_SSL_CERT_REQS", " OPTIONAL "));
    assert_eq!(env_value.redis.ssl_cert_reqs, "optional");

    let query_wins = load(
        base()
            .set("REDIS_URL", "rediss://h/0?ssl_cert_reqs=none")
            .set("REDIS_SSL_CERT_REQS", "optional"),
    );
    assert_eq!(query_wins.redis.ssl_cert_reqs, "none");

    // A blank query value is ignored by parse_qs, so the env var applies.
    let blank_query = load(
        base()
            .set("REDIS_URL", "rediss://h/0?ssl_cert_reqs=")
            .set("REDIS_SSL_CERT_REQS", "optional"),
    );
    assert_eq!(blank_query.redis.ssl_cert_reqs, "optional");

    let ca = load(base().set("REDIS_SSL_CA_CERTS", "/etc/ssl/ca.pem"));
    assert_eq!(ca.redis.ssl_ca_certs.as_deref(), Some("/etc/ssl/ca.pem"));
    assert_eq!(
        load(base().set("REDIS_SSL_CA_CERTS", ""))
            .redis
            .ssl_ca_certs,
        None
    );

    let empty_cert_reqs = load(base().set("REDIS_SSL_CERT_REQS", "  "));
    assert_eq!(empty_cert_reqs.redis.ssl_cert_reqs, "required");
}

// ENV.md §2.4: AMQP_URL wins when truthy; otherwise the URL is composed with no
// percent-encoding and the default vhost "/" produces a double slash.
#[test]
fn broker_url() {
    assert_eq!(
        load(base()).broker.url,
        "amqp://guest:guest@localhost:5672//"
    );

    let explicit = load(base().set("AMQP_URL", "amqp://u:p@h:1/v"));
    assert_eq!(explicit.broker.url, "amqp://u:p@h:1/v");

    let empty = load(base().set("AMQP_URL", ""));
    assert_eq!(empty.broker.url, "amqp://guest:guest@localhost:5672//");

    // ENV.md §1.5: valid iff 0 < value <= 10_000_000, else 360; malformed is
    // not fatal (unlike DEBUG/FILE_SIZE_LIMIT).
    assert_eq!(load(base()).broker.metrics_push_interval_minutes, 360);
    for (raw, expected) in [
        ("60", 60),
        ("10000000", 10_000_000),
        ("0", 360),
        ("-1", 360),
        ("10000001", 360),
        ("soon", 360),
        ("", 360),
    ] {
        assert_eq!(
            load(base().set("METRICS_PUSH_INTERVAL_MINUTES", raw))
                .broker
                .metrics_push_interval_minutes,
            expected,
            "{raw:?}"
        );
    }

    let composed = load(
        base()
            .set("RABBITMQ_HOST", "mq")
            .set("RABBITMQ_PORT", "55672")
            .set("RABBITMQ_USER", "plane@x")
            .set("RABBITMQ_PASSWORD", "p:w")
            .set("RABBITMQ_VHOST", "plane"),
    );
    assert_eq!(composed.broker.url, "amqp://plane@x:p:w@mq:55672/plane");
}

// ENV.md §1.6/§2.5/§5: two USE_MINIO parsers, the endpoint fallback, the two
// AWS_S3_BUCKET_NAME defaults and the derived custom domain.
#[test]
fn storage_rules() {
    let defaults = load(base());
    assert!(!defaults.storage.use_minio_settings);
    assert!(!defaults.storage.use_minio_storage);
    assert_eq!(defaults.storage.bucket_name, "uploads");
    assert_eq!(defaults.storage.bucket_name_storage, None);
    assert_eq!(defaults.storage.region, "");
    assert_eq!(defaults.storage.region_storage, None);
    assert_eq!(defaults.storage.endpoint_url, None);
    assert_eq!(defaults.storage.custom_domain, None);
    assert_eq!(defaults.storage.signed_url_expiration_raw, "3600");
    assert_eq!(defaults.storage.signed_url_expiration(), Ok(3600));

    let one = load(base().set("USE_MINIO", "1"));
    assert!(one.storage.use_minio_settings);
    assert!(one.storage.use_minio_storage);

    // int("01") == 1 but "01" != "1": the two parsers disagree.
    let padded = load(base().set("USE_MINIO", "01"));
    assert!(padded.storage.use_minio_settings);
    assert!(!padded.storage.use_minio_storage);

    // int("true") raises at import.
    let error = load_err(base().set("USE_MINIO", "true"));
    assert_eq!(error.variable.as_deref(), Some("USE_MINIO"));

    let minio_only = load(base().set("MINIO_ENDPOINT_URL", "http://minio:9000"));
    assert_eq!(
        minio_only.storage.endpoint_url.as_deref(),
        Some("http://minio:9000")
    );
    assert_eq!(minio_only.storage.aws_s3_endpoint_url, None);
    assert_eq!(
        minio_only.storage.minio_endpoint_url.as_deref(),
        Some("http://minio:9000")
    );

    // Empty endpoint variables count as absent.
    let both = load(
        base()
            .set("AWS_S3_ENDPOINT_URL", "")
            .set("MINIO_ENDPOINT_URL", "http://minio:9000"),
    );
    assert_eq!(
        both.storage.endpoint_url.as_deref(),
        Some("http://minio:9000")
    );

    // WEB_URL unset -> http://localhost, custom domain localhost/uploads.
    let custom = load(
        base()
            .set("USE_MINIO", "1")
            .set("AWS_S3_ENDPOINT_URL", "http://minio:9000")
            .set("AWS_S3_BUCKET_NAME", "uploads"),
    );
    assert_eq!(
        custom.storage.custom_domain.as_deref(),
        Some("localhost/uploads")
    );
    assert_eq!(custom.storage.url_protocol.as_deref(), Some("http:"));

    // WEB_URL set but empty is NOT replaced: urlparse("") gives ("", "").
    let empty_web = load(
        base()
            .set("USE_MINIO", "1")
            .set("AWS_S3_ENDPOINT_URL", "http://minio:9000")
            .set("WEB_URL", ""),
    );
    assert_eq!(empty_web.storage.custom_domain.as_deref(), Some("/uploads"));
    assert_eq!(empty_web.storage.url_protocol.as_deref(), Some(":"));

    // WEB_URL with a path: only the netloc is used.
    let with_path = load(
        base()
            .set("USE_MINIO", "1")
            .set("AWS_S3_ENDPOINT_URL", "http://minio:9000")
            .set("WEB_URL", "https://example.com/plane"),
    );
    assert_eq!(
        with_path.storage.custom_domain.as_deref(),
        Some("example.com/uploads")
    );
    assert_eq!(with_path.storage.url_protocol.as_deref(), Some("https:"));

    // No endpoint -> no custom domain even with MinIO on.
    let no_endpoint = load(base().set("USE_MINIO", "1"));
    assert_eq!(no_endpoint.storage.custom_domain, None);

    // SIGNED_URL_EXPIRATION is parsed at the first presign, not at import.
    let bad_expiry = load(base().set("SIGNED_URL_EXPIRATION", ""));
    assert_eq!(
        bad_expiry.storage.signed_url_expiration(),
        Err(py::PyIntError::Invalid)
    );
}

// ENV.md §1.7/§5: EMAIL_PORT is seeded as a string but the call-site fallback is
// the integer 587; EMAIL_USE_TLS defaults to on.
#[test]
fn email_rules() {
    let defaults = load(base());
    assert_eq!(defaults.email.host, None);
    assert_eq!(defaults.email.port_raw, None);
    assert_eq!(defaults.email.port(), Ok(587));
    assert!(defaults.email.use_tls);
    assert!(!defaults.email.use_ssl);
    assert_eq!(defaults.email.from, None);
    assert_eq!(defaults.email.enable_smtp, "0");
    assert_eq!(
        defaults.email.backend,
        "django.core.mail.backends.console.EmailBackend"
    );

    let seeded = load(
        base()
            .set("EMAIL_HOST", "smtp.example.com")
            .set("EMAIL_PORT", "2525")
            .set("EMAIL_USE_TLS", "0")
            .set("EMAIL_USE_SSL", "1"),
    );
    assert_eq!(seeded.email.host.as_deref(), Some("smtp.example.com"));
    assert_eq!(seeded.email.port(), Ok(2525));
    assert!(!seeded.email.use_tls);
    assert!(seeded.email.use_ssl);

    let empty_port = load(base().set("EMAIL_PORT", ""));
    assert_eq!(empty_port.email.port(), Err(py::PyIntError::Invalid));
}

// ENV.md §1.2/§1.13/§1.1: small flags and raw values.
#[test]
fn misc_flags() {
    let defaults = load(base());
    assert!(!defaults.runtime.enable_drf_spectacular);
    assert_eq!(defaults.runtime.settings_module, None);
    assert_eq!(defaults.runtime.app_version, None);
    assert!(defaults.skip_env_var);
    assert!(!defaults.integrations.is_test);
    assert_eq!(defaults.integrations.llm_api_key, None);
    assert_eq!(defaults.integrations.github_access_token, None);

    let set = load(
        base()
            .set("ENABLE_DRF_SPECTACULAR", "1")
            .set("DJANGO_SETTINGS_MODULE", "plane.settings.local")
            .set("APP_VERSION", "")
            .set("SKIP_ENV_VAR", "0")
            .set("IS_TEST", "1")
            .set("GITHUB_ACCESS_TOKEN", "x"),
    );
    assert!(set.runtime.enable_drf_spectacular);
    assert_eq!(
        set.runtime.settings_module.as_deref(),
        Some("plane.settings.local")
    );
    assert_eq!(set.runtime.app_version.as_deref(), Some(""));
    assert!(!set.skip_env_var);
    assert!(set.integrations.is_test);
    assert_eq!(set.integrations.github_access_token.as_deref(), Some("x"));

    // "01" is not "1" for the `== "1"` comparisons.
    let padded = load(base().set("IS_TEST", "01").set("SKIP_ENV_VAR", "01"));
    assert!(!padded.integrations.is_test);
    assert!(!padded.skip_env_var);
}

// ENV.md §1.11/§2.7: OTLP protocol is stripped and lower-cased; the insecure
// flag is a lower-case "true" with no strip.
#[test]
fn telemetry_rules() {
    let defaults = load(base());
    assert_eq!(
        defaults.telemetry.otlp_endpoint,
        "https://telemetry.plane.so"
    );
    assert_eq!(defaults.telemetry.otlp_metrics_protocol, "grpc");
    assert!(!defaults.telemetry.otel_metrics_insecure);
    assert_eq!(defaults.telemetry.service_name, "plane-ce-api");
    assert_eq!(defaults.telemetry.scout_monitor, None);

    let set = load(
        base()
            .set("OTLP_METRICS_PROTOCOL", " HTTP ")
            .set("OTEL_EXPORTER_OTLP_METRICS_INSECURE", "TrUe")
            .set("SERVICE_NAME", "plane-worker")
            .set("SCOUT_MONITOR", "0"),
    );
    assert_eq!(set.telemetry.otlp_metrics_protocol, "http");
    assert!(set.telemetry.otel_metrics_insecure);
    assert_eq!(set.telemetry.service_name, "plane-worker");
    // Any non-empty value is truthy in Python.
    assert_eq!(set.telemetry.scout_monitor.as_deref(), Some("0"));

    // No strip: " true" is false.
    let spaced = load(base().set("OTEL_EXPORTER_OTLP_METRICS_INSECURE", " true"));
    assert!(!spaced.telemetry.otel_metrics_insecure);

    // Empty protocol falls back to grpc (`or "grpc"`).
    let empty = load(base().set("OTLP_METRICS_PROTOCOL", ""));
    assert_eq!(empty.telemetry.otlp_metrics_protocol, "grpc");
}

// ENV.md §1.1: webhook lists. Invalid networks are skipped with a warning and
// never abort startup.
#[test]
fn webhook_rules() {
    let defaults = load(base());
    assert!(defaults.webhook.allowed_ips.is_empty());
    assert!(defaults.webhook.allowed_hosts.is_empty());
    assert!(defaults.webhook.disallowed_domains.is_empty());
    assert!(defaults.warnings.is_empty());

    let parsed = load(
        base()
            .set(
                "WEBHOOK_ALLOWED_IPS",
                "10.0.0.5/8, ::1/128, nope, 192.168.1.0/24",
            )
            .set(
                "WEBHOOK_ALLOWED_HOSTS",
                " Example.COM. ,, internal.example ",
            )
            .set(
                "WEBHOOK_DISALLOWED_DOMAINS",
                "localhost.,, Evil.Example.ORG ",
            ),
    );
    assert_eq!(
        parsed.webhook.allowed_ips,
        vec![
            "10.0.0.0/8".parse::<IpNet>().unwrap(),
            "::1/128".parse::<IpNet>().unwrap(),
            "192.168.1.0/24".parse::<IpNet>().unwrap()
        ]
    );
    assert_eq!(
        parsed.webhook.allowed_hosts,
        vec!["example.com".to_string(), "internal.example".to_string()]
    );
    assert_eq!(
        parsed.webhook.disallowed_domains,
        vec!["localhost".to_string(), "evil.example.org".to_string()]
    );
    assert_eq!(parsed.warnings.len(), 1);
    assert_eq!(parsed.warnings[0].variable, "WEBHOOK_ALLOWED_IPS");
}

// ENV.md §1.1: every form CPython 3.12's `ip_network(x, strict=False)` accepts
// must be accepted, including bare addresses, host bits, dotted netmasks and
// hostmasks; everything it rejects is warning-skipped (common.py:62-69).
#[test]
fn webhook_allowed_ips_python_network_forms() {
    let parsed = load(base().set(
        "WEBHOOK_ALLOWED_IPS",
        concat!(
            "10.0.0.5, ::1, 10.0.0.5/8, 172.16.0.0/255.240.0.0, ",
            "192.168.0.0/0.0.15.255, fe80::1%eth0/64, ",
            "10.0.0.0/33, nope"
        ),
    ));
    assert_eq!(
        parsed.webhook.allowed_ips,
        vec![
            "10.0.0.5/32".parse::<IpNet>().unwrap(),
            "::1/128".parse::<IpNet>().unwrap(),
            "10.0.0.0/8".parse::<IpNet>().unwrap(),
            "172.16.0.0/12".parse::<IpNet>().unwrap(),
            "192.168.0.0/20".parse::<IpNet>().unwrap(),
            "fe80::/64".parse::<IpNet>().unwrap(),
        ]
    );
    let invalid: Vec<&str> = parsed
        .warnings
        .iter()
        .filter(|warning| warning.variable == "WEBHOOK_ALLOWED_IPS")
        .map(|warning| warning.message.as_str())
        .collect();
    assert_eq!(invalid.len(), 2);
    assert!(invalid[0].contains("\"10.0.0.0/33\""), "{invalid:?}");
    assert!(invalid[1].contains("\"nope\""), "{invalid:?}");
}

/// `crates/plane-core/tests/golden/ip_network.jsonl` is generated by
/// `apps/server/tools/net/ip_network_corpus.py` from CPython's own
/// `ipaddress.ip_network(x, strict=False)`. Every row must agree on
/// accept/reject, address family, prefix length and network address. A failure
/// means the port differs from Django, never that the corpus is wrong.
#[test]
fn ip_network_matches_the_python_oracle() {
    #[derive(serde::Deserialize)]
    struct Row {
        id: String,
        input: String,
        ok: bool,
        #[serde(default)]
        version: u8,
        #[serde(default)]
        prefix: u8,
        #[serde(default)]
        address_hex: String,
    }

    const CORPUS: &str = include_str!("../../tests/golden/ip_network.jsonl");
    let mut cases = 0;
    let mut accepted = 0;
    for line in CORPUS.lines().filter(|line| !line.trim().is_empty()) {
        let row: Row = serde_json::from_str(line).expect("corpus rows are valid JSON");
        let parsed = parse_python_network(&row.input);
        assert_eq!(
            parsed.is_some(),
            row.ok,
            "{}: accept/reject for {:?}",
            row.id,
            row.input
        );
        if let Some(network) = parsed {
            accepted += 1;
            let (version, address_hex) = match network.addr() {
                IpAddr::V4(address) => (4, format!("{:x}", u32::from(address))),
                IpAddr::V6(address) => (6, format!("{:x}", u128::from(address))),
            };
            assert_eq!(
                version, row.version,
                "{}: family for {:?}",
                row.id, row.input
            );
            assert_eq!(
                network.prefix_len(),
                row.prefix,
                "{}: prefix for {:?}",
                row.id,
                row.input
            );
            assert_eq!(
                address_hex, row.address_hex,
                "{}: network address for {:?}",
                row.id, row.input
            );
        }
        cases += 1;
    }
    assert!(cases > 200, "corpus unexpectedly small: {cases} cases");
    assert!(accepted > 100, "corpus accepted too few cases: {accepted}");
    assert!(cases - accepted > 10, "corpus rejected too few cases");
}

// Credentials must never be printable: a `tracing::debug!` of the settings is a
// plausible future change, and AGENTS.md forbids secrets in logs.
#[test]
fn debug_redacts_credentials() {
    let settings = load(
        base()
            .set("DATABASE_URL", "postgresql://plane:s3cret@db:5432/plane")
            .set("REDIS_URL", "redis://:r3dis@cache:6379/0")
            .set("RABBITMQ_PASSWORD", "amqp-s3cret")
            .set("AWS_SECRET_ACCESS_KEY", "minio-s3cret")
            .set("EMAIL_HOST_PASSWORD", "smtp-s3cret")
            .set("LLM_API_KEY", "llm-s3cret"),
    );
    let debug = format!("{settings:?}");
    for secret in [
        "s3cret",
        "r3dis",
        "amqp-s3cret",
        "minio-s3cret",
        "smtp-s3cret",
        "llm-s3cret",
    ] {
        assert!(!debug.contains(secret), "Debug leaked {secret}: {debug}");
    }
    assert!(debug.contains("<redacted>"));
}

// ENV.md §1.3: DATABASE_URL wins only when non-empty; the discrete variables
// keep their unset-vs-empty distinction and the port default.
#[test]
fn database_source_selection() {
    let defaults = load(base());
    assert_eq!(defaults.database.url, None);
    assert_eq!(defaults.database.params.db, None);
    assert_eq!(defaults.database.params.user, None);
    assert_eq!(defaults.database.params.host, None);
    assert_eq!(defaults.database.params.port, "5432");
    assert!(!defaults.database.enable_read_replica);
    assert_eq!(defaults.database.replica_url, None);
    assert_eq!(defaults.database.replica_params.port, "5432");

    let url = load(base().set("DATABASE_URL", "postgresql://u:p@h:1/db"));
    assert_eq!(url.database.url.as_deref(), Some("postgresql://u:p@h:1/db"));

    // Set-but-empty falls back to the discrete variables.
    let empty = load(
        base()
            .set("DATABASE_URL", "")
            .set("POSTGRES_DB", "")
            .set("POSTGRES_HOST", "db"),
    );
    assert_eq!(empty.database.url, None);
    assert_eq!(empty.database.params.db.as_deref(), Some(""));
    assert_eq!(empty.database.params.host.as_deref(), Some("db"));

    let replica = load(
        base()
            .set("ENABLE_READ_REPLICA", "1")
            .set("DATABASE_READ_REPLICA_URL", "postgresql://r:p@rh:2/rdb")
            .set("POSTGRES_READ_REPLICA_PORT", "6543"),
    );
    assert!(replica.database.enable_read_replica);
    assert_eq!(
        replica.database.replica_url.as_deref(),
        Some("postgresql://r:p@rh:2/rdb")
    );
    assert_eq!(replica.database.replica_params.port, "6543");

    let replica_empty = load(
        base()
            .set("ENABLE_READ_REPLICA", "1")
            .set("DATABASE_READ_REPLICA_URL", "")
            .set("POSTGRES_READ_REPLICA_DB", "rdb"),
    );
    assert_eq!(replica_empty.database.replica_url, None);
    assert_eq!(
        replica_empty.database.replica_params.db.as_deref(),
        Some("rdb")
    );

    // Only the exact string "1" enables the replica.
    assert!(
        !load(base().set("ENABLE_READ_REPLICA", "true"))
            .database
            .enable_read_replica
    );
}

// ENV.md §6: the PLANE_RS_* knobs.
#[test]
fn rust_knobs() {
    let defaults = load(base());
    assert_eq!(defaults.rust.bind, "0.0.0.0:8000".parse().unwrap());
    assert_eq!(defaults.rust.worker_threads, None);
    assert_eq!(defaults.rust.db_pool_size, 16);
    assert_eq!(defaults.rust.log_format, LogFormat::Json);

    let port = load(base().set("PORT", "9000"));
    assert_eq!(port.rust.bind, "0.0.0.0:9000".parse().unwrap());

    let explicit = load(
        base()
            .set("PORT", "9000")
            .set("PLANE_RS_BIND", "127.0.0.1:18101"),
    );
    assert_eq!(explicit.rust.bind, "127.0.0.1:18101".parse().unwrap());

    assert_eq!(
        load_err(base().set("PLANE_RS_BIND", "nope"))
            .variable
            .as_deref(),
        Some("PLANE_RS_BIND")
    );
    assert_eq!(
        load_err(base().set("PORT", "nope")).variable.as_deref(),
        Some("PORT")
    );

    assert_eq!(
        load(base().set("PLANE_RS_WORKER_THREADS", "0"))
            .rust
            .worker_threads,
        None
    );
    assert_eq!(
        load(base().set("PLANE_RS_WORKER_THREADS", "4"))
            .rust
            .worker_threads,
        Some(4)
    );
    assert_eq!(
        load_err(base().set("PLANE_RS_WORKER_THREADS", "-1"))
            .variable
            .as_deref(),
        Some("PLANE_RS_WORKER_THREADS")
    );

    assert_eq!(
        load(base().set("PLANE_RS_DB_POOL_SIZE", "1"))
            .rust
            .db_pool_size,
        1
    );
    assert_eq!(
        load_err(base().set("PLANE_RS_DB_POOL_SIZE", "0"))
            .variable
            .as_deref(),
        Some("PLANE_RS_DB_POOL_SIZE")
    );

    assert_eq!(
        load(base().set("PLANE_RS_LOG_FORMAT", "pretty"))
            .rust
            .log_format,
        LogFormat::Pretty
    );
    assert_eq!(
        load_err(base().set("PLANE_RS_LOG_FORMAT", "xml"))
            .variable
            .as_deref(),
        Some("PLANE_RS_LOG_FORMAT")
    );
}

// ENV.md §1.9: the auth values are raw strings that call sites re-parse with
// their own defaults; nothing is validated here.
#[test]
fn auth_raw_values() {
    let defaults = load(base());
    assert_eq!(defaults.auth.enable_signup, None);
    assert_eq!(defaults.auth.enable_magic_link_login, None);
    assert_eq!(defaults.auth.gitlab_host, None);

    let set = load(
        base()
            .set("ENABLE_SIGNUP", "0")
            .set("ENABLE_MAGIC_LINK_LOGIN", "0")
            .set("GITLAB_HOST", "")
            .set("GITHUB_APP_NAME", ""),
    );
    assert_eq!(set.auth.enable_signup.as_deref(), Some("0"));
    assert_eq!(set.auth.enable_magic_link_login.as_deref(), Some("0"));
    assert_eq!(set.auth.gitlab_host.as_deref(), Some(""));
    assert_eq!(set.auth.github_app_name.as_deref(), Some(""));
}

// ENV.md §1.8: INSTANCE_CHANGELOG_URL is a plain string with an empty default.
#[test]
fn changelog_url() {
    assert_eq!(load(base()).urls.instance_changelog_url, "");
    assert_eq!(
        load(base().set("INSTANCE_CHANGELOG_URL", "https://x/changelog"))
            .urls
            .instance_changelog_url,
        "https://x/changelog"
    );
}

// ENV.md §2.1: the session cookie's Secure flag follows WEB_URL/APP_BASE_URL,
// with CORS as the second rule.
#[test]
fn session_cookie_secure_is_derived() {
    assert!(!load(base()).cookies.session_cookie_secure);
    assert!(
        load(base().set("WEB_URL", "https://plane.example.com"))
            .cookies
            .session_cookie_secure
    );
    assert!(
        load(base().set("APP_BASE_URL", "https://app.example.com"))
            .cookies
            .session_cookie_secure
    );
    assert!(
        load(base().set("CORS_ALLOWED_ORIGINS", "https://a.example.com"))
            .cookies
            .session_cookie_secure
    );
    assert!(
        !load(base().set("CORS_ALLOWED_ORIGINS", "http://a.example.com"))
            .cookies
            .session_cookie_secure
    );
}
