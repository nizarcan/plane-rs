// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's SSRF unit tests, ported one by one.
//!
//! Sources:
//!
//! * `apps/api/plane/tests/unit/bg_tasks/test_url_security.py`
//! * `apps/api/plane/tests/unit/bg_tasks/test_ssrf_advisories.py`
//! * `apps/api/plane/tests/unit/bg_tasks/test_work_item_link_task.py`
//!
//! Tests that need pure functions live here; the ones that assert on a real
//! connection stay in `live_https.rs`, and the ones that only repeat the
//! corpus are marked as covered by `corpus.rs`. The Python test name is quoted
//! above each port so coverage is auditable both ways.
//!
//! What the Python tests assert about `requests.Session` calls (the rewritten
//! URL, `allow_redirects=False`, `trust_env=False`, `proxies={...}`) has a
//! different shape here: the URL keeps the name and the pinning happens in the
//! resolver. The assertions below check the same *properties* -- pinned
//! address, `Host` header, SNI name, no proxy, no redirect following.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};

use plane_net::client::OutboundRequest;
use plane_net::dns::StubResolver;
use plane_net::preset::{
    self, FAVICON_GET, FAVICON_HEAD, LINK_TITLE, OAUTH_AVATAR, ProxyMode, WEBHOOK,
};
use plane_net::ssrf::{self, SsrfPolicy};
use plane_net::webhook;
use plane_net::{HttpEngine, pyurl};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn public_only() -> StubResolver {
    StubResolver::new(
        BTreeMap::from([(
            "example.com".to_string(),
            vec![IpAddr::from([93, 184, 216, 34])],
        )]),
        Some(vec![IpAddr::from([93, 184, 216, 34])]),
    )
}

fn private_answer() -> StubResolver {
    StubResolver::new(BTreeMap::new(), Some(vec![IpAddr::from([172, 18, 0, 5])]))
}

// ---------------------------------------------------------------------------
// test_url_security.py :: TestPinnedFetch
// ---------------------------------------------------------------------------

// `test_connects_to_validated_ip_not_hostname`
#[test]
fn test_connects_to_validated_ip_not_hostname() {
    let hop = runtime()
        .block_on(ssrf::validate_hop(
            &public_only(),
            "https://example.com/hook",
            &SsrfPolicy::strict(),
        ))
        .expect("hop");
    // The address that will be pinned is the validated one.
    assert_eq!(
        hop.addresses
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["93.184.216.34"]
    );
    // `Host` and SNI carry the original name, exactly as Python's rewritten URL
    // carries them in the header and the urllib3 pool key.
    assert_eq!(hop.target.host_header(), "example.com");
    assert_eq!(hop.target.request_url(), "https://example.com:443/hook");
    // The webhook preset never follows redirects (`allow_redirects=False`).
    assert!(!WEBHOOK.follows_redirects());
    // Ambient proxies and env credentials are not used (`trust_env=False`,
    // `proxies={"http": None, "https": None}`).
    assert_eq!(WEBHOOK.proxy, ProxyMode::Disabled);
}

// `test_non_default_port_in_host_header`
#[test]
fn test_non_default_port_in_host_header() {
    let hop = runtime()
        .block_on(ssrf::validate_hop(
            &public_only(),
            "http://example.com:8080/x",
            &SsrfPolicy::strict(),
        ))
        .expect("hop");
    assert_eq!(hop.target.port, 8080);
    assert_eq!(hop.target.host_header(), "example.com:8080");
    assert_eq!(hop.target.request_url(), "http://example.com:8080/x");
}

// `test_ipv6_validated_ip_is_bracketed`
#[test]
fn test_ipv6_validated_ip_is_bracketed() {
    let resolver = StubResolver::new(
        BTreeMap::from([(
            "example.com".to_string(),
            vec!["2606:4700:4700::1111".parse::<IpAddr>().expect("ip")],
        )]),
        None,
    );
    let hop = runtime()
        .block_on(ssrf::validate_hop(
            &resolver,
            "https://example.com/x",
            &SsrfPolicy::strict(),
        ))
        .expect("hop");
    assert_eq!(hop.target.request_url(), "https://example.com:443/x");
    // An IPv6 *literal* hostname is bracketed, as in `_request_to_ip`.
    let hop = runtime()
        .block_on(ssrf::validate_hop(
            &resolver,
            "https://[2606:4700:4700::1111]/x",
            &SsrfPolicy::strict(),
        ))
        .expect("hop");
    assert_eq!(hop.target.host_header(), "[2606:4700:4700::1111]");
    assert_eq!(
        hop.target.request_url(),
        "https://[2606:4700:4700::1111]:443/x"
    );
}

// `test_blocked_target_raises_before_any_request`
#[test]
fn test_blocked_target_raises_before_any_request() {
    let error = runtime()
        .block_on(ssrf::validate_hop(
            &private_answer(),
            "https://attacker.com/hook",
            &SsrfPolicy::strict(),
        ))
        .unwrap_err();
    assert_eq!(error.message(), ssrf::BLOCKED_NETWORK);
    // Nothing here builds a client; validation is a separate step, so a blocked
    // target cannot reach the transport.
}

// `test_allowed_host_skips_block_check_but_still_pins`
#[test]
fn test_allowed_host_skips_block_check_but_still_pins() {
    let policy = SsrfPolicy::new(vec![], ["silo".to_string()]);
    let hop = runtime()
        .block_on(ssrf::validate_hop(
            &private_answer(),
            "http://silo:3000/hook",
            &policy,
        ))
        .expect("trusted");
    assert!(hop.trusted);
    assert_eq!(hop.addresses, vec![IpAddr::from([172, 18, 0, 5])]);
    assert_eq!(hop.target.host_header(), "silo:3000");
    assert!(!WEBHOOK.follows_redirects());
}

// ---------------------------------------------------------------------------
// test_url_security.py :: TestReviewFixes
// ---------------------------------------------------------------------------

// `test_url_embedded_credentials_become_basic_auth`
#[test]
fn test_url_embedded_credentials_become_basic_auth() {
    let target = pyurl::split_target("https://user:p%40ss@example.com/hook").expect("split");
    assert_eq!(target.request_url(), "https://example.com:443/hook");
    assert_eq!(target.host_header(), "example.com");
    assert_eq!(target.auth, Some(("user".to_string(), "p@ss".to_string())));
}

// `test_no_credentials_passes_auth_none`
#[test]
fn test_no_credentials_passes_auth_none() {
    let target = pyurl::split_target("https://example.com/x").expect("split");
    assert!(target.auth.is_none());
}

// `test_ipv6_literal_host_header_is_bracketed`
#[test]
fn test_ipv6_literal_host_header_is_bracketed() {
    let target = pyurl::split_target("https://[2606:4700:4700::1111]/x").expect("split");
    assert_eq!(target.hostname, "2606:4700:4700::1111");
    assert_eq!(target.host_header(), "[2606:4700:4700::1111]");
}

// `test_idna_unicode_error_is_treated_as_unresolvable`
#[test]
fn test_idna_unicode_error_is_treated_as_unresolvable() {
    // U+FFFD is prohibited by IDNA2003 and disallowed by UTS #46; CPython's
    // `getaddrinfo` surfaces that as UnicodeError, which the validator turns
    // into ValueError("Hostname could not be resolved").
    assert!(plane_net::dns::resolver_name("a\u{fffd}b").is_err());
    let resolver = StubResolver::new(BTreeMap::new(), None);
    let error = runtime()
        .block_on(ssrf::resolve_and_validate(
            &resolver,
            "a\u{fffd}b",
            &SsrfPolicy::strict(),
            true,
        ))
        .expect_err("unresolvable");
    assert_eq!(error.message(), ssrf::UNRESOLVABLE);
}

// `test_stream_defers_session_close_until_response_close`: the Rust client reads
// the body while the connection is open, so a cap bounds memory. Exercised in
// `live_https.rs::body_caps_are_enforced_before_and_while_reading`.

// ---------------------------------------------------------------------------
// test_work_item_link_task.py :: TestValidateUrlIp / TestValidateUrlAllowlist
// ---------------------------------------------------------------------------

// TestValidateUrlIp.test_rejects_non_http_scheme
#[test]
fn test_validate_url_ip_rejects_non_http_scheme() {
    let error = runtime()
        .block_on(ssrf::validate_url_ip(&public_only(), "file:///etc/passwd"))
        .expect_err("scheme");
    // `validate_url_ip` checks the scheme *before* the hostname, unlike
    // `validate_url`, and `file:///etc/passwd` therefore reports the scheme.
    assert_eq!(error.message(), pyurl::INVALID_SCHEME);
}

// TestValidateUrlIp.test_rejects_private_ip / test_rejects_loopback
#[test]
fn test_validate_url_ip_rejects_private_and_loopback() {
    for (answer, url) in [
        ("192.168.1.1", "http://example.com"),
        ("127.0.0.1", "http://example.com"),
        ("100.64.0.1", "http://attacker.example.com"),
        ("224.0.0.1", "http://attacker.example.com"),
        ("0.0.0.0", "http://attacker.example.com"),
        ("::ffff:169.254.169.254", "http://attacker.example.com"),
        ("64:ff9b::a9fe:a9fe", "http://attacker.example.com"),
    ] {
        let resolver = StubResolver::new(
            BTreeMap::new(),
            Some(vec![answer.parse::<IpAddr>().expect("ip")]),
        );
        let error = runtime()
            .block_on(ssrf::validate_url_ip(&resolver, url))
            .expect_err(answer);
        assert_eq!(error.message(), ssrf::BLOCKED_NETWORK, "{answer}");
    }
}

// TestValidateUrlIp.test_allows_public_ip
#[test]
fn test_validate_url_ip_allows_public_ip() {
    let resolver = StubResolver::new(
        BTreeMap::new(),
        Some(vec![IpAddr::from([93, 184, 216, 34])]),
    );
    assert!(
        runtime()
            .block_on(ssrf::validate_url_ip(&resolver, "https://example.com"))
            .is_ok()
    );
}

// ---------------------------------------------------------------------------
// test_ssrf_advisories.py :: TestWebhookUrlValidation
// ---------------------------------------------------------------------------

// `test_webhook_url_to_internal_is_rejected` (GHSA-m3f8-q4wj-9grv /
// CVE-2026-30242 / GHSA-75fg-f8qg-23wv). The IP list is the Python parameter
// list; the allow-list variants are in `corpus.rs` and `ssrf::tests`.
#[test]
fn test_webhook_url_to_internal_is_rejected() {
    for ip in [
        "169.254.169.254",
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.0.1",
        "::1",
        "100.64.0.1",
        "2002:7f00:1::",
        "224.0.0.1",
        "::ffff:169.254.169.254",
    ] {
        let resolver = StubResolver::new(
            BTreeMap::new(),
            Some(vec![ip.parse::<IpAddr>().expect("ip")]),
        );
        let error = runtime()
            .block_on(webhook::validate_webhook_url(
                &resolver,
                "https://attacker.example.com/hook",
                &SsrfPolicy::strict(),
                &[],
                Some("myplane.example.com"),
            ))
            .expect_err(ip);
        assert_eq!(error, webhook::WebhookUrlError::InvalidOrDisallowed, "{ip}");
    }
}

// `test_legitimate_public_webhook_url_passes`
#[test]
fn test_legitimate_public_webhook_url_passes() {
    let resolver = StubResolver::new(
        BTreeMap::new(),
        Some(vec![IpAddr::from([93, 184, 216, 34])]),
    );
    assert_eq!(
        runtime().block_on(webhook::validate_webhook_url(
            &resolver,
            "https://hooks.example.com/x",
            &SsrfPolicy::strict(),
            &[],
            Some("myplane.example.com"),
        )),
        Ok(())
    );
}

// ---------------------------------------------------------------------------
// Preset-level assertions from the favicon / avatar advisories
// ---------------------------------------------------------------------------

// TestFaviconRedirect / TestOAuthAvatarSSRF: every path that fetches an
// attacker-controlled URL must use the pinned client.
#[test]
fn test_attacker_controlled_fetches_use_the_pinned_client() {
    for preset in [LINK_TITLE, FAVICON_HEAD, FAVICON_GET, OAUTH_AVATAR] {
        assert!(preset.ssrf, "{}", preset.name);
        assert_eq!(preset.proxy, ProxyMode::Disabled, "{}", preset.name);
    }
    // The avatar path caps the body; the crawler paths do not (Python reads the
    // whole response there).
    assert_eq!(
        OAUTH_AVATAR.max_body_bytes,
        Some(preset::DEFAULT_UPLOAD_LIMIT)
    );
    assert!(LINK_TITLE.max_body_bytes.is_none());
    // Favicon resolution only follows redirects on the GET, not on the HEAD.
    assert!(!FAVICON_HEAD.follows_redirects());
    assert_eq!(FAVICON_GET.max_redirects, Some(5));
}

// `test_avatar_uses_ssrf_safe_client` wiring guard: the engine validates before
// it builds a client, so a raw unguarded request is not reachable through the
// pinned presets.
#[test]
fn test_avatar_url_is_validated_before_any_request() {
    let error = runtime()
        .block_on(ssrf::validate_hop(
            &private_answer(),
            "http://169.254.169.254/latest/meta-data/",
            &SsrfPolicy::strict(),
        ))
        .expect_err("metadata");
    assert_eq!(error.message(), ssrf::BLOCKED_NETWORK);
}

// A redirect to an internal address is a *validation* failure, so the favicon
// path's broad `except Exception` fallback runs and the default icon is used.
// The transport half is in `live_https.rs`.
#[test]
fn test_redirect_target_is_validated_with_the_same_policy() {
    let resolver = StubResolver::new(
        BTreeMap::new(),
        Some(vec![IpAddr::from([169, 254, 169, 254])]),
    );
    let error = runtime()
        .block_on(ssrf::validate_hop(
            &resolver,
            "http://192.168.8.14:8081/",
            &SsrfPolicy::strict(),
        ))
        .expect_err("private");
    assert_eq!(error.message(), ssrf::BLOCKED_NETWORK);
}

// `test_succeeds_at_exact_max_redirects`: five hops then a 200 must succeed for
// the link crawler (`MAX_REDIRECTS == 5`); six must fail. The URL arithmetic is
// the part that matters here; the HTTP part is in `live_https.rs`.
#[test]
fn test_redirect_limit_is_five_for_the_crawler() {
    assert_eq!(LINK_TITLE.max_redirects, Some(5));
    let error = plane_net::FetchError::TooManyRedirects {
        max_redirects: 5,
        url: "https://example.com/start".to_string(),
    };
    assert_eq!(
        error.message(),
        "Exceeded 5 redirects for URL: https://example.com/start"
    );
    assert!(error.is_request_exception());
    assert!(!error.is_value_error());
}

/// A `SocketAddr` is what the pin map stores; keep the import honest for the
/// documentation example above.
#[allow(dead_code)]
fn _pin_example() -> SocketAddr {
    SocketAddr::new(IpAddr::from([93, 184, 216, 34]), 443)
}

/// `OutboundRequest` is part of the public surface every ported call site uses.
#[allow(dead_code)]
fn _request_example() -> OutboundRequest {
    OutboundRequest::new()
        .header("Content-Type", "application/json")
        .body(b"{}".to_vec())
}

/// The engine is constructible without touching the network.
#[allow(dead_code)]
fn _engine_example() -> HttpEngine {
    HttpEngine::new(std::sync::Arc::new(public_only()))
}
