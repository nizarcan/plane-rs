// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WebhookSerializer`'s URL validation, in the two places Python runs it.
//!
//! | Layer | Python | Where it fails |
//! | --- | --- | --- |
//! | field validators | `serializers.URLField(validators=[validate_schema, validate_domain])` (`serializers/webhook.py:25`) | `serializer.is_valid()` → 400, `url` maps to a **list** |
//! | save-time check | `_validate_webhook_url` (`serializers/webhook.py:27-55`), called from `create` (`:69-72`) and `update` (`:74-78`) | `serializer.save()` → 400, `url` maps to a **bare string** |
//!
//! The field chain itself lives in [`crate::write::url::run_url_validators`]
//! (it is part of DRF's `URLField` machinery); this module owns the save-time
//! half and the tests that pin both halves against the real DRF/Django output.
//!
//! `_validate_webhook_url` has four steps, all of them reproduced by
//! `plane_net::webhook::validate_webhook_url`:
//!
//! 1. `validate_url(url, WEBHOOK_ALLOWED_IPS, WEBHOOK_ALLOWED_HOSTS)`
//!    (`plane/utils/ip_address.py:159-196`). Every `ValueError` is discarded
//!    and replaced by `"Invalid or disallowed webhook URL."` — the log line
//!    keeps the original;
//! 2. `hostname = (urlparse(url).hostname or "").rstrip(".").lower()`; a host in
//!    `WEBHOOK_ALLOWED_HOSTS` **returns early**, before the disallowed-domain
//!    check (`serializers/webhook.py:41-46`);
//! 3. `disallowed = list(WEBHOOK_DISALLOWED_DOMAINS) + [request.get_host()
//!    .split(":")[0].rstrip(".").lower()]` — the request's own host is appended
//!    so a webhook cannot point back at the instance;
//! 4. an exact match, or a subdomain of a listed domain, is
//!    `"URL domain or its subdomain is not allowed."`.
//!
//! Both messages are raised as `ValidationError({"url": message})` from inside
//! `save()`, where DRF's `as_serializer_error` never runs, so the body is
//! `{"url": "…"}` — a bare string, not a one-element list. [`error_response`]
//! builds it.

use plane_drf::view::ViewResponse;
use plane_net::dns::{Resolver, SystemResolver};
use plane_net::ssrf::SsrfPolicy;
use plane_net::webhook::{self, WebhookUrlError};

use plane_drf::view::ViewCtx;

/// `_validate_webhook_url` with an explicit resolver and policy, for tests.
///
/// `disallowed_domains` is `settings.WEBHOOK_DISALLOWED_DOMAINS`;
/// `request_host` is `request.get_host()` verbatim (Python splits at the first
/// `:` itself).
///
/// # Errors
///
/// The `ValidationError` the serializer's `create`/`update` raises.
pub async fn check_with(
    resolver: &dyn Resolver,
    url: &str,
    policy: &SsrfPolicy,
    disallowed_domains: &[String],
    request_host: Option<&str>,
) -> Result<(), WebhookUrlError> {
    webhook::validate_webhook_url(resolver, url, policy, disallowed_domains, request_host).await
}

/// The save-time check a create/PATCH handler runs after `serializer.finish()`.
///
/// `Ok(())` means the write may proceed. `Err(response)` is the 400 the view
/// returns; the original `ValueError` message is logged at WARNING
/// (`serializers/webhook.py:35-36`), never sent to the client.
///
/// A PATCH that omits `url` never calls this (Python's
/// `validated_data.get("url")` is falsy and the `if url:` guard skips), and one
/// that repeats the URL resolves it again.
pub async fn check(ctx: &ViewCtx<'_>, url: &str) -> Result<(), ViewResponse> {
    let settings = &ctx.settings.webhook;
    let policy = SsrfPolicy::from_settings(settings);
    match check_with(
        &SystemResolver,
        url,
        &policy,
        &settings.disallowed_domains,
        ctx.ctx.header("host"),
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(error) => {
            tracing::warn!(
                target: "plane",
                url,
                reason = %error,
                "Webhook URL validation failed"
            );
            Err(error_response(error))
        }
    }
}

/// `ValidationError({"url": message})` rendered by DRF's exception handler.
///
/// `ValidationError` with a dict detail keeps each value as a single
/// `ErrorDetail`, so the JSON value is the string itself — unlike
/// `serializer.errors`, where a failed field is a list.
#[must_use]
pub fn error_response(error: WebhookUrlError) -> ViewResponse {
    use plane_query::json::Json;
    let body = Json::object(vec![("url", Json::string(error.message()))]).to_drf();
    ViewResponse::raw(400, plane_core::error::APPLICATION_JSON, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::UrlOptions;
    use crate::write::url::{UrlValidator, run_url_validators};
    use plane_net::dns::StubResolver;
    use plane_net::ssrf::SsrfPolicy;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
    }

    /// The field errors DRF 3.18.3 produces for
    /// `WebhookSerializer(data={"url": value})`, recorded with
    /// `apps/api/.venv/bin/python` (minimal settings, no database):
    ///
    /// ```text
    /// ftp://localhost  ["Invalid schema. Only HTTP and HTTPS are allowed.",
    ///                   "Local URLs are not allowed."]
    /// not a url        ["Invalid schema. Only HTTP and HTTPS are allowed.",
    ///                   "Enter a valid URL."]
    /// HTTP://x         ["Enter a valid URL."]
    /// localhost:8000   ["Invalid schema. Only HTTP and HTTPS are allowed.",
    ///                   "Enter a valid URL."]
    /// LOCALHOST        ["Invalid schema. Only HTTP and HTTPS are allowed.",
    ///                   "Enter a valid URL."]
    /// user@localhost   ["Invalid schema. Only HTTP and HTTPS are allowed.",
    ///                   "Enter a valid URL."]
    /// ```
    ///
    /// `HTTP://x` passes `validate_schema` (urlparse lowercases the scheme,
    /// `pyurl.rs:157-166`) and `validate_domain` (the netloc is `x`), and fails
    /// only Django's `URLValidator` because `x` is not a domain.
    #[test]
    fn field_validator_chain_matches_drf() {
        let chain = UrlOptions::WEBHOOK_URL.validators;
        let cases: &[(&str, &[&str])] = &[
            (
                "ftp://localhost",
                &[
                    "Invalid schema. Only HTTP and HTTPS are allowed.",
                    "Local URLs are not allowed.",
                ],
            ),
            (
                "not a url",
                &[
                    "Invalid schema. Only HTTP and HTTPS are allowed.",
                    "Enter a valid URL.",
                ],
            ),
            ("HTTP://x", &["Enter a valid URL."]),
            (
                "localhost:8000",
                &[
                    "Invalid schema. Only HTTP and HTTPS are allowed.",
                    "Enter a valid URL.",
                ],
            ),
            (
                "LOCALHOST",
                &[
                    "Invalid schema. Only HTTP and HTTPS are allowed.",
                    "Enter a valid URL.",
                ],
            ),
            (
                "user@localhost",
                &[
                    "Invalid schema. Only HTTP and HTTPS are allowed.",
                    "Enter a valid URL.",
                ],
            ),
            ("http://localhost", &["Local URLs are not allowed."]),
            ("http://127.0.0.1", &["Local URLs are not allowed."]),
            (
                "ftp://example.com",
                &["Invalid schema. Only HTTP and HTTPS are allowed."],
            ),
            ("http://example", &["Enter a valid URL."]),
            // `validate_domain` compares the **netloc**, so a port, mixed case
            // or userinfo defeats it and only the SSRF check stops these.
            ("http://127.0.0.1:8000", &[]),
            ("http://LOCALHOST", &[]),
            ("http://user@localhost", &[]),
            (
                "http://example.com/\u{0}",
                &["Null characters are not allowed."],
            ),
            (
                "ftp://localhost\u{0}",
                &[
                    "Invalid schema. Only HTTP and HTTPS are allowed.",
                    "Null characters are not allowed.",
                    "Enter a valid URL.",
                ],
            ),
            (
                "http://local\u{0}host",
                &["Null characters are not allowed.", "Enter a valid URL."],
            ),
        ];
        for (value, expected) in cases {
            let errors = run_url_validators(value, chain, None)
                .unwrap_or_else(|message| panic!("{value:?} raised {message}"));
            let messages: Vec<&str> = errors
                .iter()
                .map(|detail| detail.message.as_str())
                .collect();
            assert_eq!(messages, *expected, "input {value:?}");
        }
    }

    /// `urlparse` raises `ValueError` inside `validate_schema` for these; DRF
    /// does not catch it, so `is_valid()` propagates and the request is a 500.
    #[test]
    fn parse_failures_are_not_field_errors() {
        for value in ["http://[::1", "http://exa\u{2100}mple.com"] {
            let error = run_url_validators(value, UrlOptions::WEBHOOK_URL.validators, None)
                .expect_err("urlparse raises");
            assert!(!error.is_empty(), "input {value:?}");
        }
    }

    #[test]
    fn validator_chain_reuses_the_model_validators() {
        let options = UrlOptions::WEBHOOK_URL;
        assert_eq!(options.validators.len(), 2);
        assert_eq!(options.validators[0], UrlValidator::Schema);
        assert_eq!(options.validators[1], UrlValidator::Domain);
        assert!(options.required);
        assert!(!options.allow_null);
        assert!(!options.allow_blank);
        assert!(options.trim_whitespace);
        assert_eq!(options.max_length, None);
    }

    fn body(response: ViewResponse) -> String {
        let rendered = plane_drf::view::DrfResponse::render(response).expect("render");
        String::from_utf8(rendered.body).expect("utf8")
    }

    fn stub(ips: &[&str]) -> StubResolver {
        StubResolver::new(
            Default::default(),
            Some(ips.iter().map(|ip| ip.parse().expect("ip")).collect()),
        )
    }

    /// `_validate_webhook_url`'s first failure mode: any `ValueError` from
    /// `validate_url` becomes the generic message, and the response shape is a
    /// bare string.
    #[test]
    fn ssrf_failures_use_the_generic_message_and_a_bare_string() {
        let policy = SsrfPolicy::strict();
        let resolver = stub(&["127.0.0.1"]);
        let error = runtime()
            .block_on(check_with(
                &resolver,
                "https://internal.example.com/hook",
                &policy,
                &[],
                Some("plane.example.com"),
            ))
            .expect_err("loopback");
        assert_eq!(error, WebhookUrlError::InvalidOrDisallowed);
        assert_eq!(
            body(error_response(error)),
            r#"{"url":"Invalid or disallowed webhook URL."}"#
        );
    }

    #[test]
    fn disallowed_domain_uses_the_second_message() {
        let policy = SsrfPolicy::strict();
        let resolver = stub(&["93.184.216.34"]);
        let error = runtime()
            .block_on(check_with(
                &resolver,
                "https://hooks.plane.so/x",
                &policy,
                &["plane.so".to_string()],
                Some("plane.example.com"),
            ))
            .expect_err("disallowed");
        assert_eq!(error, WebhookUrlError::DomainNotAllowed);
        assert_eq!(
            body(error_response(error)),
            r#"{"url":"URL domain or its subdomain is not allowed."}"#
        );
    }

    /// The allowed-host early return: no DNS lookup happens at all, which is
    /// why this test needs no resolver table.
    #[test]
    fn allowed_hosts_skip_resolution_and_the_domain_list() {
        let policy = SsrfPolicy::new(vec![], ["hooks.example.test".to_string()]);
        let resolver = StubResolver::new(Default::default(), None);
        let result = runtime().block_on(check_with(
            &resolver,
            "https://hooks.example.test/x",
            &policy,
            &["example.test".to_string()],
            Some("plane.example.com"),
        ));
        assert_eq!(result, Ok(()));
    }

    /// `is_blocked_ip` recurses into IPv4 addresses embedded in IPv6
    /// transition formats (`plane/utils/ip_address.py:33-64`), so a resolver
    /// answer of `::ffff:127.0.0.1`, a NAT64 address, 6to4 or Teredo is
    /// refused. Every one of those outer prefixes is also on the explicit
    /// deny-list, so the recursion is defence in depth rather than the only
    /// check.
    #[test]
    fn embedded_ipv4_targets_are_blocked() {
        let policy = SsrfPolicy::strict();
        for ip in [
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::7f00:1",
            "64:ff9b::a9fe:a9fe",
            "2002:7f00:1::",
            "2001:0:4136:e378:8000:63bf:3fff:fdd2",
        ] {
            let resolver = stub(&[ip]);
            let error = runtime()
                .block_on(check_with(
                    &resolver,
                    "https://attacker.example/hook",
                    &policy,
                    &[],
                    None,
                ))
                .expect_err(ip);
            assert_eq!(error, WebhookUrlError::InvalidOrDisallowed, "{ip}");
        }
        // A public IPv6 answer still passes, so the loop above is not vacuous.
        let resolver = stub(&["2606:4700:4700::1111"]);
        assert_eq!(
            runtime().block_on(check_with(
                &resolver,
                "https://attacker.example/hook",
                &policy,
                &[],
                None
            )),
            Ok(())
        );
    }

    /// `WEBHOOK_ALLOWED_IPS` exempts an address from `is_blocked_ip` but the
    /// rest of `validate_url` still runs.
    #[test]
    fn allowed_ips_exempt_private_targets() {
        let (networks, warnings) = SsrfPolicy::parse_cidr_list("127.0.0.1");
        assert!(warnings.is_empty());
        let policy = SsrfPolicy::new(networks, []);
        let resolver = stub(&["127.0.0.1"]);
        let result = runtime().block_on(check_with(
            &resolver,
            "https://hooks.internal.example/x",
            &policy,
            &[],
            Some("plane.example.com"),
        ));
        assert_eq!(result, Ok(()));
    }

    /// The request's own host is appended to the disallowed list, and a
    /// subdomain matches as well (`serializers/webhook.py:48-55`).
    #[test]
    fn request_host_and_subdomains_are_blocked() {
        let policy = SsrfPolicy::strict();
        let resolver = stub(&["93.184.216.34"]);
        for host in ["hooks.plane.example", "plane.example"] {
            let url = format!("https://{host}/x");
            let error = runtime()
                .block_on(check_with(
                    &resolver,
                    &url,
                    &policy,
                    &[],
                    Some("plane.example:8000"),
                ))
                .expect_err("own host");
            assert_eq!(error, WebhookUrlError::DomainNotAllowed);
        }
    }

    /// A scheme the SSRF layer rejects (`ftp`) is the generic message, not the
    /// field's schema message: `validate_url` checks the scheme itself.
    #[test]
    fn ssrf_scheme_failure_is_the_generic_message() {
        let policy = SsrfPolicy::strict();
        let resolver = stub(&["93.184.216.34"]);
        let error = runtime()
            .block_on(check_with(
                &resolver,
                "ftp://hooks.example.com/x",
                &policy,
                &[],
                None,
            ))
            .expect_err("scheme");
        assert_eq!(error, WebhookUrlError::InvalidOrDisallowed);
    }
}
