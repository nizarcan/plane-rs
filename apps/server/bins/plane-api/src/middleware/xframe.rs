// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `XFrameOptionsMiddleware` (`django/middleware/clickjacking.py`, HTTP_CONTRACT §8.3).
//!
//! `X_FRAME_OPTIONS` is not configured, so the value is `DENY` (upper-cased by
//! the middleware). The header is set on every response that reaches this
//! layer: a response produced by a middleware *above* it (the `ALLOWED_HOSTS`
//! 400 from #4, a CORS preflight from #0) never does. A response created by a
//! middleware *below* it and then replaced above it (an `APPEND_SLASH` 301 made
//! by #4 from a 404) loses the header, because the replacement never passes
//! through this layer again.

use crate::response::Resp;

/// `X_FRAME_OPTIONS`.
const X_FRAME_OPTIONS: &str = "DENY";

/// `XFrameOptionsMiddleware.process_response`.
pub fn response(mut resp: Resp) -> Resp {
    if resp.header("x-frame-options").is_none() {
        resp.set_header("X-Frame-Options", X_FRAME_OPTIONS);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_the_header_unless_present() {
        let resp = response(Resp::empty(200, "application/json"));
        assert_eq!(resp.header("x-frame-options"), Some("DENY"));

        let mut exempt = Resp::empty(200, "application/json");
        exempt.set_header("X-Frame-Options", "SAMEORIGIN");
        assert_eq!(
            response(exempt).header("x-frame-options"),
            Some("SAMEORIGIN")
        );
    }
}
