// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `strip_tags` — `plane/utils/html_processor.py:7-31` — the HTML text
//! extractor the `description_stripped` hooks (H12, H23, H26, H27) use.
//!
//! Python is `HTMLParser(convert_charrefs=True)` feeding every `handle_data`
//! chunk into one buffer: text between tags survives with character references
//! decoded, comments and declarations disappear, and the contents of
//! `<script>`/`<style>` survive as text (CPython's CDATA mode still calls
//! `handle_data`).
//!
//! Ported shapes: tags, `<!-- -->`, `<!DOCTYPE>`, `<?...?>`, CDATA elements,
//! and character references. Numeric forms are exact; the named table is the
//! subset of CPython's HTML5 table that Plane's editor emits, so a named
//! entity outside it stays literal (documented divergence).

/// `strip_tags` (`plane/utils/html_processor.py:28-31`).
#[must_use]
pub fn strip_tags(html: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut index = 0;
    while index < bytes.len() {
        let Some(relative) = html[index..].find('<') else {
            out.push_str(&unescape(&html[index..]));
            break;
        };
        let tag_start = index + relative;
        if tag_start > index {
            out.push_str(&unescape(&html[index..tag_start]));
        }
        let rest = &html[tag_start..];
        // CPython's `HTMLParser` only treats `<` as markup when it starts a
        // tag (`[a-zA-Z]`), an end tag (`/`), a declaration (`!`) or a
        // processing instruction (`?`); `a < b` is data.
        let next = rest.as_bytes().get(1).copied().unwrap_or(b' ');
        if !(next.is_ascii_alphabetic() || next == b'/' || next == b'!' || next == b'?') {
            out.push('<');
            index = tag_start + 1;
            continue;
        }
        if rest.starts_with("<!--") {
            // A comment: skip to `-->`; an unterminated one consumes the rest.
            index = rest
                .find("-->")
                .map_or(bytes.len(), |end| tag_start + end + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            index = rest
                .find('>')
                .map_or(bytes.len(), |end| tag_start + end + 1);
            continue;
        }
        // A start or end tag. The name runs from the first letter to
        // whitespace, `/` or `>`.
        let name_start = if rest.starts_with("</") { 2 } else { 1 };
        let name: String = rest[name_start..]
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '-')
            .collect::<String>()
            .to_ascii_lowercase();
        let tag_end = find_tag_end(rest).map_or(bytes.len(), |end| tag_start + end + 1);
        index = tag_end;
        if rest.starts_with("</") {
            continue;
        }
        if name == "script" || name == "style" {
            // CPython's CDATA mode: the raw text up to the matching end tag is
            // data, character references are *not* converted.
            let close = format!("</{name}");
            let remaining = &html[index..];
            let lower = remaining.to_ascii_lowercase();
            match lower.find(&close) {
                Some(offset) => {
                    out.push_str(&remaining[..offset]);
                    let after = index + offset;
                    index = find_tag_end(&html[after..]).map_or(bytes.len(), |end| after + end + 1);
                }
                None => {
                    out.push_str(remaining);
                    index = bytes.len();
                }
            }
        }
    }
    out
}

/// The index of the `>` that ends a tag, skipping quoted attribute values.
fn find_tag_end(text: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (offset, byte) in text.bytes().enumerate() {
        match quote {
            Some(active) => {
                if byte == active {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => return Some(offset),
                _ => {}
            },
        }
    }
    None
}

/// `html.unescape` for the references Plane's editor emits.
///
/// Numeric references (`&#39;`, `&#x27;`) are exact. The named table covers
/// the HTML entities that appear in editor output (`amp`, `lt`, `gt`, `quot`,
/// `apos`, `nbsp`, and the common typographic ones); anything else is left
/// as-is, which is the documented divergence from CPython's full HTML5 table.
fn unescape(text: &str) -> String {
    const NAMED: [(&str, char); 10] = [
        ("amp", '&'),
        ("lt", '<'),
        ("gt", '>'),
        ("quot", '"'),
        ("apos", '\''),
        ("nbsp", '\u{a0}'),
        ("hellip", '\u{2026}'),
        ("mdash", '\u{2014}'),
        ("ndash", '\u{2013}'),
        ("rsquo", '\u{2019}'),
    ];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let candidate = &rest[start..];
        let Some(end) = candidate.find(';').filter(|end| *end <= 32) else {
            out.push('&');
            rest = &candidate[1..];
            continue;
        };
        let entity = &candidate[1..end];
        let decoded = if let Some(hex) = entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
        {
            u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
        } else if let Some(decimal) = entity.strip_prefix('#') {
            decimal.parse::<u32>().ok().and_then(char::from_u32)
        } else {
            NAMED
                .iter()
                .find(|(name, _)| *name == entity)
                .map(|(_, character)| *character)
        };
        match decoded {
            Some(character) => {
                out.push(character);
                rest = &candidate[end + 1..];
            }
            None => {
                out.push('&');
                rest = &candidate[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::strip_tags;

    #[test]
    fn strip_tags_matches_python_html_parser() {
        // Recorded from `plane.utils.html_processor.strip_tags` (see the
        // module docs for the method).
        let cases = [
            ("<p>hello</p>", "hello"),
            ("<p>a &amp; b</p>", "a & b"),
            ("<p>a &lt;b&gt; c</p>", "a <b> c"),
            ("", ""),
            ("<p>Hello <strong>world</strong>!</p>", "Hello world!"),
            ("<script>alert(1)</script>", "alert(1)"),
            ("<style>.x{color:red}</style>", ".x{color:red}"),
            ("<p>a<br/>b</p>", "ab"),
            ("<!-- c --><p>x</p>", "x"),
            ("<p>&#39;q&#39;</p>", "'q'"),
            ("plain", "plain"),
            ("<p>  spaced  </p>", "  spaced  "),
            ("<p>&nbsp;</p>", "\u{a0}"),
            ("<div><p>a</p><p>b</p></div>", "ab"),
            ("<p>a<b>c", "ac"),
            ("a < b > c", "a < b > c"),
        ];
        for (input, expected) in cases {
            assert_eq!(strip_tags(input), expected, "input {input:?}");
        }
    }
}
