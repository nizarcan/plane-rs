// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! OTLP/gRPC, over the same pinned resolver and rustls configuration as the
//! HTTP client (`D9`: one TLS stack, one SSRF policy).
//!
//! This is the transport half of `push_instance_metrics`' gRPC branch. It
//! speaks the unary gRPC protocol directly with `h2`:
//!
//! * HTTP/2 preface and SETTINGS from `h2` (the C-core's frame-level choices
//!   are not observable to the collector beyond the semantic headers);
//! * `:method POST`, `:scheme`, `:authority` = the endpoint exactly as Python's
//!   `grpc.insecure_channel`/`secure_channel` received it, and `:path` = the
//!   OTLP method;
//! * `content-type: application/grpc`, `te: trailers`,
//!   `grpc-accept-encoding: identity, deflate, gzip`, `grpc-timeout`, and a
//!   caller-supplied `user-agent` and metadata;
//! * one length-prefixed gRPC message (`1-byte compressed flag`, 4-byte
//!   big-endian length, protobuf), then END_STREAM;
//! * the response's `grpc-status`/`grpc-message` trailers. A missing trailer is
//!   `UNKNOWN`, an HTTP status without a gRPC status maps like gRPC's HTTP
//!   fallback (`401` → `UNAUTHENTICATED`, `403` → `PERMISSION_DENIED`, `404` →
//!   `UNIMPLEMENTED`, `429/502/503/504` → `UNAVAILABLE`, else `UNKNOWN`).
//!
//! A transport failure (connect, TLS, h2, reset stream) is
//! [`FetchError::Grpc`]; the exporter retries it as `UNAVAILABLE`. A local
//! deadline is returned as the `DEADLINE_EXCEEDED` status, so the exporter's
//! retry table sees the same code Python's `grpc` raises.
//!
//! TLS uses the pinned call sites' certifi roots with ALPN `h2`
//! (`TlsSettings::grpc_config`). Python's grpc C-core uses its own bundled
//! roots; D9 chooses one stack, so the port keeps certifi for consistency with
//! every other pinned call site. `OTEL_EXPORTER_OTLP_*_CERTIFICATE` is not
//! wired (it would select a second trust configuration).

use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use h2::client::handshake;
use http::{HeaderMap, HeaderName, HeaderValue, Method, Request};
use rustls::pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use crate::client::HttpEngine;
use crate::error::FetchError;
use crate::headers::HeaderList;
use crate::preset::Preset;
use crate::ssrf::{SsrfPolicy, validate_hop};

/// `grpc.StatusCode.OK`.
pub const STATUS_OK: u32 = 0;
/// `grpc.StatusCode.UNKNOWN`.
pub const STATUS_UNKNOWN: u32 = 2;
/// `grpc.StatusCode.DEADLINE_EXCEEDED`.
pub const STATUS_DEADLINE_EXCEEDED: u32 = 4;
/// `grpc.StatusCode.UNAVAILABLE`.
pub const STATUS_UNAVAILABLE: u32 = 14;

/// One unary gRPC call.
#[derive(Debug, Clone)]
pub struct GrpcRequest<'a> {
    /// `host:port`, exactly the value `get_otlp_grpc_endpoint()` produced.
    pub endpoint: &'a str,
    /// The full method path, e.g.
    /// `/opentelemetry.proto.collector.metrics.v1.MetricsService/Export`.
    pub path: &'a str,
    /// `insecure_channel` vs `secure_channel`.
    pub insecure: bool,
    /// Caller metadata: `user-agent` and any `OTEL_EXPORTER_OTLP_*_HEADERS`.
    pub metadata: Vec<(String, String)>,
    /// The serialized protobuf, **already compressed** when `compression` is
    /// set.
    pub body: Vec<u8>,
    /// `grpc-encoding`: only `gzip` is produced.
    pub compression: Option<&'static str>,
    /// The RPC deadline (`_export`'s remaining `self._timeout`).
    pub timeout: Duration,
}

/// A gRPC status code and its optional message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrpcStatus {
    pub code: u32,
    pub message: Option<String>,
}

/// The result of one RPC. `http_status` is kept for diagnostics; a 200 with a
/// `grpc-status` trailer is the normal case.
#[derive(Debug, Clone)]
pub struct GrpcResponse {
    pub http_status: u16,
    pub status: GrpcStatus,
    pub trailers: HeaderList,
}

impl HttpEngine {
    /// One unary gRPC call through the pinned transport.
    ///
    /// # Errors
    ///
    /// A URL-validation error (Python `ValueError`), or a transport failure
    /// ([`FetchError::Grpc`]). A local deadline is returned as
    /// `DEADLINE_EXCEEDED`, not as an error.
    pub async fn grpc_unary(
        &self,
        preset: &Preset,
        policy: &SsrfPolicy,
        request: &GrpcRequest<'_>,
    ) -> Result<GrpcResponse, FetchError> {
        let scheme = if request.insecure { "http" } else { "https" };
        let url = format!("{scheme}://{}/", request.endpoint);
        let hop = validate_hop(self.resolver().as_ref(), &url, policy).await?;
        let mut last: Option<FetchError> = None;
        for address in &hop.addresses {
            match self.grpc_hop(preset, request, &hop.target, *address).await {
                Ok(response) => return Ok(response),
                Err(error) => last = Some(error),
            }
        }
        Err(last.unwrap_or_else(|| {
            FetchError::Grpc(format!(
                "No reachable address for host: {}",
                hop.target.hostname
            ))
        }))
    }

    async fn grpc_hop(
        &self,
        preset: &Preset,
        request: &GrpcRequest<'_>,
        target: &crate::pyurl::Target,
        address: std::net::IpAddr,
    ) -> Result<GrpcResponse, FetchError> {
        let rpc_timeout = request.timeout;
        let connect_timeout = preset.timeout.unwrap_or(rpc_timeout);

        match tokio::time::timeout(
            rpc_timeout,
            self.grpc_exchange(request, target, address, connect_timeout, preset.ssrf),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Ok(GrpcResponse {
                http_status: 200,
                status: GrpcStatus {
                    code: STATUS_DEADLINE_EXCEEDED,
                    message: Some("Deadline Exceeded".to_string()),
                },
                trailers: HeaderList::default(),
            }),
        }
    }

    async fn grpc_exchange(
        &self,
        request: &GrpcRequest<'_>,
        target: &crate::pyurl::Target,
        address: std::net::IpAddr,
        connect_timeout: Duration,
        ssrf: bool,
    ) -> Result<GrpcResponse, FetchError> {
        let tcp = tokio::time::timeout(
            connect_timeout,
            TcpStream::connect(SocketAddr::new(address, target.port)),
        )
        .await
        .map_err(|_| FetchError::Grpc("connect timed out".to_string()))?
        .map_err(|error| FetchError::Grpc(format!("connect failed: {error}")))?;

        let io: Box<dyn AsyncReadWrite> = if request.insecure {
            Box::new(tcp)
        } else {
            let config = self
                .tls()
                .grpc_config(&self.trust_for(ssrf))
                .map_err(FetchError::Tls)?;
            let server_name = ServerName::try_from(target.hostname.clone())
                .map_err(|error| FetchError::Grpc(format!("invalid TLS name: {error}")))?;
            let stream = TlsConnector::from(config)
                .connect(server_name, tcp)
                .await
                .map_err(|error| FetchError::Grpc(format!("TLS handshake failed: {error}")))?;
            Box::new(stream)
        };

        let (mut send_request, connection) = handshake(io)
            .await
            .map_err(|error| FetchError::Grpc(format!("HTTP/2 handshake failed: {error}")))?;
        let _connection = AbortOnDrop(tokio::spawn(async move {
            let _ = connection.await;
        }));

        let scheme = if request.insecure { "http" } else { "https" };
        let uri = format!("{scheme}://{}{}", request.endpoint, request.path);
        let mut builder = Request::builder().method(Method::POST).uri(uri);
        {
            let headers = builder
                .headers_mut()
                .ok_or_else(|| FetchError::Grpc("invalid request URI".to_string()))?;
            insert_header(headers, "content-type", "application/grpc")?;
            insert_header(headers, "te", "trailers")?;
            insert_header(headers, "grpc-accept-encoding", "identity, deflate, gzip")?;
            insert_header(headers, "grpc-timeout", &grpc_timeout(request.timeout))?;
            if let Some(encoding) = request.compression {
                insert_header(headers, "grpc-encoding", encoding)?;
            }
            for (name, value) in &request.metadata {
                insert_header(headers, name, value)?;
            }
        }
        let (response, mut send) = send_request
            .send_request(
                builder
                    .body(())
                    .map_err(|error| FetchError::Grpc(format!("invalid gRPC request: {error}")))?,
                false,
            )
            .map_err(|error| FetchError::Grpc(format!("HTTP/2 send failed: {error}")))?;

        let mut frame = Vec::with_capacity(5 + request.body.len());
        frame.push(u8::from(request.compression.is_some()));
        frame.extend_from_slice(&(request.body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&request.body);
        // A large instance's body can exceed the peer's initial 65 KiB
        // stream window; gRPC's message is a byte stream, so send it in
        // flow-control-sized DATA frames.
        let mut offset = 0;
        while offset < frame.len() {
            send.reserve_capacity((frame.len() - offset).min(16 * 1024));
            let capacity = std::future::poll_fn(|context| send.poll_capacity(context))
                .await
                .ok_or_else(|| FetchError::Grpc("HTTP/2 send stream closed".to_string()))?
                .map_err(|error| FetchError::Grpc(format!("HTTP/2 send failed: {error}")))?;
            if capacity == 0 {
                continue;
            }
            let end = (offset + capacity).min(frame.len());
            send.send_data(
                Bytes::copy_from_slice(&frame[offset..end]),
                end == frame.len(),
            )
            .map_err(|error| FetchError::Grpc(format!("HTTP/2 send failed: {error}")))?;
            offset = end;
        }

        let response = response
            .await
            .map_err(|error| FetchError::Grpc(format!("gRPC response failed: {error}")))?;
        let http_status = response.status().as_u16();
        let mut body = response.into_body();
        while let Some(chunk) = body.data().await {
            chunk.map_err(|error| FetchError::Grpc(format!("gRPC body failed: {error}")))?;
        }
        let trailers = body
            .trailers()
            .await
            .map_err(|error| FetchError::Grpc(format!("gRPC trailers failed: {error}")))?
            .unwrap_or_default();

        let trailers = HeaderList::from_map(&trailers);
        let status = if http_status != 200 {
            GrpcStatus {
                code: code_for_http(http_status),
                message: Some(format!("HTTP status {http_status}")),
            }
        } else if let Some(code) = trailers
            .get("grpc-status")
            .and_then(|value| value.trim().parse::<u32>().ok())
        {
            GrpcStatus {
                code,
                message: trailers.get("grpc-message").map(str::to_string),
            }
        } else {
            GrpcStatus {
                code: STATUS_UNKNOWN,
                message: Some("No status received".to_string()),
            }
        };
        Ok(GrpcResponse {
            http_status,
            status,
            trailers,
        })
    }
}

trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

/// Aborts the HTTP/2 connection driver when the exchange ends, including when
/// the outer deadline cancels it.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn insert_header(headers: &mut HeaderMap, name: &str, value: &str) -> Result<(), FetchError> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|error| FetchError::Header(format!("invalid metadata name {name:?}: {error}")))?;
    let value = HeaderValue::from_str(value).map_err(|error| {
        FetchError::Header(format!("invalid metadata value {value:?}: {error}"))
    })?;
    headers.insert(name, value);
    Ok(())
}

/// The gRPC timeout header. gRPC picks the largest unit that keeps the value
/// under nine digits and rounds up, so a 10 s deadline (a few microseconds
/// short after `time()` calls) is `10S`.
fn grpc_timeout(timeout: Duration) -> String {
    let nanos = timeout.as_nanos();
    for (unit, scale) in [
        ('H', 3_600_000_000_000u128),
        ('M', 60_000_000_000),
        ('S', 1_000_000_000),
        ('m', 1_000_000),
        ('u', 1_000),
        ('n', 1),
    ] {
        if nanos / scale >= 1 {
            let value = nanos.div_ceil(scale);
            if value < 100_000_000 {
                return format!("{value}{unit}");
            }
        }
    }
    "1n".to_string()
}

/// `grpc`'s HTTP-status fallback for a response without a status trailer.
fn code_for_http(status: u16) -> u32 {
    match status {
        401 => 16, // UNAUTHENTICATED
        403 => 7,  // PERMISSION_DENIED
        404 => 12, // UNIMPLEMENTED
        429 | 502 | 503 | 504 => STATUS_UNAVAILABLE,
        _ => STATUS_UNKNOWN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_timeout_uses_the_grpc_units() {
        assert_eq!(grpc_timeout(Duration::from_nanos(999)), "999n");
        assert_eq!(grpc_timeout(Duration::from_micros(10_000)), "10m");
        assert_eq!(grpc_timeout(Duration::from_secs(10)), "10S");
        assert_eq!(grpc_timeout(Duration::from_secs(100)), "2M");
    }

    #[test]
    fn http_status_fallback_matches_grpc() {
        assert_eq!(code_for_http(401), 16);
        assert_eq!(code_for_http(403), 7);
        assert_eq!(code_for_http(404), 12);
        assert_eq!(code_for_http(503), STATUS_UNAVAILABLE);
        assert_eq!(code_for_http(500), STATUS_UNKNOWN);
    }
}
