// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The slice of `bs4.BeautifulSoup(html, "html.parser")` that
//! `plane/bgtasks/work_item_link_task.py:113-241` uses.
//!
//! The link crawler needs exactly two things from a page: the first `<title>`'s
//! text and the `<link>` elements' `rel`/`href`. This module is a small
//! HTML parser (not a DOM) that reproduces the parts of `html.parser` those
//! reads depend on:
//!
//! * comments, doctypes and processing instructions are skipped;
//! * `<script>` and `<style>` are raw-text elements, so their contents are not
//!   parsed (as in `html.parser`, whose `CDATA_CONTENT_ELEMENTS` are those);
//! * `<title>` is RCDATA: everything up to `</title>` is text (so a nested
//!   `<b>` stays literal, which `title_tag.get_text()` returns verbatim) and
//!   character references are decoded;
//! * attribute names are lower-cased, values are entity-decoded, and both
//!   quoted forms and unquoted values are accepted;
//! * a missing end tag reads to EOF, like `html.parser`.
//!
//! `bs4` decodes the response bytes with `UnicodeDammit`; this port handles a
//! UTF-8 BOM and UTF-8, falling back to ISO-8859-1 (the same one-byte-per-char
//! fallback `requests`' `response.text` uses). A page that declares another
//! charset in `<meta>` would be decoded differently; recorded pages in the
//! tests are UTF-8.
//!
//! Character references: numeric forms plus the common named set. An unknown
//! name is left as written, as `html.parser` does.

/// One `<link>` element, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The raw `rel` attribute, for example `"shortcut icon"`.
    pub rel: Option<String>,
    pub href: Option<String>,
}

/// What the crawler reads from a document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrawlSource {
    /// The first `<title>`'s text, stripped. `None` when the document has no
    /// `<title>` element at all (an empty one yields `Some("")`).
    pub title: Option<String>,
    pub links: Vec<Link>,
}

/// Decode response bytes the way `UnicodeDammit` does for the common cases.
#[must_use]
pub fn decode(bytes: &[u8]) -> String {
    let without_bom = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    match std::str::from_utf8(without_bom) {
        Ok(text) => text.to_string(),
        // `requests` and `UnicodeDammit` fall back to a single-byte codec;
        // ISO-8859-1 never fails and keeps every byte.
        Err(_) => without_bom.iter().map(|byte| char::from(*byte)).collect(),
    }
}

/// Parse the document and return the title and links.
#[must_use]
pub fn scan(html: &str) -> CrawlSource {
    let bytes = html.as_bytes();
    let mut source = CrawlSource::default();
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(open) = find_byte(bytes, index, b'<') else {
            break;
        };
        if bytes[open..].starts_with(b"<!--") {
            index = match find_bytes(bytes, open + 4, b"-->") {
                Some(end) => end + 3,
                None => bytes.len(),
            };
            continue;
        }
        if bytes[open..].starts_with(b"<!") || bytes[open..].starts_with(b"<?") {
            index = match find_byte(bytes, open + 2, b'>') {
                Some(end) => end + 1,
                None => bytes.len(),
            };
            continue;
        }
        let closing = bytes[open..].starts_with(b"</");
        let name_start = open + if closing { 2 } else { 1 };
        let name_end = name_start
            + bytes[name_start..]
                .iter()
                .take_while(|byte| is_name_byte(**byte))
                .count();
        let name = html[name_start..name_end].to_ascii_lowercase();
        if name.is_empty() {
            index = open + 1;
            continue;
        }
        let (attributes, after_tag, self_closing) = parse_attributes(html, bytes, name_end);
        index = after_tag;

        if closing {
            continue;
        }
        if matches!(name.as_str(), "script" | "style") && !self_closing {
            // `html.parser` treats these as CDATA: skip to the matching end
            // tag (case-insensitively), or EOF.
            index = skip_until_end_tag(html, bytes, index, &name);
            continue;
        }
        if name == "title" && source.title.is_none() {
            let (raw, next) = read_rcdata(html, bytes, index, "title");
            source.title = Some(decode_references(raw).trim().to_string());
            index = next;
            continue;
        }
        if name == "link" {
            source.links.push(Link {
                rel: attributes
                    .iter()
                    .rev()
                    .find(|(key, _)| key == "rel")
                    .map(|(_, value)| value.clone()),
                href: attributes
                    .iter()
                    .rev()
                    .find(|(key, _)| key == "href")
                    .map(|(_, value)| value.clone()),
            });
        }
    }
    source
}

/// The first `<link>` whose raw `rel` equals `selector`, as
/// `soup.select_one('link[rel="..."]')` matches it for the selectors the task
/// uses (`work_item_link_task.py:171-176`).
#[must_use]
pub fn first_link_with_rel<'a>(source: &'a CrawlSource, selector: &str) -> Option<&'a Link> {
    source
        .links
        .iter()
        .find(|link| link.rel.as_deref() == Some(selector))
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':')
}

fn find_byte(bytes: &[u8], from: usize, needle: u8) -> Option<usize> {
    bytes
        .get(from..)?
        .iter()
        .position(|byte| *byte == needle)
        .map(|offset| from + offset)
}

fn find_bytes(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let haystack = bytes.get(from..)?;
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| from + offset)
}

/// Read a raw-text element's contents up to `</name` (case-insensitive).
fn read_rcdata<'a>(html: &'a str, bytes: &[u8], from: usize, name: &str) -> (&'a str, usize) {
    let lowered = html[from..].to_ascii_lowercase();
    let needle = format!("</{name}");
    match lowered.find(&needle) {
        Some(offset) => {
            let content_end = from + offset;
            let after = match find_byte(bytes, content_end, b'>') {
                Some(end) => end + 1,
                None => bytes.len(),
            };
            (&html[from..content_end], after)
        }
        None => (&html[from..], bytes.len()),
    }
}

fn skip_until_end_tag(html: &str, bytes: &[u8], from: usize, name: &str) -> usize {
    let lowered = html[from..].to_ascii_lowercase();
    let needle = format!("</{name}");
    match lowered.find(&needle) {
        Some(offset) => find_byte(bytes, from + offset, b'>')
            .map(|end| end + 1)
            .unwrap_or(bytes.len()),
        None => bytes.len(),
    }
}

/// Parse a start tag's attributes, returning `(attrs, index after '>', self_closing)`.
fn parse_attributes(
    html: &str,
    bytes: &[u8],
    mut index: usize,
) -> (Vec<(String, String)>, usize, bool) {
    let mut attributes = Vec::new();
    let mut self_closing = false;
    loop {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            return (attributes, bytes.len(), self_closing);
        }
        match bytes[index] {
            b'>' => return (attributes, index + 1, self_closing),
            b'/' => {
                self_closing = bytes.get(index + 1) == Some(&b'>');
                index += 1;
                continue;
            }
            _ => {}
        }
        let name_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && !matches!(bytes[index], b'=' | b'>' | b'/')
        {
            index += 1;
        }
        let name = html[name_start..index].to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut value = String::new();
        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if index < bytes.len() && matches!(bytes[index], b'"' | b'\'') {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while index < bytes.len() && bytes[index] != quote {
                    index += 1;
                }
                value = decode_references(&html[value_start..index]);
                if index < bytes.len() {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < bytes.len()
                    && !bytes[index].is_ascii_whitespace()
                    && bytes[index] != b'>'
                {
                    index += 1;
                }
                value = decode_references(&html[value_start..index]);
            }
        }
        if !name.is_empty() {
            attributes.push((name, value));
        }
    }
}

/// The numeric references and the common named references `html.parser`
/// decodes. Unknown names are kept verbatim.
#[must_use]
pub fn decode_references(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if ch != '&' {
            out.push(ch);
            continue;
        }
        let rest = &text[offset + 1..];
        let Some(end) = rest.find(';').filter(|end| *end <= 32) else {
            out.push('&');
            continue;
        };
        let name = &rest[..end];
        let decoded =
            if let Some(digits) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(digits, 16)
                    .ok()
                    .and_then(char::from_u32)
            } else if let Some(digits) = name.strip_prefix('#') {
                digits.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                named_reference(name)
            };
        match decoded {
            Some(value) => {
                out.push(value);
                // Consume the digits and the semicolon.
                let consumed = 1 + end + 1;
                for _ in 0..consumed.saturating_sub(1) {
                    chars.next();
                }
            }
            None => out.push('&'),
        }
    }
    out
}

fn named_reference(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '\u{a9}',
        "reg" => '\u{ae}',
        "trade" => '\u{2122}',
        "hellip" => '\u{2026}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "bull" => '\u{2022}',
        "middot" => '\u{b7}',
        "laquo" => '\u{ab}',
        "raquo" => '\u{bb}',
        "times" => '\u{d7}',
        "divide" => '\u{f7}',
        "deg" => '\u{b0}',
        "plusmn" => '\u{b1}',
        "frac12" => '\u{bd}',
        "frac14" => '\u{bc}',
        "frac34" => '\u{be}',
        "sup2" => '\u{b2}',
        "sup3" => '\u{b3}',
        "micro" => '\u{b5}',
        "para" => '\u{b6}',
        "sect" => '\u{a7}',
        "cent" => '\u{a2}',
        "pound" => '\u{a3}',
        "yen" => '\u{a5}',
        "euro" => '\u{20ac}',
        "prime" => '\u{2032}',
        "Prime" => '\u{2033}',
        "larr" => '\u{2190}',
        "uarr" => '\u{2191}',
        "rarr" => '\u{2192}',
        "darr" => '\u{2193}',
        "harr" => '\u{2194}',
        "spades" => '\u{2660}',
        "clubs" => '\u{2663}',
        "hearts" => '\u{2665}',
        "diams" => '\u{2666}',
        "ensp" => '\u{2002}',
        "emsp" => '\u{2003}',
        "thinsp" => '\u{2009}',
        "zwnj" => '\u{200c}',
        "zwj" => '\u{200d}',
        "shy" => '\u{ad}',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_title_the_way_beautifulsoup_does() {
        let source =
            scan("<!DOCTYPE html><html><head><title>  Hello &amp; World  </title></head></html>");
        assert_eq!(source.title.as_deref(), Some("Hello & World"));

        // Title is RCDATA: a nested tag is text, and an empty title is Some("").
        let source = scan("<title> <b>Bold</b> rest </title>");
        assert_eq!(source.title.as_deref(), Some("<b>Bold</b> rest"));
        let source = scan("<title></title>");
        assert_eq!(source.title.as_deref(), Some(""));
        let source = scan("<p>no title</p>");
        assert_eq!(source.title, None);
    }

    #[test]
    fn ignores_script_and_comment_content() {
        let source = scan(
            "<!-- <title>comment</title> --><script>var t = '<title>x</title>';</script>\
             <style>/* <title>y</title> */</style><title>real</title>",
        );
        assert_eq!(source.title.as_deref(), Some("real"));
    }

    #[test]
    fn collects_links_in_document_order_with_raw_rel_values() {
        let source = scan(
            "<link rel=\"shortcut icon\" href=\"/s.ico\"><link rel=icon href=i.png>\
             <link rel=\"apple-touch-icon\" href='/a.png'><link rel=\"icon\" href=\"\">",
        );
        assert_eq!(source.links.len(), 4);
        assert_eq!(
            first_link_with_rel(&source, "icon").and_then(|link| link.href.as_deref()),
            Some("i.png")
        );
        assert_eq!(
            first_link_with_rel(&source, "shortcut icon").and_then(|link| link.href.as_deref()),
            Some("/s.ico")
        );
        assert_eq!(
            first_link_with_rel(&source, "apple-touch-icon").and_then(|link| link.href.as_deref()),
            Some("/a.png")
        );
        assert_eq!(
            first_link_with_rel(&source, "apple-touch-icon-precomposed"),
            None
        );
    }

    #[test]
    fn decodes_references_and_keeps_unknown_names() {
        assert_eq!(
            decode_references("a &amp; b &#39;c&#x27; &bogus; &"),
            "a & b 'c' &bogus; &"
        );
        assert_eq!(decode_references("caf&eacute;"), "caf&eacute;");
        assert_eq!(decode_references("&lt;tag&gt;"), "<tag>");
    }

    #[test]
    fn decodes_bytes_like_unicodedammit() {
        assert_eq!(decode(b"\xef\xbb\xbfcaf\xc3\xa9"), "café");
        assert_eq!(decode(b"caf\xe9"), "café");
    }
}
