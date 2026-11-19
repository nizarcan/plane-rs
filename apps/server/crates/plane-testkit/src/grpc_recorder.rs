// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The OTLP/gRPC half of a `difftest task` case.
//!
//! One HTTP/2 (h2c) server per side accepts the unary `MetricsService/Export`
//! call. It records the semantic request — method, path, metadata (minus the
//! time-dependent `grpc-timeout`), and the gRPC message after decompression —
//! and answers with the mode's `grpc-status` trailer. `drop` returns a service
//! error after reading the request, which resets the stream (the transport
//! failure the exporter retries as `UNAVAILABLE`).
//!
//! Both servers bind `127.0.0.1:0`, so concurrent runs cannot collide. The
//! harness sets each side's `OTLP_ENDPOINT` to its server's `host:port` and
//! `OTEL_EXPORTER_OTLP_METRICS_INSECURE=true`; nothing leaves the machine.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use http_body::Frame;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Incoming;
use hyper::header::{HeaderMap, HeaderName, HeaderValue};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The case's gRPC configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrpcCase {
    /// `ok`, `unavailable`, `invalid`, `deadline` or `drop`.
    #[serde(default = "default_mode")]
    pub mode: String,
    /// `deadline`: how long the handler waits before answering.
    #[serde(default)]
    pub delay_ms: Option<u64>,
    /// `OTEL_EXPORTER_OTLP_METRICS_HEADERS` for both sides.
    #[serde(default)]
    pub headers: Option<String>,
    /// `OTEL_EXPORTER_OTLP_METRICS_COMPRESSION` for both sides (`gzip`).
    #[serde(default)]
    pub compression: Option<String>,
}

fn default_mode() -> String {
    "ok".to_string()
}

/// One recorded RPC. `status` is the trailer the server sent; `None` for the
/// two transport cases (`deadline` and `drop`), where the client never sees
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrpcRecord {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub message: Vec<u8>,
    pub status: Option<u32>,
}

/// One side's server.
pub struct GrpcRecorder {
    endpoint: String,
    log: Arc<Mutex<Vec<GrpcRecord>>>,
    handle: JoinHandle<()>,
}

impl GrpcRecorder {
    pub async fn start(case: &GrpcCase) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("binding the grpc recorder")?;
        let address = listener.local_addr().context("grpc recorder address")?;
        let log = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&log);
        let mode = case.mode.clone();
        let delay = case.delay_ms;
        let handle = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let log = Arc::clone(&shared);
                let mode = mode.clone();
                let drop = Arc::new(tokio::sync::Notify::new());
                let service_drop = Arc::clone(&drop);
                let task = tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        serve(
                            request,
                            Arc::clone(&log),
                            mode.clone(),
                            delay,
                            Arc::clone(&service_drop),
                        )
                    });
                    let _ = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
                // `drop` closes the whole connection, not just the stream:
                // aborting the connection task drops the socket. The watcher
                // ends with the connection so a normal case leaks no task.
                let abort = task.abort_handle();
                tokio::spawn(async move {
                    tokio::select! {
                        _ = drop.notified() => abort.abort(),
                        _ = task => {}
                    }
                });
            }
        });
        Ok(Self {
            endpoint: format!("127.0.0.1:{}", address.port()),
            log,
            handle,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn request_count(&self) -> usize {
        self.log.lock().expect("grpc recorder log").len()
    }

    pub fn slice(&self, from: usize) -> Vec<GrpcRecord> {
        self.log
            .lock()
            .expect("grpc recorder log")
            .get(from..)
            .map(<[GrpcRecord]>::to_vec)
            .unwrap_or_default()
    }
}

impl Drop for GrpcRecorder {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// The two per-side servers of one case.
pub struct GrpcCapture {
    pub a: GrpcRecorder,
    pub b: GrpcRecorder,
}

impl GrpcCapture {
    pub async fn start(case: &GrpcCase) -> Result<Self> {
        Ok(Self {
            a: GrpcRecorder::start(case).await?,
            b: GrpcRecorder::start(case).await?,
        })
    }
}

async fn serve(
    request: Request<Incoming>,
    log: Arc<Mutex<Vec<GrpcRecord>>>,
    mode: String,
    delay_ms: Option<u64>,
    drop: Arc<tokio::sync::Notify>,
) -> Result<Response<GrpcBody>, std::io::Error> {
    let method = request.method().as_str().to_string();
    let path = request.uri().path().to_string();
    let headers = request.headers().clone();
    let body = request
        .into_body()
        .collect()
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?
        .to_bytes();
    let message = decode_message(&body, &headers)?;
    let metadata = comparable_headers(&headers);
    let status = match mode.as_str() {
        "ok" => Some(0),
        "unavailable" => Some(14),
        "invalid" => Some(3),
        _ => None,
    };
    log.lock().expect("grpc recorder log").push(GrpcRecord {
        method,
        path,
        headers: metadata,
        message,
        status,
    });
    if mode == "drop" {
        drop.notify_one();
        // The connection task is aborted by the watcher, which drops the
        // socket; never answer on this stream.
        std::future::pending::<()>().await;
        unreachable!("the drop watcher aborts the connection");
    }
    if mode == "deadline"
        && let Some(delay) = delay_ms
    {
        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
    }
    let mut trailers = HeaderMap::new();
    trailers.insert(
        HeaderName::from_static("grpc-status"),
        HeaderValue::from_static(match mode.as_str() {
            "unavailable" => "14",
            "invalid" => "3",
            _ => "0",
        }),
    );
    if let Some(message) = match mode.as_str() {
        "unavailable" => Some("try later"),
        "invalid" => Some("bad request"),
        _ => None,
    } {
        trailers.insert(
            HeaderName::from_static("grpc-message"),
            HeaderValue::from_static(message),
        );
    }
    let body = StreamBody::new(futures_util::stream::iter(vec![Ok::<
        _,
        std::convert::Infallible,
    >(Frame::trailers(
        trailers,
    ))]))
    .boxed();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/grpc")
        .body(body)
        .map_err(|error| std::io::Error::other(error.to_string()))
}

type GrpcBody = BoxBody<Bytes, std::convert::Infallible>;

/// The headers the comparison keeps: every semantic gRPC/HTTP/2 header except
/// the time-dependent `grpc-timeout`.
fn comparable_headers(headers: &HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(name, _)| name.as_str() != "grpc-timeout")
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

/// Strip the 5-byte gRPC frame header and gunzip when the frame's compressed
/// flag is set.
fn decode_message(body: &[u8], headers: &HeaderMap) -> Result<Vec<u8>, std::io::Error> {
    if body.len() < 5 {
        return Err(std::io::Error::other("short gRPC frame"));
    }
    let compressed = body[0] == 1;
    let length = u32::from_be_bytes([body[1], body[2], body[3], body[4]]) as usize;
    let message = body.get(5..5 + length).ok_or_else(|| {
        std::io::Error::other(format!(
            "gRPC frame declares {length} bytes, {} present",
            body.len() - 5
        ))
    })?;
    if !compressed {
        return Ok(message.to_vec());
    }
    let encoding = headers
        .get("grpc-encoding")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if encoding != "gzip" {
        return Err(std::io::Error::other(format!(
            "unsupported gRPC encoding {encoding:?}"
        )));
    }
    let mut decoder = flate2::read::GzDecoder::new(message);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// Compare the two sides' calls, in order.
pub fn compare_grpc(a: &[GrpcRecord], b: &[GrpcRecord]) -> Result<()> {
    if a == b {
        println!("PASS: grpc: {} call(s) identical", a.len());
        return Ok(());
    }
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index);
        let right = b.get(index);
        if left != right {
            bail!(
                "grpc call {index} differs:\n  django: {}\n  rust:   {}",
                left.map_or_else(|| "<none>".to_string(), describe),
                right.map_or_else(|| "<none>".to_string(), describe),
            );
        }
    }
    bail!("grpc: call sequences differ")
}

fn describe(record: &GrpcRecord) -> String {
    format!(
        "{}{} status={:?} message={}B {:02x?} metadata={:?}",
        record.method,
        record.path,
        record.status,
        record.message.len(),
        &record.message[..record.message.len().min(32)],
        record.headers,
    )
}
