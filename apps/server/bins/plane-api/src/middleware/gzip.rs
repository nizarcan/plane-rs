// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GZipMiddleware` (`django/middleware/gzip.py`, HTTP_CONTRACT §8.4).
//!
//! The order is Django 5.2's (`apps/api/.venv/.../django/middleware/gzip.py`),
//! and each step matters:
//!
//! 1. A **non-streaming** body shorter than 200 bytes is returned untouched
//!    (`if not response.streaming and len(response.content) < 200`). A
//!    streaming response has no length to judge, so it is always considered.
//! 2. A response that already has `Content-Encoding` is returned untouched —
//!    **before** `Vary` is patched.
//! 3. `patch_vary_headers(response, ("Accept-Encoding",))` runs **before** the
//!    `Accept-Encoding` check, so a client that does not accept gzip still gets
//!    `Vary: Accept-Encoding` (and, for a short non-streaming body, does not).
//! 4. The client must send a case-sensitive `\bgzip\b` match; there is no
//!    q-value parsing, so `gzip;q=0` still compresses.
//! 5. A streaming body is compressed whatever its length and keeps **no**
//!    `Content-Length` (Django deletes it: the compressed size is not known
//!    until the stream ends); a non-streaming body is replaced only when the
//!    compressed bytes are strictly shorter, and `Content-Length` is rewritten.
//! 6. A strong `ETag` is weakened (`W/"…"`), then `Content-Encoding: gzip` is
//!    set.
//!
//! ## Streaming
//!
//! Django wraps `response.streaming_content` in `compress_sequence`
//! (`django/utils/text.py:375-388`): **one** gzip stream, written chunk by
//! chunk with `GzipFile(filename=…, compresslevel=6, mtime=0)`, flushed after
//! every chunk. The handler here has already collected the whole body (the
//! pipeline renders into one buffer), so compressing it once produces the same
//! single gzip member and therefore the same decompressed bytes. The chunk
//! boundaries are not observable in the compared contract (see below).
//!
//! ## Compressed bytes are not compared
//!
//! Django deliberately randomises the gzip header: `max_random_bytes = 100`
//! makes `compress_string`/`compress_sequence` insert `b"a" * randbelow(100)`
//! as the gzip `FNAME` (`django/utils/text.py:347-363`), so two identical
//! responses differ on the wire. The differential harness therefore decodes the
//! body (`crates/plane-testkit/src/http.rs:197-214`) and compares the decoded
//! bytes, and never compares `Content-Length` on a response that carries
//! `Content-Encoding` (`crates/plane-testkit/src/runner.rs:586-593`). Level 6
//! and `mtime=0` are pinned anyway so a future raw-byte check starts from the
//! right shape.

use std::io::Write;
use std::sync::LazyLock;

use flate2::{Compression, GzBuilder};
use plane_core::context::RequestContext;
use regex::Regex;

use crate::response::Resp;

/// `len(response.content) < 200` returns unchanged; 200 is inclusive. Only for
/// a **non-streaming** response.
const MIN_LENGTH: usize = 200;
/// `gzip.compress(..., compresslevel=6)` / `GzipFile(compresslevel=6)`.
const COMPRESSION_LEVEL: u32 = 6;

static ACCEPTS_GZIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgzip\b").expect("constant regex"));

/// `GZipMiddleware.process_response`.
pub fn response(ctx: &RequestContext, mut resp: Resp) -> Resp {
    // 1. `if not response.streaming and len(response.content) < 200: return`
    if !resp.streaming && resp.body.len() < MIN_LENGTH {
        return resp;
    }
    // 2. `if response.has_header("Content-Encoding"): return` — note this comes
    //    before `Vary` is patched.
    if resp.header("content-encoding").is_some() {
        return resp;
    }
    // 3. `patch_vary_headers(response, ("Accept-Encoding",))`, before the
    //    Accept-Encoding check.
    resp.patch_vary("Accept-Encoding");

    // 4. `if not re_accepts_gzip.search(ae): return`
    let accept_encoding = ctx.header("accept-encoding").unwrap_or("");
    if !ACCEPTS_GZIP.is_match(accept_encoding) {
        return resp;
    }

    if resp.streaming {
        // 5a. `compress_sequence`, and `del response.headers["Content-Length"]`:
        // the compressed size is unknown until the stream ends. `Resp` never
        // has a `Content-Length` here because `set_content_length` skips a
        // streaming response; removing it defensively keeps a handler-set one
        // from leaking through.
        resp.body = compress(&resp.body);
        resp.remove_header("Content-Length");
    } else {
        // 5b. `if len(compressed) >= len(response.content): return response`
        let compressed = compress(&resp.body);
        if compressed.len() >= resp.body.len() {
            return resp;
        }
        resp.body = compressed;
        resp.set_header("Content-Length", resp.body.len().to_string());
    }

    // 6. `etag = response.get("ETag"); if etag and etag.startswith('"'): …`
    if let Some(etag) = resp.header("etag")
        && etag.starts_with('"')
    {
        resp.set_header("ETag", format!("W/{etag}"));
    }
    resp.set_header("Content-Encoding", "gzip");
    resp
}

fn compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = GzBuilder::new().mtime(0).write(
        Vec::with_capacity(data.len() / 2),
        Compression::new(COMPRESSION_LEVEL),
    );
    if encoder.write_all(data).is_err() {
        // Writing to a Vec cannot fail; be defensive rather than panicking on a
        // request path.
        return data.to_vec();
    }
    encoder.finish().unwrap_or_else(|_| data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, Method};

    fn ctx(accept_encoding: Option<&str>) -> RequestContext {
        let mut headers = HeaderMap::new();
        if let Some(value) = accept_encoding {
            headers.insert("accept-encoding", value.parse().unwrap());
        }
        RequestContext::new(Method::GET, "/", "", headers, None)
    }

    fn body(len: usize) -> Vec<u8> {
        // Repetitive data so gzip is clearly shorter.
        b"{\"detail\":\""
            .iter()
            .cycle()
            .take(len)
            .copied()
            .collect()
    }

    fn streaming(content_type: &'static str, body: Vec<u8>) -> Resp {
        let mut resp = Resp::new(200, content_type, body);
        resp.streaming = true;
        resp
    }

    fn decoded(bytes: &[u8]) -> Vec<u8> {
        let mut decoder = flate2::read::GzDecoder::new(bytes);
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut out).expect("gzip");
        out
    }

    #[test]
    fn short_bodies_are_untouched() {
        let resp = response(
            &ctx(Some("gzip")),
            Resp::new(200, "application/json", body(199)),
        );
        assert_eq!(resp.body.len(), 199);
        assert!(resp.header("vary").is_none());
        assert!(resp.header("content-encoding").is_none());
    }

    #[test]
    fn long_body_with_gzip() {
        let mut original = Resp::new(200, "application/json", body(500));
        original.set_header("ETag", "\"abc\"");
        let resp = response(&ctx(Some("gzip")), original);
        assert_eq!(resp.header("vary"), Some("Accept-Encoding"));
        assert_eq!(resp.header("content-encoding"), Some("gzip"));
        assert_eq!(resp.header("etag"), Some("W/\"abc\""));
        let content_length = resp.body.len().to_string();
        assert_eq!(resp.header("content-length"), Some(content_length.as_str()));
        // The compressed body decompresses to the original.
        assert_eq!(decoded(&resp.body), body(500));
    }

    #[test]
    fn long_body_without_gzip_only_gets_vary() {
        let resp = response(&ctx(None), Resp::new(200, "application/json", body(500)));
        assert_eq!(resp.header("vary"), Some("Accept-Encoding"));
        assert!(resp.header("content-encoding").is_none());
        assert_eq!(resp.body.len(), 500);

        // Case-sensitive word search: "GZIP" does not match.
        let resp = response(
            &ctx(Some("GZIP")),
            Resp::new(200, "application/json", body(500)),
        );
        assert!(resp.header("content-encoding").is_none());
    }

    #[test]
    fn already_encoded_bodies_are_untouched() {
        // Django checks `Content-Encoding` before patching `Vary`, so a
        // response that arrives already encoded gains no `Vary` either.
        let mut original = Resp::new(200, "application/json", body(500));
        original.set_header("Content-Encoding", "br");
        let resp = response(&ctx(Some("gzip")), original);
        assert!(resp.header("vary").is_none());
        assert_eq!(resp.header("content-encoding"), Some("br"));
    }

    #[test]
    fn incompressible_bodies_keep_their_length() {
        // 200 bytes of random-ish bytes compress larger than the original.
        let mut data = Vec::with_capacity(200);
        let mut state: u32 = 0x9e37_79b9;
        for _ in 0..200 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.push((state >> 24) as u8);
        }
        let resp = response(
            &ctx(Some("gzip")),
            Resp::new(200, "application/octet-stream", data.clone()),
        );
        assert_eq!(resp.header("vary"), Some("Accept-Encoding"));
        assert!(resp.header("content-encoding").is_none());
        assert_eq!(resp.body, data);
    }

    /// A streaming body is compressed below 200 bytes, keeps no `Content-Length`
    /// and gains `Vary` — the three ways Django treats it differently from a
    /// `HttpResponse` (`django/middleware/gzip.py:27-30,52-67`).
    #[test]
    fn short_streaming_bodies_are_compressed() {
        let resp = response(
            &ctx(Some("gzip")),
            streaming("application/octet-stream", body(33)),
        );
        assert_eq!(resp.header("vary"), Some("Accept-Encoding"));
        assert_eq!(resp.header("content-encoding"), Some("gzip"));
        assert!(
            resp.header("content-length").is_none(),
            "a streaming response has no Content-Length"
        );
        assert_eq!(decoded(&resp.body), body(33));
    }

    /// `del response.headers["Content-Length"]` must also drop a header the
    /// handler set (no handler does today, but the rule is unconditional).
    #[test]
    fn a_streaming_body_drops_a_handler_set_content_length() {
        let mut resp = streaming("application/octet-stream", body(33));
        resp.set_header("Content-Length", "33");
        let resp = response(&ctx(Some("gzip")), resp);
        assert!(resp.header("content-length").is_none());
        assert!(resp.header("content-encoding").is_some());
    }

    /// An empty streaming body is still a gzip member whose decoded bytes are
    /// empty — `compress_sequence([b""])` writes the header and trailer.
    #[test]
    fn an_empty_streaming_body_is_a_gzip_member() {
        let resp = response(
            &ctx(Some("gzip")),
            streaming("application/octet-stream", Vec::new()),
        );
        assert_eq!(resp.header("content-encoding"), Some("gzip"));
        assert_eq!(decoded(&resp.body), Vec::<u8>::new());
        assert!(!resp.body.is_empty());
    }

    /// Without `Accept-Encoding: gzip` a streaming body is not compressed, but
    /// the streaming response still has no `Content-Length` and still gets
    /// `Vary`.
    #[test]
    fn a_streaming_body_without_gzip_is_not_compressed() {
        let resp = response(&ctx(None), streaming("application/octet-stream", body(33)));
        assert!(resp.header("content-encoding").is_none());
        assert_eq!(resp.header("vary"), Some("Accept-Encoding"));
        assert_eq!(resp.body, body(33));
        assert!(resp.header("content-length").is_none());
    }

    /// A streaming response that is already encoded is untouched, before the
    /// `Vary` patch.
    #[test]
    fn an_already_encoded_streaming_body_is_untouched() {
        let mut resp = streaming("application/octet-stream", body(33));
        resp.set_header("Content-Encoding", "br");
        let resp = response(&ctx(Some("gzip")), resp);
        assert_eq!(resp.header("content-encoding"), Some("br"));
        assert!(resp.header("vary").is_none());
    }
}
