// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Webhook URL validation, as three separate layers in Django:
//!
//! 1. `URLField` runs Django's `URLValidator` (a separate component, not ported
//!    here: it rejects e.g. `not a url` with "Enter a valid URL." before any of
//!    the checks below run);
//! 2. `plane/db/models/webhook.py:21` `validate_schema` and `:27`
//!    `validate_domain` run as field validators, in that order;
//! 3. `plane/app/serializers/webhook.py:27` `_validate_webhook_url` runs in
//!    `create`/`update`, calling `plane.utils.ip_address.validate_url` and then
//!    the disallowed-domain guard, which appends the *request's own host* so a
//!    webhook cannot point at the instance that serves it.
//!
//! Layer 2's `validate_domain` is easily misread: it compares the whole
//! **netloc** against `"localhost"` and `"127.0.0.1"`, so `localhost:8080`,
//! `127.0.0.1:8000` and `LOCALHOST` all pass it. That is the behaviour today
//! and it is ported as is; the SSRF check is what actually stops those URLs.
//! `validate_schema` is likewise redundant with `URLField` but is kept because
//! both produce different error messages.
//!
//! `_validate_webhook_url`'s message is wrapped by DRF as
//! `{"url": ["Invalid or disallowed webhook URL."]}`; the strings below are what
//! ends up inside that list.

use crate::dns::Resolver;
use crate::pyurl::{self, PyUrlError};
use crate::ssrf::{self, SsrfPolicy};

pub const INVALID_OR_DISALLOWED: &str = "Invalid or disallowed webhook URL.";
pub const DOMAIN_NOT_ALLOWED: &str = "URL domain or its subdomain is not allowed.";
pub const INVALID_SCHEMA: &str = "Invalid schema. Only HTTP and HTTPS are allowed.";
pub const LOCAL_URL: &str = "Local URLs are not allowed.";

/// A `serializers.ValidationError` raised by the webhook URL policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookUrlError {
    /// Any `ValueError` from `validate_url`: bad scheme, no host, unresolvable,
    /// or a blocked address. The original message is logged by Plane and
    /// replaced with [`INVALID_OR_DISALLOWED`] in the response.
    InvalidOrDisallowed,
    /// The host is the instance's own host, or a subdomain of a configured
    /// disallowed domain.
    DomainNotAllowed,
}

impl WebhookUrlError {
    /// The string DRF puts inside `{"url": [...]}`.
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidOrDisallowed => INVALID_OR_DISALLOWED,
            Self::DomainNotAllowed => DOMAIN_NOT_ALLOWED,
        }
    }
}

impl std::fmt::Display for WebhookUrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for WebhookUrlError {}

/// `WebhookSerializer._validate_webhook_url`.
///
/// `disallowed_domains` is `settings.WEBHOOK_DISALLOWED_DOMAINS` (already
/// lowercased and stripped of trailing dots by the settings loader; this
/// function compares verbatim, exactly as Python does). `request_host` is
/// `request.get_host()`; Python splits it at the first `:` and appends the
/// result to the disallowed list.
pub async fn validate_webhook_url(
    resolver: &dyn Resolver,
    url: &str,
    policy: &SsrfPolicy,
    disallowed_domains: &[String],
    request_host: Option<&str>,
) -> Result<(), WebhookUrlError> {
    ssrf::validate_url(resolver, url, policy)
        .await
        .map_err(|_| WebhookUrlError::InvalidOrDisallowed)?;

    // Cannot fail: `validate_url` parsed the same string already.
    let hostname = pyurl::urlparse(url)
        .ok()
        .and_then(|parsed| parsed.hostname())
        .unwrap_or_default();
    let hostname = ssrf::normalise_host(&hostname);

    // `WEBHOOK_ALLOWED_HOSTS` bypasses the disallowed-domain check: the host is
    // already trusted for SSRF, and the loop-back guard would otherwise get in
    // the way of legitimate sibling services on the same parent domain.
    if policy.is_trusted_host(&hostname) {
        return Ok(());
    }

    let request_host = request_host
        .map(|host| ssrf::normalise_host(host.split(':').next().unwrap_or("")))
        .unwrap_or_default();

    let blocked = disallowed_domains
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(request_host.as_str()))
        .any(|domain| {
            !domain.is_empty() && (hostname == domain || hostname.ends_with(&format!(".{domain}")))
        });
    if blocked {
        return Err(WebhookUrlError::DomainNotAllowed);
    }
    Ok(())
}

/// `plane.db.models.webhook.validate_schema`.
pub fn validate_schema(url: &str) -> Result<(), PyUrlError> {
    let parsed = pyurl::urlparse(url)?;
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return Err(PyUrlError::new(INVALID_SCHEMA));
    }
    Ok(())
}

/// `plane.db.models.webhook.validate_domain`. Exact netloc match only.
pub fn validate_domain(url: &str) -> Result<(), PyUrlError> {
    let parsed = pyurl::urlparse(url)?;
    if parsed.netloc == "localhost" || parsed.netloc == "127.0.0.1" {
        return Err(PyUrlError::new(LOCAL_URL));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::StubResolver;
    use std::net::IpAddr;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
    }

    fn stub(ips: &[&str]) -> StubResolver {
        StubResolver::new(
            Default::default(),
            Some(ips.iter().map(|ip| ip.parse().expect("ip")).collect()),
        )
    }

    #[test]
    fn request_host_and_subdomains_are_rejected() {
        // Ported from TestWebhookPatchContextGuard.test_request_host_is_blocked_when_context_present.
        let policy = SsrfPolicy::strict();
        let resolver = stub(&["93.184.216.34"]);
        let error = runtime()
            .block_on(validate_webhook_url(
                &resolver,
                "https://myplane.example.com/hook",
                &policy,
                &[],
                Some("myplane.example.com"),
            ))
            .expect_err("loop-back guard");
        assert_eq!(error, WebhookUrlError::DomainNotAllowed);
        let error = runtime()
            .block_on(validate_webhook_url(
                &resolver,
                "https://sub.myplane.example.com/hook",
                &policy,
                &[],
                Some("myplane.example.com"),
            ))
            .expect_err("subdomain");
        assert_eq!(error, WebhookUrlError::DomainNotAllowed);
    }

    #[test]
    fn unrelated_public_host_passes() {
        // Ported from TestWebhookPatchContextGuard.test_unrelated_public_host_passes_with_context.
        let policy = SsrfPolicy::strict();
        let resolver = stub(&["93.184.216.34"]);
        let result = runtime().block_on(validate_webhook_url(
            &resolver,
            "https://hooks.partner.com/x",
            &policy,
            &[],
            Some("myplane.example.com"),
        ));
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn allowed_hosts_bypass_the_disallowed_domain_check() {
        let policy = SsrfPolicy::new(vec![], ["myplane.example.com".to_string()]);
        let resolver = stub(&["93.184.216.34"]);
        let result = runtime().block_on(validate_webhook_url(
            &resolver,
            "https://myplane.example.com/hook",
            &policy,
            &["example.com".to_string()],
            Some("myplane.example.com"),
        ));
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn request_host_keeps_pythons_split_at_first_colon() {
        // "[::1]:8000".split(":")[0] == "[", which matches nothing; ported as is.
        let policy = SsrfPolicy::strict();
        let resolver = StubResolver::new(
            Default::default(),
            Some(vec!["8.8.8.8".parse::<IpAddr>().unwrap()]),
        );
        let result = runtime().block_on(validate_webhook_url(
            &resolver,
            "https://hooks.partner.com/x",
            &policy,
            &[],
            Some("[::1]:8000"),
        ));
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn private_targets_are_rejected_with_the_generic_message() {
        let policy = SsrfPolicy::strict();
        let resolver = stub(&["169.254.169.254"]);
        let error = runtime()
            .block_on(validate_webhook_url(
                &resolver,
                "https://attacker.example.com/hook",
                &policy,
                &[],
                Some("myplane.example.com"),
            ))
            .expect_err("metadata");
        assert_eq!(error, WebhookUrlError::InvalidOrDisallowed);
        assert_eq!(error.message(), INVALID_OR_DISALLOWED);
    }

    #[test]
    fn model_validators_match_django() {
        assert_eq!(
            validate_schema("ftp://example.com").unwrap_err().message(),
            INVALID_SCHEMA
        );
        assert!(validate_schema("https://example.com").is_ok());
        // Netloc comparison, not host comparison: a port defeats it.
        assert!(validate_domain("http://127.0.0.1").is_err());
        assert!(validate_domain("http://127.0.0.1:8000").is_ok());
        assert!(validate_domain("http://localhost").is_err());
        assert!(validate_domain("http://localhost:3000").is_ok());
        assert!(validate_domain("http://LOCALHOST").is_ok());
    }
}
