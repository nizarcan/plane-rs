// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A trivial keep-alive HTTP server on localhost.
//!
//! Its only job is the client-headroom measurement: if the load generator can
//! issue N requests/s against a server that does nothing but answer `200` with
//! an empty body, then N is an upper bound on what it can measure against a
//! real target. It is not a fair comparison with Django or `plane-api` — it is
//! a bound on the measuring instrument.
//!
//! The server shares the machine with the client, so the number is a lower
//! bound on the client's capacity (and an upper bound on any target's
//! measurable throughput at that concurrency). `BENCH.md` records it next to
//! the real numbers.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::header::{CONNECTION, HeaderValue};
use hyper::service::service_fn;
use hyper::{Response, StatusCode};
use hyper_util::rt::TokioIo;

/// A running keep-alive server. Dropping it stops accepting connections.
pub struct TrivialServer {
    pub addr: SocketAddr,
    pub base_url: String,
    accept_task: tokio::task::JoinHandle<()>,
}

impl Drop for TrivialServer {
    fn drop(&mut self) {
        self.accept_task.abort();
    }
}

impl TrivialServer {
    /// Start a server that answers `status` to every request. With
    /// `close_after_response` it sends `Connection: close`, which exercises the
    /// client's reconnect path.
    pub async fn start(status: u16, close_after_response: bool) -> Result<Self> {
        let status = StatusCode::from_u16(status).context("invalid status for the test server")?;
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("binding the trivial server")?;
        let addr = listener.local_addr().context("trivial server address")?;
        let accept_task = tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let service = service_fn(move |_request| async move {
                        let mut response = Response::new(Full::new(Bytes::new()));
                        *response.status_mut() = status;
                        if close_after_response {
                            response
                                .headers_mut()
                                .insert(CONNECTION, HeaderValue::from_static("close"));
                        }
                        Ok::<_, Infallible>(response)
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .keep_alive(true)
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        Ok(Self {
            addr,
            base_url: format!("http://{addr}"),
            accept_task,
        })
    }
}

/// Give a freshly bound listener a moment to be reachable. Not needed on
/// loopback, but it keeps the headroom measurement from counting a failed
/// first connection on a loaded machine.
pub async fn settle() {
    tokio::time::sleep(Duration::from_millis(20)).await;
}
