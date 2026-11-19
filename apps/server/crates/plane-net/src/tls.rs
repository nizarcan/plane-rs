// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! TLS trust, matching what `requests` does for the two kinds of call site.
//!
//! `requests` verifies against **certifi** (a Mozilla root bundle shipped with
//! the package) and honours `REQUESTS_CA_BUNDLE` / `CURL_CA_BUNDLE` from the
//! environment -- but only when `Session.trust_env` is true. The SSRF-pinned
//! client sets `session.trust_env = False` (`url_security.py:122`), so those
//! variables are *ignored* for webhooks, link unfurling and OAuth avatars, while
//! OAuth token/userinfo calls, Unsplash and the GitHub release check do honour
//! them.
//!
//! [`TlsTrust::Certifi`] and [`TlsTrust::Environment`] model those two modes.
//! The root bundle is `webpki-roots` (the Mozilla store as compiled into the
//! binary); certifi and webpki-roots track the same store but can lag each
//! other by a release, and a self-hosting operator who installs a private root
//! must use [`TlsTrust::Custom`] or `REQUESTS_CA_BUNDLE` for the unpinned
//! call sites -- a pinned call site cannot be pointed at a private CA without a
//! code change, exactly as in Python.
//!
//! One rustls `CryptoProvider` for the whole binary (`aws-lc-rs`, D9). This
//! module builds it explicitly instead of relying on a process default, so it
//! composes with `plane-db`'s `tokio-postgres-rustls` configuration.

use std::io::BufReader;
use std::sync::{Arc, OnceLock};

use rustls::ClientConfig;
use rustls::RootCertStore;
use rustls::pki_types::{CertificateDer, TrustAnchor};

/// Which roots a call site trusts.
#[derive(Debug, Clone)]
pub enum TlsTrust {
    /// `requests`' certifi bundle, environment ignored (`trust_env=False`).
    Certifi,
    /// `REQUESTS_CA_BUNDLE`, then `CURL_CA_BUNDLE`, else the certifi bundle.
    Environment,
    /// Exactly these PEM/DER roots, for the live tests and for an eventual
    /// operator-configured private CA.
    Custom(Vec<CertificateDer<'static>>),
}

/// The two preconfigured `ClientConfig`s an [`crate::HttpEngine`] reuses.
#[derive(Debug)]
pub struct TlsSettings {
    certifi: OnceLock<Arc<ClientConfig>>,
    environment: OnceLock<Arc<ClientConfig>>,
    /// The same roots with ALPN `h2`, for the OTLP/gRPC client.
    grpc_certifi: OnceLock<Arc<ClientConfig>>,
    grpc_environment: OnceLock<Arc<ClientConfig>>,
}

impl Default for TlsSettings {
    fn default() -> Self {
        Self {
            certifi: OnceLock::new(),
            environment: OnceLock::new(),
            grpc_certifi: OnceLock::new(),
            grpc_environment: OnceLock::new(),
        }
    }
}

impl TlsSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// A `ClientConfig` for `trust`, cached per mode.
    pub fn config(&self, trust: &TlsTrust) -> Result<Arc<ClientConfig>, String> {
        match trust {
            TlsTrust::Certifi => Ok(self
                .certifi
                .get_or_init(|| {
                    // A build failure here is a packaging bug, not a runtime
                    // condition: cache the successful config only.
                    Arc::new(build(&webpki_store(), HTTP1_ALPN).expect("webpki root store builds"))
                })
                .clone()),
            TlsTrust::Environment => {
                if let Some(config) = self.environment.get() {
                    return Ok(config.clone());
                }
                let config = Arc::new(build(&environment_store()?, HTTP1_ALPN)?);
                let _ = self.environment.set(config.clone());
                Ok(config)
            }
            TlsTrust::Custom(roots) => {
                let mut store = RootCertStore::empty();
                for cert in roots {
                    store
                        .add(cert.clone())
                        .map_err(|error| format!("invalid root certificate: {error}"))?;
                }
                Ok(Arc::new(build(&store, HTTP1_ALPN)?))
            }
        }
    }

    /// A `ClientConfig` for `trust` with ALPN `h2`, for the OTLP/gRPC client.
    /// The roots are the same as [`Self::config`]: the pinned call sites use
    /// certifi (D9), and the gRPC exporter is SSRF-pinned.
    pub fn grpc_config(&self, trust: &TlsTrust) -> Result<Arc<ClientConfig>, String> {
        match trust {
            TlsTrust::Certifi => Ok(self
                .grpc_certifi
                .get_or_init(|| {
                    Arc::new(build(&webpki_store(), GRPC_ALPN).expect("webpki root store builds"))
                })
                .clone()),
            TlsTrust::Environment => {
                if let Some(config) = self.grpc_environment.get() {
                    return Ok(config.clone());
                }
                let config = Arc::new(build(&environment_store()?, GRPC_ALPN)?);
                let _ = self.grpc_environment.set(config.clone());
                Ok(config)
            }
            TlsTrust::Custom(roots) => {
                let mut store = RootCertStore::empty();
                for cert in roots {
                    store
                        .add(cert.clone())
                        .map_err(|error| format!("invalid root certificate: {error}"))?;
                }
                Ok(Arc::new(build(&store, GRPC_ALPN)?))
            }
        }
    }
}

const HTTP1_ALPN: &[&[u8]] = &[b"http/1.1"];
const GRPC_ALPN: &[&[u8]] = &[b"h2"];

fn build(roots: &RootCertStore, alpn: &[&[u8]]) -> Result<ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS12, &rustls::version::TLS13])
        .map_err(|error| format!("unsupported TLS versions: {error}"))?;
    let mut config = builder
        .with_root_certificates(roots.clone())
        .with_no_client_auth();
    // urllib3 2.x sets ALPN to http/1.1 only; gRPC needs h2.
    config.alpn_protocols = alpn.iter().map(|value| value.to_vec()).collect();
    Ok(config)
}

fn webpki_store() -> RootCertStore {
    let mut store = RootCertStore::empty();
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    store
}

fn environment_store() -> Result<RootCertStore, String> {
    let bundle = std::env::var_os("REQUESTS_CA_BUNDLE")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("CURL_CA_BUNDLE").filter(|value| !value.is_empty()));
    let Some(path) = bundle else {
        return Ok(webpki_store());
    };
    let data = std::fs::read(&path).map_err(|error| {
        format!(
            "Could not find a suitable TLS CA certificate bundle, invalid path: {} ({error})",
            path.to_string_lossy()
        )
    })?;
    let mut store = RootCertStore::empty();
    let mut reader = BufReader::new(data.as_slice());
    let mut added = 0usize;
    for cert in rustls_pemfile::certs(&mut reader) {
        let cert = cert.map_err(|error| format!("invalid CA bundle: {error}"))?;
        store
            .add(cert)
            .map_err(|error| format!("invalid CA bundle: {error}"))?;
        added += 1;
    }
    if added == 0 {
        return Err("Could not find a suitable TLS CA certificate bundle: no certificates".into());
    }
    Ok(store)
}

/// Parse a PEM certificate bundle (used by the live tests and by deployment
/// wiring that reads a private CA from a file).
pub fn certificates_from_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, String> {
    let mut reader = BufReader::new(pem);
    let mut out = Vec::new();
    for cert in rustls_pemfile::certs(&mut reader) {
        out.push(cert.map_err(|error| format!("invalid PEM certificate: {error}"))?);
    }
    Ok(out)
}

/// Convenience for asserting the trust anchors we expect, used by tests.
pub fn trust_anchor_count() -> usize {
    webpki_roots::TLS_SERVER_ROOTS.len()
}

#[allow(dead_code)]
fn _assert_trust_anchor_type(_: &TrustAnchor<'static>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_config_negotiates_h2() {
        let settings = TlsSettings::new();
        let config = settings.grpc_config(&TlsTrust::Certifi).expect("config");
        assert_eq!(config.alpn_protocols, vec![b"h2".to_vec()]);
        let http = settings.config(&TlsTrust::Certifi).expect("config");
        assert_eq!(http.alpn_protocols, vec![b"http/1.1".to_vec()]);
        assert!(trust_anchor_count() > 0);
    }
}
