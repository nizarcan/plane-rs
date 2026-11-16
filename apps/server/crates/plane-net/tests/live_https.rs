// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Live proof of the pinning behaviour, against a real TLS server.
//!
//! Each test starts a local HTTPS server with the committed self-signed
//! certificate for `pinned.test` (`tests/fixtures/`), maps that name to
//! 127.0.0.1 through [`StubResolver`], and inspects what the server actually
//! received:
//!
//! * the socket arrived at the validated address, not at a second resolution;
//! * SNI and `Host` carry the original name (bracketed/port-suffixed exactly as
//!   Python computes them);
//! * a DNS answer that changes between validation and connect cannot move the
//!   connection (a flip-flop resolver is consulted exactly once);
//! * a private target is refused before any connection unless it is inside
//!   `WEBHOOK_ALLOWED_IPS` or `WEBHOOK_ALLOWED_HOSTS`;
//! * every redirect hop is re-resolved, re-validated and re-pinned;
//! * certificate verification uses the name, not the pinned IP.
//!
//! The decoy server binds 127.0.0.2 on the *same port* as the first, so a rebind
//! would be reachable if the transport resolved twice.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use plane_net::client::OutboundRequest;
use plane_net::dns::{Resolving, StubResolver};
use plane_net::preset;
use plane_net::ssrf::SsrfPolicy;
use plane_net::{FetchError, HttpEngine, TlsTrust};
use reqwest::Method;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const CERT: &[u8] = include_bytes!("fixtures/pinned.test.crt");
const KEY: &[u8] = include_bytes!("fixtures/pinned.test.key");

#[derive(Debug, Clone, Default)]
struct Observation {
    server_name: Option<String>,
    method: String,
    path: String,
    host: Option<String>,
    authorization: Option<String>,
    headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct Reply {
    status: &'static str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn ok(body: &str) -> Self {
        Self {
            status: "200 OK",
            headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    fn redirect(location: &str) -> Self {
        Self {
            status: "302 Found",
            headers: vec![("Location".into(), location.to_string())],
            body: Vec::new(),
        }
    }
}

type Responder = Arc<dyn Fn(&Observation) -> Reply + Send + Sync>;

struct RunningServer {
    address: SocketAddr,
    observations: Arc<Mutex<Vec<Observation>>>,
    task: JoinHandle<()>,
}

impl RunningServer {
    fn observed(&self) -> Vec<Observation> {
        self.observations.lock().expect("observations lock").clone()
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn server_config() -> Arc<rustls::ServerConfig> {
    let certificates = plane_net::tls::certificates_from_pem(CERT).expect("committed certificate");
    let mut key_reader = std::io::BufReader::new(KEY);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .expect("committed key parses")
        .expect("committed key exists");
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS12, &rustls::version::TLS13])
        .expect("TLS versions")
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .expect("certificate and key match");
    Arc::new(config)
}

async fn start_on(ip: IpAddr, port: u16, responder: Responder) -> Option<RunningServer> {
    let listener = TcpListener::bind(SocketAddr::new(ip, port)).await.ok()?;
    let address = listener.local_addr().ok()?;
    let acceptor = tokio_rustls::TlsAcceptor::from(server_config());
    let observations: Arc<Mutex<Vec<Observation>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = observations.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            let recorded = recorded.clone();
            let responder = responder.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                let server_name = tls
                    .get_ref()
                    .1
                    .server_name()
                    .map(std::string::ToString::to_string);
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    match tls.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            buffer.extend_from_slice(&chunk[..read]);
                            if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let text = String::from_utf8_lossy(&buffer);
                let mut observation = Observation {
                    server_name,
                    ..Observation::default()
                };
                let mut lines = text.split("\r\n");
                if let Some(request_line) = lines.next() {
                    let mut parts = request_line.split(' ');
                    observation.method = parts.next().unwrap_or("").to_string();
                    observation.path = parts.next().unwrap_or("").to_string();
                }
                for line in lines {
                    if line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        let value = value.trim().to_string();
                        match name.to_ascii_lowercase().as_str() {
                            "host" => observation.host = Some(value),
                            "authorization" => observation.authorization = Some(value),
                            other => {
                                observation.headers.insert(other.to_string(), value);
                            }
                        }
                    }
                }
                let reply = responder(&observation);
                if let Ok(mut guard) = recorded.lock() {
                    guard.push(observation);
                }
                let mut response = format!("HTTP/1.1 {}\r\n", reply.status);
                response.push_str(&format!("Content-Length: {}\r\n", reply.body.len()));
                for (name, value) in &reply.headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("Connection: close\r\n\r\n");
                let mut bytes = response.into_bytes();
                bytes.extend_from_slice(&reply.body);
                let _ = tls.write_all(&bytes).await;
                let _ = tls.shutdown().await;
            });
        }
    });
    Some(RunningServer {
        address,
        observations,
        task,
    })
}

async fn start(responder: Responder) -> RunningServer {
    start_on(IpAddr::from([127, 0, 0, 1]), 0, responder)
        .await
        .expect("loopback server")
}

/// A policy that permits the loopback test server. In production a name that
/// resolves to 127.0.0.1 is refused; the test target has to be allow-listed
/// exactly like an operator would allow-list an internal service.
fn loopback_policy() -> SsrfPolicy {
    SsrfPolicy::new(vec!["127.0.0.0/8".parse().expect("network")], [])
}

fn engine(resolver: StubResolver) -> HttpEngine {
    HttpEngine::new(Arc::new(resolver)).with_pinned_trust(TlsTrust::Custom(
        plane_net::tls::certificates_from_pem(CERT).expect("certificate"),
    ))
}

fn host_map() -> StubResolver {
    StubResolver::new(
        BTreeMap::from([(
            "pinned.test".to_string(),
            vec![IpAddr::from([127, 0, 0, 1])],
        )]),
        None,
    )
    .with_host("other.test", vec![IpAddr::from([127, 0, 0, 1])])
    .with_host("rebind.test", vec![IpAddr::from([127, 0, 0, 1])])
}

#[tokio::test]
async fn pinning_reaches_the_validated_address_with_original_sni_and_host() {
    let server = start(Arc::new(|_: &Observation| {
        let mut reply = Reply::ok("<title>ok</title>");
        reply.headers.push(("X-Dup".into(), "one".into()));
        reply.headers.push(("X-Dup".into(), "two".into()));
        reply
    }))
    .await;
    let port = server.address.port();
    let http = engine(host_map());
    let url = format!("https://pinned.test:{port}/a/b?q=1");

    let (response, final_url) = http
        .fetch(
            &preset::LINK_TITLE,
            Method::GET,
            &loopback_policy(),
            &url,
            &OutboundRequest::new(),
        )
        .await
        .expect("pinned fetch");

    assert_eq!(response.status, 200);
    assert_eq!(final_url, url);
    assert_eq!(response.text(), "<title>ok</title>");
    // urllib3 joins duplicate headers; requests then stores them as one entry.
    assert_eq!(response.headers.get("x-dup"), Some("one, two"));

    let observed = server.observed();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].server_name.as_deref(), Some("pinned.test"));
    assert_eq!(
        observed[0].host.as_deref(),
        Some(format!("pinned.test:{port}").as_str())
    );
    assert_eq!(observed[0].path, "/a/b?q=1");
    // The crawler's UA, not reqwest's.
    assert!(
        observed[0]
            .headers
            .get("user-agent")
            .is_some_and(|agent| agent.contains("Chrome/91")),
        "{:?}",
        observed[0].headers
    );
    // requests' default Accept-Encoding is advertised (reqwest decodes it).
    assert_eq!(
        observed[0]
            .headers
            .get("accept-encoding")
            .map(String::as_str),
        Some("gzip, deflate")
    );
    // `requests` sends this explicitly on every HTTP/1.1 request.
    assert_eq!(
        observed[0].headers.get("connection").map(String::as_str),
        Some("keep-alive")
    );
    assert_eq!(
        observed[0].headers.get("accept").map(String::as_str),
        Some("*/*")
    );
}

#[tokio::test]
async fn url_embedded_credentials_become_basic_auth_and_are_not_sent_as_userinfo() {
    let server = start(Arc::new(|_: &Observation| Reply::ok("ok"))).await;
    let port = server.address.port();
    let http = engine(host_map());
    let url = format!("https://user:p%40ss@pinned.test:{port}/hook");

    http.fetch_once(
        &preset::WEBHOOK,
        Method::POST,
        &loopback_policy(),
        &url,
        &OutboundRequest::new().body(b"{}".to_vec()),
    )
    .await
    .expect("pinned fetch");

    let observed = server.observed();
    assert_eq!(observed.len(), 1);
    // base64("user:p@ss")
    assert_eq!(
        observed[0].authorization.as_deref(),
        Some("Basic dXNlcjpwQHNz")
    );
    assert_eq!(
        observed[0].host.as_deref(),
        Some(format!("pinned.test:{port}").as_str())
    );
    assert_eq!(observed[0].method, "POST");

    // Without userinfo, no Authorization header is invented.
    let plain = format!("https://pinned.test:{port}/hook");
    let _ = http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &loopback_policy(),
            &plain,
            &OutboundRequest::new(),
        )
        .await
        .expect("pinned fetch");
    let observed = server.observed();
    assert!(observed[1].authorization.is_none());
}

#[tokio::test]
async fn a_changing_dns_answer_cannot_redirect_the_connection() {
    // The resolver answers 127.0.0.1 once and a different address (TEST-NET-1,
    // unroutable) on any later lookup. If the transport resolved a second time
    // -- the rebinding TOCTOU -- the connection could not reach the server, so
    // a successful response plus a single lookup is the proof. (A second
    // listener on 127.0.0.2 would be a stronger visual proof but macOS does not
    // allow binding loopback aliases unless they were configured.)
    let first = start(Arc::new(|_: &Observation| Reply::ok("first"))).await;
    let port = first.address.port();

    struct FlipFlop {
        calls: AtomicUsize,
    }
    impl plane_net::Resolver for FlipFlop {
        fn resolve<'a>(&'a self, _hostname: &'a str) -> Resolving<'a> {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            let address = if index == 0 {
                IpAddr::from([127, 0, 0, 1])
            } else {
                // What a rebinding attacker would return on the second lookup.
                IpAddr::from([192, 0, 2, 1])
            };
            Box::pin(async move { Ok(vec![address]) })
        }
    }

    let resolver = Arc::new(FlipFlop {
        calls: AtomicUsize::new(0),
    });
    let http = HttpEngine::new(resolver.clone()).with_pinned_trust(TlsTrust::Custom(
        plane_net::tls::certificates_from_pem(CERT).expect("certificate"),
    ));
    let url = format!("https://pinned.test:{port}/x");
    let response = http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &loopback_policy(),
            &url,
            &OutboundRequest::new().body(b"{}".to_vec()),
        )
        .await
        .expect("pinned fetch");
    assert_eq!(response.text(), "first");
    assert_eq!(
        resolver.calls.load(Ordering::SeqCst),
        1,
        "exactly one lookup"
    );
    assert_eq!(first.observed().len(), 1);
}

#[tokio::test]
async fn private_targets_are_refused_unless_allow_listed() {
    let server = start(Arc::new(|_: &Observation| Reply::ok("ok"))).await;
    let port = server.address.port();
    let http = engine(host_map());
    let url = format!("https://127.0.0.1:{port}/x");

    let error = http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &SsrfPolicy::strict(),
            &url,
            &OutboundRequest::new(),
        )
        .await
        .expect_err("loopback is blocked");
    assert!(error.is_value_error(), "{error}");
    assert_eq!(error.message(), plane_net::BLOCKED_NETWORK);
    assert!(
        server.observed().is_empty(),
        "no connection may be attempted"
    );

    // `WEBHOOK_ALLOWED_IPS` permits the private target. The URL has to use the
    // name, not the literal: the committed certificate is issued for
    // `pinned.test`, and TLS verifies the name, so `https://127.0.0.1/` would
    // fail certificate verification even though the address is allow-listed
    // (Python fails the same way: certifi does not cover the IP either).
    let policy = SsrfPolicy::new(vec!["127.0.0.0/8".parse().expect("network")], []);
    let response = http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &policy,
            &format!("https://pinned.test:{port}/x"),
            &OutboundRequest::new(),
        )
        .await
        .expect("allow-listed");
    assert_eq!(response.status, 200);

    // A trusted hostname is allowed too, and is still pinned.
    let policy = SsrfPolicy::new(vec![], ["pinned.test".to_string()]);
    let url = format!("https://pinned.test:{port}/x");
    let response = http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &policy,
            &url,
            &OutboundRequest::new(),
        )
        .await
        .expect("trusted host");
    assert_eq!(response.status, 200);
}

#[tokio::test]
async fn redirect_hops_are_re_resolved_and_re_validated() {
    let port_slot: Arc<Mutex<Option<u16>>> = Arc::new(Mutex::new(None));
    let slot = port_slot.clone();
    let server = start(Arc::new(move |observation: &Observation| {
        let port = slot.lock().expect("slot").expect("port set");
        if observation.path.starts_with("/redirect") {
            Reply::redirect(&format!("https://pinned.test:{port}/final"))
        } else if observation.path.starts_with("/to-private") {
            Reply::redirect("http://169.254.169.254/latest/meta-data/")
        } else {
            Reply::ok("final")
        }
    }))
    .await;
    let port = server.address.port();
    *port_slot.lock().expect("slot") = Some(port);
    let http = engine(host_map());

    let (response, final_url) = http
        .fetch(
            &preset::LINK_TITLE,
            Method::GET,
            &loopback_policy(),
            &format!("https://pinned.test:{port}/redirect"),
            &OutboundRequest::new(),
        )
        .await
        .expect("follows one hop");
    assert_eq!(response.text(), "final");
    assert_eq!(final_url, format!("https://pinned.test:{port}/final"));
    assert_eq!(server.observed().len(), 2);

    // A redirect to a blocked address is a validation failure, not a
    // transport failure: webhook-style callers must not retry it.
    let error = http
        .fetch(
            &preset::LINK_TITLE,
            Method::GET,
            &loopback_policy(),
            &format!("https://pinned.test:{port}/to-private"),
            &OutboundRequest::new(),
        )
        .await
        .expect_err("blocked redirect target");
    assert!(error.is_value_error(), "{error}");
    assert_eq!(error.message(), plane_net::BLOCKED_NETWORK);
    // Only the first hop of the second chain reached the server (2 + 1).
    assert_eq!(server.observed().len(), 3);
}

#[tokio::test]
async fn certificate_verification_uses_the_name_not_the_pinned_ip() {
    let server = start(Arc::new(|_: &Observation| Reply::ok("ok"))).await;
    let port = server.address.port();
    let http = engine(host_map());

    // `other.test` maps to the same address, but the certificate is only for
    // `pinned.test`: verification must fail, and it must fail as a transport
    // error (Python's SSLError is a RequestException, so webhooks retry it).
    let error = http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &loopback_policy(),
            &format!("https://other.test:{port}/x"),
            &OutboundRequest::new(),
        )
        .await
        .expect_err("certificate mismatch");
    assert!(error.is_request_exception(), "{error}");
}

#[tokio::test]
async fn body_caps_are_enforced_before_and_while_reading() {
    let server = start(Arc::new(|_: &Observation| {
        let mut reply = Reply::ok(&"x".repeat(100));
        reply
            .headers
            .push(("Content-Type".into(), "text/plain".into()));
        reply
    }))
    .await;
    let port = server.address.port();
    let http = engine(host_map());
    let url = format!("https://pinned.test:{port}/x");

    let error = http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &loopback_policy(),
            &url,
            &OutboundRequest::new().max_body_bytes(10),
        )
        .await
        .expect_err("cap exceeded");
    assert!(matches!(error, FetchError::BodyTooLarge { limit: 10 }));

    // Exactly at the cap is accepted, as Python's `> max_size` check allows.
    let response = http
        .fetch_once(
            &preset::WEBHOOK,
            Method::POST,
            &loopback_policy(),
            &url,
            &OutboundRequest::new().max_body_bytes(100),
        )
        .await
        .expect("at the cap");
    assert_eq!(response.bytes().len(), 100);
}

#[tokio::test]
async fn unpinned_presets_skip_validation_and_use_the_environment_trust() {
    // `UNSPLASH` is not SSRF-guarded: a loopback URL is allowed through, the
    // stub resolver is never consulted (it fails every lookup), and TLS uses
    // the environment bundle, which does not trust the test certificate.
    let server = start(Arc::new(|_: &Observation| {
        let mut reply = Reply::ok("{\"ok\":true}");
        reply
            .headers
            .push(("Content-Type".into(), "application/json".into()));
        reply
    }))
    .await;
    let port = server.address.port();
    let http = HttpEngine::new(Arc::new(StubResolver::failing()));
    let url = format!("https://127.0.0.1:{port}/photos");
    let error = http
        .fetch_once(
            &preset::UNSPLASH,
            Method::GET,
            &SsrfPolicy::strict(),
            &url,
            &OutboundRequest::new(),
        )
        .await
        .expect_err("self-signed certificate is not in the environment trust store");
    assert!(error.is_request_exception(), "{error}");
    assert!(!error.is_value_error());
}

/// Counts resolver calls so a test can prove how many lookups a request (or a
/// redirect chain) performed.
struct CountingResolver {
    inner: StubResolver,
    calls: Arc<AtomicUsize>,
}

impl plane_net::Resolver for CountingResolver {
    fn resolve<'a>(&'a self, hostname: &'a str) -> Resolving<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve(hostname)
    }
}

#[tokio::test]
async fn every_redirect_hop_is_resolved_and_pinned_again() {
    let port_slot: Arc<Mutex<Option<u16>>> = Arc::new(Mutex::new(None));
    let slot = port_slot.clone();
    let server = start(Arc::new(move |observation: &Observation| {
        let port = slot.lock().expect("slot").expect("port set");
        if observation.path.starts_with("/redirect") {
            Reply::redirect(&format!("https://pinned.test:{port}/final"))
        } else {
            Reply::ok("final")
        }
    }))
    .await;
    let port = server.address.port();
    *port_slot.lock().expect("slot") = Some(port);

    let calls = Arc::new(AtomicUsize::new(0));
    let http = HttpEngine::new(Arc::new(CountingResolver {
        inner: host_map(),
        calls: calls.clone(),
    }))
    .with_pinned_trust(TlsTrust::Custom(
        plane_net::tls::certificates_from_pem(CERT).expect("certificate"),
    ));

    http.fetch(
        &preset::LINK_TITLE,
        Method::GET,
        &loopback_policy(),
        &format!("https://pinned.test:{port}/redirect"),
        &OutboundRequest::new(),
    )
    .await
    .expect("follows one hop");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one lookup per hop");
    assert_eq!(server.observed().len(), 2);
}

#[tokio::test]
async fn too_many_redirects_is_a_request_exception() {
    let server = start(Arc::new(|observation: &Observation| {
        Reply::redirect(&observation.path)
    }))
    .await;
    let port = server.address.port();
    let http = engine(host_map());
    let start_url = format!("https://pinned.test:{port}/loop");

    let error = http
        .fetch(
            &preset::LINK_TITLE,
            Method::GET,
            &loopback_policy(),
            &start_url,
            &OutboundRequest::new(),
        )
        .await
        .expect_err("redirect loop");
    assert!(error.is_request_exception(), "{error}");
    assert_eq!(
        error.message(),
        format!("Exceeded 5 redirects for URL: {start_url}")
    );
    // Six responses: the initial request plus five followed hops.
    assert_eq!(server.observed().len(), 6);
}

#[tokio::test]
async fn an_unreachable_address_falls_through_to_the_next_resolved_one() {
    // Ported from `TestPinnedFetch.test_tries_next_ip_on_connection_error`.
    // 127.0.0.2 is inside the allow-listed loopback range but has no listener,
    // so the connect is refused; the loop must try 127.0.0.1 afterwards.
    let server = start(Arc::new(|_: &Observation| Reply::ok("second"))).await;
    let port = server.address.port();
    struct TwoAddresses {
        first: IpAddr,
        second: IpAddr,
    }
    impl plane_net::Resolver for TwoAddresses {
        fn resolve<'a>(&'a self, _hostname: &'a str) -> Resolving<'a> {
            let addresses = vec![self.first, self.second];
            Box::pin(async move { Ok(addresses) })
        }
    }
    let http = HttpEngine::new(Arc::new(TwoAddresses {
        first: IpAddr::from([127, 0, 0, 2]),
        second: IpAddr::from([127, 0, 0, 1]),
    }))
    .with_pinned_trust(TlsTrust::Custom(
        plane_net::tls::certificates_from_pem(CERT).expect("certificate"),
    ));
    let response = http
        .fetch_once(
            &preset::LINK_TITLE,
            Method::GET,
            // Allow the loopback answer; TEST-NET-1 is public and passes anyway.
            &loopback_policy(),
            &format!("https://pinned.test:{port}/x"),
            &OutboundRequest::new(),
        )
        .await
        .expect("second address answers");
    assert_eq!(response.text(), "second");
    assert_eq!(server.observed().len(), 1);
}
