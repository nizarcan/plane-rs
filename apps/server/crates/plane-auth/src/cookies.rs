// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `django.http.response.HttpResponse.set_cookie` / `delete_cookie`, byte for
//! byte.
//!
//! Django builds each cookie with `http.cookies.SimpleCookie`; the header value
//! is `Morsel.OutputString()` (`CPython Lib/http/cookies.py`):
//!
//! * the value is quoted only when it is not "legal" (`[A-Za-z0-9!#$%&'*+\-.^_`
//!   `|~:]+`), which is why an emptied cookie is `name=""`;
//! * attributes are emitted in **sorted attribute-name order**:
//!   `comment, domain, expires, httponly, max-age, path, samesite, secure`;
//! * `secure`/`httponly` print as bare flags, `expires`/`max-age` as
//!   `expires=…`/`Max-Age=…`, and an empty attribute value is skipped.
//!
//! `set_cookie(max_age=…)` derives `expires` from `time.time() + max_age` when
//! the caller did not pass one (`response.py:262-268`); the format is
//! `http_date` = `email.utils.formatdate(…, usegmt=True)`
//! (`django/utils/http.py:85-96`).

use chrono::{DateTime, Utc};
use plane_core::config::Settings;

/// One cookie, with the attributes `set_cookie` accepts.
#[derive(Debug, Clone)]
pub struct Cookie<'a> {
    pub name: &'a str,
    pub value: &'a str,
    pub max_age: Option<i64>,
    pub expires: Option<&'a str>,
    pub path: Option<&'a str>,
    pub domain: Option<&'a str>,
    pub secure: bool,
    pub httponly: bool,
    pub samesite: Option<&'a str>,
}

impl Cookie<'_> {
    /// `Morsel.OutputString()`.
    pub fn to_header(&self) -> String {
        // `sorted(self.items())` over the lower-case attribute keys:
        // `domain`, `expires`, `httponly`, `max-age`, `path`, `samesite`,
        // `secure`. `Morsel` stores flags as `True`, and `OutputString` prints
        // them bare.
        let mut attributes: Vec<(&'static str, String)> = Vec::new();
        if let Some(domain) = self.domain.filter(|value| !value.is_empty()) {
            attributes.push(("domain", format!("Domain={domain}")));
        }
        if let Some(expires) = self.expires.filter(|value| !value.is_empty()) {
            attributes.push(("expires", format!("expires={expires}")));
        }
        if self.httponly {
            attributes.push(("httponly", "HttpOnly".to_string()));
        }
        if let Some(max_age) = self.max_age {
            attributes.push(("max-age", format!("Max-Age={max_age}")));
        }
        if let Some(path) = self.path.filter(|value| !value.is_empty()) {
            attributes.push(("path", format!("Path={path}")));
        }
        if let Some(samesite) = self.samesite.filter(|value| !value.is_empty()) {
            attributes.push(("samesite", format!("SameSite={samesite}")));
        }
        if self.secure {
            attributes.push(("secure", "Secure".to_string()));
        }
        attributes.sort_by_key(|(name, _)| *name);

        let mut header = String::with_capacity(64);
        header.push_str(self.name);
        header.push('=');
        header.push_str(&quote_cookie_value(self.value));
        for (_, rendered) in attributes {
            header.push_str("; ");
            header.push_str(&rendered);
        }
        header
    }
}

/// `http.cookies._is_legal_key`: `[A-Za-z0-9!#$%&'*+\-.^_`|~:]+` fullmatch.
fn is_legal_cookie_value(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(
                    character,
                    '!' | '#'
                        | '$'
                        | '%'
                        | '&'
                        | '\''
                        | '*'
                        | '+'
                        | '-'
                        | '.'
                        | '^'
                        | '_'
                        | '`'
                        | '|'
                        | '~'
                        | ':'
                )
        })
}

/// `http.cookies._quote`: wrap in double quotes with `\` escapes when the value
/// is not legal. An empty value becomes `""`.
fn quote_cookie_value(value: &str) -> String {
    if is_legal_cookie_value(value) {
        return value.to_string();
    }
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// `django.utils.http.http_date`: RFC 5322 in UTC with a literal `GMT`.
pub fn http_date(epoch_seconds: i64) -> String {
    let value = DateTime::<Utc>::from_timestamp(epoch_seconds, 0)
        .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_default());
    // chrono's `%a`/`%b` are English unless the `unstable-locales` feature is
    // enabled, and it is not.
    value.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// `HttpResponse.set_cookie(key, value, max_age=…, expires=…, path=…,
/// domain=…, secure=…, httponly=…, samesite=…)`.
///
/// The argument list is Django's, deliberately: keeping the call sites
/// one-to-one with `response.py:218-280` is worth more than a builder here.
#[allow(clippy::too_many_arguments)]
pub fn set_cookie(
    name: &str,
    value: &str,
    max_age: Option<i64>,
    expires: Option<String>,
    path: Option<&str>,
    domain: Option<&str>,
    secure: bool,
    httponly: bool,
    samesite: Option<&str>,
) -> String {
    // `if max_age is not None: … if not expires: expires = http_date(now+age)`.
    let expires = match (expires, max_age) {
        (Some(expires), _) => Some(expires),
        (None, Some(max_age)) => Some(http_date(Utc::now().timestamp() + max_age)),
        (None, None) => None,
    };
    Cookie {
        name,
        value,
        max_age,
        expires: expires.as_deref(),
        path,
        domain,
        secure,
        httponly,
        samesite,
    }
    .to_header()
}

/// `HttpResponse.delete_cookie`: value `""`, `Max-Age=0`, the literal epoch
/// `Expires`, no `HttpOnly` and no `Secure` (unless the name is `__Host-` /
/// `__Secure-`, or SameSite=None).
pub fn delete_cookie(
    name: &str,
    path: Option<&str>,
    domain: Option<&str>,
    samesite: Option<&str>,
) -> String {
    let secure = name.starts_with("__Secure-")
        || name.starts_with("__Host-")
        || samesite.is_some_and(|value| value.eq_ignore_ascii_case("none"));
    Cookie {
        name,
        value: "",
        max_age: Some(0),
        expires: Some("Thu, 01 Jan 1970 00:00:00 GMT"),
        path,
        domain,
        secure,
        httponly: false,
        samesite,
    }
    .to_header()
}

/// `SessionMiddleware`'s `response.set_cookie(cookie_name, session_key, …)`
/// (`plane/authentication/middleware/session.py:82-91`).
pub fn session_cookie(
    settings: &Settings,
    name: &str,
    value: &str,
    max_age: Option<i64>,
    expires: Option<String>,
) -> String {
    set_cookie(
        name,
        value,
        max_age,
        expires,
        Some(settings.cookies.session_cookie_path),
        settings.cookies.session_cookie_domain.as_deref(),
        settings.cookies.session_cookie_secure,
        settings.cookies.session_cookie_httponly,
        Some(settings.cookies.session_cookie_samesite),
    )
}

/// The delete branch of the session middleware (`session.py:60-70`): no
/// `httponly`, no `secure` (unless the name is a `__Secure-` one).
pub fn session_delete_cookie(settings: &Settings, name: &str) -> String {
    delete_cookie(
        name,
        Some(settings.cookies.session_cookie_path),
        settings.cookies.session_cookie_domain.as_deref(),
        Some(settings.cookies.session_cookie_samesite),
    )
}

/// `CsrfViewMiddleware._set_csrf_cookie` (`django/middleware/csrf.py:256-269`).
pub fn csrf_cookie(settings: &Settings, secret: &str) -> String {
    set_cookie(
        settings.cookies.csrf_cookie_name,
        secret,
        Some(settings.cookies.csrf_cookie_age),
        None,
        Some(settings.cookies.csrf_cookie_path),
        settings.cookies.csrf_cookie_domain.as_deref(),
        settings.cookies.csrf_cookie_secure,
        settings.cookies.csrf_cookie_httponly,
        Some(settings.cookies.csrf_cookie_samesite),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // HTTP_CONTRACT §5.1 and §A.2: the observed Set-Cookie values.
    #[test]
    fn session_cookie_formats() {
        let env = plane_core::config::EnvMap::new()
            .set("REDIS_URL", "redis://127.0.0.1:6379/0")
            .set("WEB_URL", "http://127.0.0.1:18080");
        let settings =
            Settings::from_source(&env, plane_core::config::Role::Api).expect("settings");
        let key = "a".repeat(128);
        let header = session_cookie(
            &settings,
            "session-id",
            &key,
            Some(604800),
            Some("Sun, 22 Nov 2026 17:26:57 GMT".into()),
        );
        assert_eq!(
            header,
            format!(
                "session-id={key}; expires=Sun, 22 Nov 2026 17:26:57 GMT; HttpOnly; Max-Age=604800; Path=/; SameSite=Lax"
            )
        );

        let admin = session_cookie(
            &settings,
            "admin-session-id",
            &key,
            Some(3600),
            Some("Sun, 15 Nov 2026 18:26:57 GMT".into()),
        );
        assert_eq!(
            admin,
            format!(
                "admin-session-id={key}; expires=Sun, 15 Nov 2026 18:26:57 GMT; HttpOnly; Max-Age=3600; Path=/; SameSite=Lax"
            )
        );

        // Browser-close: no Max-Age, no expires.
        let browser_close = session_cookie(&settings, "session-id", &key, None, None);
        assert_eq!(
            browser_close,
            format!("session-id={key}; HttpOnly; Path=/; SameSite=Lax")
        );
    }

    #[test]
    fn delete_cookie_format() {
        assert_eq!(
            delete_cookie("session-id", Some("/"), None, Some("Lax")),
            "session-id=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; SameSite=Lax"
        );
        // A __Secure- name is deleted with Secure so browsers honour it.
        assert_eq!(
            delete_cookie("__Secure-session", Some("/"), None, None),
            "__Secure-session=\"\"; expires=Thu, 01 Jan 1970 00:00:00 GMT; Max-Age=0; Path=/; Secure"
        );
    }

    #[test]
    fn domain_and_secure_ordering() {
        let header = set_cookie(
            "csrftoken",
            "SECRET",
            Some(31449600),
            None,
            Some("/"),
            Some(".plane.so"),
            true,
            true,
            Some("Lax"),
        );
        let parts: Vec<&str> = header.split("; ").collect();
        assert_eq!(parts[0], "csrftoken=SECRET");
        assert_eq!(parts[1], "Domain=.plane.so");
        assert!(parts[2].starts_with("expires="));
        assert_eq!(parts[3], "HttpOnly");
        assert_eq!(parts[4], "Max-Age=31449600");
        assert_eq!(parts[5], "Path=/");
        assert_eq!(parts[6], "SameSite=Lax");
        assert_eq!(parts[7], "Secure");
    }

    #[test]
    fn http_date_is_rfc_5322_utc() {
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        assert_eq!(http_date(1_777_777_777), "Sun, 03 May 2026 03:09:37 GMT");
        assert_eq!(http_date(1_781_000_000), "Tue, 09 Jun 2026 10:13:20 GMT");
    }

    #[test]
    fn empty_value_is_quoted() {
        assert_eq!(quote_cookie_value(""), "\"\"");
        assert_eq!(quote_cookie_value("abc123"), "abc123");
        assert_eq!(quote_cookie_value("a b"), "\"a b\"");
    }
}
