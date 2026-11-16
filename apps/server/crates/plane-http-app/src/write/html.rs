// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! HTML on the write path: `strip_tags` for `description_stripped` and the
//! nh3 sanitizer `validate_html_content` runs on `description_html`.
//!
//! `strip_tags` itself lives in `plane_db::hooks::html` (the H23/H12/H26
//! hooks need it and `plane-db` cannot depend on this crate); this module
//! re-exports it and owns the sanitizer, which needs `ammonia`.
//!
//! ## Sanitisation
//!
//! `plane/utils/content_validator.py:209-239` calls `nh3.clean` with
//! `ALLOWED_TAGS` (ammonia's defaults plus four editor tags), the `ATTRIBUTES`
//! map and `SAFE_PROTOCOLS`. nh3 0.2.18 is ammonia **4.0.0**
//! (`nh3-0.2.18/Cargo.lock`), so this module builds the same
//! `ammonia::Builder` and pins the crate to `=4.0.0`
//! (`apps/server/Cargo.toml`). `tests` below compare against a corpus recorded
//! from the real `nh3`.

use std::collections::HashSet;

pub use plane_db::hooks::html::strip_tags;

/// nh3's `ALLOWED_TAGS = nh3.ALLOWED_TAGS | CUSTOM_TAGS`
/// (`content_validator.py:44-51`).
const CUSTOM_TAGS: [&str; 4] = ["mention-component", "label", "input", "image-component"];

/// `ATTRIBUTES["*"]` (`content_validator.py:54-83`).
const GENERIC_ATTRIBUTES: [&str; 25] = [
    "class",
    "id",
    "title",
    "role",
    "aria-label",
    "aria-hidden",
    "style",
    "start",
    "type",
    "xmlns",
    "data-tight",
    "data-node-type",
    "data-type",
    "data-checked",
    "data-background-color",
    "data-text-color",
    "data-name",
    "data-id",
    "data-icon-name",
    "data-icon-color",
    "data-background",
    "data-emoji-unicode",
    "data-emoji-url",
    "data-logo-in-use",
    "data-block-type",
];

/// The per-tag attribute lists (`content_validator.py:84-118`).
const TAG_ATTRIBUTES: [(&str, &[&str]); 10] = [
    ("a", &["href", "target"]),
    (
        "image-component",
        &[
            "id",
            "width",
            "height",
            "aspectRatio",
            "aspectratio",
            "src",
            "alignment",
            "status",
        ],
    ),
    (
        "img",
        &[
            "width",
            "height",
            "aspectRatio",
            "aspectratio",
            "alignment",
            "src",
            "alt",
            "title",
        ],
    ),
    (
        "mention-component",
        &["id", "entity_identifier", "entity_name"],
    ),
    (
        "th",
        &["colspan", "rowspan", "colwidth", "background", "style"],
    ),
    (
        "td",
        &[
            "colspan",
            "rowspan",
            "colwidth",
            "background",
            "textColor",
            "textcolor",
            "style",
        ],
    ),
    ("tr", &["background", "textColor", "textcolor", "style"]),
    ("pre", &["language"]),
    ("code", &["language", "spellcheck"]),
    ("input", &["type", "checked"]),
];

/// `SAFE_PROTOCOLS` (`content_validator.py:121`).
const SAFE_PROTOCOLS: [&str; 4] = ["http", "https", "mailto", "tel"];

/// `validate_html_content`'s success value: the sanitized HTML
/// (`content_validator.py:228-239`).
///
/// A 10 MB input is refused by the caller before this runs (the serializer's
/// `validate()` reads the same constant), so the function is infallible.
#[must_use]
pub fn sanitize(html: &str) -> String {
    let mut builder = ammonia::Builder::default();
    // nh3 passes each set wholesale, and ammonias' setters *replace* the
    // defaults, so the Python maps are reproduced rather than merged.
    builder
        .add_tags(CUSTOM_TAGS)
        .generic_attributes(GENERIC_ATTRIBUTES.into_iter().collect::<HashSet<_>>())
        .url_schemes(SAFE_PROTOCOLS.into_iter().collect::<HashSet<_>>());
    builder.tag_attributes(
        TAG_ATTRIBUTES
            .into_iter()
            .map(|(tag, attributes)| (tag, attributes.iter().copied().collect::<HashSet<_>>()))
            .collect(),
    );
    builder.clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_matches_nh3_corpus() {
        // Recorded from `plane.utils.content_validator.validate_html_content`
        // with nh3 0.2.18 (ammonia 4.0.0).
        let cases = [
            ("<p>hello</p>", "<p>hello</p>"),
            ("<script>alert(1)</script>", ""),
            ("<p onclick='x'>a</p>", "<p>a</p>"),
            (
                "<a href='javascript:alert(1)'>x</a>",
                "<a rel=\"noopener noreferrer\">x</a>",
            ),
            (
                "<a href='https://e.test/x?y=1&amp;z=2'>x</a>",
                "<a href=\"https://e.test/x?y=1&amp;z=2\" rel=\"noopener noreferrer\">x</a>",
            ),
            (
                "<img src='/api/assets/v2/static/x/' width='10'>",
                "<img src=\"/api/assets/v2/static/x/\" width=\"10\">",
            ),
            ("<iframe src='x'></iframe>", ""),
            ("<p>a &amp; b</p>", "<p>a &amp; b</p>"),
            (
                "<mention-component id='1' entity_name='user'></mention-component>",
                "<mention-component id=\"1\" entity_name=\"user\"></mention-component>",
            ),
            ("<p style='color:red'>x</p>", "<p style=\"color:red\">x</p>"),
            (
                "<pre language='js'><code>a</code></pre>",
                "<pre language=\"js\"><code>a</code></pre>",
            ),
            (
                "<div data-tight='true'>x</div>",
                "<div data-tight=\"true\">x</div>",
            ),
            ("<p>line1\nline2</p>", "<p>line1\nline2</p>"),
        ];
        for (input, expected) in cases {
            assert_eq!(sanitize(input), expected, "input {input:?}");
        }
    }
}
