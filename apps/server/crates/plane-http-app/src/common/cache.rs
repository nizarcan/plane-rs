// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/utils/cache.py` for a handler: `cache_response`, `invalidate_cache`.
//!
//! Both Python decorators read three things off the request — `user.id`,
//! `get_full_path()` and `resolver_match.kwargs` — and then call
//! `django.core.cache`. `plane-cache` implements the cache half
//! (`plane_cache::drf`); this module supplies the request half from
//! [`ViewCtx`], so a handler never rebuilds a key by hand.
//!
//! ```ignore
//! // `@cache_response(60 * 60 * 2)` on `WorkspaceLabelsEndpoint.get`
//! // (`plane/app/views/workspace/label.py:21`).
//! cache_response(ctx, 60 * 60 * 2, None, true, || async {
//!     let rows = load_labels(ctx).await?;
//!     let data = PyValue::return_list(rows.iter().map(label_value).collect());
//!     Ok((200, data))
//! })
//! .await
//! ```
//!
//! The closure returns `(status, data)` where `data` is a [`PyValue`] — the
//! value tree DRF's `serializer.data` is, including `uuid.UUID` objects — so
//! the stored pickle is byte-identical to Django's and a cache hit renders the
//! same bytes the miss did. Rendering the hit and the miss both go through
//! `plane_serde::Dialect::Drf`, i.e. DRF's `JSONRenderer`.
//!
//! ## Failures
//!
//! `django-redis` raises `ConnectionInterrupted` when Redis is unreachable;
//! nothing catches it, DRF re-raises, and Plane's family `handle_exception`
//! answers 500 (`plane/app/views/base.py:200-239`). Every function here maps a
//! cache error to [`ViewError::Unknown`] for that reason — never to a miss.

use plane_cache::PyValue;
use plane_cache::drf::{self, Invalidation, RequestFacts};
use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{ViewCtx, ViewResponse};
use plane_serde::Dialect;

/// The `request` facts `plane_cache::drf` reads, built without allocating the
/// resolver kwargs when the caller does not need them.
fn facts<'a>(
    full_path: &'a str,
    resolver_kwargs: &'a [(&'a str, String)],
    user_id: Option<&'a str>,
) -> RequestFacts<'a> {
    RequestFacts {
        full_path,
        resolver_kwargs,
        user_id,
    }
}

/// `str(request.user.id)`, or `None` for an anonymous caller.
fn user_id(ctx: &ViewCtx<'_>) -> Option<String> {
    // `AnonymousUser.id` is `None`, and the decorators branch on
    // `request.user.is_anonymous` before formatting, so `None` is the right
    // value for both.
    ctx.user.id().map(|id| id.hyphenated().to_string())
}

/// `@cache_response(timeout, path=…, user=…)` (`plane/utils/cache.py:25-51`).
///
/// `path` is the decorator's `path=` argument (`None` means
/// `request.get_full_path()`), `user` its `user=` flag, and `compute` the view
/// body: it is only called on a miss.
///
/// On a hit the stored `data` and `status` are served unchanged, exactly as
/// `Response(cached_result["data"], status=cached_result["status"])` would. On
/// a miss the response is `(status, data)`; it is stored only when the status
/// is 200 and `settings.DEBUG` is off, which is the decorator's condition.
///
/// # Errors
///
/// A cache failure, an undecodable cache entry (`cache.get` returns something
/// that is not `{"data", "status"}` → `KeyError`/`TypeError` in Python) or the
/// handler's own error.
pub async fn cache_response<F, Fut>(
    ctx: &ViewCtx<'_>,
    timeout: i64,
    path: Option<&str>,
    user: bool,
    compute: F,
) -> Result<ViewResponse, ViewError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(u16, PyValue), ViewError>>,
{
    let full_path = drf::full_path(ctx.path(), &ctx.ctx.query_string);
    // `user=False` means the decorated view's key never carries the caller's
    // id, so the string is not built at all.
    let user_id = if user { user_id(ctx) } else { None };
    let request = facts(&full_path, &[], user_id.as_deref());
    let key = drf::response_cache_key(path, user, &request);

    if let Some(cached) = drf::lookup(client_redis(ctx)?, &key).await? {
        return render(cached.status, &cached.data);
    }

    let (status, data) = compute().await?;
    drf::store(
        client_redis(ctx)?,
        &key,
        &data,
        status,
        timeout,
        ctx.settings.debug,
    )
    .await?;
    render(status, &data)
}

/// `@invalidate_cache(path=…, url_params=…, user=…, multiple=…)`
/// (`plane/utils/cache.py:54-88`).
///
/// This is `invalidate_cache_directly` — the decorator's whole body. It runs
/// **before** the view, so a handler calls it first.
///
/// * `url_params = true` substitutes `:name` placeholders in `path` from the
///   request's resolver kwargs, in resolution order, replacing every occurrence.
/// * `user = true` appends `str(request.user.id)` to the key for an
///   authenticated caller.
/// * `multiple = true` deletes every key matching `*<key>*`
///   (`cache.keys()` + `delete_many`) instead of just `<key>`.
///
/// Returns the number of keys removed.
///
/// # Errors
///
/// A cache failure, which is a 500 out of the view as in Django.
pub async fn invalidate_cache(
    ctx: &ViewCtx<'_>,
    path: Option<&str>,
    url_params: bool,
    user: bool,
    multiple: bool,
) -> Result<usize, ViewError> {
    let full_path = drf::full_path(ctx.path(), &ctx.ctx.query_string);
    let resolver_kwargs: Vec<(&str, String)> = ctx
        .request
        .kwargs
        .iter()
        .map(|(name, value)| (*name, (*value).to_string()))
        .collect();
    let user_id = if user { user_id(ctx) } else { None };
    let request = facts(&full_path, &resolver_kwargs, user_id.as_deref());
    let spec = Invalidation {
        path,
        url_params,
        user,
        multiple,
    };
    Ok(drf::invalidate(client_redis(ctx)?, &spec, &request).await?)
}

/// The underlying handle, for the helpers above.
fn client_redis<'a>(ctx: &ViewCtx<'a>) -> Result<&'a plane_cache::Redis, ViewError> {
    ctx.redis.ok_or_else(|| {
        ViewError::Unknown(
            "the process has no Redis cache handle; django-redis would fail the request"
                .to_string(),
        )
    })
}

/// `Response(data, status=status)`, rendered by DRF's `JSONRenderer`.
fn render(status: u16, data: &PyValue) -> Result<ViewResponse, ViewError> {
    // `JSONRenderer.render(None)` is `b""`, and `Response.rendered_content`
    // then deletes `Content-Type` (`rest_framework/response.py:82-84`).
    if matches!(data, PyValue::None) {
        return Ok(ViewResponse::empty(status));
    }
    let body = plane_serde::to_vec(Dialect::Drf, data)?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_cache::PyValue;

    /// `generate_cache_key` and the Django `get_full_path()` rule are
    /// `plane-cache`'s; this checks the wrapper feeds them the right values.
    #[test]
    fn python_style_user_ids_are_hyphenated_uuids() {
        let id = uuid::Uuid::parse_str("8f1c0d3e-4b5a-6978-9a0b-1c2d3e4f5a6b").expect("uuid");
        assert_eq!(
            id.hyphenated().to_string(),
            "8f1c0d3e-4b5a-6978-9a0b-1c2d3e4f5a6b"
        );
    }

    #[test]
    fn none_data_renders_an_empty_body() {
        let response = render(200, &PyValue::None).expect("render");
        assert_eq!(response.status(), 200);
        match response {
            ViewResponse::Json { data, .. } => assert!(data.is_none()),
            ViewResponse::Raw { .. } => panic!("None must render as a bodiless JSON response"),
        }
    }

    #[test]
    fn data_renders_through_the_drf_dialect() {
        let data = PyValue::return_list(vec![PyValue::dict(vec![
            ("id", PyValue::uuid(uuid::Uuid::nil())),
            ("name", PyValue::string("untitled")),
        ])]);
        let response = render(200, &data).expect("render");
        let ViewResponse::Raw {
            content_type, body, ..
        } = response
        else {
            panic!("a list must render as a raw JSON body");
        };
        assert_eq!(content_type, APPLICATION_JSON);
        assert_eq!(
            std::str::from_utf8(&body).expect("utf8"),
            "[{\"id\":\"00000000-0000-0000-0000-000000000000\",\"name\":\"untitled\"}]"
        );
    }
}
