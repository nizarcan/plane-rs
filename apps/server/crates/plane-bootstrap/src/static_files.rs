// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The eight files of `apps/api/plane/static`, embedded byte for byte.
//!
//! Django serves them through WhiteNoise in production
//! (`plane/settings/common.py:125`): `STATIC_ROOT` is populated by
//! `collectstatic` (`docker-entrypoint-api.sh:36`) and
//! `whitenoise.middleware.WhiteNoiseMiddleware` answers `/static/*` before the
//! view pipeline. After cutover `plane-api` serves the same paths
//! (`plans/CUTOVER.md` §3, `Caddyfile.ce:18`); the distroless image has no
//! files, so the bytes are `include_bytes!` here.
//!
//! `tools/boot/static_files.py --check` keeps `assets/static/` byte-identical to
//! `apps/api/plane/static`.
//!
//! The response headers are WhiteNoise's for a non-hashed, uncompressed file
//! (`whitenoise/base.py:197-236`): `Content-Type` from `mimetypes` (with
//! `; charset=\"utf-8\"` for `text/*`), `Cache-Control: max-age=60, public`,
//! `Access-Control-Allow-Origin: *`, `ETag`, `Last-Modified` and
//! `Content-Length`. None of these eight files gets a gzip/brotli variant:
//! `Compressor.should_compress` skips images and the three text files are empty
//! (`whitenoise/compress.py:81-128`), so there is no `Vary: Accept-Encoding`.
//! WhiteNoise derives `ETag`/`Last-Modified` from the collected file's mtime,
//! which an embedded copy does not have; [`etag`] uses the content hash and
//! [`LAST_MODIFIED`] is fixed. The differential scenario normalises those two
//! headers only.

/// One embedded file.
#[derive(Debug)]
pub struct StaticFile {
    /// The URL path, including `/static/`.
    pub path: &'static str,
    pub content_type: &'static str,
    pub bytes: &'static [u8],
}

/// WhiteNoise's `max_age` with `DEBUG=0` (`whitenoise/middleware.py:44-49`).
pub const CACHE_CONTROL: &str = "max-age=60, public";

/// The collected files' fixed mtime, `2000-01-01T00:00:00Z`. WhiteNoise derives
/// `Last-Modified` and `ETag` from the collected file's mtime
/// (`whitenoise/responders.py:176-196`); `tools/boot/django-static-serve.sh`
/// touches its static root to this value, and the embedded copy uses it too, so
/// `Last-Modified` is compared byte for byte. `ETag` still differs by the host
/// timezone WhiteNoise's `mktime` applies; the scenario normalises that one
/// header.
pub const ETAG_MTIME: u64 = 946_684_800;

/// `email.utils.formatdate(946684800, usegmt=True)`.
pub const LAST_MODIFIED: &str = "Sat, 01 Jan 2000 00:00:00 GMT";

pub const FILES: &[StaticFile] = &[
    StaticFile {
        path: "/static/css/style.css",
        content_type: "text/css; charset=\"utf-8\"",
        bytes: include_bytes!("../assets/static/css/style.css"),
    },
    StaticFile {
        path: "/static/humans.txt",
        content_type: "text/plain; charset=\"utf-8\"",
        bytes: include_bytes!("../assets/static/humans.txt"),
    },
    StaticFile {
        path: "/static/js/script.js",
        content_type: "text/javascript; charset=\"utf-8\"",
        bytes: include_bytes!("../assets/static/js/script.js"),
    },
    StaticFile {
        path: "/static/logos/Logo.png",
        content_type: "image/png",
        bytes: include_bytes!("../assets/static/logos/Logo.png"),
    },
    StaticFile {
        path: "/static/logos/github_32px.png",
        content_type: "image/png",
        bytes: include_bytes!("../assets/static/logos/github_32px.png"),
    },
    StaticFile {
        path: "/static/logos/linkedin_32px.png",
        content_type: "image/png",
        bytes: include_bytes!("../assets/static/logos/linkedin_32px.png"),
    },
    StaticFile {
        path: "/static/logos/twitter_32px.png",
        content_type: "image/png",
        bytes: include_bytes!("../assets/static/logos/twitter_32px.png"),
    },
    StaticFile {
        path: "/static/logos/website_32px.png",
        content_type: "image/png",
        bytes: include_bytes!("../assets/static/logos/website_32px.png"),
    },
];

/// The file for a request path, or `None` (WhiteNoise falls through to Django's
/// URL resolver, which 404s).
#[must_use]
pub fn lookup(path: &str) -> Option<&'static StaticFile> {
    FILES.iter().find(|file| file.path == path)
}

/// A strong content-derived `ETag`, in WhiteNoise's quoted form. Django's is
/// `"{mtime:x}-{size:x}"`; the value is implementation detail, the shape is not.
#[must_use]
pub fn etag(file: &StaticFile) -> String {
    // WhiteNoise's shape: `"{mtime:x}-{size:x}"`.
    format!("\"{:x}-{:x}\"", ETAG_MTIME, file.bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_files_are_embedded_and_looked_up() {
        assert_eq!(FILES.len(), 8);
        let logo = lookup("/static/logos/github_32px.png").expect("github logo");
        assert_eq!(logo.content_type, "image/png");
        assert_eq!(logo.bytes.len(), 1626);
        assert_eq!(
            logo.bytes[..8],
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]
        );
        assert!(lookup("/static/nope.txt").is_none());
    }

    #[test]
    fn etag_has_whitenoise_shape() {
        let file = lookup("/static/logos/github_32px.png").expect("github logo");
        let tag = etag(file);
        assert!(tag.starts_with('"') && tag.ends_with('"'));
        assert!(tag.ends_with("-65a\""), "{tag}");
        assert_eq!(tag, "\"386d4380-65a\"");
    }
}
