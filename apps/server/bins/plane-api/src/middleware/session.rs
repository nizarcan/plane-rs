// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `SessionMiddleware` (`plane/authentication/middleware/session.py`),
//! response side only.
//!
//! Django's session middleware saves or deletes the session row and cookie when
//! the session was modified, was emptied or `SESSION_SAVE_EVERY_REQUEST` is on.
//! None of that can happen until the session store (signed payloads, the
//! `sessions` table) lands in Phase 2. What *is* observable today is the `Vary`
//! header: Django adds `Vary: Cookie` whenever `request.session.accessed` is
//! true (HTTP_CONTRACT §1.1). In Plane almost every request accesses the session
//! because the request logger evaluates `request.user`
//! (`plane/middleware/logger.py:56-58`), which loads the session.
//!
//! Phase 2 adds, in this order, before the `Vary` handling:
//!
//! * admin cookie selection: `"instances" in request.path` (a substring test,
//!   `session.py:22-27,43-44`);
//! * `delete_cookie` when the cookie was sent and the session is empty
//!   (`session.py:60-70`), with the quoted-empty value and no `HttpOnly`;
//! * `save()` + `set_cookie` when `modified || SESSION_SAVE_EVERY_REQUEST`
//!   (`session.py:72-90`), including the admin `Max-Age`
//!   (`ADMIN_SESSION_COOKIE_AGE`) and the `Morsel` attribute order documented in
//!   HTTP_CONTRACT §5.1.

use plane_core::config::Settings;
use plane_core::context::RequestContext;

use crate::response::Resp;

/// `SessionMiddleware.process_response`, restricted to the `Vary` header.
pub fn response(_settings: &Settings, ctx: &RequestContext, mut resp: Resp) -> Resp {
    if ctx.session_accessed {
        resp.patch_vary("Cookie");
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, Method};

    fn load() -> Settings {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("WEB_URL", "http://localhost");
        Settings::from_source(&env, plane_core::config::Role::Api).expect("settings")
    }

    #[test]
    fn vary_cookie_only_when_accessed() {
        let settings = load();
        let ctx = RequestContext::new(Method::GET, "/", "", HeaderMap::new(), None);
        let resp = response(&settings, &ctx, Resp::empty(200, "application/json"));
        assert!(resp.header("vary").is_none());

        let mut accessed = ctx.clone();
        accessed.session_accessed = true;
        let resp = response(&settings, &accessed, Resp::empty(200, "application/json"));
        assert_eq!(resp.header("vary"), Some("Cookie"));

        // It appends to an existing Vary rather than replacing it.
        let mut existing = Resp::empty(200, "application/json");
        existing.patch_vary("origin");
        let resp = response(&settings, &accessed, existing);
        assert_eq!(resp.header("vary"), Some("origin, Cookie"));
    }
}
