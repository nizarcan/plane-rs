// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/work_item_link_task.py:249-259` — `crawl_work_item_link_title`.
//!
//! One `IssueLink.metadata` write with `{title, favicon, url, favicon_url}`:
//!
//! 1. `crawl_work_item_link_title_and_favicon(url)` (`:113-159`):
//!    * `safe_get(url, headers=UA)` — a **one-second** timeout, five re-validated
//!      redirects, the browser UA (`:126`, `:77-99`; presets `LINK_TITLE`);
//!    * `BeautifulSoup(response.content, "html.parser")`, the first `<title>`
//!      with `.get_text().strip()` (`:135-136`); any `RequestException`,
//!      `ValueError` or `RuntimeError` leaves the title `None` and still tries
//!      the favicon (`:138-141`);
//!    * `fetch_and_encode_favicon` (`:201-241`): `find_favicon_url`
//!      (`:161-198`) looks for `link[rel="icon"]`, `link[rel="shortcut icon"]`,
//!      `link[rel="apple-touch-icon"]`, `link[rel="apple-touch-icon-precomposed"]`
//!      **in that order**, `urljoin`s the href against the **final** URL and
//!      validates the result; without a match it `HEAD`s `<scheme>://<netloc>/favicon.ico`
//!      with a two-second timeout (`FAVICON_HEAD`); a missing favicon yields the
//!      inline SVG constant `DEFAULT_FAVICON` (`:23`);
//!    * the favicon body is `data:<content-type>;base64,<b64>` with the
//!      response's `content-type` header or `image/x-icon` (`:228-236`);
//!      any exception in this helper yields the default SVG (`:238-241`).
//! 2. `IssueLink.objects.get(id=id)` (`:252-256`): a missing row logs and
//!    returns — **not** an error. A malformed id raises `ValidationError`,
//!    which is *outside* the try and fails the task.
//! 3. `issue_link.metadata = meta_data; issue_link.save()` (`:258-259`). A bare
//!    `save()` writes every column: `updated_at` moves (`auto_now`) and
//!    `BaseModel.save` nulls `created_by`/`updated_by` because a task has no
//!    current user (`plane/db/models/base.py:23-50`). `metadata` is `jsonb`, so
//!    its key order is Postgres's, not the dict's.
//!
//! # SSRF
//!
//! Python validates with `plane/utils/url_security.py` and resolves+pins every
//! hop; there is no allow-list at this call site. The port uses
//! `SsrfPolicy::strict()` and `plane_net`'s presets. The loopback escape for
//! the harness is compiled in only with the non-default `difftest` feature
//! (`Cargo.toml`), which the harness enables when it builds `plane-worker`:
//! `PLANE_RS_DIFFTEST_ALLOW_LOOPBACK=1` is otherwise ignored, so an environment
//! variable cannot widen a release worker's SSRF guard.
//!
//! # Ownership
//!
//! Four producers, all Rust routes (`TASKS.md:892-911`): `/api/` routes 74-75
//! and `/api/v1/` rows 319-320, 331-332 (`ROUTES.md`). The task moves with this
//! change.

use std::sync::Arc;

use base64::Engine;
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use plane_net::pyurl::{urljoin, urlparse};
use plane_net::{HttpEngine, Method, OutboundRequest, SsrfPolicy, preset};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::html::{self, CrawlSource};
use crate::{TaskContext, args, kwargs, string_value, swallow, uuid_value};

/// The Celery task name (`plane.bgtasks.work_item_link_task.crawl_work_item_link_title`).
pub const TASK: &str = "plane.bgtasks.work_item_link_task.crawl_work_item_link_title";

/// `DEFAULT_FAVICON` (`work_item_link_task.py:23`), base64 of the inline SVG.
const DEFAULT_FAVICON: &str = "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyNCIgaGVpZ2h0PSIyNCIgdmlld0JveD0iMCAwIDI0IDI0IiBmaWxsPSJub25lIiBzdHJva2U9ImN1cnJlbnRDb2xvciIgc3Ryb2tlLXdpZHRoPSIyIiBzdHJva2UtbGluZWNhcD0icm91bmQiIHN0cm9rZS1saW5lam9pbj0icm91bmQiIGNsYXNzPSJsdWNpZGUgbHVjaWRlLWxpbmstaWNvbiBsdWNpZGUtbGluayI+PHBhdGggZD0iTTEwIDEzYTUgNSAwIDAgMCA3LjU0LjU0bDMtM2E1IDUgMCAwIDAtNy4wNy03LjA3bC0xLjcyIDEuNzEiLz48cGF0aCBkPSJNMTQgMTFhNSA1IDAgMCAwLTcuNTQtLjU0bC0zIDNhNSA1IDAgMCAwNy4wNyA3LjA3bDEuNzEtMS43MSIvPjwvc3ZnPg==";

/// The favicon selectors, in `find_favicon_url`'s order (`:171-176`).
const FAVICON_SELECTORS: [&str; 4] = [
    "icon",
    "shortcut icon",
    "apple-touch-icon",
    "apple-touch-icon-precomposed",
];

/// Register the handler; the worker's registry owns the closure.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    registry.register(TASK, move |job| {
        let context = Arc::clone(&context);
        Box::pin(async move { handle(&context, job).await })
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Crawl {
    id: Uuid,
    url: String,
}

/// `id` and `url` from positional arguments or keywords.
fn decode(payload: &serde_json::Value) -> Option<Crawl> {
    if let Some(kw) = kwargs(payload)
        && let (Some(id), Some(url)) = (uuid_value(kw.get("id")), string_value(kw.get("url")))
    {
        return Some(Crawl { id, url });
    }
    let items = args(payload)?;
    Some(Crawl {
        id: uuid_value(items.first())?,
        url: string_value(items.get(1))?,
    })
}

/// The handler entry point.
///
/// A malformed payload is Django's `ValidationError` outside the task's `try`,
/// so it fails the job (no retry: `policy::DEAD_ON_FAILURE`). A missing
/// `IssueLink` succeeds without writing.
async fn handle(context: &TaskContext, job: JobContext) -> Result<(), HandlerError> {
    let Some(crawl) = decode(&job.payload) else {
        return Err(HandlerError::fatal(
            "invalid crawl_work_item_link_title payload",
        ));
    };
    let policy = policy();
    let metadata = crawl_metadata(&context.http, &policy, &crawl.url).await;

    let found = job
        .client
        .query_opt(
            "SELECT 1 FROM issue_links WHERE id = $1 AND deleted_at IS NULL",
            &[&crawl.id],
        )
        .await
        .map_err(|error| HandlerError::retryable(error.to_string()))?;
    if found.is_none() {
        // `IssueLink.DoesNotExist`: a warning, not an error.
        swallow(
            format!(
                "IssueLink not found for the id {} and the url {}",
                crawl.id, crawl.url
            ),
            TASK,
        );
        return Ok(());
    }
    save_metadata(&job.client, crawl.id, &metadata).await
}

/// `issue_link.metadata = metadata; issue_link.save()`.
async fn save_metadata(client: &Client, id: Uuid, metadata: &Value) -> Result<(), HandlerError> {
    client
        .execute(
            "UPDATE issue_links SET metadata = $2, updated_at = now(), \
                    created_by_id = NULL, updated_by_id = NULL \
             WHERE id = $1",
            &[&id, metadata],
        )
        .await
        .map_err(|error| HandlerError::retryable(error.to_string()))?;
    Ok(())
}

/// `crawl_work_item_link_title_and_favicon`, reduced to the observable result.
async fn crawl_metadata(engine: &HttpEngine, policy: &SsrfPolicy, url: &str) -> Value {
    let headers = OutboundRequest::new();
    let (source, final_url) = match engine
        .fetch(&preset::LINK_TITLE, Method::GET, policy, url, &headers)
        .await
    {
        Ok((response, final_url)) => {
            let text = html::decode(response.bytes());
            (Some(html::scan(&text)), final_url)
        }
        Err(error) => {
            // `except requests.RequestException` / `(ValueError, RuntimeError)`:
            // a warning, then the favicon is still attempted with the original
            // URL as the base.
            tracing::warn!(target: "plane.tasks", task = TASK, error = %error, "link title fetch failed");
            (None, url.to_string())
        }
    };

    let title = source.as_ref().and_then(|source| source.title.clone());
    let (favicon_url, favicon) = fetch_favicon(engine, policy, source.as_ref(), &final_url).await;
    json!({
        "title": title,
        "favicon": favicon,
        "url": url,
        "favicon_url": favicon_url,
    })
}

/// `fetch_and_encode_favicon` (`work_item_link_task.py:201-241`).
async fn fetch_favicon(
    engine: &HttpEngine,
    policy: &SsrfPolicy,
    source: Option<&CrawlSource>,
    base_url: &str,
) -> (Option<String>, String) {
    let Some(favicon_url) = find_favicon_url(engine, policy, source, base_url).await else {
        return (None, format!("data:image/svg+xml;base64,{DEFAULT_FAVICON}"));
    };
    let headers = OutboundRequest::new();
    match engine
        .fetch(
            &preset::FAVICON_GET,
            Method::GET,
            policy,
            &favicon_url,
            &headers,
        )
        .await
    {
        Ok((response, _)) => {
            // `response.headers.get("content-type", "image/x-icon")`.
            let content_type = response.header("content-type").unwrap_or("image/x-icon");
            let encoded = base64::engine::general_purpose::STANDARD.encode(response.bytes());
            (
                Some(favicon_url),
                format!("data:{content_type};base64,{encoded}"),
            )
        }
        Err(error) => {
            tracing::warn!(target: "plane.tasks", task = TASK, error = %error, "favicon fetch failed");
            (None, format!("data:image/svg+xml;base64,{DEFAULT_FAVICON}"))
        }
    }
}

/// `find_favicon_url` (`work_item_link_task.py:161-198`).
async fn find_favicon_url(
    engine: &HttpEngine,
    policy: &SsrfPolicy,
    source: Option<&CrawlSource>,
    base_url: &str,
) -> Option<String> {
    if let Some(source) = source {
        for selector in FAVICON_SELECTORS {
            let Some(link) = html::first_link_with_rel(source, selector) else {
                continue;
            };
            let Some(href) = link.href.as_deref().filter(|href| !href.is_empty()) else {
                continue;
            };
            // `urljoin(base_url, href)`, then `validate_url_ip(favicon_href)`:
            // a `ValueError` aborts the whole helper, so the caller falls back
            // to the default SVG.
            let Ok(favicon_href) = urljoin(base_url, href) else {
                return None;
            };
            // `validate_url_ip` resolves afresh (the later pinned request
            // resolves again too, exactly like `requests` in Python).
            if plane_net::validate_hop(&plane_net::SystemResolver, &favicon_href, policy)
                .await
                .is_err()
            {
                return None;
            }
            return Some(favicon_href);
        }
    }

    // Fallback: `<scheme>://<netloc>/favicon.ico`, HEAD with a two-second
    // timeout. A transport error logs and returns `None`.
    let Ok(parsed) = urlparse(base_url) else {
        return None;
    };
    let fallback_url = format!("{}://{}/favicon.ico", parsed.scheme, parsed.netloc);
    let headers = OutboundRequest::new();
    match engine
        .fetch_once(
            &preset::FAVICON_HEAD,
            Method::HEAD,
            policy,
            &fallback_url,
            &headers,
        )
        .await
    {
        Ok(response) if response.status == 200 => Some(fallback_url),
        Ok(_) => None,
        Err(error) => {
            tracing::warn!(target: "plane.tasks", task = TASK, error = %error, "favicon probe failed");
            None
        }
    }
}

/// The task's SSRF policy: strict. The only exemption is the `difftest`
/// feature's loopback escape, whose environment variable a default build never
/// reads.
fn policy() -> SsrfPolicy {
    policy_with(|name| std::env::var(name).ok())
}

/// [`policy`] with the environment lookup injected, so the test can prove which
/// builds honour the escape without mutating the process environment (edition
/// 2024 makes `std::env::set_var` unsafe, and `unsafe` is forbidden).
#[cfg(feature = "difftest")]
fn policy_with(read: impl Fn(&str) -> Option<String>) -> SsrfPolicy {
    if read("PLANE_RS_DIFFTEST_ALLOW_LOOPBACK").as_deref() == Some("1") {
        SsrfPolicy::new(
            vec![
                "127.0.0.0/8".parse().expect("loopback v4 network"),
                "::1/128".parse().expect("loopback v6 network"),
            ],
            [],
        )
    } else {
        SsrfPolicy::strict()
    }
}

/// A default build ignores `PLANE_RS_DIFFTEST_ALLOW_LOOPBACK` entirely: the
/// SSRF guard has no environment-controlled widening.
#[cfg(not(feature = "difftest"))]
fn policy_with(_read: impl Fn(&str) -> Option<String>) -> SsrfPolicy {
    SsrfPolicy::strict()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(value: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"args": [], "kwargs": value})
    }

    #[test]
    fn the_difftest_override_requires_the_difftest_feature() {
        let widened = policy_with(|_| Some("1".to_string()));
        let loopback: std::net::IpAddr = "127.0.0.1".parse().expect("loopback");
        #[cfg(feature = "difftest")]
        assert!(
            widened.allows_ip(loopback),
            "a difftest build honours the harness's loopback escape"
        );
        #[cfg(not(feature = "difftest"))]
        assert!(
            !widened.allows_ip(loopback),
            "a default build must ignore PLANE_RS_DIFFTEST_ALLOW_LOOPBACK"
        );
    }

    #[test]
    fn reads_the_positional_and_keyword_forms() {
        let id = Uuid::nil();
        let positional = serde_json::json!({
            "args": [id.to_string(), "https://example.com/a"],
            "kwargs": {}
        });
        assert_eq!(
            decode(&positional),
            Some(Crawl {
                id,
                url: "https://example.com/a".to_string()
            })
        );
        assert_eq!(
            decode(&payload(serde_json::json!({
                "id": id.to_string(),
                "url": "https://example.com/b",
            }))),
            Some(Crawl {
                id,
                url: "https://example.com/b".to_string()
            })
        );
        assert_eq!(decode(&payload(serde_json::json!({"id": "bad"}))), None);
    }

    #[test]
    fn the_default_favicon_is_the_svg_constant() {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(DEFAULT_FAVICON)
            .expect("base64");
        let svg = String::from_utf8(decoded).expect("utf8");
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.ends_with("</svg>"));
    }
}
