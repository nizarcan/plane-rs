// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET /api/unsplash/` — `plane.app.views.external.base.UnsplashEndpoint`
//! (`apps/api/plane/app/views/external/base.py:215-243`).
//!
//! The view has two branches, and the second one is a live call to
//! `api.unsplash.com`:
//!
//! 1. **No access key** (no `UNSPLASH_ACCESS_KEY` in the instance
//!    configuration, or an empty one — the default seed): `Response([], 200)`,
//!    body `[]`. This is what the fixture's instance configuration produces
//!    (`value = ""`, `is_encrypted = true`), and therefore what the
//!    differential scenario compares.
//! 2. **A key**: `requests.get(url, headers={"Content-Type": "application/json"})`
//!    with **no timeout** and `requests`' 30-redirect limit, then
//!    `Response(resp.json(), status=resp.status_code)` — Unsplash's own JSON
//!    re-rendered by DRF, with Unsplash's status. The call site is the
//!    `UNSPLASH` preset of `crates/plane-net` (DECISIONS.md D9), which is the
//!    same transport policy: environment proxies, no SSRF pinning (the host is
//!    a constant), `python-requests/2.33.0`, gzip/deflate, `Accept: */*` and
//!    the call site's `Content-Type` header on a GET.
//!
//! Two upstream details are preserved rather than fixed:
//!
//! * the search URL has a **literal `$`** before the page number
//!   (`external/base.py:235`): `…&page=$1&per_page=1`. Unsplash ignores the
//!   unknown `page$` parameter, so a search always returns its first page;
//! * neither the access key nor `query` is URL-encoded before interpolation.
//!
//! ## Where the key comes from
//!
//! `get_configuration_value` (`plane/license/utils/instance_value.py:17-39`)
//! reads the database whenever `SKIP_ENV_VAR` is on (the default), and the
//! environment otherwise. An encrypted row is decrypted with the Fernet scheme
//! of `plane/license/utils/encryption.py`; a decryption failure degrades to
//! `""` (and therefore to the keyless branch), exactly as the Python's
//! `except Exception: return ""` does. The key derivation and the token layout
//! are pinned by `ENV.md` §3.5, the test vector in
//! `tools/inventory/encryption_vector.py`, and [`crate::common::fernet`], which
//! owns the implementation now that more than one domain reads instance
//! configuration.

use std::sync::OnceLock;

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_net::{HttpEngine, OutboundRequest, SsrfPolicy, preset};
use plane_serde::Dialect;

/// `api.unsplash.com`, the host both branches build from
/// (`external/base.py:234-238`).
const BASE_URL: &str = "https://api.unsplash.com";

/// `settings.SKIP_ENV_VAR` is on by default (`plane/settings/common.py:401`), so
/// the instance configuration is the source unless a deployment turns it off.
const KEY_NAME: &str = "UNSPLASH_ACCESS_KEY";

/// Register the endpoint's `GET`.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.app.views.external.base.UnsplashEndpoint")?
        .method(Method::GET, get)?;
    Ok(())
}

/// `UnsplashEndpoint.get` (`external/base.py:216-243`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(access_key) = access_key(ctx).await? else {
            // `Response([], status=200)` — DRF renders an empty list as `[]`.
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, b"[]".to_vec()));
        };
        fetch(
            BASE_URL,
            &access_key,
            ctx.query().get("query"),
            ctx.query().get_or("page", "1"),
            ctx.query().get_or("per_page", "20"),
        )
        .await
    })
}

/// `requests.get(…)` + `Response(resp.json(), status=resp.status_code)`.
///
/// `base` is a parameter instead of the constant so the keyed branch can be
/// exercised against a local stub; the handler always passes [`BASE_URL`].
async fn fetch(
    base: &str,
    access_key: &str,
    query: Option<&str>,
    page: &str,
    per_page: &str,
) -> Result<ViewResponse, ViewError> {
    let url = request_url(base, access_key, query, page, per_page);

    // No timeout, `requests`' redirect limit, environment proxies.
    let (response, _) = engine()
        .fetch(
            &preset::UNSPLASH,
            Method::GET,
            &SsrfPolicy::strict(),
            &url,
            &OutboundRequest::new(),
        )
        .await
        .map_err(|error| ViewError::Unknown(format!("the Unsplash request failed: {error}")))?;

    // `resp.json()` is `json.loads(resp.text)`: a decode error is a `ValueError`
    // subclass, i.e. a 500 through `handle_exception`, and the body is
    // re-rendered by DRF's `JSONRenderer`.
    let text = response.text();
    if text.trim() == "null" {
        // `Response(None)` renders an empty body (`JSONRenderer.render`).
        return Ok(ViewResponse::raw(
            response.status,
            APPLICATION_JSON,
            Vec::new(),
        ));
    }
    let mut body = Vec::with_capacity(text.len());
    plane_serde::transcode_json(Dialect::Drf, &text, &mut body).map_err(|error| {
        ViewError::Unknown(format!("the Unsplash response was not JSON: {error}"))
    })?;
    Ok(ViewResponse::raw(response.status, APPLICATION_JSON, body))
}

/// `f"…{UNSPLASH_ACCESS_KEY}…"` (`external/base.py:234-238`).
///
/// `base` is a parameter instead of the constant so the keyed branch can be
/// exercised against a local stub; the handler always passes [`BASE_URL`].
fn request_url(
    base: &str,
    access_key: &str,
    query: Option<&str>,
    page: &str,
    per_page: &str,
) -> String {
    match query {
        // `if query:` — an empty `?query=` is falsy in Python.
        Some(query) if !query.is_empty() => format!(
            "{base}/search/photos/?client_id={access_key}&query={query}&page=${page}&per_page={per_page}"
        ),
        _ => format!("{base}/photos/?client_id={access_key}&page={page}&per_page={per_page}"),
    }
}

/// The process-wide outbound client. `HttpEngine::system()` holds the TLS
/// configuration, so it is built once (`DECISIONS.md` D9).
fn engine() -> &'static HttpEngine {
    static ENGINE: OnceLock<HttpEngine> = OnceLock::new();
    ENGINE.get_or_init(HttpEngine::system)
}

/// `get_configuration_value([{"key": "UNSPLASH_ACCESS_KEY", "default":
/// os.environ.get("UNSPLASH_ACCESS_KEY")}])[0]`
/// (`plane/license/utils/instance_value.py:17-39`).
///
/// `None` means "falsy": an absent row with no environment value, an empty
/// value, or a value that could not be decrypted — every one of them makes the
/// view answer `[]`.
async fn access_key(ctx: &ViewCtx<'_>) -> Result<Option<String>, ViewError> {
    let environment = || std::env::var(KEY_NAME).ok();
    if !ctx.settings.skip_env_var {
        return Ok(environment().filter(|value| !value.is_empty()));
    }

    // The Python evaluates the whole table once per call; the values are
    // unparsed and untrimmed (`ENV.md` §3.4 rule 5).
    let rows = crate::common::client(ctx)?
        .query(plane_db::queries::LOAD_INSTANCE_CONFIGURATIONS.sql, &[])
        .await?;
    for row in &rows {
        let key: &str = row.try_get(1)?;
        if key != KEY_NAME {
            continue;
        }
        let value: Option<&str> = row.try_get(2)?;
        let is_encrypted: bool = row.try_get(4)?;
        let value = value.unwrap_or_default();
        let value = if is_encrypted {
            crate::common::fernet::decrypt_data(ctx.settings.secret_key.value(), value)
        } else {
            value.to_string()
        };
        return Ok(Some(value).filter(|value| !value.is_empty()));
    }
    // No row: the call site's default, which is the environment variable.
    Ok(environment().filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn the_search_url_keeps_the_literal_dollar_sign() {
        // `external/base.py:235`: `…&page=${page}&per_page=${per_page}` — the
        // `$` is a bug, and Unsplash ignores the resulting `page$` parameter.
        assert_eq!(
            request_url(BASE_URL, "key", Some("plane"), "2", "5"),
            "https://api.unsplash.com/search/photos/?client_id=key&query=plane&page=$2&per_page=5"
        );
    }

    #[test]
    fn an_absent_or_empty_query_uses_the_photos_url() {
        // `if query:` — `False` and `""` are both falsy.
        let photos = "https://api.unsplash.com/photos/?client_id=key&page=1&per_page=20";
        assert_eq!(request_url(BASE_URL, "key", None, "1", "20"), photos);
        assert_eq!(request_url(BASE_URL, "key", Some(""), "1", "20"), photos);
    }

    /// The keyed branch, end to end, against a stub bound to `127.0.0.1`: the
    /// real handler's `fetch` builds the URL, the `UNSPLASH` preset's transport
    /// sends it, and the body is re-rendered the way DRF renders
    /// `resp.json()`. No request can reach `api.unsplash.com`.
    ///
    /// The preset keeps `requests`' environment-proxy behaviour (`DECISIONS.md`
    /// D9), so this test needs `127.0.0.1` to be outside any `HTTP_PROXY` —
    /// which is also what Python needs. CI sets no proxy variables.
    #[tokio::test]
    async fn the_keyed_branch_sends_the_preset_request_to_a_local_stub() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        let stub = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let read = socket.read(&mut chunk).await.expect("read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"[{"id":"x","width":640,"height":480,"ratio":1.3333333333333333}]"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.expect("write");
            socket.flush().await.expect("flush");
            String::from_utf8(request).expect("ascii request")
        });

        let base = format!("http://127.0.0.1:{port}");
        let response = fetch(&base, "test-key", Some("plane"), "3", "7")
            .await
            .expect("the stub answers");
        let recorded = stub.await.expect("stub task");

        // The request line is `requests`' URL, `$` included.
        assert!(
            recorded.starts_with(
                "GET /search/photos/?client_id=test-key&query=plane&page=$3&per_page=7 HTTP/1.1"
            ),
            "{recorded}"
        );
        let lower = recorded.to_ascii_lowercase();
        assert!(
            lower.contains("content-type: application/json"),
            "{recorded}"
        );
        assert!(
            lower.contains("user-agent: python-requests/2.33.0"),
            "{recorded}"
        );
        assert!(
            lower.contains("accept-encoding: gzip, deflate"),
            "{recorded}"
        );
        assert!(lower.contains("accept: */*"), "{recorded}");

        // The body is DRF's rendering of `resp.json()`: a Python float repr
        // (`1.3333333333333333`), compact separators.
        assert_eq!(response.status(), 200);
        let ViewResponse::Raw {
            content_type, body, ..
        } = response
        else {
            panic!("a raw response");
        };
        assert_eq!(content_type, APPLICATION_JSON);
        assert_eq!(
            String::from_utf8(body).expect("utf-8"),
            r#"[{"id":"x","width":640,"height":480,"ratio":1.3333333333333333}]"#
        );
    }

    /// The status is Unsplash's, and a body that is not JSON is a 500 — Python
    /// raises `requests.exceptions.JSONDecodeError` there.
    #[tokio::test]
    async fn a_non_json_body_is_a_rendering_failure_at_the_upstream_status() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut chunk = [0u8; 1024];
            let _ = socket.read(&mut chunk).await;
            socket
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Type: text/html\r\nContent-Length: 3\r\nConnection: close\r\n\r\nno!",
                )
                .await
                .expect("write");
        });
        let error = fetch(&format!("http://127.0.0.1:{port}"), "k", None, "1", "20")
            .await
            .expect_err("json.loads fails");
        assert!(matches!(error, ViewError::Unknown(_)), "{error:?}");
    }
}
