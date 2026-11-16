// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/ip_address.py` and the SSRF half of
//! `plane/utils/url_security.py`, with the same ordering, the same allow-list
//! semantics and the same error strings.
//!
//! # What is checked, in Python's order
//!
//! `validate_url(url, allowed_ips, allowed_hosts)` (create/update time):
//!
//! 1. `urlparse` (so bracket and NFKC errors surface first);
//! 2. `hostname` missing -> `Invalid URL: No hostname found`;
//! 3. scheme not `http`/`https` -> `Invalid URL scheme. Only HTTP and HTTPS are
//!    allowed`;
//! 4. host in `allowed_hosts` (exact, case-insensitive, trailing dot ignored)
//!    -> **accepted without resolving at all**;
//! 5. resolve every address and reject if any is blocked.
//!
//! `validate_hop(url, ...)` (delivery time, `_fetch_validated_hop`):
//!
//! 1. `_split_target` (scheme first, then hostname, then port -- a *different*
//!    order from `validate_url`, so `http://example.com:abc/` is accepted at
//!    create time and rejected at delivery time, exactly as in Python);
//! 2. host in `allowed_hosts` -> `require_safe = false`, i.e. private
//!    addresses are allowed but the name is **still resolved and pinned**, so
//!    an operator-trusted name cannot be rebound;
//! 3. resolve every address; reject the first one that is blocked and not
//!    inside `allowed_ips`.
//!
//! # Allow lists
//!
//! `WEBHOOK_ALLOWED_IPS` entries are `ip_network(cidr, strict=False)`: host
//! bits are masked (`10.0.0.5/8` becomes `10.0.0.0/8`), a bare address is a
//! `/32` or `/128`, and an address inside a network of the *other* family never
//! matches. [`SsrfPolicy::parse_cidr_list`] reproduces that parsing because
//! `plane-core` currently rejects bare addresses (`IpNet::from_str` requires a
//! prefix) while Python accepts them -- see the crate report.
//!
//! `WEBHOOK_ALLOWED_HOSTS` matches the hostname exactly (never a suffix) after
//! `rstrip(".").lower()`. Subdomains do **not** inherit the exemption.

use std::collections::HashSet;
use std::net::IpAddr;

use ipnet::IpNet;
use plane_core::config::WebhookSettings;

use crate::dns::Resolver;
use crate::ipaddr::is_blocked_ip;
use crate::pyurl::{self, PyUrlError, Target};

pub const BLOCKED_NETWORK: &str = "Access to private/internal networks is not allowed";
pub const NO_ADDRESSES: &str = "No IP addresses found for the hostname";
pub const UNRESOLVABLE: &str = "Hostname could not be resolved";

/// `WEBHOOK_ALLOWED_IPS` + `WEBHOOK_ALLOWED_HOSTS`, pre-normalised.
#[derive(Debug, Clone, Default)]
pub struct SsrfPolicy {
    allowed_ips: Vec<IpNet>,
    allowed_hosts: HashSet<String>,
}

impl SsrfPolicy {
    pub fn new(allowed_ips: Vec<IpNet>, allowed_hosts: impl IntoIterator<Item = String>) -> Self {
        Self {
            allowed_ips,
            allowed_hosts: allowed_hosts
                .into_iter()
                .map(|host| normalise_host(&host))
                .collect(),
        }
    }

    /// From the parsed environment (`plane-core::config::WebhookSettings`).
    pub fn from_settings(settings: &WebhookSettings) -> Self {
        Self::new(settings.allowed_ips.clone(), settings.allowed_hosts.clone())
    }

    /// No exemptions: every private or blocked address is refused.
    pub fn strict() -> Self {
        Self::default()
    }

    /// `ip_network(cidr, strict=False)` for a comma-separated list, skipping
    /// invalid entries the way `plane/settings/common.py` does (the caller logs
    /// the returned warnings at WARNING level).
    ///
    /// Unlike `plane-core`'s parser this accepts a bare address (`10.0.0.5` ->
    /// `10.0.0.5/32`), which `ipaddress.ip_network` does.
    pub fn parse_cidr_list(raw: &str) -> (Vec<IpNet>, Vec<String>) {
        let mut networks = Vec::new();
        let mut warnings = Vec::new();
        for entry in raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            match parse_python_network(entry) {
                Some(network) => networks.push(network),
                None => warnings.push(entry.to_string()),
            }
        }
        (networks, warnings)
    }

    /// `normalized_host in {(h or "").rstrip(".").lower() for h in allowed_hosts
    /// if h}`.
    pub fn is_trusted_host(&self, hostname: &str) -> bool {
        !self.allowed_hosts.is_empty() && self.allowed_hosts.contains(&normalise_host(hostname))
    }

    /// `_is_allowed_ip`: a matching network of the same family.
    pub fn allows_ip(&self, ip: IpAddr) -> bool {
        self.allowed_ips.iter().any(|network| network.contains(&ip))
    }

    pub fn allowed_ips(&self) -> &[IpNet] {
        &self.allowed_ips
    }

    pub fn allowed_hosts(&self) -> &HashSet<String> {
        &self.allowed_hosts
    }
}

/// `hostname.rstrip(".").lower()`.
pub fn normalise_host(hostname: &str) -> String {
    hostname.trim_end_matches('.').to_lowercase()
}

/// `ipaddress.ip_network(value, strict=False)`.
pub fn parse_python_network(value: &str) -> Option<IpNet> {
    if let Ok(address) = value.parse::<IpAddr>() {
        return Some(IpNet::from(address));
    }
    if let Ok(network) = value.parse::<IpNet>() {
        return Some(network.trunc());
    }
    // Python also accepts an address with an explicit prefix whose host bits
    // are set; `IpNet` handles that, but an IPv4-mapped IPv6 network such as
    // `::ffff:10.0.0.0/104` is normalised differently. `trunc` above already
    // reproduces `strict=False`.
    None
}

/// `resolve_and_validate(hostname, allowed_ips, require_safe)`.
///
/// Every resolved address is checked (fail closed on the first blocked one) and
/// the surviving addresses are returned in resolver order, de-duplicated, to be
/// pinned by the transport.
pub async fn resolve_and_validate(
    resolver: &dyn Resolver,
    hostname: &str,
    policy: &SsrfPolicy,
    require_safe: bool,
) -> Result<Vec<IpAddr>, PyUrlError> {
    let addresses = resolver
        .resolve(hostname)
        .await
        .map_err(|_| PyUrlError::new(UNRESOLVABLE))?;
    if addresses.is_empty() {
        return Err(PyUrlError::new(NO_ADDRESSES));
    }
    let mut validated: Vec<IpAddr> = Vec::with_capacity(addresses.len());
    for address in addresses {
        if require_safe && !policy.allows_ip(address) && is_blocked_ip(address) {
            return Err(PyUrlError::new(BLOCKED_NETWORK));
        }
        if !validated.contains(&address) {
            validated.push(address);
        }
    }
    Ok(validated)
}

/// `plane.utils.ip_address.validate_url` (webhook create/update). Note that a
/// trusted host is accepted *without* resolving.
pub async fn validate_url(
    resolver: &dyn Resolver,
    url: &str,
    policy: &SsrfPolicy,
) -> Result<(), PyUrlError> {
    let parsed = pyurl::urlparse(url)?;
    let hostname = parsed
        .hostname()
        .ok_or_else(|| PyUrlError::new(pyurl::NO_HOSTNAME))?;
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return Err(PyUrlError::new(pyurl::INVALID_SCHEME));
    }
    if policy.is_trusted_host(&hostname) {
        return Ok(());
    }
    resolve_and_validate(resolver, &hostname, policy, true).await?;
    Ok(())
}

/// One validated hop: the parsed target, the addresses it may connect to, and
/// whether it was exempted by `WEBHOOK_ALLOWED_HOSTS`.
#[derive(Debug, Clone)]
pub struct ValidatedHop {
    pub target: Target,
    pub addresses: Vec<IpAddr>,
    pub trusted: bool,
}

/// `bgtasks/work_item_link_task.py:29` `validate_url_ip`: the link crawler's
/// early pre-filter for a favicon URL.
///
/// It is deliberately weaker than [`validate_url`]: no allow-list, no trusted
/// hosts, no port parsing, and it does not pin. It exists so a bad favicon href
/// never becomes a request at all; the actual fetch still goes through
/// [`validate_hop`]. Ported so the favicon handler can keep the same two-layer
/// structure, not because it is load-bearing.
pub async fn validate_url_ip(resolver: &dyn Resolver, url: &str) -> Result<(), PyUrlError> {
    let parsed = pyurl::urlsplit(url)?;
    if parsed.scheme != "http" && parsed.scheme != "https" {
        return Err(PyUrlError::new(pyurl::INVALID_SCHEME));
    }
    let hostname = parsed
        .hostname()
        .ok_or_else(|| PyUrlError::new(pyurl::NO_HOSTNAME))?;
    let addresses = resolver
        .resolve(&hostname)
        .await
        .map_err(|_| PyUrlError::new(UNRESOLVABLE))?;
    if addresses.is_empty() {
        return Err(PyUrlError::new(NO_ADDRESSES));
    }
    for address in addresses {
        if is_blocked_ip(address) {
            return Err(PyUrlError::new(BLOCKED_NETWORK));
        }
    }
    Ok(())
}

/// `_fetch_validated_hop`'s validation half: split, decide trust, resolve and
/// validate every address. The transport then pins one of `addresses`.
pub async fn validate_hop(
    resolver: &dyn Resolver,
    url: &str,
    policy: &SsrfPolicy,
) -> Result<ValidatedHop, PyUrlError> {
    let target = pyurl::split_target(url)?;
    let trusted = policy.is_trusted_host(&target.hostname);
    let addresses = resolve_and_validate(resolver, &target.hostname, policy, !trusted).await?;
    Ok(ValidatedHop {
        target,
        addresses,
        trusted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::StubResolver;

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

    // Ported from apps/api/plane/tests/unit/bg_tasks/test_url_security.py
    // (`TestResolveAndValidate`), with the same names in the comments.
    #[test]
    fn test_returns_public_ips() {
        let resolver = stub(&["93.184.216.34"]);
        let result = runtime().block_on(resolve_and_validate(
            &resolver,
            "example.com",
            &SsrfPolicy::strict(),
            true,
        ));
        assert_eq!(
            result
                .unwrap()
                .iter()
                .map(|ip| ip.to_string())
                .collect::<Vec<_>>(),
            vec!["93.184.216.34"]
        );
    }

    #[test]
    fn test_raises_on_private() {
        let resolver = stub(&["10.0.0.1"]);
        let error = runtime()
            .block_on(resolve_and_validate(
                &resolver,
                "internal.example.com",
                &SsrfPolicy::strict(),
                true,
            ))
            .expect_err("blocked");
        assert_eq!(error.message(), BLOCKED_NETWORK);
    }

    #[test]
    fn test_raises_if_any_resolved_ip_is_private() {
        let resolver = stub(&["93.184.216.34", "127.0.0.1"]);
        let error = runtime()
            .block_on(resolve_and_validate(
                &resolver,
                "rebinder.example.com",
                &SsrfPolicy::strict(),
                true,
            ))
            .expect_err("fail closed");
        assert_eq!(error.message(), BLOCKED_NETWORK);
    }

    #[test]
    fn test_allowlist_permits_private() {
        let policy = SsrfPolicy::new(vec!["10.0.0.0/8".parse().unwrap()], []);
        let resolver = stub(&["10.0.0.5"]);
        let result = runtime().block_on(resolve_and_validate(&resolver, "internal", &policy, true));
        assert_eq!(result.unwrap()[0].to_string(), "10.0.0.5");
    }

    #[test]
    fn test_unresolvable_raises() {
        let error = runtime()
            .block_on(resolve_and_validate(
                &StubResolver::failing(),
                "nope.invalid",
                &SsrfPolicy::strict(),
                true,
            ))
            .expect_err("unresolvable");
        assert_eq!(error.message(), UNRESOLVABLE);
    }

    #[test]
    fn test_allowed_hosts_bypasses_private_ip_check() {
        // `test_allowed_hosts_skips_dns_lookup`: a failing resolver proves the
        // create-time check returns before resolving at all.
        let policy = SsrfPolicy::new(vec![], ["silo".to_string()]);
        let resolver = StubResolver::failing();
        let result = runtime().block_on(validate_url(&resolver, "http://silo:3000/hook", &policy));
        assert!(result.is_ok());
    }

    #[test]
    fn trusted_host_still_resolves_at_delivery_time() {
        // Deliberate difference between the two layers: `validate_url` skips the
        // lookup, `validate_hop` never does (pinning needs an address).
        let policy = SsrfPolicy::new(vec![], ["silo".to_string()]);
        let hop = runtime()
            .block_on(validate_hop(
                &StubResolver::failing(),
                "http://silo/hook",
                &policy,
            ))
            .expect_err("cannot pin without an address");
        assert_eq!(hop.message(), UNRESOLVABLE);
    }

    #[test]
    fn test_allowed_hosts_matches_case_insensitively() {
        let policy = SsrfPolicy::new(vec![], ["silo.namespace.svc.cluster.local".to_string()]);
        let resolver = stub(&["10.0.0.1"]);
        let result = runtime().block_on(validate_url(
            &resolver,
            "http://Silo.Namespace.Svc.Cluster.Local/x",
            &policy,
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn test_allowed_hosts_requires_exact_match() {
        let policy = SsrfPolicy::new(vec![], ["silo.internal".to_string()]);
        let resolver = stub(&["192.168.1.1"]);
        let error = runtime()
            .block_on(validate_url(
                &resolver,
                "http://attacker.silo.internal/x",
                &policy,
            ))
            .expect_err("subdomains do not inherit the exemption");
        assert_eq!(error.message(), BLOCKED_NETWORK);
    }

    #[test]
    fn test_rejects_hardened_bypass_ranges() {
        for ip in [
            "100.64.0.1",
            "224.0.0.1",
            "0.0.0.0",
            "::ffff:169.254.169.254",
            "64:ff9b::a9fe:a9fe",
        ] {
            let resolver = stub(&[ip]);
            let error = runtime()
                .block_on(validate_url(
                    &resolver,
                    "http://attacker.example.com",
                    &SsrfPolicy::strict(),
                ))
                .expect_err("blocked range");
            assert_eq!(error.message(), BLOCKED_NETWORK, "{ip}");
        }
    }

    #[test]
    fn trusted_hosts_are_still_resolved_and_pinned() {
        // `test_allowed_host_skips_block_check_but_still_pins`: require_safe is
        // false, but the addresses are returned so the connection can pin.
        let policy = SsrfPolicy::new(vec![], ["silo".to_string()]);
        let resolver = stub(&["172.18.0.5"]);
        let hop = runtime()
            .block_on(validate_hop(&resolver, "http://silo:3000/hook", &policy))
            .expect("trusted hop");
        assert!(hop.trusted);
        assert_eq!(hop.addresses.len(), 1);
        assert_eq!(hop.target.host_header(), "silo:3000");
    }

    #[test]
    fn validate_url_checks_hostname_before_scheme() {
        // `file:///etc/passwd` has no hostname, so Python reports the hostname,
        // not the scheme.
        let error = runtime()
            .block_on(validate_url(
                &StubResolver::failing(),
                "file:///etc/passwd",
                &SsrfPolicy::strict(),
            ))
            .expect_err("no host");
        assert_eq!(error.message(), pyurl::NO_HOSTNAME);
    }

    #[test]
    fn bare_address_is_a_host_route() {
        let (networks, warnings) =
            SsrfPolicy::parse_cidr_list("10.0.0.5/8, 192.168.1.7, ::1, nope");
        assert_eq!(warnings, vec!["nope".to_string()]);
        assert_eq!(networks.len(), 3);
        assert!(networks[0].contains(&"10.1.1.1".parse::<IpAddr>().unwrap()));
        assert!(networks[1].contains(&"192.168.1.7".parse::<IpAddr>().unwrap()));
        assert!(networks[2].contains(&"::1".parse::<IpAddr>().unwrap()));
    }
}
