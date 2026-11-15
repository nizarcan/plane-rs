// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `RequestBodySizeLimitMiddleware`
//! (`plane/middleware/request_body_size.py:18-31`, HTTP_CONTRACT §9.1).
//!
//! Reads the whole body like Django's `request.body` and returns the 413
//! `JsonResponse` when it exceeds `DATA_UPLOAD_MAX_MEMORY_SIZE` (which Plane sets
//! from `FILE_SIZE_LIMIT`). Two things about the response are easy to get wrong:
//!
//! * it is a *returned* response from middleware #10, not an exception, so the
//!   response phases 10→0 run: it gets `X-Frame-Options`, `Content-Length` and
//!   `Vary: origin`, but **no** `Vary: Cookie` because the logger (#12) never
//!   ran;
//! * the pre-check uses the declared `Content-Length` before reading, exactly
//!   like `HttpRequest.body`, so an oversized declared length is rejected even
//!   if the client would send less.
//!
//! Anything that is *not* this middleware (e.g. a body rejected by Django's
//! form parser) becomes Django's 400 page, not 413. Phase 1 has no parser.

use axum::body::Body;
use plane_core::config::Settings;
use plane_core::context::RequestContext;
use plane_core::error::ApiError;

/// `RequestBodySizeLimitMiddleware.__call__`, minus the routing.
pub async fn request(
    settings: &Settings,
    ctx: &mut RequestContext,
    body: &mut Option<Body>,
) -> Result<(), ApiError> {
    if ctx.body.is_some() {
        return Ok(());
    }
    let limit = settings.limits.data_upload_max_memory_size;
    if limit < 0 {
        // `int("−1")` is accepted by Django and makes every read fail.
        return Err(ApiError::RequestBodyTooLarge);
    }
    if let Some(declared) = ctx
        .header("content-length")
        .and_then(|value| value.parse::<u64>().ok())
        && declared > limit as u64
    {
        return Err(ApiError::RequestBodyTooLarge);
    }
    let Some(body) = body.take() else {
        ctx.body = Some(Vec::new());
        return Ok(());
    };
    match axum::body::to_bytes(body, limit as usize).await {
        Ok(bytes) => {
            ctx.body = Some(bytes.to_vec());
            Ok(())
        }
        // `RequestDataTooBig`: more than the limit while reading (chunked
        // bodies without a Content-Length).
        Err(_) => Err(ApiError::RequestBodyTooLarge),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, Method};

    fn settings(limit: &str) -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("FILE_SIZE_LIMIT", limit);
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    fn ctx(content_length: Option<&str>) -> RequestContext {
        let mut headers = HeaderMap::new();
        if let Some(value) = content_length {
            headers.insert("content-length", value.parse().unwrap());
        }
        RequestContext::new(Method::POST, "/", "", headers, None)
    }

    #[tokio::test]
    async fn rejects_a_declared_length_over_the_limit() {
        let config = settings("10");
        let mut oversized = ctx(Some("11"));
        let mut body = Some(Body::from(vec![0u8; 11]));
        assert_eq!(
            request(&config, &mut oversized, &mut body).await,
            Err(ApiError::RequestBodyTooLarge)
        );
        // Exactly the limit is accepted.
        let mut at_limit = ctx(Some("10"));
        let mut body = Some(Body::from(vec![0u8; 10]));
        assert_eq!(request(&config, &mut at_limit, &mut body).await, Ok(()));
        assert_eq!(at_limit.body.as_ref().map(Vec::len), Some(10));
    }

    #[tokio::test]
    async fn rejects_a_chunked_body_over_the_limit() {
        let config = settings("4");
        let mut request_ctx = ctx(None);
        let mut body = Some(Body::from(vec![0u8; 5]));
        assert_eq!(
            request(&config, &mut request_ctx, &mut body).await,
            Err(ApiError::RequestBodyTooLarge)
        );
    }

    #[tokio::test]
    async fn negative_limit_rejects_everything() {
        let config = settings("-1");
        let mut request_ctx = ctx(None);
        let mut body = Some(Body::empty());
        assert_eq!(
            request(&config, &mut request_ctx, &mut body).await,
            Err(ApiError::RequestBodyTooLarge)
        );
    }
}
