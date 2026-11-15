// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! TLS for Postgres, using the single rustls provider the binary installs
//! (`aws-lc-rs`, DECISIONS.md D9).
//!
//! `sslmode` follows libpq:
//!
//! | mode          | encrypted | chain verified | hostname verified |
//! | ------------- | --------- | -------------- | ----------------- |
//! | `disable`     | no        | –              | –                 |
//! | `allow`       | if required by the server | – | –          |
//! | `prefer` (default) | if available | –       | –                 |
//! | `require`     | yes       | no             | no                |
//! | `verify-ca`   | yes       | yes            | no                |
//! | `verify-full` | yes       | yes            | yes               |
//!
//! `tokio-postgres-rustls` always verifies certificates, so the "no
//! verification" modes use a custom [`ServerCertVerifier`] that still checks the
//! TLS handshake signature against the presented certificate (libpq does too;
//! what `require` skips is the chain and name check).
//!
//! An unknown `sslmode` is a configuration error: psycopg raises
//! `ProgrammingError` at connect time and Django's settings import does not
//! validate it, so the port reports it when it builds the pool (before the first
//! connection) instead of silently downgrading.

use std::sync::{Arc, Once};

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio_postgres_rustls::MakeRustlsConnect;

/// Install `aws-lc-rs` as the process-wide rustls provider. Idempotent: a second
/// call (another crate installing it first) is ignored.
pub fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// The libpq `sslmode` values the port understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    Disable,
    Allow,
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    /// psycopg's default when `sslmode` is absent.
    pub const DEFAULT: Self = Self::Prefer;

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "disable" => Some(Self::Disable),
            "allow" => Some(Self::Allow),
            "prefer" => Some(Self::Prefer),
            "require" => Some(Self::Require),
            "verify-ca" => Some(Self::VerifyCa),
            "verify-full" => Some(Self::VerifyFull),
            _ => None,
        }
    }

    /// The closest `tokio_postgres::SslMode`.
    pub fn as_tokio(self) -> tokio_postgres::config::SslMode {
        use tokio_postgres::config::SslMode as Tokio;
        match self {
            Self::Disable => Tokio::Disable,
            // libpq's `allow` tries plaintext first and TLS only when the
            // server asks; tokio-postgres has no equivalent, and `Prefer` (TLS
            // first) is the safe approximation.
            Self::Allow | Self::Prefer => Tokio::Prefer,
            Self::Require | Self::VerifyCa | Self::VerifyFull => Tokio::Require,
        }
    }
}

/// Why the TLS configuration could not be built.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("invalid sslmode")]
    InvalidSslMode,
    #[error("cannot read sslrootcert {path:?}: {source}")]
    RootCertRead {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("sslrootcert {path:?} contains no certificates")]
    NoRootCerts { path: String },
    #[error("rustls rejected the verifier configuration: {0}")]
    Verifier(#[from] rustls::client::VerifierBuilderError),
    #[error("rustls rejected the protocol versions: {0}")]
    ProtocolVersions(#[from] rustls::Error),
}

/// Build the `tokio-postgres-rustls` connector for a mode.
///
/// `sslrootcert` is a filesystem path (a PEM bundle), as libpq accepts. Without
/// it the `verify-*` modes trust the Mozilla root program via `webpki-roots`.
pub fn connector(mode: SslMode, sslrootcert: Option<&str>) -> Result<MakeRustlsConnect, TlsError> {
    install_crypto_provider();
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));

    let mut roots = RootCertStore::empty();
    let mut loaded_custom_roots = false;
    if let Some(path) = sslrootcert {
        let pem = std::fs::read(path).map_err(|source| TlsError::RootCertRead {
            path: path.to_string(),
            source,
        })?;
        let mut reader = std::io::BufReader::new(std::io::Cursor::new(pem));
        for certificate in rustls_pemfile::certs(&mut reader) {
            let certificate = certificate.map_err(|source| TlsError::RootCertRead {
                path: path.to_string(),
                source,
            })?;
            roots.add(certificate).map_err(|_| TlsError::NoRootCerts {
                path: path.to_string(),
            })?;
            loaded_custom_roots = true;
        }
        if !loaded_custom_roots {
            return Err(TlsError::NoRootCerts {
                path: path.to_string(),
            });
        }
    }

    let verifier: Arc<dyn ServerCertVerifier> = match mode {
        SslMode::VerifyCa => {
            if !loaded_custom_roots {
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            }
            Arc::new(VerifyCaVerifier {
                roots: Arc::new(roots),
                provider: Arc::clone(&provider),
            })
        }
        SslMode::VerifyFull => {
            if !loaded_custom_roots {
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            }
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::clone(&provider))
                .build()?
        }
        // disable/prefer/allow/require: encryption without verification.
        SslMode::Disable | SslMode::Allow | SslMode::Prefer | SslMode::Require => {
            Arc::new(NoVerification {
                provider: Arc::clone(&provider),
            })
        }
    };

    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(MakeRustlsConnect::new(config))
}

/// Accepts any certificate chain, but still verifies the TLS handshake
/// signature with the presented certificate's key (`sslmode=require`).
#[derive(Debug)]
struct NoVerification {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for NoVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Verifies the chain but not the server name (`sslmode=verify-ca`).
#[derive(Debug)]
struct VerifyCaVerifier {
    roots: Arc<RootCertStore>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for VerifyCaVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let parsed = rustls::server::ParsedCertificate::try_from(end_entity)?;
        rustls::client::verify_server_cert_signed_by_trust_anchor(
            &parsed,
            &self.roots,
            intermediates,
            now,
            self.provider.signature_verification_algorithms.all,
        )?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sslmode_values() {
        for (value, mode) in [
            ("disable", SslMode::Disable),
            ("allow", SslMode::Allow),
            ("prefer", SslMode::Prefer),
            ("require", SslMode::Require),
            ("verify-ca", SslMode::VerifyCa),
            ("verify-full", SslMode::VerifyFull),
        ] {
            assert_eq!(SslMode::parse(value), Some(mode), "{value}");
        }
        for value in ["", "REQUIRE", "true", "no-verify"] {
            assert_eq!(SslMode::parse(value), None, "{value}");
        }
        assert_eq!(SslMode::DEFAULT, SslMode::Prefer);
    }

    #[test]
    fn tokio_modes() {
        use tokio_postgres::config::SslMode as Tokio;
        assert_eq!(SslMode::Disable.as_tokio(), Tokio::Disable);
        assert_eq!(SslMode::Prefer.as_tokio(), Tokio::Prefer);
        assert_eq!(SslMode::Require.as_tokio(), Tokio::Require);
        assert_eq!(SslMode::VerifyFull.as_tokio(), Tokio::Require);
    }

    #[test]
    fn connectors_build_for_every_mode() {
        for mode in [
            SslMode::Disable,
            SslMode::Allow,
            SslMode::Prefer,
            SslMode::Require,
            SslMode::VerifyCa,
            SslMode::VerifyFull,
        ] {
            connector(mode, None).unwrap_or_else(|error| panic!("{mode:?}: {error}"));
        }
    }

    #[test]
    fn missing_root_cert_is_an_error() {
        match connector(SslMode::VerifyFull, Some("/nonexistent/ca.pem")) {
            Err(error) => assert!(matches!(error, TlsError::RootCertRead { .. }), "{error}"),
            Ok(_) => panic!("a missing sslrootcert must be rejected"),
        }
    }
}
