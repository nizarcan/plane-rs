// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GZipMiddleware` (`django/middleware/gzip.py`, HTTP_CONTRACT §8.4).
//!
//! Order matters: the `Vary: Accept-Encoding` header is added *before* the
//! request's `Accept-Encoding` is examined, and only for bodies of at least 200
//! bytes with no `Content-Encoding` yet. A compressed body replaces the body and
//! `Content-Length` only when it is strictly shorter; a strong `ETag` is
//! weakened. Django's `gzip.compress(..., mtime=0)` also writes a random gzip
//! filename, so the compressed *bytes* are not reproducible (and not compared);
//! headers and the decoded body are.
//!
//! `Accept-Encoding: gzip;q=0` still compresses: the check is a case-sensitive
//! `\bgzip\b` search, with no q-value parsing (HTTP_CONTRACT §8.4).

use std::io::Write;
use std::sync::LazyLock;

use flate2::{Compression, GzBuilder};
use plane_core::context::RequestContext;
use regex::Regex;

use crate::response::Resp;

/// `len(response.content) < 200` returns unchanged; 200 is inclusive.
const MIN_LENGTH: usize = 200;
/// `gzip.compress(..., compresslevel=6)`.
const COMPRESSION_LEVEL: u32 = 6;

static ACCEPTS_GZIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgzip\b").expect("constant regex"));

/// `GZipMiddleware.process_response`.
pub fn response(ctx: &RequestContext, mut resp: Resp) -> Resp {
    if resp.body.len() < MIN_LENGTH {
        return resp;
    }
    if resp.header("content-encoding").is_some() {
        return resp;
    }
    resp.patch_vary("Accept-Encoding");

    let accept_encoding = ctx.header("accept-encoding").unwrap_or("");
    if !ACCEPTS_GZIP.is_match(accept_encoding) {
        return resp;
    }

    let compressed = compress(&resp.body);
    if compressed.len() >= resp.body.len() {
        return resp;
    }
    resp.body = compressed;
    resp.set_header("Content-Length", resp.body.len().to_string());

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
        let mut decoder = flate2::read::GzDecoder::new(&resp.body[..]);
        let mut decoded = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut decoded).expect("gzip");
        assert_eq!(decoded, body(500));
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
        let mut original = Resp::new(200, "application/json", body(500));
        original.set_header("Content-Encoding", "br");
        let resp = response(&ctx(Some("gzip")), original);
        assert!(resp.header("vary").is_none());
        assert_eq!(resp.header("content-encoding"), Some("br"));
    }

    #[test]
    fn incompressible_bodies_keep_there_length() {
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
}
