// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! One entry per outbound HTTP call site in `apps/api/plane`, with the settings
//! the Python code actually uses. Port these values, do not re-invent them: a
//! webhook receiver sees the timeout, the `User-Agent` and whether redirects
//! are followed.
//!
//! | Preset | Python | Method | Timeout | Redirects | SSRF | Proxy |
//! | --- | --- | --- | --- | --- | --- | --- |
//! | `WEBHOOK` | `bgtasks/webhook_task.py:317` | POST | 30 s | never | on, pinning | off |
//! | `LINK_TITLE` | `bgtasks/work_item_link_task.py:126` | GET | 1 s | 5, re-validated | on, pinning | off |
//! | `FAVICON_HEAD` | `bgtasks/work_item_link_task.py:194` | HEAD | 2 s | never | on, pinning | off |
//! | `FAVICON_GET` | `bgtasks/work_item_link_task.py:226` | GET | 1 s | 5, re-validated | on, pinning | off |
//! | `OAUTH_AVATAR` | `authentication/adapter/base.py:156` | GET | 10 s | 5, re-validated | on, pinning, 5 MiB cap | off |
//! | `OAUTH_TOKEN` | `authentication/adapter/oauth.py:78` | POST | none | 30 | off | env |
//! | `OAUTH_USERINFO` | `authentication/adapter/oauth.py:89` | GET | none | 30 | off | env |
//! | `GITHUB_EMAILS` | `authentication/provider/oauth/github.py:112` | GET | none | 30 | off | env |
//! | `GITHUB_ORG_MEMBERSHIP` | `authentication/provider/oauth/github.py:144` | GET | none | 30 | off | env |
//! | `GITEA_EMAILS` | `authentication/provider/oauth/gitea.py:120` | GET | none | 30 | off | env |
//! | `GITLAB_*` | `authentication/provider/oauth/gitlab.py` | GET | none | 30 | off | env |
//! | `UNSPLASH` | `app/views/external/base.py:242` | GET | none | 30 | off | env |
//! | `GITHUB_RELEASES` | `license/management/commands/register_instance.py:42` | GET | 10 s | 30 | off | env |
//! | `LIVE_CONVERT` | `bgtasks/copy_s3_object.py:82` | POST | none | 30 | off | env |
//! | `LLM` | `app/views/external/base.py:131` (OpenAI SDK) | POST | 600 s | SDK | off | env |
//!
//! Notes on individual rows:
//!
//! * **Timeouts.** `requests`' single `timeout=` value bounds *each socket
//!   operation* (connect and every read), not the whole request; a server that
//!   trickles one byte a second never trips it. The client therefore maps it
//!   to `connect_timeout` + `read_timeout`, never to reqwest's total
//!   `timeout`. The link crawler really does use **one second**
//!   (`safe_get`'s default, `work_item_link_task.py:77`), which is why unfurling
//!   of slow sites yields no title in Python.
//! * **Redirects.** The webhook never follows (`allow_redirects=False`): a 3xx
//!   is delivered to the receiver's log as the response. The link crawler,
//!   favicon and avatar paths follow at most five hops, re-resolving,
//!   re-validating and re-pinning each one; `requests`' own limit for the
//!   unguarded call sites is 30.
//! * **Proxies.** The pinned client sets `session.trust_env = False` and
//!   `proxies={"http": None, "https": None}` on purpose: a CONNECT tunnel to a
//!   proxy would resolve the *hostname* at the proxy and bypass IP pinning
//!   entirely (`url_security.py:41`). Unguarded call sites keep `requests`'
//!   default behaviour, which reads `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`
//!   and `NO_PROXY`.
//! * **User-Agent.** `requests` sends `python-requests/2.33.0` when a call site
//!   does not set one. The vendored OAuth code does not, so a Gitea/GitLab
//!   token endpoint sees that string today; the presets reproduce it. Bump it
//!   when `apps/api/base.txt` moves.
//! * **Body limits.** Only the avatar download caps the body
//!   (`DATA_UPLOAD_MAX_MEMORY_SIZE`), because it reads incrementally. Webhook
//!   delivery and link unfurling read the **whole** body with no limit in
//!   Python; those presets keep `max_body_bytes = None` so behaviour does not
//!   change. Callers that want a cap can set it on the request.
//! * **gzip.** `requests` advertises `gzip, deflate` and decompresses; the
//!   presets send the same header and reqwest decodes it.
//! * **`UNSPLASH`** sends `Content-Type: application/json` on a *GET*; that is
//!   what `app/views/external/base.py:240` does.
//! * **`LLM`** goes through the `openai` SDK (httpx): `connect=5 s`,
//!   `read/write/pool=600 s`, two retries, environment proxies. The preset keeps
//!   the 600 s budget and documents that retries live in the future handler.

use std::time::Duration;

/// How a preset handles `HTTP(S)_PROXY` / `NO_PROXY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    /// `requests`' default: environment proxies, `NO_PROXY` honoured.
    Environment,
    /// `no_proxy()`: never tunnel, because a tunnel resolves the host at the
    /// proxy and defeats IP pinning.
    Disabled,
}

/// `requests.utils.default_headers()` for the pinned requests version.
pub const REQUESTS_USER_AGENT: &str = "python-requests/2.33.0";

/// The browser UA the link crawler and favicon fetcher send.
pub const CRAWLER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";

/// `requests.sessions.DEFAULT_REDIRECT_LIMIT`.
pub const REQUESTS_REDIRECT_LIMIT: u32 = 30;

/// `settings.DATA_UPLOAD_MAX_MEMORY_SIZE`'s default (`FILE_SIZE_LIMIT`).
pub const DEFAULT_UPLOAD_LIMIT: usize = 5 * 1024 * 1024;

/// A call site's transport policy.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    /// Stable identifier for logs and tests.
    pub name: &'static str,
    /// `apps/api/...:line` the values were read from.
    pub source: &'static str,
    /// Whether the target is resolved, validated and pinned.
    pub ssrf: bool,
    /// Connect *and* per-read timeout, when Python passes one.
    pub timeout: Option<Duration>,
    /// `Some(n)` follows up to `n` redirects, re-validating each hop;
    /// `None` returns a 3xx to the caller (Python's `allow_redirects=False`).
    pub max_redirects: Option<u32>,
    /// Proxy behaviour.
    pub proxy: ProxyMode,
    /// `User-Agent` sent even when the caller sets none.
    pub user_agent: &'static str,
    /// Headers the call site adds on every request.
    pub headers: &'static [(&'static str, &'static str)],
    /// Body cap. `None` reads everything, as Python does.
    pub max_body_bytes: Option<usize>,
}

impl Preset {
    pub fn follows_redirects(&self) -> bool {
        self.max_redirects.is_some()
    }
}

#[allow(clippy::too_many_arguments)] // one argument per field of the table
const fn preset(
    name: &'static str,
    source: &'static str,
    ssrf: bool,
    timeout: Option<u64>,
    max_redirects: Option<u32>,
    proxy: ProxyMode,
    user_agent: &'static str,
    headers: &'static [(&'static str, &'static str)],
    max_body_bytes: Option<usize>,
) -> Preset {
    Preset {
        name,
        source,
        ssrf,
        timeout: match timeout {
            Some(secs) => Some(Duration::from_secs(secs)),
            None => None,
        },
        max_redirects,
        proxy,
        user_agent,
        headers,
        max_body_bytes,
    }
}

/// `pinned_fetch("POST", webhook.url, ..., timeout=30)`.
pub const WEBHOOK: Preset = preset(
    "webhook",
    "apps/api/plane/bgtasks/webhook_task.py:317",
    true,
    Some(30),
    None,
    ProxyMode::Disabled,
    "Autopilot",
    &[],
    None,
);

/// `safe_get(url, headers=...)` with its default `timeout=1`.
pub const LINK_TITLE: Preset = preset(
    "link-title",
    "apps/api/plane/bgtasks/work_item_link_task.py:126",
    true,
    Some(1),
    Some(5),
    ProxyMode::Disabled,
    CRAWLER_USER_AGENT,
    &[],
    None,
);

/// `pinned_fetch("HEAD", fallback_url, timeout=2)` for `/favicon.ico`.
pub const FAVICON_HEAD: Preset = preset(
    "favicon-head",
    "apps/api/plane/bgtasks/work_item_link_task.py:194",
    true,
    Some(2),
    None,
    ProxyMode::Disabled,
    CRAWLER_USER_AGENT,
    &[],
    None,
);

/// `safe_get(favicon_url, headers=headers)`: one-second timeout, five hops.
pub const FAVICON_GET: Preset = preset(
    "favicon-get",
    "apps/api/plane/bgtasks/work_item_link_task.py:226",
    true,
    Some(1),
    Some(5),
    ProxyMode::Disabled,
    CRAWLER_USER_AGENT,
    &[],
    None,
);

/// `pinned_fetch_following_redirects("GET", avatar_url, ..., timeout=10,
/// max_redirects=5, stream=True)` plus the `DATA_UPLOAD_MAX_MEMORY_SIZE` cap.
pub const OAUTH_AVATAR: Preset = preset(
    "oauth-avatar",
    "apps/api/plane/authentication/adapter/base.py:156",
    true,
    Some(10),
    Some(5),
    ProxyMode::Disabled,
    REQUESTS_USER_AGENT,
    &[],
    Some(DEFAULT_UPLOAD_LIMIT),
);

/// `requests.post(token_url, data=..., headers=...)`: no timeout.
pub const OAUTH_TOKEN: Preset = preset(
    "oauth-token",
    "apps/api/plane/authentication/adapter/oauth.py:78",
    false,
    None,
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

/// `requests.get(userinfo_url, headers=...)`: no timeout.
pub const OAUTH_USERINFO: Preset = preset(
    "oauth-userinfo",
    "apps/api/plane/authentication/adapter/oauth.py:89",
    false,
    None,
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

/// GitHub's `/user/emails` call (no timeout).
pub const GITHUB_EMAILS: Preset = preset(
    "github-emails",
    "apps/api/plane/authentication/provider/oauth/github.py:112",
    false,
    None,
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

/// GitHub's organisation membership check (no timeout).
pub const GITHUB_ORG_MEMBERSHIP: Preset = preset(
    "github-org-membership",
    "apps/api/plane/authentication/provider/oauth/github.py:144",
    false,
    None,
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

/// Gitea's `/emails` call (no timeout).
pub const GITEA_EMAILS: Preset = preset(
    "gitea-emails",
    "apps/api/plane/authentication/provider/oauth/gitea.py:120",
    false,
    None,
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

/// GitLab's user/email calls (no timeout; the provider reuses
/// `OauthAdapter.get_user_token`/`get_user_response`).
pub const GITLAB_OAUTH: Preset = preset(
    "gitlab-oauth",
    "apps/api/plane/authentication/provider/oauth/gitlab.py",
    false,
    None,
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

/// `requests.get(url, headers={"Content-Type": "application/json"})`.
pub const UNSPLASH: Preset = preset(
    "unsplash",
    "apps/api/plane/app/views/external/base.py:242",
    false,
    None,
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[("Content-Type", "application/json")],
    None,
);

/// `requests.get(..., timeout=10)` for the latest release tag.
pub const GITHUB_RELEASES: Preset = preset(
    "github-releases",
    "apps/api/plane/license/management/commands/register_instance.py:42",
    false,
    Some(10),
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

/// `requests.post(LIVE_URL + "/convert-document/", json=...)`; the URL comes
/// from deployment configuration, not from a user, so it is not SSRF-guarded.
pub const LIVE_CONVERT: Preset = preset(
    "live-convert",
    "apps/api/plane/bgtasks/copy_s3_object.py:82",
    false,
    None,
    Some(REQUESTS_REDIRECT_LIMIT),
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

/// The `openai` SDK's default timeout budget (`Timeout(connect=5.0,
/// read=600, write=600, pool=600)` in `openai._constants.DEFAULT_TIMEOUT`,
/// pinned at 1.63.2). Retries (2) belong to the future handler.
pub const LLM: Preset = preset(
    "llm",
    "apps/api/plane/app/views/external/base.py:131",
    false,
    Some(600),
    None,
    ProxyMode::Environment,
    REQUESTS_USER_AGENT,
    &[],
    None,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_presets_never_use_an_environment_proxy() {
        for preset in [WEBHOOK, LINK_TITLE, FAVICON_HEAD, FAVICON_GET, OAUTH_AVATAR] {
            assert!(preset.ssrf, "{}", preset.name);
            assert_eq!(preset.proxy, ProxyMode::Disabled, "{}", preset.name);
        }
        for preset in [
            OAUTH_TOKEN,
            OAUTH_USERINFO,
            GITHUB_EMAILS,
            GITHUB_ORG_MEMBERSHIP,
            GITEA_EMAILS,
            GITLAB_OAUTH,
            UNSPLASH,
            GITHUB_RELEASES,
            LIVE_CONVERT,
            LLM,
        ] {
            assert!(!preset.ssrf, "{}", preset.name);
            assert_eq!(preset.proxy, ProxyMode::Environment, "{}", preset.name);
        }
    }

    #[test]
    fn webhook_does_not_follow_redirects() {
        assert!(!WEBHOOK.follows_redirects());
        assert_eq!(WEBHOOK.timeout, Some(Duration::from_secs(30)));
        assert_eq!(WEBHOOK.user_agent, "Autopilot");
    }
}
