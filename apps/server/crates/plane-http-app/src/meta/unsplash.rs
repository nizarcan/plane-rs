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
//! are pinned by `ENV.md` §3.5 and by the test vector in
//! `tools/inventory/encryption_vector.py`.

use std::num::NonZeroU32;
use std::sync::OnceLock;

use aws_lc_rs::cipher::{AES_128, DecryptingKey, DecryptionContext, UnboundCipherKey};
use aws_lc_rs::iv::{FixedLength, IV_LEN_128_BIT};
use base64::Engine as _;
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
            decrypt_data(ctx.settings.secret_key.value(), value)
        } else {
            value.to_string()
        };
        return Ok(Some(value).filter(|value| !value.is_empty()));
    }
    // No row: the call site's default, which is the environment variable.
    Ok(environment().filter(|value| !value.is_empty()))
}

// ---------------------------------------------------------------------------
// `plane/license/utils/encryption.py`
// ---------------------------------------------------------------------------

/// `decrypt_data(encrypted_data)` (`plane/license/utils/encryption.py:31-39`).
///
/// Every failure mode returns `""` — the Python catches `Exception` and logs
/// it, and every caller treats the result as "not configured" (`ENV.md` §3.5).
fn decrypt_data(secret_key: &str, token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    decrypt_token(secret_key, token).unwrap_or_default()
}

/// One Fernet token (`ENV.md` §3.5):
/// `urlsafe_base64(0x80 || timestamp_u64_be || IV(16) || ciphertext || HMAC(32))`,
/// AES-128-CBC with PKCS7 padding, HMAC-SHA256 over everything before the MAC,
/// key = `PBKDF2-HMAC-SHA256(SECRET_KEY, b"salt", 100_000, 32)` with the first
/// 16 bytes as the MAC key and the last 16 as the AES key.
fn decrypt_token(secret_key: &str, token: &str) -> Option<String> {
    // `token.encode("ascii")`, then `base64.urlsafe_b64decode`.
    if !token.is_ascii() {
        return None;
    }
    let raw = base64::engine::general_purpose::URL_SAFE
        .decode(token)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token))
        .ok()?;
    // version(1) + timestamp(8) + IV(16) + at least one block + HMAC(32).
    if raw.len() < 73 || raw[0] != 0x80 {
        return None;
    }
    let body_end = raw.len() - 32;
    let key = fernet_key(secret_key);

    let hmac_key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, &key[..16]);
    aws_lc_rs::hmac::verify(&hmac_key, &raw[..body_end], &raw[body_end..]).ok()?;

    let iv_bytes: [u8; IV_LEN_128_BIT] = raw.get(9..25)?.try_into().ok()?;
    let ciphertext = raw.get(25..body_end)?;
    let unbound = UnboundCipherKey::new(&AES_128, &key[16..]).ok()?;
    let decrypting = DecryptingKey::cbc(unbound).ok()?;
    let mut buffer = ciphertext.to_vec();
    let plain = decrypting
        .decrypt(
            &mut buffer,
            DecryptionContext::Iv128(FixedLength::from(iv_bytes)),
        )
        .ok()?;

    // PKCS7: every padding byte repeats the length, 1..=16.
    let padding = *plain.last()? as usize;
    if padding == 0 || padding > 16 || padding > plain.len() {
        return None;
    }
    if !plain[plain.len() - padding..]
        .iter()
        .all(|&byte| byte as usize == padding)
    {
        return None;
    }
    String::from_utf8(plain[..plain.len() - padding].to_vec()).ok()
}

/// `derive_key(SECRET_KEY)` (`plane/license/utils/encryption.py:14-17`),
/// cached: the 100 000 PBKDF2 iterations are paid once per process
/// (`ENV.md` §3.5).
fn fernet_key(secret_key: &str) -> [u8; 32] {
    static KEY: OnceLock<([u8; 32], String)> = OnceLock::new();
    let (key, stored) = KEY.get_or_init(|| (derive_key(secret_key), secret_key.to_string()));
    if stored == secret_key {
        *key
    } else {
        // A process has one SECRET_KEY; a different one means a test or a
        // misconfiguration, and its key must not be confused with the cached one.
        derive_key(secret_key)
    }
}

/// `PBKDF2-HMAC-SHA256(SECRET_KEY, b"salt", 100_000, 32)`
/// (`plane/license/utils/encryption.py:14-17`).
fn derive_key(secret_key: &str) -> [u8; 32] {
    const ITERATIONS: NonZeroU32 = match NonZeroU32::new(100_000) {
        Some(iterations) => iterations,
        None => panic!("100000 is not zero"),
    };
    let mut derived = [0u8; 32];
    aws_lc_rs::pbkdf2::derive(
        aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA256,
        ITERATIONS,
        b"salt",
        secret_key.as_bytes(),
        &mut derived,
    );
    derived
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// The deterministic Fernet token from `ENV.md` §3.5, produced by
    /// `tools/inventory/encryption_vector.py` with `time.time` and
    /// `os.urandom` patched. Verified against the repository's own
    /// `decrypt_data` before it was pinned here:
    ///
    /// ```sh
    /// SECRET_KEY=plane-rs-fernet-vector-not-a-real-secret \
    ///   DJANGO_SETTINGS_MODULE=plane.settings.production \
    ///   apps/api/.venv/bin/python -c 'import django; django.setup();
    /// from plane.license.utils.encryption import decrypt_data;
    /// print(repr(decrypt_data(TOKEN)))'
    /// ```
    const VECTOR_SECRET: &str = "plane-rs-fernet-vector-not-a-real-secret";
    const VECTOR_PLAINTEXT: &str = "postgres://plane:plane@plane-db:5432/plane";
    const VECTOR_TOKEN: &str = "gAAAAABlU_EAAAECAwQFBgcICQoLDA0OD_SnVf1mrlHhHb4c5e-aA8GMdLq0PUT_Pk9CZUuJwCqdkAkvnySvjAzQmocDM0BPwT_NuJmtB5ybzGqAEwvR6aTjQMEBapMa1RrzA6bVtWdw";

    #[test]
    fn fernet_decrypts_the_recorded_vector() {
        assert_eq!(decrypt_data(VECTOR_SECRET, VECTOR_TOKEN), VECTOR_PLAINTEXT);
    }

    #[test]
    fn fernet_failures_degrade_to_an_empty_string() {
        // `except Exception: return ""` (`encryption.py:37-39`).
        assert_eq!(decrypt_data(VECTOR_SECRET, ""), "");
        assert_eq!(decrypt_data(VECTOR_SECRET, "not-a-token"), "");
        assert_eq!(
            decrypt_data(VECTOR_SECRET, &VECTOR_TOKEN[..VECTOR_TOKEN.len() - 4]),
            "",
            "a truncated token is an InvalidToken in Python"
        );
        assert_eq!(
            decrypt_data("another-secret", VECTOR_TOKEN),
            "",
            "a different SECRET_KEY must not decrypt"
        );
        assert_eq!(decrypt_data(VECTOR_SECRET, "gAAAAE=\u{e9}"), "");
    }

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
