// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H26 — `Page.save` (`plane/db/models/page.py:70-77`, MODEL_HOOKS §2.11).
//!
//! ```python
//! def save(self, *args, **kwargs):
//!     # Strip the html tags using html parser
//!     self.description_stripped = (
//!         None
//!         if (self.description_html == "" or self.description_html is None)
//!         else strip_tags(self.description_html)
//!     )
//!     super(Page, self).save(*args, **kwargs)
//! ```
//!
//! Two facts a write path must reproduce:
//!
//! * `description_stripped` is recomputed on **every** full save, so a PATCH
//!   that only changes `name` (or the soft delete that re-saves the instance)
//!   re-derives it from the stored `description_html`;
//! * only `""` and `None` collapse to SQL NULL. `"<p></p>"` — the column
//!   default — strips to the empty string, which is written as `''`, not NULL.
//!
//! `strip_tags` here is Plane's `html.parser`-based stripper
//! (`plane/utils/html_processor.py:7-31`, imported at `models/page.py:12`), not
//! `django.utils.html.strip_tags`: character references are decoded and the
//! text of `<script>`/`<style>` survives. [`super::html::strip_tags`] is that
//! port, shared with H23 (sticky) and the description pipeline.

/// `Page.save()`'s `description_stripped` assignment.
///
/// The `Option` models Python's `description_html is None` arm: the column is
/// `NOT NULL` (`plane/db/models/page.py:34`), so such an instance only exists
/// in memory and its INSERT fails afterwards, but the hook is total.
#[must_use]
pub fn before_save(description_html: Option<&str>) -> Option<String> {
    match description_html {
        None | Some("") => None,
        Some(html) => Some(super::html::strip_tags(html)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected values recorded from
    /// `plane.utils.html_processor.strip_tags` (CPython 3.12.13): tags,
    /// comments, entities (decoded), whitespace (kept verbatim) and the
    /// `""`/`None` NULL rule. `django.utils.html.strip_tags` differs on
    /// entities (it keeps `&amp;` literal), which is why the model's own
    /// import matters.
    #[test]
    fn the_null_rule_matches_page_save() {
        assert_eq!(before_save(None), None);
        assert_eq!(before_save(Some("")), None);
        // The default is neither `""` nor `None`, so it strips to `""`.
        assert_eq!(before_save(Some("<p></p>")), Some(String::new()));
    }

    #[test]
    fn plain_html_is_stripped() {
        assert_eq!(before_save(Some("plain")), Some("plain".to_owned()));
        assert_eq!(
            before_save(Some("<p>Hello <b>world</b></p>")),
            Some("Hello world".to_owned())
        );
        assert_eq!(
            before_save(Some("<p>one</p> <p>two</p>")),
            Some("one two".to_owned())
        );
        // `<script>` content is data in CPython's CDATA mode.
        assert_eq!(
            before_save(Some("<script>alert('x')</script>")),
            Some("alert('x')".to_owned())
        );
        // A `<` that does not open markup is text.
        assert_eq!(before_save(Some("a < b")), Some("a < b".to_owned()));
    }

    #[test]
    fn whitespace_is_kept_verbatim() {
        assert_eq!(
            before_save(Some("<div>  spaced   text  </div>")),
            Some("  spaced   text  ".to_owned())
        );
        assert_eq!(
            before_save(Some("<p>x</p>\n<p>y</p>")),
            Some("x\ny".to_owned())
        );
        assert_eq!(before_save(Some("  ")), Some("  ".to_owned()));
        assert_eq!(before_save(Some("\n\t")), Some("\n\t".to_owned()));
    }

    #[test]
    fn character_references_are_decoded() {
        assert_eq!(
            before_save(Some("<p>a &amp; b</p>")),
            Some("a & b".to_owned())
        );
        assert_eq!(
            before_save(Some("<p>a &nbsp; b</p>")),
            Some("a \u{a0} b".to_owned())
        );
        assert_eq!(
            before_save(Some("<p>&lt;tag&gt; &amp; &#39;quotes&#39;</p>")),
            Some("<tag> & 'quotes'".to_owned())
        );
        assert_eq!(
            before_save(Some("<p>&copy; &mdash; caf\u{e9}</p>")),
            Some("\u{a9} \u{2014} caf\u{e9}".to_owned())
        );
        // The longest named prefix wins; an unknown reference stays literal.
        assert_eq!(
            before_save(Some("<p>&notit; &unknown;</p>")),
            Some("\u{ac}it; &unknown;".to_owned())
        );
    }
}
