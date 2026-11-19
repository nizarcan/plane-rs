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
///
/// Python feeds the whole string to `HTMLParser(convert_charrefs=True)` once
/// and reads back every `handle_data` chunk, so this is a structural port of
/// CPython 3.12's `html/parser.py:goahead` rather than a tag regex. The
/// behaviours that matter for `description_stripped`:
///
/// * data runs are unescaped with [`unescape`]; tags, comments, declarations
///   and processing instructions disappear;
/// * `CDATA_CONTENT_ELEMENTS` (`script`, `style`, `xmp`, `iframe`, `noembed`,
///   `noframes`) switch to RAWTEXT: their content is data *verbatim* (no entity
///   decoding) up to `</name` followed by one of `\t \n \r \f / >`
///   (`set_cdata_mode`); `plaintext` swallows the rest of the input;
///   `RCDATA_CONTENT_ELEMENTS` (`textarea`, `title`) do the same but decode
///   entities (`_escapable`);
/// * any markup construct that is not terminated (`<b`, `<!--`, `<!`, `<?`,
///   `<![CDATA[`) drops the rest of the input — `feed()` never sees EOF, so
///   `goahead(0)` leaves the tail buffered and it is never emitted;
/// * a text run that runs to the end of the input and holds an `&` in its last
///   34 bytes with no `\t \n \r \f space ;` after it is dropped whole (the
///   "wait for the other half of the entity" branch, which never resumes);
/// * `<` that does not start markup (`a < b`) is data.
#[must_use]
pub fn strip_tags(html: &str) -> String {
    /// `CDATA_CONTENT_ELEMENTS` (RAWTEXT: no entity decoding).
    const RAW_TEXT: [&str; 6] = ["script", "style", "xmp", "iframe", "noembed", "noframes"];
    /// `RCDATA_CONTENT_ELEMENTS` (tags dropped, entities decoded).
    const RCDATA: [&str; 2] = ["textarea", "title"];

    let mut out = String::with_capacity(html.len());
    let mut index = 0usize;
    // `(element, escapable)` — `HTMLParser.cdata_elem` plus `_escapable`.
    let mut cdata: Option<(&str, bool)> = None;
    while index < html.len() {
        if let Some((element, escapable)) = cdata {
            if element == "plaintext" {
                out.push_str(&html[index..]);
                return out;
            }
            let Some(offset) = cdata_end(&html[index..], element) else {
                // `interesting.search` found nothing: the rest stays buffered
                // and is never emitted.
                return out;
            };
            let data = &html[index..index + offset];
            if escapable {
                out.push_str(&unescape(data));
            } else {
                out.push_str(data);
            }
            index += offset;
            let Some(end) = find_tag_end(&html[index..]) else {
                return out;
            };
            index += end + 1;
            cdata = None;
            continue;
        }
        match html[index..].find('<') {
            None => {
                // The final run: `goahead` waits for the rest of an entity that
                // could still arrive, and `feed()` is never closed.
                let window = index.max(html.len().saturating_sub(34));
                if let Some(amp) = html[window..].rfind('&').map(|offset| window + offset)
                    && !html[amp..].chars().any(|character| {
                        matches!(character, '\t' | '\n' | '\r' | '\u{c}' | ' ' | ';')
                    })
                {
                    return out;
                }
                out.push_str(&unescape(&html[index..]));
                index = html.len();
            }
            Some(offset) => {
                let tag_start = index + offset;
                out.push_str(&unescape(&html[index..tag_start]));
                index = tag_start;
                let rest = &html[index..];
                let after = rest.as_bytes().get(1).copied().unwrap_or(0);
                if after.is_ascii_alphabetic() {
                    // `parse_starttag` + `check_for_whole_start_tag`.
                    let Some(end) = find_tag_end(rest) else {
                        return out;
                    };
                    let text = &rest[..end];
                    let name: String = text[1..]
                        .chars()
                        .take_while(|character| {
                            !matches!(character, '\t' | '\n' | '\r' | '\u{c}' | ' ' | '/' | '>')
                        })
                        .collect::<String>()
                        .to_ascii_lowercase();
                    index += end + 1;
                    // `<span />` is a startendtag: it never enters text mode.
                    let self_closing = text.trim_end().ends_with('/');
                    if !self_closing {
                        if RAW_TEXT.contains(&name.as_str()) {
                            cdata = Some((
                                RAW_TEXT
                                    .iter()
                                    .copied()
                                    .find(|candidate| *candidate == name)
                                    .unwrap_or("script"),
                                false,
                            ));
                        } else if RCDATA.contains(&name.as_str()) {
                            cdata = Some((
                                RCDATA
                                    .iter()
                                    .copied()
                                    .find(|candidate| *candidate == name)
                                    .unwrap_or("textarea"),
                                true,
                            ));
                        } else if name == "plaintext" {
                            cdata = Some(("plaintext", false));
                        }
                    }
                } else if after == b'/' {
                    let following = rest.as_bytes().get(2).copied().unwrap_or(0);
                    if following == b'>' {
                        // `</>` is ignored.
                        index += 3;
                    } else if following.is_ascii_alphabetic() {
                        let Some(end) = find_tag_end(rest) else {
                            return out;
                        };
                        index += end + 1;
                    } else {
                        // A bogus comment: consumed up to the next `>`.
                        let Some(end) = rest.find('>') else {
                            return out;
                        };
                        index += end + 1;
                    }
                } else if rest.starts_with("<!--") {
                    match comment_end(rest) {
                        Some(end) => index += end,
                        None => return out,
                    }
                } else if rest.starts_with("<?") {
                    let Some(end) = rest.find('>') else {
                        return out;
                    };
                    index += end + 1;
                } else if rest.starts_with("<![CDATA[") {
                    // `_support_cdata` is True in 3.12's `reset()`.
                    let Some(end) = rest.find("]]>") else {
                        return out;
                    };
                    index += end + 3;
                } else if rest
                    .get(..9)
                    .is_some_and(|head| head.eq_ignore_ascii_case("<!doctype"))
                {
                    let Some(end) = rest[9..].find('>') else {
                        return out;
                    };
                    index += 9 + end + 1;
                } else if rest.starts_with("<!") {
                    // `<![x]>`, `<!x>`: consumed up to the next `>`.
                    let Some(end) = rest.find('>') else {
                        return out;
                    };
                    index += end + 1;
                } else {
                    // `a < b`: the `<` is data.
                    out.push('<');
                    index += 1;
                }
            }
        }
    }
    out
}

/// `commentclose = r'--!?>'`, falling back to `commentabruptclose = r'-?>'`
/// (`parse_comment`): the bytes a comment occupies, closing token included.
fn comment_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    // `commentclose.search(rawdata, i+4)` then `commentabruptclose.match(i+4)`.
    let mut index = 4;
    while index + 1 < bytes.len() {
        if bytes[index] == b'-' && bytes[index + 1] == b'-' {
            let mut cursor = index + 2;
            if bytes.get(cursor) == Some(&b'!') {
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&b'>') {
                return Some(cursor + 1);
            }
        }
        index += 1;
    }
    // `-?>` anchored at i+4.
    let abrupt = &text[4..];
    let mut cursor = 0;
    if abrupt.starts_with('-') {
        cursor = 1;
    }
    if abrupt[cursor..].starts_with('>') {
        return Some(4 + cursor + 1);
    }
    None
}

/// `locatetagend`: the byte offset of the `>` that ends a tag, skipping quoted
/// attribute values. `None` when the tag is not terminated (the Python
/// `locatetagend` matches to EOF and `rawdata[j-1] != ">"` returns -1).
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

/// `interesting = re.compile(r"</%s(?=[\t\n\r\f />])", re.I|re.A)`
/// (`set_cdata_mode`): the byte offset of the end tag that closes a text-mode
/// element, or `None` when it never appears.
fn cdata_end(text: &str, element: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'<' && bytes[index + 1] == b'/' {
            let start = index + 2;
            let end = start + element.len();
            if end <= bytes.len()
                && text[start..end].eq_ignore_ascii_case(element)
                && matches!(
                    bytes.get(end),
                    Some(b'\t' | b'\n' | b'\r' | 0x0C | b' ' | b'/' | b'>')
                )
            {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

/// `html.unescape` (`Lib/html/__init__.py:78-120`, CPython 3.12.13) for one
/// run of data.
///
/// Byte-identical to CPython's, and also the decoder `HTMLParser` applies to
/// every tag attribute value (`html/parser.py:434-435`), which is what
/// `page_transaction`'s component ids go through.
///
/// The reference grammar is the module's `_charref` regex:
///
/// ```text
/// &(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)
/// ```
///
/// * a numeric reference goes through `_invalid_charrefs` (the windows-1252
///   remap and `&#13;` -> CR), then surrogates and values above `0x10FFFF`
///   become U+FFFD, then `_invalid_codepoints` are dropped;
/// * a named reference is looked up verbatim (the table holds the
///   semicolon-less legacy keys too), else the longest prefix of at least two
///   characters wins and the rest is kept as text (`&notit;` -> `¬it;`);
/// * anything else stays literal (`&;`, `&#;`, `&unknown;`).
///
/// The tables are generated from the interpreter
/// (`tools/descriptions/gen_html_entities.py`), not transcribed.
pub fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(offset) = rest.find('&') {
        out.push_str(&rest[..offset]);
        let candidate = &rest[offset..];
        match charref(candidate) {
            Some((consumed, replacement)) => {
                out.push_str(&replacement);
                rest = &candidate[consumed..];
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

/// One `_charref` match at the start of `text`, as `(bytes consumed, decoded)`.
fn charref(text: &str) -> Option<(usize, String)> {
    debug_assert!(text.starts_with('&'));
    let bytes = text.as_bytes();
    if bytes.get(1) == Some(&b'#') {
        let (radix, digits_start) = match bytes.get(2) {
            Some(b'x' | b'X') => (16u32, 3usize),
            _ => (10, 2),
        };
        let mut end = digits_start;
        while end < bytes.len() && (bytes[end] as char).is_digit(radix) {
            end += 1;
        }
        if end == digits_start {
            return None;
        }
        let consumed = if bytes.get(end) == Some(&b';') {
            end + 1
        } else {
            end
        };
        // A reference too long for `u64` is far above `0x10FFFF`, which Python
        // (arbitrary precision) maps to U+FFFD.
        let value = u64::from_str_radix(&text[digits_start..end], radix).unwrap_or(u64::MAX);
        let decoded = if let Some((_, replacement)) = super::html_entities::INVALID_CHARREFS
            .iter()
            .find(|(code, _)| u64::from(*code) == value)
        {
            (*replacement).to_owned()
        } else if (0xD800..=0xDFFF).contains(&value) || value > 0x10FFFF {
            "\u{FFFD}".to_owned()
        } else if super::html_entities::INVALID_CODEPOINTS
            .iter()
            .any(|code| u64::from(*code) == value)
        {
            String::new()
        } else {
            char::from_u32(u32::try_from(value).unwrap_or(u32::MAX))
                .map(String::from)
                .unwrap_or_else(|| "\u{FFFD}".to_owned())
        };
        return Some((consumed, decoded));
    }

    // `[^\t\n\f <&#;]{1,32};?` — at most 32 characters, none of the
    // excluded ones, then an optional semicolon.
    let mut end = 1;
    let mut count = 0;
    for (offset, character) in text[1..].char_indices() {
        if count == 32 {
            break;
        }
        if matches!(
            character,
            '\t' | '\n' | '\u{c}' | ' ' | '<' | '&' | '#' | ';'
        ) {
            break;
        }
        end = 1 + offset + character.len_utf8();
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let mut name_end = end;
    if bytes.get(end) == Some(&b';') {
        name_end = end + 1;
    }
    let name = &text[1..name_end];
    if let Ok(index) = super::html_entities::NAMED.binary_search_by(|(key, _)| key.cmp(&name)) {
        return Some((name_end, super::html_entities::NAMED[index].1.to_owned()));
    }
    // `for x in range(len(s)-1, 1, -1): if s[:x] in _html5: ...`
    let boundaries: Vec<usize> = name
        .char_indices()
        .map(|(offset, character)| offset + character.len_utf8())
        .collect();
    for boundary in boundaries.iter().rev().skip(1) {
        let prefix = &name[..*boundary];
        if let Ok(index) = super::html_entities::NAMED.binary_search_by(|(key, _)| key.cmp(&prefix))
        {
            let mut decoded = super::html_entities::NAMED[index].1.to_owned();
            decoded.push_str(&name[*boundary..]);
            return Some((name_end, decoded));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::strip_tags;

    /// Recorded from `plane.utils.html_processor.strip_tags` (CPython 3.12.13,
    /// `tools/descriptions/gen_description_corpus.py`): tags, comments,
    /// declarations, CDATA elements, character references (named, legacy
    /// semicolon-less, numeric, invalid), unicode and malformed markup.
    const GOLDEN: &str = include_str!("../../tests/golden/strip_tags.json");

    #[test]
    fn strip_tags_matches_python_html_parser() {
        let parsed: serde_json::Value = serde_json::from_str(GOLDEN).expect("golden json");
        let cases = parsed["cases"].as_array().expect("cases");
        assert!(cases.len() > 50, "corpus shrank: {}", cases.len());
        for case in cases {
            let input = case["input"].as_str().expect("input");
            let expected = case["output"].as_str().expect("output");
            assert_eq!(strip_tags(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn an_unterminated_run_with_half_an_entity_is_dropped() {
        // `goahead` waits for the other half of the reference and `feed()` is
        // never closed, so the whole run disappears (`html/parser.py:124-131`).
        assert_eq!(strip_tags("a &amp"), "");
        assert_eq!(strip_tags("<p>a &amp"), "");
        assert_eq!(strip_tags("a &amp x"), "a & x");
        assert_eq!(strip_tags("<p>t</p> &am"), "t");
    }
}
