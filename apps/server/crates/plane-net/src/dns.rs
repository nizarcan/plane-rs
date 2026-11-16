// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Name resolution, behind a trait so the security tests can be deterministic.
//!
//! Python calls `socket.getaddrinfo(hostname, None)` inside
//! `plane/utils/ip_address.py:resolve_and_validate`. Three behaviours of that
//! call matter and are reproduced here:
//!
//! 1. a numeric literal resolves to itself, without a resolver lookup;
//! 2. a name with a `%zone` suffix keeps its zone until the caller strips it;
//! 3. **only non-ASCII names are IDNA-encoded**, with the `idna` codec
//!    (IDNA2003, i.e. `nameprep` + punycode; `ß` becomes `ss`). CPython's
//!    `socket` module calls `PyUnicode_AsEncodedString(host, "idna", ...)` only
//!    when the name is not ASCII, and a failure there surfaces as
//!    `UnicodeError`, which `resolve_and_validate` maps to
//!    `ValueError("Hostname could not be resolved")`.
//!
//! Behaviour 3 is security-relevant: `WEBHOOK_ALLOWED_HOSTS` matching happens on
//! the *raw* hostname, but the address that is validated is the one the
//! resolver produces. A different IDNA implementation would let
//! `straße.example` resolve to a different name than Python's does. The
//! transitional mapping below is the one UTS #46 defines as equivalent to
//! IDNA2003, applied before `idna`'s (non-transitional) UTS #46 processing.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use crate::ipaddr::parse_python_ip;

/// A resolution failure: `socket.gaierror` or IDNA `UnicodeError`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsError {
    /// `getaddrinfo` failed, or the name could not be IDNA-encoded.
    Unresolvable,
}

impl std::fmt::Display for DnsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("hostname could not be resolved")
    }
}

impl std::error::Error for DnsError {}

pub type Resolving<'a> = Pin<Box<dyn Future<Output = Result<Vec<IpAddr>, DnsError>> + Send + 'a>>;

/// The resolver seam. Implementations must return addresses in resolution
/// order and without duplicates (the validator de-duplicates as well, but the
/// order decides which address is tried first).
pub trait Resolver: Send + Sync {
    fn resolve<'a>(&'a self, hostname: &'a str) -> Resolving<'a>;
}

/// A shared resolver, as stored by [`crate::HttpEngine`].
pub type SharedResolver = Arc<dyn Resolver>;

/// `getaddrinfo` through the host resolver, the same libc call CPython makes.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolver;

impl Resolver for SystemResolver {
    fn resolve<'a>(&'a self, hostname: &'a str) -> Resolving<'a> {
        Box::pin(async move {
            if let Some(literal) = parse_python_ip(hostname) {
                return Ok(vec![literal]);
            }
            let name = resolver_name(hostname)?;
            let mut addresses = tokio::net::lookup_host((name.as_ref(), 0))
                .await
                .map_err(|_| DnsError::Unresolvable)?;
            let mut out: Vec<IpAddr> = Vec::new();
            for address in addresses.by_ref() {
                let ip = address.ip();
                if !out.contains(&ip) {
                    out.push(ip);
                }
            }
            Ok(out)
        })
    }
}

/// The name handed to the resolver, following CPython's `socket` module.
pub fn resolver_name(hostname: &str) -> Result<Cow<'_, str>, DnsError> {
    if hostname.is_ascii() {
        return Ok(Cow::Borrowed(hostname));
    }
    let mapped = transitional_map(hostname);
    idna::domain_to_ascii(mapped.as_ref())
        .map(Cow::Owned)
        .map_err(|_| DnsError::Unresolvable)
}

/// UTS #46 transitional processing, which is what IDNA2003's `nameprep` does
/// for the four characters that matter (`idna` 1.x only exposes the
/// non-transitional algorithm without the deprecated `Config` API).
fn transitional_map(domain: &str) -> Cow<'_, str> {
    if !domain
        .chars()
        .any(|c| matches!(c, 'ß' | 'ẞ' | 'ς' | '\u{200C}' | '\u{200D}'))
    {
        return Cow::Borrowed(domain);
    }
    let mut out = String::with_capacity(domain.len());
    for c in domain.chars() {
        match c {
            'ß' | 'ẞ' => out.push_str("ss"),
            'ς' => out.push('σ'),
            '\u{200C}' | '\u{200D}' => {}
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// A deterministic resolver: numeric literals answer with themselves, names
/// come from the table, missing names fall back to `default` (or fail).
///
/// This is the Rust half of the stub in `tools/net/ssrf_corpus.py`; the two
/// must agree or the golden corpus fails.
#[derive(Debug, Clone, Default)]
pub struct StubResolver {
    hosts: BTreeMap<String, Vec<IpAddr>>,
    default: Option<Vec<IpAddr>>,
    error: bool,
}

impl StubResolver {
    pub fn new(hosts: BTreeMap<String, Vec<IpAddr>>, default: Option<Vec<IpAddr>>) -> Self {
        Self {
            hosts,
            default,
            error: false,
        }
    }

    /// A resolver that fails every lookup, like a `gaierror`.
    pub fn failing() -> Self {
        Self {
            hosts: BTreeMap::new(),
            default: None,
            error: true,
        }
    }

    pub fn with_host(mut self, hostname: &str, addresses: Vec<IpAddr>) -> Self {
        self.hosts.insert(hostname.to_string(), addresses);
        self
    }
}

impl Resolver for StubResolver {
    fn resolve<'a>(&'a self, hostname: &'a str) -> Resolving<'a> {
        Box::pin(async move {
            if self.error {
                return Err(DnsError::Unresolvable);
            }
            if let Some(literal) = parse_python_ip(hostname) {
                return Ok(vec![literal]);
            }
            let key = resolver_name(hostname)?;
            if let Some(addresses) = self.hosts.get(key.as_ref()) {
                return Ok(addresses.clone());
            }
            self.default.clone().ok_or(DnsError::Unresolvable)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_names_are_not_idna_encoded() {
        // CPython passes ASCII through untouched, including underscores and
        // upper case (DNS matching is case-insensitive, the URL parser has
        // already lowercased the host).
        assert_eq!(resolver_name("example.com").unwrap(), "example.com");
        assert_eq!(
            resolver_name("_sip._tcp.example.com").unwrap(),
            "_sip._tcp.example.com"
        );
        assert_eq!(resolver_name("EXAMPLE.com").unwrap(), "EXAMPLE.com");
    }

    #[test]
    fn unicode_names_use_idna2003_mapping() {
        assert_eq!(
            resolver_name("münchen.example").unwrap(),
            "xn--mnchen-3ya.example"
        );
        assert_eq!(
            resolver_name("例子.example").unwrap(),
            "xn--fsqu00a.example"
        );
        // IDNA2003 maps ß to ss; UTS 46 non-transitional would produce
        // xn--fa-hia.de instead.
        assert_eq!(resolver_name("faß.de").unwrap(), "fass.de");
        assert_eq!(resolver_name("straße.example").unwrap(), "strasse.example");
    }

    #[test]
    fn literals_resolve_without_the_table() {
        // A resolver with no table and no default still answers literals; this
        // mirrors getaddrinfo, which never consults DNS for a numeric address.
        let resolver = StubResolver::new(BTreeMap::new(), None);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        let addresses = runtime
            .block_on(resolver.resolve("127.0.0.1"))
            .expect("literal resolves even when the resolver fails");
        assert_eq!(addresses.len(), 1);
        let addresses = runtime
            .block_on(resolver.resolve("fe80::1%eth0"))
            .expect("zoned literal resolves");
        assert_eq!(addresses[0].to_string(), "fe80::1");
        assert!(runtime.block_on(resolver.resolve("example.com")).is_err());
    }
}
